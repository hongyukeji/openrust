#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]
#![deny(missing_docs)]

use proc_macro::TokenStream;

use quote::quote;
use syn::{ImplItem, ItemImpl, parse_macro_input};

/// 根据 controller 固有实现中的 action 方法生成 HTTP [`Resource`](https://docs.rs/openrust/latest/openrust/http/resource/trait.Resource.html) 实现。
///
/// 支持的方法为 `list`、`get`、`create`、`update` 和 `delete`。这些方法必须是无 receiver、
/// 无方法级泛型的关联函数。其他方法会保留。
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
        impl #impl_generics ::openrust::routing::Resource for #self_type #where_clause {
            fn actions(
                actions: ::openrust::routing::ResourceActions,
            ) -> ::openrust::routing::ResourceActions {
                actions #(#action_calls)*
            }
        }
    }
    .into()
}
