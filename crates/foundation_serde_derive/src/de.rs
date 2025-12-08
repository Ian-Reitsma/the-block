//! Deserialize derive implementation.

use proc_macro2::TokenStream;
use quote::{quote, format_ident};
use syn::spanned::Spanned;
use syn::{Data, DeriveInput, Fields, Ident};

pub fn expand_derive_deserialize(input: &DeriveInput) -> syn::Result<TokenStream> {
    let name = &input.ident;
    let generics = &input.generics;

    // Get the serde crate path from attributes or default to foundation_serde
    let serde_path = get_serde_path(&input.attrs);

    // Get the original type generics (without 'de lifetime) BEFORE calling impl functions
    let (_, ty_generics, _) = generics.split_for_impl();

    // Collect type parameters for visitor
    let type_params: Vec<_> = generics.type_params().map(|param| &param.ident).collect();

    let deserialize_impl = match &input.data {
        Data::Struct(data) => impl_deserialize_struct(name, &ty_generics, &type_params, &data.fields, &serde_path)?,
        Data::Enum(data) => impl_deserialize_enum(name, &ty_generics, &type_params, data, &serde_path)?,
        Data::Union(_) => {
            return Err(syn::Error::new(
                input.span(),
                "Deserialize cannot be derived for unions",
            ))
        }
    };

    // Add 'de lifetime and Deserialize trait bounds to generics for impl block
    let mut impl_generics = generics.clone();
    impl_generics.params.insert(0, syn::parse_quote!('de));

    // Collect all type parameters
    let type_params: Vec<_> = generics.type_params().map(|param| &param.ident).collect();

    // Add Deserialize bounds to where clause
    let where_clause = impl_generics.make_where_clause();
    for type_param in type_params {
        where_clause.predicates.push(syn::parse_quote!(#type_param: #serde_path::Deserialize<'de>));
    }

    let (impl_generics, _, where_clause) = impl_generics.split_for_impl();

    Ok(quote! {
        #[automatically_derived]
        impl #impl_generics #serde_path::Deserialize<'de> for #name #ty_generics #where_clause {
            fn deserialize<D>(deserializer: D) -> ::core::result::Result<Self, D::Error>
            where
                D: #serde_path::Deserializer<'de>,
            {
                #deserialize_impl
            }
        }
    })
}

fn impl_deserialize_struct(
    name: &Ident,
    ty_generics: &syn::TypeGenerics,
    type_params: &[&Ident],
    fields: &Fields,
    serde_path: &TokenStream,
) -> syn::Result<TokenStream> {
    match fields {
        Fields::Named(fields) => {
            let field_names: Vec<_> = fields.named.iter()
                .map(|f| f.ident.as_ref().unwrap())
                .collect();
            let field_name_strs: Vec<_> = fields.named.iter()
                .map(|f| get_field_name(f))
                .collect();
            let field_count = fields.named.len();

            let visitor_name = format_ident!("__Visitor");

            let field_enum_variants: Vec<_> = (0..field_count).map(|i| {
                format_ident!("Field{}", i)
            }).collect();

            let field_matches = field_name_strs.iter().zip(&field_enum_variants).map(|(field_str, variant)| {
                quote! { #field_str => Ok(__Field::#variant) }
            });

            // Create zipped iteration for match arms
            let field_match_arms: Vec<_> = field_enum_variants.iter()
                .zip(&field_names)
                .zip(&field_name_strs)
                .map(|((variant, field_name), field_name_str)| {
                    quote! {
                        __Field::#variant => {
                            if #field_name.is_some() {
                                return Err(#serde_path::de::Error::duplicate_field(#field_name_str));
                            }
                            #field_name = Some(map.next_value()?);
                        }
                    }
                })
                .collect();

            Ok(quote! {
                #[allow(non_camel_case_types)]
                enum __Field {
                    #(#field_enum_variants,)*
                    __Ignore,
                }

                impl<'de> #serde_path::Deserialize<'de> for __Field {
                    fn deserialize<D>(deserializer: D) -> ::core::result::Result<__Field, D::Error>
                    where
                        D: #serde_path::Deserializer<'de>,
                    {
                        struct __FieldVisitor;

                        impl<'de> #serde_path::de::Visitor<'de> for __FieldVisitor {
                            type Value = __Field;

                            fn expecting(&self, formatter: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
                                formatter.write_str("field identifier")
                            }

                            fn visit_str<E>(self, value: &str) -> ::core::result::Result<__Field, E>
                            where
                                E: #serde_path::de::Error,
                            {
                                match value {
                                    #(#field_matches,)*
                                    _ => Ok(__Field::__Ignore),
                                }
                            }
                        }

                        deserializer.deserialize_identifier(__FieldVisitor)
                    }
                }

                struct #visitor_name<#(#type_params),*>(::core::marker::PhantomData<(#(#type_params,)*)>);

                impl<'de, #(#type_params),*> #serde_path::de::Visitor<'de> for #visitor_name<#(#type_params),*>
                where
                    #(#type_params: #serde_path::Deserialize<'de>),*
                {
                    type Value = #name #ty_generics;

                    fn expecting(&self, formatter: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
                        formatter.write_str(concat!("struct ", stringify!(#name)))
                    }

                    fn visit_map<A>(self, mut map: A) -> ::core::result::Result<Self::Value, A::Error>
                    where
                        A: #serde_path::de::MapAccess<'de>,
                    {
                        #(let mut #field_names = None;)*

                        while let Some(__field) = map.next_key::<__Field>()? {
                            match __field {
                                #(#field_match_arms)*
                                __Field::__Ignore => {
                                    let _ = map.next_value::<#serde_path::de::IgnoredAny>()?;
                                }
                            }
                        }

                        #(
                            let #field_names = #field_names.ok_or_else(|| #serde_path::de::Error::missing_field(#field_name_strs))?;
                        )*

                        Ok(#name {
                            #(#field_names,)*
                        })
                    }
                }

                const FIELDS: &[&str] = &[#(#field_name_strs),*];
                deserializer.deserialize_struct(stringify!(#name), FIELDS, #visitor_name(::core::marker::PhantomData))
            })
        }
        Fields::Unnamed(fields) => {
            let field_count = fields.unnamed.len();
            let field_vars: Vec<_> = (0..field_count)
                .map(|i| format_ident!("__field{}", i))
                .collect();
            let field_indices: Vec<_> = (0..field_count).collect();
            let visitor_name = format_ident!("__Visitor");

            Ok(quote! {
                struct #visitor_name<#(#type_params),*>(::core::marker::PhantomData<(#(#type_params,)*)>);

                impl<'de, #(#type_params),*> #serde_path::de::Visitor<'de> for #visitor_name<#(#type_params),*>
                where
                    #(#type_params: #serde_path::Deserialize<'de>),*
                {
                    type Value = #name #ty_generics;

                    fn expecting(&self, formatter: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
                        formatter.write_str(concat!("tuple struct ", stringify!(#name)))
                    }

                    fn visit_seq<A>(self, mut seq: A) -> ::core::result::Result<Self::Value, A::Error>
                    where
                        A: #serde_path::de::SeqAccess<'de>,
                    {
                        #(
                            let #field_vars = seq.next_element()?
                                .ok_or_else(|| #serde_path::de::Error::invalid_length(#field_indices, &self))?;
                        )*

                        Ok(#name(#(#field_vars),*))
                    }
                }

                deserializer.deserialize_tuple_struct(stringify!(#name), #field_count, #visitor_name(::core::marker::PhantomData))
            })
        }
        Fields::Unit => {
            let visitor_name = format_ident!("__Visitor");

            Ok(quote! {
                struct #visitor_name<#(#type_params),*>(::core::marker::PhantomData<(#(#type_params,)*)>);

                impl<'de, #(#type_params),*> #serde_path::de::Visitor<'de> for #visitor_name<#(#type_params),*>
                where
                    #(#type_params: #serde_path::Deserialize<'de>),*
                {
                    type Value = #name #ty_generics;

                    fn expecting(&self, formatter: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
                        formatter.write_str(concat!("unit struct ", stringify!(#name)))
                    }

                    fn visit_unit<E>(self) -> ::core::result::Result<Self::Value, E>
                    where
                        E: #serde_path::de::Error,
                    {
                        Ok(#name)
                    }
                }

                deserializer.deserialize_unit_struct(stringify!(#name), #visitor_name(::core::marker::PhantomData))
            })
        }
    }
}

fn impl_deserialize_enum(
    name: &Ident,
    ty_generics: &syn::TypeGenerics,
    type_params: &[&Ident],
    data: &syn::DataEnum,
    serde_path: &TokenStream,
) -> syn::Result<TokenStream> {
    let variant_name_strs: Vec<_> = data.variants.iter()
        .map(|v| get_variant_name(v))
        .collect();

    let variant_arms = data.variants.iter().enumerate().map(|(i, variant)| {
        let variant_name = &variant.ident;
        let variant_index = format_ident!("Variant{}", i);

        match &variant.fields {
            Fields::Unit => {
                quote! {
                    __Variant::#variant_index => {
                        variant.unit_variant()?;
                        Ok(#name::#variant_name)
                    }
                }
            }
            Fields::Unnamed(fields) => {
                let field_count = fields.unnamed.len();
                let field_vars: Vec<_> = (0..field_count)
                    .map(|i| format_ident!("__field{}", i))
                    .collect();

                quote! {
                    __Variant::#variant_index => {
                        struct __Visitor<'de, #(#type_params),*>(::core::marker::PhantomData<(&'de (), #(#type_params,)*)>);

                        impl<'de, #(#type_params),*> #serde_path::de::Visitor<'de> for __Visitor<'de, #(#type_params),*>
                        where
                            #(#type_params: #serde_path::Deserialize<'de>),*
                        {
                            type Value = #name #ty_generics;

                            fn expecting(&self, formatter: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
                                formatter.write_str("tuple variant")
                            }

                            fn visit_seq<A>(self, mut seq: A) -> ::core::result::Result<Self::Value, A::Error>
                            where
                                A: #serde_path::de::SeqAccess<'de>,
                            {
                                #(
                                    let #field_vars = seq.next_element()?
                                        .ok_or_else(|| #serde_path::de::Error::invalid_length(0, &self))?;
                                )*
                                Ok(#name::#variant_name(#(#field_vars),*))
                            }
                        }

                        variant.tuple_variant(#field_count, __Visitor(::core::marker::PhantomData))
                    }
                }
            }
            Fields::Named(fields) => {
                let field_names: Vec<_> = fields.named.iter()
                    .map(|f| f.ident.as_ref().unwrap())
                    .collect();
                let field_name_strs: Vec<_> = fields.named.iter()
                    .map(|f| get_field_name(f))
                    .collect();

                quote! {
                    __Variant::#variant_index => {
                        struct __Visitor<'de, #(#type_params),*>(::core::marker::PhantomData<(&'de (), #(#type_params,)*)>);

                        impl<'de, #(#type_params),*> #serde_path::de::Visitor<'de> for __Visitor<'de, #(#type_params),*>
                        where
                            #(#type_params: #serde_path::Deserialize<'de>),*
                        {
                            type Value = #name #ty_generics;

                            fn expecting(&self, formatter: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
                                formatter.write_str("struct variant")
                            }

                            fn visit_map<A>(self, mut map: A) -> ::core::result::Result<Self::Value, A::Error>
                            where
                                A: #serde_path::de::MapAccess<'de>,
                            {
                                #(let mut #field_names = None;)*

                                while let Some(key) = map.next_key::<String>()? {
                                    match key.as_str() {
                                        #(
                                            #field_name_strs => {
                                                if #field_names.is_some() {
                                                    return Err(#serde_path::de::Error::duplicate_field(#field_name_strs));
                                                }
                                                #field_names = Some(map.next_value()?);
                                            }
                                        )*
                                        _ => { let _ = map.next_value::<#serde_path::de::IgnoredAny>()?; }
                                    }
                                }

                                #(
                                    let #field_names = #field_names.ok_or_else(|| #serde_path::de::Error::missing_field(#field_name_strs))?;
                                )*

                                Ok(#name::#variant_name { #(#field_names),* })
                            }
                        }

                        const FIELDS: &[&str] = &[#(#field_name_strs),*];
                        variant.struct_variant(FIELDS, __Visitor(::core::marker::PhantomData))
                    }
                }
            }
        }
    });

    let variant_indices = (0..data.variants.len()).map(|i| format_ident!("Variant{}", i));
    let variant_matches = variant_name_strs.iter().enumerate().map(|(i, name)| {
        let idx = format_ident!("Variant{}", i);
        quote! { #name => Ok(__Variant::#idx) }
    });

    Ok(quote! {
        #[allow(non_camel_case_types)]
        enum __Variant {
            #(#variant_indices,)*
        }

        impl<'de> #serde_path::Deserialize<'de> for __Variant {
            fn deserialize<D>(deserializer: D) -> ::core::result::Result<__Variant, D::Error>
            where
                D: #serde_path::Deserializer<'de>,
            {
                struct __VariantVisitor;

                impl<'de> #serde_path::de::Visitor<'de> for __VariantVisitor {
                    type Value = __Variant;

                    fn expecting(&self, formatter: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
                        formatter.write_str("variant identifier")
                    }

                    fn visit_str<E>(self, value: &str) -> ::core::result::Result<__Variant, E>
                    where
                        E: #serde_path::de::Error,
                    {
                        match value {
                            #(#variant_matches,)*
                            _ => Err(#serde_path::de::Error::unknown_variant(value, VARIANTS)),
                        }
                    }
                }

                deserializer.deserialize_identifier(__VariantVisitor)
            }
        }

        struct __Visitor<#(#type_params),*>(::core::marker::PhantomData<(#(#type_params,)*)>);

        impl<'de, #(#type_params),*> #serde_path::de::Visitor<'de> for __Visitor<#(#type_params),*>
        where
            #(#type_params: #serde_path::Deserialize<'de>),*
        {
            type Value = #name #ty_generics;

            fn expecting(&self, formatter: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
                formatter.write_str(concat!("enum ", stringify!(#name)))
            }

            fn visit_enum<A>(self, data: A) -> ::core::result::Result<Self::Value, A::Error>
            where
                A: #serde_path::de::EnumAccess<'de>,
            {
                let (variant_tag, variant) = data.variant()?;

                #[allow(unused_imports)]
                use #serde_path::de::VariantAccess;

                match variant_tag {
                    #(#variant_arms)*
                }
            }
        }

        const VARIANTS: &[&str] = &[#(#variant_name_strs),*];
        deserializer.deserialize_enum(stringify!(#name), VARIANTS, __Visitor(::core::marker::PhantomData))
    })
}

fn get_serde_path(attrs: &[syn::Attribute]) -> TokenStream {
    for attr in attrs {
        if attr.path().is_ident("serde") {
            if let Ok(meta_list) = attr.meta.require_list() {
                // Parse as comma-separated nested meta items
                let parsed = meta_list.parse_args_with(
                    syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated
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
    for attr in &field.attrs {
        if attr.path().is_ident("serde") {
            if let Ok(meta_list) = attr.meta.require_list() {
                let tokens = &meta_list.tokens;
                let tokens_str = tokens.to_string();
                if tokens_str.starts_with("rename = ") {
                    let name = tokens_str.trim_start_matches("rename = ")
                        .trim_matches('"')
                        .trim();
                    return name.to_string();
                }
            }
        }
    }
    field.ident.as_ref().unwrap().to_string()
}

fn get_variant_name(variant: &syn::Variant) -> String {
    for attr in &variant.attrs {
        if attr.path().is_ident("serde") {
            if let Ok(meta_list) = attr.meta.require_list() {
                let tokens = &meta_list.tokens;
                let tokens_str = tokens.to_string();
                if tokens_str.starts_with("rename = ") {
                    let name = tokens_str.trim_start_matches("rename = ")
                        .trim_matches('"')
                        .trim();
                    return name.to_string();
                }
            }
        }
    }
    variant.ident.to_string()
}
