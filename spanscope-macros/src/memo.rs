//! `#[memo]` expansion: option parsing, signature checks, and the cached body.
//!
//! The original body runs inside an immediately invoked closure so that
//! `return` and `?` keep their meaning. No cache lock is held while it runs.

use proc_macro2::{Span, TokenStream};
use quote::{quote, quote_spanned, ToTokens};
use syn::parse::Parser;
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::visit::Visit;
use syn::{Error, Expr, ImplItemFn, ItemFn, Lit, LitStr, Meta, Token, TraitItemFn};

#[derive(Default)]
struct Options {
    key: Option<Vec<Expr>>,
    max_entries: Option<Expr>,
    max_bytes: Option<Expr>,
    clear_on_full: bool,
    toggle: Option<LitStr>,
    verify: Option<LitStr>,
    verify_eq: Option<Expr>,
    value_size: Option<Expr>,
    name: Option<LitStr>,
    field: Option<Expr>,
    cache_none: bool,
}

fn string(value: Expr, option: &str) -> Result<LitStr, Error> {
    match value {
        Expr::Lit(syn::ExprLit {
            lit: Lit::Str(lit), ..
        }) => Ok(lit),
        other => Err(Error::new_spanned(
            other,
            format!("{option} must be a string literal"),
        )),
    }
}

fn parse_options(tokens: TokenStream) -> Result<Options, Error> {
    let metas = Punctuated::<Meta, Token![,]>::parse_terminated.parse2(tokens)?;
    let mut options = Options::default();
    let mut seen = Vec::<String>::new();
    for meta in metas {
        let Meta::NameValue(pair) = meta else {
            return Err(Error::new_spanned(
                meta,
                "expected `option = value`; see the spanscope::memo documentation",
            ));
        };
        let option = pair
            .path
            .get_ident()
            .map(ToString::to_string)
            .unwrap_or_default();
        if seen.contains(&option) {
            return Err(Error::new_spanned(
                &pair.path,
                format!("duplicate {option} option"),
            ));
        }
        seen.push(option.clone());
        let value = pair.value;
        match option.as_str() {
            "key" => {
                options.key = Some(match value {
                    Expr::Tuple(tuple) => tuple.elems.into_iter().collect(),
                    Expr::Paren(paren) => vec![*paren.expr],
                    other => vec![other],
                })
            }
            "max_entries" => options.max_entries = Some(value),
            "max_bytes" => options.max_bytes = Some(value),
            "on_full" => match &value {
                Expr::Path(path) if path.path.is_ident("clear") => options.clear_on_full = true,
                Expr::Path(path) if path.path.is_ident("skip") => options.clear_on_full = false,
                other => {
                    return Err(Error::new_spanned(
                        other,
                        "on_full must be `clear` or `skip`",
                    ))
                }
            },
            "toggle" => options.toggle = Some(string(value, "toggle")?),
            "verify" => options.verify = Some(string(value, "verify")?),
            "verify_eq" => options.verify_eq = Some(value),
            "value_size" => options.value_size = Some(value),
            "name" => options.name = Some(string(value, "name")?),
            "cache_none" => match value {
                Expr::Lit(syn::ExprLit {
                    lit: Lit::Bool(flag),
                    ..
                }) => options.cache_none = flag.value,
                other => return Err(Error::new_spanned(other, "cache_none must be a bool")),
            },
            "scope" => match value {
                Expr::Path(path) if path.path.is_ident("global") => {}
                Expr::Call(call)
                    if matches!(&*call.func, Expr::Path(path) if path.path.is_ident("field"))
                        && call.args.len() == 1 =>
                {
                    options.field = call.args.into_iter().next();
                }
                other => {
                    return Err(Error::new_spanned(
                        other,
                        "scope must be `global` or `field(self.cache)`",
                    ))
                }
            },
            _ => {
                return Err(Error::new_spanned(
                    &pair.path,
                    "unknown memo option; expected key, max_entries, max_bytes, on_full, \
                     toggle, verify, verify_eq, value_size, name, scope, or cache_none",
                ))
            }
        }
    }
    if let (Some(_), Some(bytes)) = (&options.max_entries, &options.max_bytes) {
        return Err(Error::new_spanned(
            bytes,
            "max_entries and max_bytes are mutually exclusive",
        ));
    }
    Ok(options)
}

struct ImplTraitFinder(Option<Span>);

impl Visit<'_> for ImplTraitFinder {
    fn visit_type_impl_trait(&mut self, node: &syn::TypeImplTrait) {
        self.0.get_or_insert(node.span());
    }
}

fn impl_trait(ty: &syn::Type) -> Option<Span> {
    let mut finder = ImplTraitFinder(None);
    finder.visit_type(ty);
    finder.0
}

fn check_signature(signature: &syn::Signature) -> Result<(), Error> {
    if let Some(token) = &signature.asyncness {
        return Err(Error::new_spanned(
            token,
            "memo does not support async fn; memoize a synchronous helper instead",
        ));
    }
    if let Some(token) = &signature.constness {
        return Err(Error::new_spanned(
            token,
            "memo does not support const fn: caching needs runtime state",
        ));
    }
    if let Some(token) = &signature.unsafety {
        return Err(Error::new_spanned(
            token,
            "memo does not support unsafe fn; memoize a safe wrapper instead",
        ));
    }
    if let Some(param) = signature
        .generics
        .params
        .iter()
        .find(|param| !matches!(param, syn::GenericParam::Lifetime(_)))
    {
        return Err(Error::new_spanned(
            param,
            "memo does not support generic functions: one static cache cannot hold every \
             instantiation; memoize a concrete wrapper instead",
        ));
    }
    for input in &signature.inputs {
        if let syn::FnArg::Typed(typed) = input {
            if let Some(span) = impl_trait(&typed.ty) {
                return Err(Error::new(
                    span,
                    "memo does not support `impl Trait` arguments: they make the function \
                     generic",
                ));
            }
        }
    }
    if let syn::ReturnType::Type(_, ty) = &signature.output {
        if let Some(span) = impl_trait(ty) {
            return Err(Error::new(
                span,
                "memo does not support `impl Trait` return types: the cached value needs a \
                 concrete Clone type",
            ));
        }
    }
    Ok(())
}

/// Default key: every typed argument, by reference. The receiver is never included.
fn default_key(signature: &syn::Signature, field: bool) -> Result<Vec<Expr>, Error> {
    let mut key = Vec::new();
    for input in &signature.inputs {
        match input {
            syn::FnArg::Receiver(receiver) if !field => {
                return Err(Error::new_spanned(
                    receiver,
                    "methods need an explicit key = (...) with global scope: the receiver is \
                     never part of the default key",
                ))
            }
            syn::FnArg::Receiver(_) => {}
            syn::FnArg::Typed(typed) => {
                let syn::Pat::Ident(pat) = &*typed.pat else {
                    return Err(Error::new_spanned(
                        &typed.pat,
                        "the default key needs named arguments; use key = (...)",
                    ));
                };
                if let syn::Type::Reference(reference) = &*typed.ty {
                    if reference.mutability.is_some() {
                        return Err(Error::new_spanned(
                            typed,
                            "`&mut` arguments cannot be part of the key; exclude them with \
                             key = (...) and make sure they do not affect the result",
                        ));
                    }
                }
                let ident = &pat.ident;
                key.push(syn::parse_quote!(#ident));
            }
        }
    }
    Ok(key)
}

fn option_like(ty: &syn::Type) -> Option<&'static str> {
    let syn::Type::Path(path) = ty else {
        return None;
    };
    match path.path.segments.last()?.ident.to_string().as_str() {
        "Option" => Some("is_some"),
        "Result" => Some("is_ok"),
        _ => None,
    }
}

/// `#[trace]` above `#[memo]` has already inserted its descriptor and guard.
/// They stay outside the cached closure so hits are still timed.
fn hoist_trace(block: &mut syn::Block) -> Vec<syn::Stmt> {
    let is_guard = |stmt: &syn::Stmt| {
        matches!(stmt, syn::Stmt::Local(local)
            if matches!(&local.pat, syn::Pat::Ident(pat) if pat.ident == "_spanscope_guard"))
    };
    let is_descriptor = |stmt: &syn::Stmt| matches!(stmt, syn::Stmt::Item(syn::Item::Static(item)) if item.ident == "DESCRIPTOR");
    if block.stmts.len() >= 2 && is_descriptor(&block.stmts[0]) && is_guard(&block.stmts[1]) {
        block.stmts.drain(..2).collect()
    } else {
        Vec::new()
    }
}

fn runtime_path() -> Result<TokenStream, Error> {
    match proc_macro_crate::crate_name("spanscope") {
        Ok(proc_macro_crate::FoundCrate::Itself) => Ok(quote!(::spanscope)),
        Ok(proc_macro_crate::FoundCrate::Name(name)) => {
            let ident = syn::Ident::new(&name, Span::call_site());
            Ok(quote!(::#ident))
        }
        Err(error) => Err(Error::new(Span::call_site(), error)),
    }
}

fn memoize_block(
    signature: &syn::Signature,
    block: &mut syn::Block,
    options: Options,
) -> Result<(), Error> {
    check_signature(signature)?;
    let key = match options.key {
        Some(key) => key,
        None => default_key(signature, options.field.is_some())?,
    };
    let rt = runtime_path()?;
    let m = quote!(#rt::__memo);
    let ident = &signature.ident;
    let name = options.name.as_ref().map_or_else(
        || quote!(module_path!(), "::", stringify!(#ident)),
        |name| quote!(#name),
    );
    let output = match &signature.output {
        syn::ReturnType::Default => quote!(()),
        syn::ReturnType::Type(_, ty) => ty.to_token_stream(),
    };
    let key_value = match key.as_slice() {
        [single] => quote!(#m::MemoKey::memo_key(&#single)),
        many => quote!(#m::MemoKey::memo_key(&(#(&#many,)*))),
    };
    let option = |value: &Option<Expr>| match value {
        Some(value) => quote!(::core::option::Option::Some(#value)),
        None => quote!(::core::option::Option::None),
    };
    let max_entries = option(&options.max_entries);
    let max_bytes = option(&options.max_bytes);
    let literal = |value: &Option<LitStr>| match value {
        Some(value) => quote!(::core::option::Option::Some(#value)),
        None => quote!(::core::option::Option::None),
    };
    let toggle = literal(&options.toggle);
    let verify = literal(&options.verify);
    let clear = options.clear_on_full;
    let cacheable = match signature_output(signature).and_then(option_like) {
        Some(method) if !options.cache_none => {
            let method = syn::Ident::new(method, Span::call_site());
            quote!(|__value: &#output| __value.#method())
        }
        _ => quote!(|_: &#output| true),
    };
    let measured = options.max_bytes.is_some() || options.value_size.is_some();
    let size = match (&options.value_size, measured) {
        (Some(path), _) => {
            quote!(|__key, __value| #m::MemoSize::memo_size(__key) + (#path)(__value))
        }
        (None, true) => {
            quote!(|__key, __value| #m::MemoSize::memo_size(__key) + #m::MemoSize::memo_size(__value))
        }
        (None, false) => quote!(#m::shallow_size),
    };
    let equal = match (&options.verify, &options.verify_eq) {
        (_, Some(path)) => quote!(|__a: &#output, __b: &#output| (#path)(__a, __b)),
        (Some(_), None) => quote!(|__a: &#output, __b: &#output| #m::MemoEq::memo_eq(__a, __b)),
        (None, None) => quote!(|_: &#output, _: &#output| true),
    };
    let store = match &options.field {
        Some(field) => quote_spanned!(field.span()=> #m::field(&__SPANSCOPE_MEMO, &(#field))),
        None => quote!(#m::global(&__SPANSCOPE_MEMO)),
    };
    let hoisted = hoist_trace(block);
    let body = &*block;
    *block = syn::parse_quote!({
        #(#hoisted)*
        static __SPANSCOPE_MEMO: #m::MemoFn = #m::MemoFn::new(#m::Spec {
            name: concat!(#name),
            attribute: concat!("memo.", #name),
            max_entries: #max_entries,
            max_bytes: #max_bytes,
            clear_on_full: #clear,
            toggle: #toggle,
            verify: #verify,
        });
        let __spanscope_key = #key_value;
        let __spanscope_probe = match #m::lookup::<_, #output>(
            &__SPANSCOPE_MEMO,
            #store,
            &__spanscope_key,
        ) {
            #m::Probe::Hit(__value) => return __value,
            __probe => __probe,
        };
        #[allow(clippy::redundant_closure_call)]
        let __spanscope_value: #output = (|| -> #output #body)();
        #m::finish(
            &__SPANSCOPE_MEMO,
            #store,
            __spanscope_key,
            __spanscope_probe,
            __spanscope_value,
            #cacheable,
            #equal,
            #size,
        )
    });
    Ok(())
}

fn signature_output(signature: &syn::Signature) -> Option<&syn::Type> {
    match &signature.output {
        syn::ReturnType::Default => None,
        syn::ReturnType::Type(_, ty) => Some(ty),
    }
}

pub(crate) fn expand(arguments: TokenStream, function: TokenStream) -> TokenStream {
    let result = parse_options(arguments).and_then(|options| {
        if let Ok(mut item) = syn::parse2::<ItemFn>(function.clone()) {
            memoize_block(&item.sig, &mut item.block, options)?;
            Ok(item.into_token_stream())
        } else if let Ok(mut item) = syn::parse2::<ImplItemFn>(function.clone()) {
            memoize_block(&item.sig, &mut item.block, options)?;
            Ok(item.into_token_stream())
        } else if let Ok(mut item) = syn::parse2::<TraitItemFn>(function.clone()) {
            let Some(block) = item.default.as_mut() else {
                return Err(Error::new_spanned(
                    item.sig,
                    "memo requires a function body",
                ));
            };
            memoize_block(&item.sig.clone(), block, options)?;
            Ok(item.into_token_stream())
        } else {
            Err(Error::new_spanned(
                function,
                "memo supports functions and methods with bodies",
            ))
        }
    });
    result.unwrap_or_else(Error::into_compile_error)
}
