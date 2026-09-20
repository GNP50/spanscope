//! Attribute macro entry point for spanscope.
//!
//! Disabled expansion returns the input tokens without parsing the function or
//! its arguments. Enabled expansion instruments synchronous and async functions.

use proc_macro::TokenStream;

/// Profiles one function invocation when the runtime's `enabled` feature is set.
///
/// Supported syntax is `#[trace]`, `#[trace(root)]`, and
/// `#[trace(name = "label", tags("one", "two"))]`.
#[proc_macro_attribute]
pub fn trace(arguments: TokenStream, function: TokenStream) -> TokenStream {
    #[cfg(not(feature = "enabled"))]
    {
        let _ = arguments;
        function
    }

    #[cfg(feature = "enabled")]
    {
        enabled::expand(arguments, function)
    }
}

#[cfg(feature = "enabled")]
mod enabled {
    use proc_macro::TokenStream;
    use proc_macro2::TokenStream as TokenStream2;
    use quote::{quote, ToTokens};
    use syn::parse::Parser;
    use syn::punctuated::Punctuated;
    use syn::{
        parse_quote, Error, Expr, ImplItemFn, ItemFn, Lit, LitStr, Meta, Token, TraitItemFn,
    };

    #[derive(Default)]
    struct Options {
        root: bool,
        name: Option<LitStr>,
        tags: Vec<LitStr>,
        has_tags: bool,
    }

    fn parse_options(tokens: TokenStream2) -> Result<Options, Error> {
        let parser = Punctuated::<Meta, Token![,]>::parse_terminated;
        let metas = parser.parse2(tokens)?;
        let mut options = Options::default();
        for meta in metas {
            match meta {
                Meta::Path(path) if path.is_ident("root") => {
                    if options.root {
                        return Err(Error::new_spanned(path, "duplicate root option"));
                    }
                    options.root = true;
                }
                Meta::NameValue(pair) if pair.path.is_ident("name") => {
                    if options.name.is_some() {
                        return Err(Error::new_spanned(pair, "duplicate name option"));
                    }
                    match pair.value {
                        Expr::Lit(expr) => match expr.lit {
                            Lit::Str(lit) => options.name = Some(lit),
                            lit => {
                                return Err(Error::new_spanned(
                                    lit,
                                    "name must be a string literal",
                                ))
                            }
                        },
                        other => {
                            return Err(Error::new_spanned(other, "name must be a string literal"))
                        }
                    }
                }
                Meta::List(list) if list.path.is_ident("tags") => {
                    if options.has_tags {
                        return Err(Error::new_spanned(list, "duplicate tags option"));
                    }
                    options.has_tags = true;
                    options.tags = Punctuated::<LitStr, Token![,]>::parse_terminated
                        .parse2(list.tokens)?
                        .into_iter()
                        .collect();
                }
                other => {
                    return Err(Error::new_spanned(
                        other,
                        "expected root, name = \"...\", or tags(\"...\")",
                    ))
                }
            }
        }
        Ok(options)
    }

    fn runtime_path() -> Result<TokenStream2, Error> {
        match proc_macro_crate::crate_name("spanscope") {
            Ok(proc_macro_crate::FoundCrate::Itself) => Ok(quote!(::spanscope)),
            Ok(proc_macro_crate::FoundCrate::Name(name)) => {
                let ident = syn::Ident::new(&name, proc_macro2::Span::call_site());
                Ok(quote!(::#ident))
            }
            Err(error) => Err(Error::new(proc_macro2::Span::call_site(), error)),
        }
    }

    fn check_signature(signature: &syn::Signature) -> Result<(), Error> {
        if let Some(constness) = signature.constness.as_ref() {
            return Err(Error::new_spanned(
                constness,
                "const fn instrumentation requires runtime timing and is unsupported",
            ));
        }
        if let Some(unsafety) = signature.unsafety.as_ref() {
            return Err(Error::new_spanned(
                unsafety,
                "unsafe fn instrumentation is not supported; use a safe wrapper or manual span",
            ));
        }
        Ok(())
    }

    fn instrument_block(
        signature: &syn::Signature,
        block: &mut syn::Block,
        options: Options,
    ) -> Result<(), Error> {
        check_signature(signature)?;
        let runtime = runtime_path()?;
        let ident = &signature.ident;
        let name = options.name.map_or_else(
            || quote!(concat!(module_path!(), "::", stringify!(#ident))),
            |literal| quote!(#literal),
        );
        let root = options.root;
        let tags = options.tags;
        let descriptor: syn::Stmt = parse_quote! {
            static DESCRIPTOR: #runtime::__private::SpanDescriptor =
                #runtime::__private::SpanDescriptor::new(
                    #name, file!(), line!(), &[#(#tags),*]
                );
        };
        if signature.asyncness.is_some() {
            let original = block.clone();
            *block = parse_quote!({
                #descriptor
                #runtime::__private::trace_future(async move #original, &DESCRIPTOR, #root).await
            });
        } else {
            block.stmts.insert(
                0,
                parse_quote! {
                    let _spanscope_guard = #runtime::__private::enter(&DESCRIPTOR, #root);
                },
            );
            block.stmts.insert(0, descriptor);
        }
        Ok(())
    }

    fn instrument(mut function: ItemFn, options: Options) -> Result<TokenStream2, Error> {
        instrument_block(&function.sig, &mut function.block, options)?;
        Ok(function.into_token_stream())
    }

    fn instrument_method(
        mut function: ImplItemFn,
        options: Options,
    ) -> Result<TokenStream2, Error> {
        instrument_block(&function.sig, &mut function.block, options)?;
        Ok(function.into_token_stream())
    }

    fn instrument_trait_method(
        mut function: TraitItemFn,
        options: Options,
    ) -> Result<TokenStream2, Error> {
        let Some(block) = function.default.as_mut() else {
            return Err(Error::new_spanned(
                function.sig,
                "trace requires a function body",
            ));
        };
        instrument_block(&function.sig, block, options)?;
        Ok(function.into_token_stream())
    }

    pub(super) fn expand(arguments: TokenStream, function: TokenStream) -> TokenStream {
        let arguments: TokenStream2 = arguments.into();
        let function: TokenStream2 = function.into();
        let result = parse_options(arguments).and_then(|options| {
            // Free functions and methods share syntax for many signatures.
            if let Ok(item) = syn::parse2::<ItemFn>(function.clone()) {
                instrument(item, options)
            } else if let Ok(item) = syn::parse2::<ImplItemFn>(function.clone()) {
                instrument_method(item, options)
            } else if let Ok(item) = syn::parse2::<TraitItemFn>(function.clone()) {
                instrument_trait_method(item, options)
            } else {
                Err(Error::new_spanned(
                    function,
                    "trace supports functions and methods with bodies",
                ))
            }
        });
        result.unwrap_or_else(Error::into_compile_error).into()
    }
}
