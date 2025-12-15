//! Serialize derive implementation.

use proc_macro2::TokenStream;
use quote::quote;
use syn::spanned::Spanned;
use syn::{Data, DeriveInput, Fields, Ident};

pub fn expand_derive_serialize(input: &DeriveInput) -> syn::Result<TokenStream> {
    let name = &input.ident;
    let generics = &input.generics;

    // Get the serde crate path from attributes or default to foundation_serde
    let serde_path = get_serde_path(&input.attrs);

    let serialize_impl = match &input.data {
        Data::Struct(data) => impl_serialize_struct(name, &data.fields, &serde_path)?,
        Data::Enum(data) => impl_serialize_enum(name, data, &serde_path)?,
        Data::Union(_) => {
            return Err(syn::Error::new(
                input.span(),
                "Serialize cannot be derived for unions",
            ))
        }
    };

    // Add Serialize trait bounds for all generic type parameters through where clause
    let mut generics_with_bounds = generics.clone();

    // Collect all type parameters
    let type_params: Vec<_> = generics.type_params().map(|param| &param.ident).collect();

    // Add Serialize bounds to where clause
    let where_clause = generics_with_bounds.make_where_clause();
    for type_param in type_params {
        where_clause
            .predicates
            .push(syn::parse_quote!(#type_param: #serde_path::Serialize));
    }

    let (impl_generics, ty_generics, where_clause) = generics_with_bounds.split_for_impl();

    Ok(quote! {
        #[automatically_derived]
        impl #impl_generics #serde_path::Serialize for #name #ty_generics #where_clause {
            fn serialize<S>(&self, serializer: S) -> ::core::result::Result<S::Ok, S::Error>
            where
                S: #serde_path::Serializer,
            {
                #serialize_impl
            }
        }
    })
}

fn impl_serialize_struct(
    name: &Ident,
    fields: &Fields,
    serde_path: &TokenStream,
) -> syn::Result<TokenStream> {
    match fields {
        Fields::Named(fields) => {
            let field_count = fields.named.len();
            let field_serializations = fields.named.iter().map(|f| {
                let field_name = &f.ident;
                let field_name_str = get_field_name(f);
                quote! {
                    state.serialize_field(#field_name_str, &self.#field_name)?;
                }
            });

            Ok(quote! {
                use #serde_path::ser::SerializeStruct;
                let mut state = serializer.serialize_struct(stringify!(#name), #field_count)?;
                #(#field_serializations)*
                state.end()
            })
        }
        Fields::Unnamed(fields) => {
            let field_count = fields.unnamed.len();
            let field_serializations = (0..field_count).map(|i| {
                let index = syn::Index::from(i);
                quote! {
                    state.serialize_field(&self.#index)?;
                }
            });

            Ok(quote! {
                use #serde_path::ser::SerializeTupleStruct;
                let mut state = serializer.serialize_tuple_struct(stringify!(#name), #field_count)?;
                #(#field_serializations)*
                state.end()
            })
        }
        Fields::Unit => Ok(quote! {
            serializer.serialize_unit_struct(stringify!(#name))
        }),
    }
}

fn impl_serialize_enum(
    name: &Ident,
    data: &syn::DataEnum,
    serde_path: &TokenStream,
) -> syn::Result<TokenStream> {
    let variant_arms = data
        .variants
        .iter()
        .enumerate()
        .map(|(variant_index, variant)| {
            let variant_name = &variant.ident;
            let variant_name_str = get_variant_name(variant);
            let variant_index = variant_index as u32;

            match &variant.fields {
                Fields::Unit => {
                    quote! {
                        #name::#variant_name => {
                            serializer.serialize_unit_variant(
                                stringify!(#name),
                                #variant_index,
                                #variant_name_str,
                            )
                        }
                    }
                }
                Fields::Unnamed(fields) => {
                    let field_count = fields.unnamed.len();
                    let field_names: Vec<Ident> = (0..field_count)
                        .map(|i| quote::format_ident!("__field{}", i))
                        .collect();

                    let serialize_fields = field_names.iter().map(|field_name| {
                        quote! {
                            state.serialize_field(#field_name)?;
                        }
                    });

                    quote! {
                        #name::#variant_name(#(#field_names),*) => {
                            use #serde_path::ser::SerializeTupleVariant;
                            let mut state = serializer.serialize_tuple_variant(
                                stringify!(#name),
                                #variant_index,
                                #variant_name_str,
                                #field_count,
                            )?;
                            #(#serialize_fields)*
                            state.end()
                        }
                    }
                }
                Fields::Named(fields) => {
                    let field_count = fields.named.len();
                    let field_names: Vec<&Ident> = fields
                        .named
                        .iter()
                        .map(|f| f.ident.as_ref().unwrap())
                        .collect();

                    let serialize_fields = fields.named.iter().map(|f| {
                        let field_name = &f.ident;
                        let field_name_str = get_field_name(f);
                        quote! {
                            state.serialize_field(#field_name_str, #field_name)?;
                        }
                    });

                    quote! {
                        #name::#variant_name { #(#field_names),* } => {
                            use #serde_path::ser::SerializeStructVariant;
                            let mut state = serializer.serialize_struct_variant(
                                stringify!(#name),
                                #variant_index,
                                #variant_name_str,
                                #field_count,
                            )?;
                            #(#serialize_fields)*
                            state.end()
                        }
                    }
                }
            }
        });

    Ok(quote! {
        match self {
            #(#variant_arms)*
        }
    })
}

fn get_serde_path(attrs: &[syn::Attribute]) -> TokenStream {
    for attr in attrs {
        if attr.path().is_ident("serde") {
            if let Ok(meta_list) = attr.meta.require_list() {
                // Parse as comma-separated nested meta items
                let parsed = meta_list.parse_args_with(
                    syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
                );

                if let Ok(nested) = parsed {
                    for meta in nested {
                        if let syn::Meta::NameValue(nv) = meta {
                            if nv.path.is_ident("crate") {
                                if let syn::Expr::Lit(lit) = &nv.value {
                                    if let syn::Lit::Str(lit_str) = &lit.lit {
                                        let path_str = lit_str.value();
                                        if let Ok(path) = syn::parse_str::<syn::Path>(&path_str) {
                                            return quote! { #path };
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    // Default to foundation_serialization::serde (the public facade)
    quote! { foundation_serialization::serde }
}

fn get_field_name(field: &syn::Field) -> String {
    // Check for #[serde(rename = "...")] attribute
    for attr in &field.attrs {
        if attr.path().is_ident("serde") {
            if let Ok(meta_list) = attr.meta.require_list() {
                let tokens = &meta_list.tokens;
                let tokens_str = tokens.to_string();
                if tokens_str.starts_with("rename = ") {
                    let name = tokens_str
                        .trim_start_matches("rename = ")
                        .trim_matches('"')
                        .trim();
                    return name.to_string();
                }
            }
        }
    }
    // Default to field name
    field.ident.as_ref().unwrap().to_string()
}

fn get_variant_name(variant: &syn::Variant) -> String {
    // Check for #[serde(rename = "...")] attribute
    for attr in &variant.attrs {
        if attr.path().is_ident("serde") {
            if let Ok(meta_list) = attr.meta.require_list() {
                let tokens = &meta_list.tokens;
                let tokens_str = tokens.to_string();
                if tokens_str.starts_with("rename = ") {
                    let name = tokens_str
                        .trim_start_matches("rename = ")
                        .trim_matches('"')
                        .trim();
                    return name.to_string();
                }
            }
        }
    }
    // Default to variant name
    variant.ident.to_string()
}
