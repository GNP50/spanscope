//! `#[derive(Tracked)]` and the `track(...)` rewrite of instrumented functions.

use crate::enabled::runtime_path;
use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{quote, quote_spanned, ToTokens};
use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::visit_mut::{self, VisitMut};
use syn::{parse_quote, BinOp, Block, Data, DeriveInput, Error, Expr, Fields, Ident, Member, Stmt};

enum Mode {
    Track,
    Skip,
    Opaque,
    Id,
}

fn field_mode(field: &syn::Field) -> Result<Mode, Error> {
    let mut mode = Mode::Track;
    for attr in field
        .attrs
        .iter()
        .filter(|attr| attr.path().is_ident("tracked"))
    {
        attr.parse_nested_meta(|meta| {
            mode = if meta.path.is_ident("id") {
                Mode::Id
            } else if meta.path.is_ident("skip") {
                Mode::Skip
            } else if meta.path.is_ident("opaque") {
                Mode::Opaque
            } else {
                return Err(meta.error("expected tracked(id), tracked(skip), or tracked(opaque)"));
            };
            Ok(())
        })?;
    }
    Ok(mode)
}

fn label(ident: Option<&Ident>, index: usize) -> String {
    match ident {
        Some(ident) => ident.to_string().trim_start_matches("r#").to_owned(),
        None => index.to_string(),
    }
}

/// Field statements for one struct body or enum variant.
fn field_statements(
    fields: &Fields,
    access: impl Fn(usize, &syn::Field) -> TokenStream2,
    allow_id: bool,
    runtime: &TokenStream2,
    bounds: &mut Vec<TokenStream2>,
) -> Result<Vec<TokenStream2>, Error> {
    let mut statements = Vec::new();
    let mut id = false;
    for (index, field) in fields.iter().enumerate() {
        let name = label(field.ident.as_ref(), index);
        let value = access(index, field);
        let ty = &field.ty;
        match field_mode(field)? {
            Mode::Skip => {}
            Mode::Id if !allow_id => {
                return Err(Error::new_spanned(
                    field,
                    "tracked(id) is supported on struct fields only",
                ))
            }
            Mode::Id if id => {
                return Err(Error::new_spanned(field, "duplicate tracked(id) field"));
            }
            Mode::Id => {
                id = true;
                bounds.push(quote!(#ty: #runtime::objects::InstanceKeySource));
                statements.push(quote! {
                    __object.instance(#runtime::objects::InstanceKeySource::instance_key(#value));
                });
            }
            Mode::Opaque => {
                bounds.push(quote!(#ty: ::core::hash::Hash));
                statements.push(quote!(__object.opaque(#name, #value);));
            }
            Mode::Track => {
                bounds.push(quote!(#ty: #runtime::objects::Track));
                statements.push(quote!(__object.field(#name, #value);));
            }
        }
    }
    Ok(statements)
}

fn expand_derive(input: DeriveInput) -> Result<TokenStream2, Error> {
    let runtime = runtime_path()?;
    let name = &input.ident;
    let mut bounds = Vec::new();
    let body = match &input.data {
        Data::Struct(data) => {
            let statements = field_statements(
                &data.fields,
                |index, field| match &field.ident {
                    Some(ident) => quote!(&self.#ident),
                    None => {
                        let index = syn::Index::from(index);
                        quote!(&self.#index)
                    }
                },
                true,
                &runtime,
                &mut bounds,
            )?;
            quote!(#(#statements)*)
        }
        Data::Enum(data) => {
            let mut arms = Vec::new();
            for variant in &data.variants {
                let variant_ident = &variant.ident;
                let variant_name = label(Some(variant_ident), 0);
                let bindings = variant
                    .fields
                    .iter()
                    .enumerate()
                    .map(|(index, _)| Ident::new(&format!("__field{index}"), Span::call_site()))
                    .collect::<Vec<_>>();
                let statements = field_statements(
                    &variant.fields,
                    |index, _| {
                        let binding = &bindings[index];
                        quote!(#binding)
                    },
                    false,
                    &runtime,
                    &mut bounds,
                )?;
                let pattern = match &variant.fields {
                    Fields::Named(fields) => {
                        let names = fields.named.iter().map(|field| &field.ident);
                        quote!(Self::#variant_ident { #(#names: #bindings),* })
                    }
                    Fields::Unnamed(_) => quote!(Self::#variant_ident(#(#bindings),*)),
                    Fields::Unit => quote!(Self::#variant_ident),
                };
                arms.push(quote! {
                    #[allow(unused_variables)]
                    #pattern => {
                        __object.variant(#variant_name);
                        #(#statements)*
                    }
                });
            }
            quote!(match self { #(#arms)* })
        }
        Data::Union(data) => {
            return Err(Error::new_spanned(
                data.union_token,
                "Tracked cannot be derived for unions",
            ))
        }
    };
    let (impl_generics, type_generics, where_clause) = input.generics.split_for_impl();
    let mut predicates = where_clause
        .map(|clause| {
            clause
                .predicates
                .iter()
                .map(ToTokens::to_token_stream)
                .collect()
        })
        .unwrap_or_else(Vec::new);
    // Field bounds only matter for generic types; concrete fields are checked by the body.
    if !input.generics.params.is_empty() {
        predicates.extend(bounds);
    }
    let where_clause = if predicates.is_empty() {
        quote!()
    } else {
        quote!(where #(#predicates),*)
    };
    Ok(quote! {
        impl #impl_generics #runtime::objects::Track for #name #type_generics #where_clause {
            fn track(
                &self,
                __context: &mut #runtime::objects::TrackContext,
            ) -> #runtime::objects::TrackNode {
                let mut __object = #runtime::objects::TrackNode::object(
                    concat!(module_path!(), "::", stringify!(#name)),
                    __context,
                );
                #body
                __object.finish()
            }
        }
    })
}

pub(crate) fn derive(input: TokenStream) -> TokenStream {
    syn::parse::<DeriveInput>(input)
        .and_then(expand_derive)
        .unwrap_or_else(Error::into_compile_error)
        .into()
}

/// How a tracked path appears in source.
#[derive(Clone, Copy)]
enum Access {
    Read,
    Write,
    Call,
}

struct Found {
    arg: usize,
    path: String,
    access: Access,
    span: Span,
}

/// Collects tracked accesses in one statement, stopping at nested blocks,
/// closures, loop bodies, match arms, and macros, which get their own sites.
struct Collector<'a> {
    tracked: &'a [Ident],
    found: Vec<Found>,
}

/// A place rooted at a tracked argument: argument index, path, index expressions.
fn place<'e>(tracked: &[Ident], expr: &'e Expr) -> Option<(usize, String, Vec<&'e Expr>)> {
    match expr {
        Expr::Path(path) if path.qself.is_none() => {
            let ident = path.path.get_ident()?;
            let arg = tracked.iter().position(|tracked| tracked == ident)?;
            Some((arg, String::new(), Vec::new()))
        }
        Expr::Field(field) => {
            let (arg, mut path, indexes) = place(tracked, &field.base)?;
            if !path.is_empty() {
                path.push('.');
            }
            match &field.member {
                Member::Named(ident) => path.push_str(ident.to_string().trim_start_matches("r#")),
                Member::Unnamed(index) => path.push_str(&index.index.to_string()),
            }
            Some((arg, path, indexes))
        }
        Expr::Index(index) => {
            let (arg, mut path, mut indexes) = place(tracked, &index.expr)?;
            path.push_str("[*]");
            indexes.push(&index.index);
            Some((arg, path, indexes))
        }
        Expr::Paren(paren) => place(tracked, &paren.expr),
        Expr::Unary(unary) if matches!(unary.op, syn::UnOp::Deref(_)) => {
            place(tracked, &unary.expr)
        }
        _ => None,
    }
}

impl Collector<'_> {
    /// Records `expr` if it is a tracked place; returns whether it was.
    fn record(&mut self, expr: &Expr, access: impl FnOnce(&str) -> Access) -> bool {
        let Some((arg, path, indexes)) = place(self.tracked, expr) else {
            return false;
        };
        let access = access(&path);
        self.found.push(Found {
            arg,
            path,
            access,
            span: expr.span(),
        });
        for index in indexes {
            self.visit_expr(index);
        }
        true
    }
}

fn compound(op: &BinOp) -> bool {
    matches!(
        op,
        BinOp::AddAssign(_)
            | BinOp::SubAssign(_)
            | BinOp::MulAssign(_)
            | BinOp::DivAssign(_)
            | BinOp::RemAssign(_)
            | BinOp::BitXorAssign(_)
            | BinOp::BitAndAssign(_)
            | BinOp::BitOrAssign(_)
            | BinOp::ShlAssign(_)
            | BinOp::ShrAssign(_)
    )
}

impl<'ast> Visit<'ast> for Collector<'_> {
    fn visit_expr(&mut self, expr: &'ast Expr) {
        // A bare tracked argument is passed on whole; its use is opaque.
        let recorded = self.record(expr, |path| {
            if path.is_empty() {
                Access::Call
            } else {
                Access::Read
            }
        });
        if !recorded {
            visit::visit_expr(self, expr);
        }
    }

    fn visit_expr_assign(&mut self, assign: &'ast syn::ExprAssign) {
        if !self.record(&assign.left, |_| Access::Write) {
            self.visit_expr(&assign.left);
        }
        self.visit_expr(&assign.right);
    }

    fn visit_expr_binary(&mut self, binary: &'ast syn::ExprBinary) {
        if !(compound(&binary.op) && self.record(&binary.left, |_| Access::Write)) {
            self.visit_expr(&binary.left);
        }
        self.visit_expr(&binary.right);
    }

    fn visit_expr_reference(&mut self, reference: &'ast syn::ExprReference) {
        let mutable = reference.mutability.is_some();
        let recorded = self.record(&reference.expr, |_| {
            if mutable {
                Access::Write
            } else {
                Access::Read
            }
        });
        if !recorded {
            self.visit_expr(&reference.expr);
        }
    }

    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        if !self.record(&call.receiver, |_| Access::Call) {
            self.visit_expr(&call.receiver);
        }
        for argument in &call.args {
            self.visit_expr(argument);
        }
    }

    fn visit_expr_if(&mut self, expr: &'ast syn::ExprIf) {
        self.visit_expr(&expr.cond);
    }

    fn visit_expr_while(&mut self, expr: &'ast syn::ExprWhile) {
        self.visit_expr(&expr.cond);
    }

    fn visit_expr_for_loop(&mut self, expr: &'ast syn::ExprForLoop) {
        self.visit_expr(&expr.expr);
    }

    fn visit_arm(&mut self, arm: &'ast syn::Arm) {
        if let Some((_, guard)) = &arm.guard {
            self.visit_expr(guard);
        }
    }

    fn visit_block(&mut self, _: &'ast Block) {}
    fn visit_expr_closure(&mut self, _: &'ast syn::ExprClosure) {}
    fn visit_expr_async(&mut self, _: &'ast syn::ExprAsync) {}
    fn visit_item(&mut self, _: &'ast syn::Item) {}
    fn visit_macro(&mut self, _: &'ast syn::Macro) {}
}

/// Inserts one counting statement before each statement that accesses a
/// tracked argument. Counting statements never mention user variables, so
/// they cannot change borrowing or moves.
struct Rewriter<'a> {
    tracked: &'a [Ident],
    runtime: &'a TokenStream2,
    sites: Vec<TokenStream2>,
}

fn blockify(expr: &mut Expr) {
    if !matches!(expr, Expr::Block(_)) {
        let inner = expr.clone();
        *expr = parse_quote!({ #inner });
    }
}

impl VisitMut for Rewriter<'_> {
    fn visit_block_mut(&mut self, block: &mut Block) {
        let runtime = self.runtime;
        for mut statement in std::mem::take(&mut block.stmts) {
            if !matches!(statement, Stmt::Item(_) | Stmt::Macro(_)) {
                self.visit_stmt_mut(&mut statement);
                let mut collector = Collector {
                    tracked: self.tracked,
                    found: Vec::new(),
                };
                collector.visit_stmt(&statement);
                for found in collector.found {
                    let index = self.sites.len();
                    let arg = found.arg;
                    let path = found.path;
                    let access = match found.access {
                        Access::Read => quote!(Read),
                        Access::Write => quote!(Write),
                        Access::Call => quote!(Call),
                    };
                    let line = quote_spanned!(found.span=> line!());
                    self.sites.push(quote! {
                        #runtime::__private::FieldSite::new(
                            #arg, #path, #runtime::__private::Access::#access, #line
                        )
                    });
                    block
                        .stmts
                        .push(parse_quote!(__spanscope_track_ref.touch(#index);));
                }
            }
            block.stmts.push(statement);
        }
    }

    fn visit_expr_closure_mut(&mut self, closure: &mut syn::ExprClosure) {
        blockify(&mut closure.body);
        visit_mut::visit_expr_closure_mut(self, closure);
    }

    fn visit_arm_mut(&mut self, arm: &mut syn::Arm) {
        blockify(&mut arm.body);
        visit_mut::visit_arm_mut(self, arm);
    }

    fn visit_expr_if_mut(&mut self, expr: &mut syn::ExprIf) {
        if let Some((_, otherwise)) = &mut expr.else_branch {
            blockify(otherwise);
        }
        visit_mut::visit_expr_if_mut(self, expr);
    }

    fn visit_item_mut(&mut self, _: &mut syn::Item) {}
    fn visit_macro_mut(&mut self, _: &mut syn::Macro) {}
}

fn mentions_impl(tokens: TokenStream2) -> bool {
    tokens.into_iter().any(|token| match token {
        proc_macro2::TokenTree::Ident(ident) => ident == "impl",
        proc_macro2::TokenTree::Group(group) => mentions_impl(group.stream()),
        _ => false,
    })
}

/// Rewrites a function body whose `track(...)` arguments were validated.
pub(crate) fn instrument_tracked(
    signature: &syn::Signature,
    block: &Block,
    tracked: &[Ident],
    runtime: &TokenStream2,
    descriptor: &Stmt,
    root: bool,
) -> Block {
    let mut body = block.clone();
    let mut rewriter = Rewriter {
        tracked,
        runtime,
        sites: Vec::new(),
    };
    rewriter.visit_block_mut(&mut body);
    let sites = rewriter.sites;
    let count = sites.len();
    let observe = tracked
        .iter()
        .map(|ident| quote!(#runtime::__private::observe(&#ident)))
        .collect::<Vec<_>>();
    let output = match &signature.output {
        syn::ReturnType::Default => Some(quote!(())),
        syn::ReturnType::Type(_, ty) if !mentions_impl(ty.to_token_stream()) => {
            Some(ty.to_token_stream())
        }
        syn::ReturnType::Type(..) => None,
    };
    let sites_static = quote! {
        static __SPANSCOPE_SITES: [#runtime::__private::FieldSite; #count] = [#(#sites),*];
    };
    if signature.asyncness.is_some() {
        let annotation = output.map(|ty| quote!(: #ty));
        parse_quote!({
            #descriptor
            #sites_static
            #runtime::__private::trace_future(async move {
                let __spanscope_track = #runtime::__private::TrackCall::enter(
                    &DESCRIPTOR, &__SPANSCOPE_SITES, [#(#observe),*]
                );
                let __spanscope_track_ref = &__spanscope_track;
                let __spanscope_result #annotation = async #body.await;
                __spanscope_track.exit([#(#observe),*]);
                __spanscope_result
            }, &DESCRIPTOR, #root).await
        })
    } else {
        let annotation = output.map(|ty| quote!(-> #ty));
        parse_quote!({
            #descriptor
            #sites_static
            let _spanscope_guard = #runtime::__private::enter(&DESCRIPTOR, #root);
            let __spanscope_track = #runtime::__private::TrackCall::enter(
                &DESCRIPTOR, &__SPANSCOPE_SITES, [#(#observe),*]
            );
            let __spanscope_track_ref = &__spanscope_track;
            #[allow(clippy::redundant_closure_call)]
            let __spanscope_result = (|| #annotation #body)();
            __spanscope_track.exit([#(#observe),*]);
            __spanscope_result
        })
    }
}
