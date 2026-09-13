//! `#[derive(Component)]` for argus-ecs.
//!
//! ```ignore
//! #[derive(Component, Clone, Copy, Default)]
//! #[repr(C)]
//! #[argus(doc = "Freezes an entity's simulation for N frames after a hit.")]
//! struct HitStop {
//!     #[argus(doc = "Remaining frozen frames", range(0, 30))]
//!     frames: u8,
//!     #[argus(doc = "Time scale while frozen. 0 = full stop", range(0, 1), tune)]
//!     scale: f32,
//! }
//! ```
//!
//! Struct attributes: `name = "..."` (defaults to the type name), `doc = "..."`,
//! `version = N`. Field attributes: `doc = "..."`, `range(lo, hi)`, `tune`.
//! The struct must be `#[repr(C)]` (unless it is a unit struct, which becomes
//! a tag) and implement `Default`, which supplies the field defaults.

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, Data, DeriveInput, Expr, Fields, Lit, Meta, Token};

#[proc_macro_derive(Component, attributes(argus))]
pub fn derive_component(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match expand(input) {
        Ok(ts) => ts.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

struct StructAttrs {
    name: Option<String>,
    doc: String,
    version: u32,
}

struct FieldAttrs {
    doc: String,
    range: Option<(f64, f64)>,
    tune: bool,
}

fn lit_str(e: &Expr) -> syn::Result<String> {
    if let Expr::Lit(l) = e {
        if let Lit::Str(s) = &l.lit {
            return Ok(s.value());
        }
    }
    Err(syn::Error::new_spanned(e, "expected a string literal"))
}

fn lit_num(e: &Expr) -> syn::Result<f64> {
    let (neg, inner) = match e {
        Expr::Unary(u) if matches!(u.op, syn::UnOp::Neg(_)) => (true, &*u.expr),
        other => (false, other),
    };
    if let Expr::Lit(l) = inner {
        let v = match &l.lit {
            Lit::Int(i) => i.base10_parse::<f64>()?,
            Lit::Float(f) => f.base10_parse::<f64>()?,
            _ => return Err(syn::Error::new_spanned(e, "expected a number")),
        };
        return Ok(if neg { -v } else { v });
    }
    Err(syn::Error::new_spanned(e, "expected a number"))
}

fn parse_struct_attrs(attrs: &[syn::Attribute]) -> syn::Result<StructAttrs> {
    let mut out = StructAttrs {
        name: None,
        doc: String::new(),
        version: 1,
    };
    for attr in attrs.iter().filter(|a| a.path().is_ident("argus")) {
        let metas =
            attr.parse_args_with(syn::punctuated::Punctuated::<Meta, Token![,]>::parse_terminated)?;
        for m in metas {
            match &m {
                Meta::NameValue(nv) if nv.path.is_ident("name") => out.name = Some(lit_str(&nv.value)?),
                Meta::NameValue(nv) if nv.path.is_ident("doc") => out.doc = lit_str(&nv.value)?,
                Meta::NameValue(nv) if nv.path.is_ident("version") => out.version = lit_num(&nv.value)? as u32,
                other => {
                    return Err(syn::Error::new_spanned(
                        other,
                        "unknown #[argus] struct attribute; expected name = \"..\", doc = \"..\", version = N",
                    ))
                }
            }
        }
    }
    Ok(out)
}

fn parse_field_attrs(attrs: &[syn::Attribute]) -> syn::Result<FieldAttrs> {
    let mut out = FieldAttrs {
        doc: String::new(),
        range: None,
        tune: false,
    };
    for attr in attrs.iter().filter(|a| a.path().is_ident("argus")) {
        let metas =
            attr.parse_args_with(syn::punctuated::Punctuated::<Meta, Token![,]>::parse_terminated)?;
        for m in metas {
            match &m {
                Meta::NameValue(nv) if nv.path.is_ident("doc") => out.doc = lit_str(&nv.value)?,
                Meta::Path(p) if p.is_ident("tune") => out.tune = true,
                Meta::List(l) if l.path.is_ident("range") => {
                    let args = l.parse_args_with(
                        syn::punctuated::Punctuated::<Expr, Token![,]>::parse_terminated,
                    )?;
                    if args.len() != 2 {
                        return Err(syn::Error::new_spanned(
                            l,
                            "range takes two numbers: range(lo, hi)",
                        ));
                    }
                    out.range = Some((lit_num(&args[0])?, lit_num(&args[1])?));
                }
                other => return Err(syn::Error::new_spanned(
                    other,
                    "unknown #[argus] field attribute; expected doc = \"..\", range(lo, hi), tune",
                )),
            }
        }
    }
    Ok(out)
}

fn has_repr_c(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().filter(|a| a.path().is_ident("repr")).any(|a| {
        let mut found = false;
        let _ = a.parse_nested_meta(|meta| {
            if meta.path.is_ident("C") {
                found = true;
            }
            Ok(())
        });
        found
    })
}

fn expand(input: DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let ident = &input.ident;
    if !input.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &input.generics,
            "generic components are not supported",
        ));
    }
    let sattrs = parse_struct_attrs(&input.attrs)?;
    let name = sattrs.name.unwrap_or_else(|| ident.to_string());
    let doc = sattrs.doc;
    let version = sattrs.version;

    let fields = match &input.data {
        Data::Struct(s) => match &s.fields {
            Fields::Named(n) => n.named.iter().collect::<Vec<_>>(),
            Fields::Unit => Vec::new(),
            Fields::Unnamed(u) => {
                return Err(syn::Error::new_spanned(
                    u,
                    "tuple structs are not supported; use named fields",
                ))
            }
        },
        _ => {
            return Err(syn::Error::new_spanned(
                ident,
                "Component can only be derived for structs",
            ))
        }
    };

    if !fields.is_empty() && !has_repr_c(&input.attrs) {
        return Err(syn::Error::new_spanned(
            ident,
            "components with fields must be #[repr(C)] so the column layout matches the schema",
        ));
    }

    let mut field_tokens = Vec::new();
    for f in &fields {
        let fident = f.ident.as_ref().unwrap();
        let fname = fident.to_string();
        let fty = &f.ty;
        let fa = parse_field_attrs(&f.attrs)?;
        let fdoc = fa.doc;
        let tune = fa.tune;
        let range = match fa.range {
            Some((lo, hi)) => quote! { ::core::option::Option::Some((#lo, #hi)) },
            None => quote! { ::core::option::Option::None },
        };
        field_tokens.push(quote! {
            ::argus_ecs::__private::FieldSchema {
                name: ::std::string::String::from(#fname),
                ty: <#fty as ::argus_ecs::__private::FieldValue>::TYPE,
                offset: ::argus_ecs::__private::offset_of!(#ident, #fident),
                default: ::argus_ecs::__private::FieldValue::to_value(__default.#fident),
                range: #range,
                doc: ::std::string::String::from(#fdoc),
                tune: #tune,
            }
        });
    }

    let size = if fields.is_empty() {
        quote! { 0usize }
    } else {
        quote! { ::argus_ecs::__private::size_of::<#ident>() }
    };
    let align = if fields.is_empty() {
        quote! { 1usize }
    } else {
        quote! { ::argus_ecs::__private::align_of::<#ident>() }
    };

    Ok(quote! {
        impl ::argus_ecs::Component for #ident {
            const NAME: &'static str = #name;
            fn schema() -> ::argus_ecs::__private::ComponentSchema {
                #[allow(unused_variables)]
                let __default = <#ident as ::core::default::Default>::default();
                ::argus_ecs::__private::ComponentSchema {
                    name: ::std::string::String::from(#name),
                    doc: ::std::string::String::from(#doc),
                    version: #version,
                    fields: ::std::vec![ #(#field_tokens),* ],
                    size: #size,
                    align: #align,
                    source: ::core::option::Option::Some(
                        ::std::format!("{}:{}", ::core::file!(), ::core::line!())
                    ),
                    rust_type: ::core::option::Option::None,
                }
            }
        }
    })
}
