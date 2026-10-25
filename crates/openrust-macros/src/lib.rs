#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]
#![deny(missing_docs)]

use proc_macro::TokenStream;

use quote::quote;
use syn::{ImplItem, ItemImpl, parse_macro_input};

/// Generates an HTTP [`Resource`](https://docs.rs/openrust-http/latest/openrust_http/trait.Resource.html)
/// implementation from action methods in an inherent controller implementation.
///
/// Recognized methods are `list`, `get`, `create`, `update`, and `delete`. They must be
/// associated functions without a receiver or method generics. Other methods are preserved.
#[proc_macro_attribute]
pub fn resource(args: TokenStream, input: TokenStream) -> TokenStream {
    let implementation = parse_macro_input!(input as ItemImpl);
    if !args.is_empty() {
        return syn::Error::new_spanned(&implementation, "`resource` does not accept arguments")
            .to_compile_error()
            .into();
    }
    if implementation.trait_.is_some() {
        return syn::Error::new_spanned(
            &implementation,
            "`resource` must be applied to an inherent `impl` block",
        )
        .to_compile_error()
        .into();
    }

    let mut action_methods = Vec::new();
    for item in &implementation.items {
        let ImplItem::Fn(method) = item else {
            continue;
        };
        let name = method.sig.ident.to_string();
        if !matches!(
            name.as_str(),
            "list" | "get" | "create" | "update" | "delete"
        ) {
            continue;
        }
        if method.sig.receiver().is_some() {
            return syn::Error::new_spanned(
                &method.sig,
                format!("resource action `{name}` must not take a receiver"),
            )
            .to_compile_error()
            .into();
        }
        if !method.sig.generics.params.is_empty() {
            return syn::Error::new_spanned(
                &method.sig.generics,
                format!("resource action `{name}` must not be generic"),
            )
            .to_compile_error()
            .into();
        }
        action_methods.push((method.sig.ident.clone(), name));
    }

    let action_calls = action_methods.iter().map(|(handler, action)| {
        let action = syn::Ident::new(action, handler.span());
        quote! { .#action(Self::#handler) }
    });
    let (impl_generics, _, where_clause) = implementation.generics.split_for_impl();
    let self_type = &implementation.self_ty;
    quote! {
        #implementation

        #[allow(clippy::unused_async_trait_impl)]
        impl #impl_generics ::openrust::http::Resource for #self_type #where_clause {
            fn actions(
                actions: ::openrust::http::ResourceActions,
            ) -> ::openrust::http::ResourceActions {
                actions #(#action_calls)*
            }
        }
    }
    .into()
}
