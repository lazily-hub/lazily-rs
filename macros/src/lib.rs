use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{
    FnArg, ItemFn, Pat, ReturnType, Token, parse_macro_input, punctuated::Punctuated,
    spanned::Spanned,
};

#[proc_macro_attribute]
pub fn slot(args: TokenStream, input: TokenStream) -> TokenStream {
    expand_factory("slot", args, input)
}

#[proc_macro_attribute]
pub fn cell(args: TokenStream, input: TokenStream) -> TokenStream {
    expand_factory("cell", args, input)
}

fn expand_factory(kind: &str, args: TokenStream, input: TokenStream) -> TokenStream {
    let args = parse_macro_input!(args with Punctuated::<syn::Meta, Token![,]>::parse_terminated);
    if !args.is_empty() {
        return syn::Error::new(
            args.span(),
            "#[lazily::slot] and #[lazily::cell] do not take arguments",
        )
        .to_compile_error()
        .into();
    }

    let item = parse_macro_input!(input as ItemFn);
    match expand_factory_item(kind, item) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

fn expand_factory_item(kind: &str, item: ItemFn) -> syn::Result<proc_macro2::TokenStream> {
    let attrs = item.attrs;
    let vis = item.vis;
    let sig = item.sig;
    let body = item.block;
    let name = sig.ident;

    if sig.asyncness.is_some() {
        return Err(syn::Error::new(
            sig.asyncness.span(),
            "lazily factory decorators do not support async functions",
        ));
    }
    if sig.constness.is_some() {
        return Err(syn::Error::new(
            sig.constness.span(),
            "lazily factory decorators do not support const functions",
        ));
    }
    if sig.unsafety.is_some() {
        return Err(syn::Error::new(
            sig.unsafety.span(),
            "lazily factory decorators do not support unsafe functions",
        ));
    }
    if sig.abi.is_some() {
        return Err(syn::Error::new(
            sig.abi.span(),
            "lazily factory decorators do not support extern functions",
        ));
    }
    if !sig.generics.params.is_empty() || sig.generics.where_clause.is_some() {
        return Err(syn::Error::new(
            sig.generics.span(),
            "lazily factory decorators currently support non-generic functions",
        ));
    }

    if sig.inputs.len() != 1 {
        return Err(syn::Error::new(
            sig.inputs.span(),
            "lazily factory decorators require exactly one context argument",
        ));
    }

    let ctx_ident = match sig.inputs.first().expect("checked len") {
        FnArg::Typed(arg) => match arg.pat.as_ref() {
            Pat::Ident(ident) => ident.ident.clone(),
            other => {
                return Err(syn::Error::new(
                    other.span(),
                    "lazily factory context argument must be a plain identifier",
                ));
            }
        },
        FnArg::Receiver(receiver) => {
            return Err(syn::Error::new(
                receiver.span(),
                "lazily factory decorators do not support methods",
            ));
        }
    };

    let value_ty = match sig.output {
        ReturnType::Type(_, ty) => ty,
        ReturnType::Default => {
            return Err(syn::Error::new(
                name.span(),
                "lazily factory decorators require an explicit return type",
            ));
        }
    };

    let key_ident = format_ident!("__lazily_{kind}_factory_for_{name}");
    let schema_ty = quote! { <C as ::lazily::TypedFactoryContext>::Schema };

    let memoized_call = match kind {
        "slot" => quote! {
            ::lazily::TypedFactoryContext::memoized_slot::<#key_ident, #value_ty, _>(
                #ctx_ident,
                move |#ctx_ident| #body,
            )
        },
        "cell" => quote! {
            ::lazily::TypedFactoryContext::memoized_cell::<#key_ident, #value_ty, _>(
                #ctx_ident,
                move |#ctx_ident| #body,
            )
        },
        _ => unreachable!("unknown lazily factory kind"),
    };

    let handle_ty = match kind {
        "slot" => quote! { ::lazily::TypedSlotHandle<#schema_ty, #value_ty> },
        "cell" => quote! { ::lazily::TypedCellHandle<#schema_ty, #value_ty> },
        _ => unreachable!("unknown lazily factory kind"),
    };

    Ok(quote! {
        #(#attrs)*
        #vis fn #name<C>(#ctx_ident: &C) -> #handle_ty
        where
            C: ::lazily::TypedFactoryContext + ?Sized,
        {
            #[allow(non_camel_case_types)]
            struct #key_ident;
            #memoized_call
        }
    })
}
