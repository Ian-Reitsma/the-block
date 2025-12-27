use crate::ast::{Data, DeriveInput, Field, Fields, Variant};
use crate::attr::FieldDefault;
use crate::error::Error;
use crate::generics::{Generics, ParamKind};
use std::fmt::Write;

pub fn expand(input: &DeriveInput) -> Result<String, Error> {
    let serde_path = input
        .container_attr
        .crate_path
        .clone()
        .unwrap_or_else(|| "foundation_serialization::serde".to_string());
    let (impl_generics, ty_generics, where_clause) =
        split_generics_for_deserialize(&input.generics, &serde_path);
    let type_params: Vec<String> = input
        .generics
        .type_params()
        .map(|param| param.name.clone())
        .collect();
    let body = match &input.data {
        Data::Struct(struct_data) => deserialize_struct(
            &input.name,
            &ty_generics,
            struct_data,
            &serde_path,
            &type_params,
        ),
        Data::Enum(variants) => deserialize_enum(
            &input.name,
            &ty_generics,
            variants,
            &serde_path,
            &type_params,
        ),
    };
    let impl_block = format!(
        "#[automatically_derived]\nimpl{impl_generics} {serde_path}::Deserialize<'de> for {name}{ty_generics} {where_clause} {{\n    fn deserialize<D>(deserializer: D) -> ::core::result::Result<Self, D::Error>\n    where\n        D: {serde_path}::Deserializer<'de>,\n    {{\n        {body}\n    }}\n}}",
        impl_generics = impl_generics,
        serde_path = serde_path,
        name = input.name,
        ty_generics = ty_generics,
        where_clause = where_clause,
        body = body,
    );
    Ok(impl_block)
}

fn split_generics_for_deserialize(
    generics: &Generics,
    serde_path: &str,
) -> (String, String, String) {
    let (impl_generics, ty_generics, mut where_clause) = generics.split_for_impl();
    let mut impl_generics = impl_generics;
    let has_de_lifetime = generics
        .params
        .iter()
        .any(|param| matches!(param.kind, ParamKind::Lifetime) && param.name == "'de");
    if !has_de_lifetime {
        if impl_generics.is_empty() {
            impl_generics = "<'de>".to_string();
        } else {
            let inner = impl_generics
                .trim_start_matches('<')
                .trim_end_matches('>')
                .trim();
            if inner.is_empty() {
                impl_generics = "<'de>".to_string();
            } else {
                impl_generics = format!("<'de, {inner}>");
            }
        }
    }
    let mut bounds = Vec::new();
    for param in generics.type_params() {
        bounds.push(format!("{}: {}::Deserialize<'de>", param.name, serde_path));
    }
    if !bounds.is_empty() {
        if where_clause.is_empty() {
            where_clause = format!(" where {}", bounds.join(", "));
        } else {
            where_clause.push_str(", ");
            where_clause.push_str(&bounds.join(", "));
        }
    }
    (impl_generics, ty_generics, where_clause)
}

fn visitor_generics(type_params: &[String]) -> String {
    if type_params.is_empty() {
        "<'de>".to_string()
    } else {
        format!("<'de, {}>", type_params.join(", "))
    }
}

fn visitor_phantom(type_params: &[String]) -> String {
    if type_params.is_empty() {
        "::core::marker::PhantomData<fn(&'de ())>".to_string()
    } else {
        format!(
            "::core::marker::PhantomData<fn(&'de (), ({}))>",
            type_params.join(", ")
        )
    }
}

fn visitor_where_clause(type_params: &[String], serde_path: &str) -> String {
    if type_params.is_empty() {
        String::new()
    } else {
        format!(
            " where {}",
            type_params
                .iter()
                .map(|param| format!(
                    "{param}: {serde_path}::Deserialize<'de>",
                    param = param,
                    serde_path = serde_path
                ))
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

fn deserialize_struct(
    name: &str,
    ty_generics: &str,
    data: &crate::ast::Struct,
    serde_path: &str,
    type_params: &[String],
) -> String {
    match &data.fields {
        Fields::Named(fields) => {
            deserialize_struct_named(name, ty_generics, fields, serde_path, type_params)
        }
        Fields::Unnamed(fields) => {
            deserialize_struct_tuple(name, ty_generics, fields, serde_path, type_params)
        }
        Fields::Unit => format!(
            "struct __Visitor{visitor_generics}({phantom});\nimpl{visitor_generics} {serde_path}::de::Visitor<'de> for __Visitor{visitor_generics} {visitor_where} {{\n    type Value = {name}{ty_generics};\n    fn expecting(&self, formatter: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {{\n        formatter.write_str(\"unit struct {name}\")\n    }}\n    fn visit_unit<E>(self) -> ::core::result::Result<Self::Value, E>\n    where\n        E: {serde_path}::de::Error,\n    {{\n        Ok({name})\n    }}\n}}\ndeserializer.deserialize_unit_struct(stringify!({name}), __Visitor(::core::marker::PhantomData))",
            serde_path = serde_path,
            name = name,
            ty_generics = ty_generics,
            visitor_generics = visitor_generics(type_params),
            visitor_where = visitor_where_clause(type_params, serde_path),
            phantom = visitor_phantom(type_params)
        ),
    }
}

fn deserialize_struct_named(
    name: &str,
    ty_generics: &str,
    fields: &[Field],
    serde_path: &str,
    type_params: &[String],
) -> String {
    let mut out = String::new();

    writeln!(out, "#[allow(non_camel_case_types)]").unwrap();
    writeln!(out, "enum __Field {{").unwrap();
    for idx in 0..fields.len() {
        writeln!(out, "    Field{idx},").unwrap();
    }
    writeln!(out, "    __Ignore,").unwrap();
    writeln!(out, "}}").unwrap();

    writeln!(
        out,
        "impl<'de> {serde_path}::Deserialize<'de> for __Field {{"
    )
    .unwrap();
    writeln!(
        out,
        "    fn deserialize<D>(deserializer: D) -> ::core::result::Result<Self, D::Error>"
    )
    .unwrap();
    writeln!(out, "    where").unwrap();
    writeln!(out, "        D: {serde_path}::Deserializer<'de>,").unwrap();
    writeln!(out, "    {{").unwrap();
    writeln!(out, "        struct __FieldVisitor;").unwrap();
    writeln!(
        out,
        "        impl<'de> {serde_path}::de::Visitor<'de> for __FieldVisitor {{"
    )
    .unwrap();
    writeln!(out, "            type Value = __Field;").unwrap();
    writeln!(
        out,
        "            fn expecting(&self, formatter: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {{"
    )
    .unwrap();
    writeln!(
        out,
        "                formatter.write_str(\"field identifier\")"
    )
    .unwrap();
    writeln!(out, "            }}").unwrap();
    writeln!(
        out,
        "            fn visit_str<E>(self, value: &str) -> ::core::result::Result<__Field, E>"
    )
    .unwrap();
    writeln!(out, "            where").unwrap();
    writeln!(out, "                E: {serde_path}::de::Error,").unwrap();
    writeln!(out, "            {{").unwrap();
    writeln!(out, "                match value {{").unwrap();
    for (idx, field) in fields.iter().enumerate() {
        let key = field
            .attr
            .rename
            .clone()
            .unwrap_or_else(|| format!("\"{}\"", field.name.as_ref().unwrap()));
        writeln!(
            out,
            "                    {key} => Ok(__Field::Field{idx}),",
            key = key,
            idx = idx
        )
        .unwrap();
    }
    writeln!(out, "                    _ => Ok(__Field::__Ignore),").unwrap();
    writeln!(out, "                }}").unwrap();
    writeln!(out, "            }}").unwrap();
    writeln!(out, "        }}").unwrap();
    writeln!(
        out,
        "        deserializer.deserialize_identifier(__FieldVisitor)"
    )
    .unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out, "}}").unwrap();

    let field_names: Vec<String> = fields
        .iter()
        .map(|field| {
            field
                .attr
                .rename
                .clone()
                .unwrap_or_else(|| format!("\"{}\"", field.name.as_ref().unwrap()))
        })
        .collect();
    writeln!(
        out,
        "const FIELDS: &[&str] = &[{}];",
        field_names.join(", ")
    )
    .unwrap();

    let visitor_generics = visitor_generics(type_params);
    let visitor_where = visitor_where_clause(type_params, serde_path);
    let visitor_phantom = visitor_phantom(type_params);
    writeln!(
        out,
        "struct __Visitor{visitor_generics}({visitor_phantom});",
        visitor_generics = visitor_generics,
        visitor_phantom = visitor_phantom
    )
    .unwrap();
    writeln!(
        out,
        "impl{visitor_generics} {serde_path}::de::Visitor<'de> for __Visitor{visitor_generics} {visitor_where} {{",
        visitor_generics = visitor_generics,
        serde_path = serde_path,
        visitor_where = visitor_where
    )
    .unwrap();
    writeln!(out, "    type Value = {name}{ty_generics};").unwrap();
    writeln!(
        out,
        "    fn expecting(&self, formatter: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {{"
    )
    .unwrap();
    writeln!(out, "        formatter.write_str(\"struct {name}\")").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(
        out,
        "    fn visit_map<A>(self, mut map: A) -> ::core::result::Result<Self::Value, A::Error>"
    )
    .unwrap();
    writeln!(out, "    where").unwrap();
    writeln!(out, "        A: {serde_path}::de::MapAccess<'de>,").unwrap();
    writeln!(out, "    {{").unwrap();
    for field in fields {
        let ident = field.name.as_ref().unwrap();
        writeln!(out, "        let mut {ident} = None;").unwrap();
    }
    writeln!(
        out,
        "        while let Some(__field) = map.next_key::<__Field>()? {{"
    )
    .unwrap();
    writeln!(out, "            match __field {{").unwrap();
    for (idx, field) in fields.iter().enumerate() {
        let ident = field.name.as_ref().unwrap();
        let key = field
            .attr
            .rename
            .clone()
            .unwrap_or_else(|| format!("\"{ident}\""));
        writeln!(out, "                __Field::Field{idx} => {{").unwrap();
        writeln!(out, "                    if {ident}.is_some() {{").unwrap();
        writeln!(
            out,
            "                        return Err({serde_path}::de::Error::duplicate_field({key}));",
            serde_path = serde_path,
            key = key
        )
        .unwrap();
        writeln!(out, "                    }}").unwrap();
        writeln!(
            out,
            "                    {ident} = Some(map.next_value()?);"
        )
        .unwrap();
        writeln!(out, "                }},").unwrap();
    }
    writeln!(
        out,
        "                __Field::__Ignore => {{ let _ = map.next_value::<{serde_path}::de::IgnoredAny>()?; }},",
        serde_path = serde_path
    )
    .unwrap();
    writeln!(out, "            }}").unwrap();
    writeln!(out, "        }}").unwrap();
    writeln!(out, "        Ok({name} {{").unwrap();
    for field in fields {
        let ident = field.name.as_ref().unwrap();
        let key = field
            .attr
            .rename
            .clone()
            .unwrap_or_else(|| format!("\"{ident}\""));
        match &field.attr.default {
            FieldDefault::None => {
                writeln!(
                    out,
                    "            {ident}: {ident}.ok_or_else(|| {serde_path}::de::Error::missing_field({key}))?,",
                    ident = ident,
                    serde_path = serde_path,
                    key = key
                )
                .unwrap();
            }
            FieldDefault::Default => {
                writeln!(
                    out,
                    "            {ident}: {ident}.unwrap_or_else(::core::default::Default::default),",
                    ident = ident
                )
                .unwrap();
            }
            FieldDefault::Function(func) => {
                writeln!(
                    out,
                    "            {ident}: {ident}.unwrap_or_else(|| {func}()),",
                    ident = ident,
                    func = func
                )
                .unwrap();
            }
        }
    }
    writeln!(out, "        }})").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out, "}}").unwrap();
    writeln!(
        out,
        "deserializer.deserialize_struct(stringify!({name}), FIELDS, __Visitor(::core::marker::PhantomData))"
    )
    .unwrap();

    out
}

fn deserialize_struct_tuple(
    name: &str,
    ty_generics: &str,
    fields: &[Field],
    serde_path: &str,
    type_params: &[String],
) -> String {
    let len = fields.len();
    let mut seq_lines = Vec::new();
    for idx in 0..len {
        seq_lines.push(format!(
            "let value{idx} = seq.next_element()?.ok_or_else(|| {serde_path}::de::Error::invalid_length({idx}, &self))?;",
            idx = idx,
            serde_path = serde_path
        ));
    }
    let result = (0..len)
        .map(|idx| format!("value{idx}", idx = idx))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "struct __Visitor{visitor_generics}({phantom});\nimpl{visitor_generics} {serde_path}::de::Visitor<'de> for __Visitor{visitor_generics} {visitor_where} {{\n    type Value = {name}{ty_generics};\n    fn expecting(&self, formatter: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {{\n        formatter.write_str(\"tuple struct {name}\")\n    }}\n    fn visit_seq<A>(self, mut seq: A) -> ::core::result::Result<Self::Value, A::Error>\n    where\n        A: {serde_path}::de::SeqAccess<'de>,\n    {{\n        {seq_lines}\n        Ok({name}({result}))\n    }}\n}}\ndeserializer.deserialize_tuple_struct(stringify!({name}), {len}, __Visitor(::core::marker::PhantomData))",
        serde_path = serde_path,
        name = name,
        ty_generics = ty_generics,
        seq_lines = seq_lines.join("\n        "),
        result = result,
        len = len,
        visitor_generics = visitor_generics(type_params),
        visitor_where = visitor_where_clause(type_params, serde_path),
        phantom = visitor_phantom(type_params)
    )
}

fn deserialize_enum(
    name: &str,
    ty_generics: &str,
    variants: &[Variant],
    serde_path: &str,
    type_params: &[String],
) -> String {
    let variant_names: Vec<String> = variants
        .iter()
        .enumerate()
        .map(|(idx, _)| format!("Variant{idx}"))
        .collect();
    let mut ident_matches = Vec::new();
    let mut variant_match_arms = Vec::new();
    let mut names = Vec::new();
    for (idx, variant) in variants.iter().enumerate() {
        let key = variant
            .attr
            .rename
            .clone()
            .unwrap_or_else(|| format!("\"{}\"", variant.name));
        ident_matches.push(format!(
            "{key} => Ok(__Variant::Variant{idx})",
            key = key,
            idx = idx
        ));
        names.push(key.clone());
        variant_match_arms.push(deserialize_variant(
            name,
            ty_generics,
            idx,
            variant,
            serde_path,
            type_params,
        ));
    }
    let variants_const = format!("const VARIANTS: &[&str] = &[{}];", names.join(", "));
    let visitor_generics = visitor_generics(type_params);
    let visitor_where = visitor_where_clause(type_params, serde_path);
    let visitor_phantom = visitor_phantom(type_params);
    format!(
        "#[allow(non_camel_case_types)]\nenum __Variant {{\n{variants}\n}}\nimpl<'de> {serde_path}::Deserialize<'de> for __Variant {{\n    fn deserialize<D>(deserializer: D) -> ::core::result::Result<Self, D::Error>\n    where\n        D: {serde_path}::Deserializer<'de>,\n    {{\n        struct __VariantVisitor;\n        impl<'de> {serde_path}::de::Visitor<'de> for __VariantVisitor {{\n            type Value = __Variant;\n            fn expecting(&self, formatter: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {{\n                formatter.write_str(\"variant identifier\")\n            }}\n            fn visit_str<E>(self, value: &str) -> ::core::result::Result<__Variant, E>\n            where\n                E: {serde_path}::de::Error,\n            {{\n                match value {{\n                    {ident_matches},\n                    _ => Err({serde_path}::de::Error::unknown_variant(value, VARIANTS)),\n                }}\n            }}\n        }}\n        deserializer.deserialize_identifier(__VariantVisitor)\n    }}\n}}\n{variants_const}\nstruct __Visitor{visitor_generics}({visitor_phantom});\nimpl{visitor_generics} {serde_path}::de::Visitor<'de> for __Visitor{visitor_generics} {visitor_where} {{\n    type Value = {name}{ty_generics};\n    fn expecting(&self, formatter: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {{\n        formatter.write_str(\"enum {name}\")\n    }}\n    fn visit_enum<A>(self, data: A) -> ::core::result::Result<Self::Value, A::Error>\n    where\n        A: {serde_path}::de::EnumAccess<'de>,\n    {{\n                use {serde_path}::de::VariantAccess;
        let (variant, access) = data.variant()?;\n        match variant {{\n            {variant_match_arms}\n        }}\n    }}\n}}\ndeserializer.deserialize_enum(stringify!({name}), VARIANTS, __Visitor(::core::marker::PhantomData))",
        variants = variant_names
            .iter()
            .map(|v| format!("{v},"))
            .collect::<Vec<_>>()
            .join("\n"),
        serde_path = serde_path,
        name = name,
        ty_generics = ty_generics,
        ident_matches = ident_matches.join(",\n                    "),
        variant_match_arms = variant_match_arms.join("\n            "),
        variants_const = variants_const,
        visitor_generics = visitor_generics,
        visitor_where = visitor_where,
        visitor_phantom = visitor_phantom
    )
}

fn deserialize_variant(
    name: &str,
    ty_generics: &str,
    index: usize,
    variant: &Variant,
    serde_path: &str,
    type_params: &[String],
) -> String {
    match &variant.fields {
        Fields::Unit => format!(
            "__Variant::Variant{idx} => {{\n                    access.unit_variant()?;\n                    Ok({name}::{variant})\n                }},",
            idx = index,
            name = name,
            variant = variant.name
        ),
        Fields::Unnamed(fields) => {
            let len = fields.len();
            let bindings: Vec<String> = (0..len).map(|i| format!("value{i}")).collect();
            let mut seq_lines = Vec::new();
            for (i, binding) in bindings.iter().enumerate() {
                seq_lines.push(format!(
                    "let {binding} = seq.next_element()?.ok_or_else(|| {serde_path}::de::Error::invalid_length({i}, &self))?;",
                    serde_path = serde_path,
                    binding = binding,
                    i = i
                ));
            }
            let result = bindings.join(", ");
            let visitor_generics = visitor_generics(type_params);
            let visitor_where = visitor_where_clause(type_params, serde_path);
            let visitor_phantom = visitor_phantom(type_params);
            format!(
"__Variant::Variant{idx} => {{\n                    struct __VisitorTuple{visitor_generics}({visitor_phantom});\n                    impl{visitor_generics} {serde_path}::de::Visitor<'de> for __VisitorTuple{visitor_generics} {visitor_where} {{\n                        type Value = {name}{ty_generics};\n                        fn expecting(&self, formatter: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {{\n                            formatter.write_str(\"tuple variant\")\n                        }}\n                        fn visit_seq<A>(self, mut seq: A) -> ::core::result::Result<Self::Value, A::Error>\n                        where\n                            A: {serde_path}::de::SeqAccess<'de>,\n                        {{\n                            {seq_lines}\n                            Ok({name}::{variant}({result}))\n                        }}\n                    }}\n                    <A::Variant as {serde_path}::de::VariantAccess>::tuple_variant(access, {len}, __VisitorTuple(::core::marker::PhantomData))\n                }},",
                idx = index,
                serde_path = serde_path,
                name = name,
                ty_generics = ty_generics,
                variant = variant.name,
                len = len,
                seq_lines = seq_lines.join("\n                            "),
                result = result,
                visitor_generics = visitor_generics,
                visitor_where = visitor_where,
                visitor_phantom = visitor_phantom
            )
        }
        Fields::Named(fields) => {
            let mut field_enum = String::new();
            writeln!(field_enum, "#[allow(non_camel_case_types)]").unwrap();
            writeln!(field_enum, "enum __Field {{").unwrap();
            for idx in 0..fields.len() {
                writeln!(field_enum, "    Field{idx},").unwrap();
            }
            writeln!(field_enum, "    __Ignore,").unwrap();
            writeln!(field_enum, "}}").unwrap();
            writeln!(
                field_enum,
                "impl<'de> {serde_path}::Deserialize<'de> for __Field {{",
                serde_path = serde_path
            )
            .unwrap();
            writeln!(
                field_enum,
                "    fn deserialize<D>(deserializer: D) -> ::core::result::Result<Self, D::Error>"
            )
            .unwrap();
            writeln!(field_enum, "    where").unwrap();
            writeln!(
                field_enum,
                "        D: {serde_path}::Deserializer<'de>,",
                serde_path = serde_path
            )
            .unwrap();
            writeln!(field_enum, "    {{").unwrap();
            writeln!(field_enum, "        struct __FieldVisitor;").unwrap();
            writeln!(
                field_enum,
                "        impl<'de> {serde_path}::de::Visitor<'de> for __FieldVisitor {{",
                serde_path = serde_path
            )
            .unwrap();
            writeln!(field_enum, "            type Value = __Field;").unwrap();
            writeln!(
                field_enum,
                "            fn expecting(&self, formatter: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {{"
            )
            .unwrap();
            writeln!(
                field_enum,
                "                formatter.write_str(\"field identifier\")"
            )
            .unwrap();
            writeln!(field_enum, "            }}").unwrap();
            writeln!(
                field_enum,
                "            fn visit_str<E>(self, value: &str) -> ::core::result::Result<__Field, E>"
            )
            .unwrap();
            writeln!(field_enum, "            where").unwrap();
            writeln!(
                field_enum,
                "                E: {serde_path}::de::Error,",
                serde_path = serde_path
            )
            .unwrap();
            writeln!(field_enum, "            {{").unwrap();
            writeln!(field_enum, "                match value {{").unwrap();
            for (field_index, field) in fields.iter().enumerate() {
                let field_name = field.name.as_ref().unwrap();
                let key = field
                    .attr
                    .rename
                    .clone()
                    .unwrap_or_else(|| format!("\"{field_name}\""));
                writeln!(
                    field_enum,
                    "                    {key} => Ok(__Field::Field{field_index}),",
                    key = key,
                    field_index = field_index
                )
                .unwrap();
            }
            writeln!(
                field_enum,
                "                    _ => Ok(__Field::__Ignore),"
            )
            .unwrap();
            writeln!(field_enum, "                }}").unwrap();
            writeln!(field_enum, "            }}").unwrap();
            writeln!(field_enum, "        }}").unwrap();
            writeln!(
                field_enum,
                "        deserializer.deserialize_identifier(__FieldVisitor)"
            )
            .unwrap();
            writeln!(field_enum, "    }}").unwrap();
            writeln!(field_enum, "}}").unwrap();

            let mut map_lines = Vec::new();
            for field in fields {
                map_lines.push(format!(
                    "let mut {name} = None;",
                    name = field.name.as_ref().unwrap()
                ));
            }
            let mut match_arms = Vec::new();
            for (idx, field) in fields.iter().enumerate() {
                let ident = field.name.as_ref().unwrap();
                let key = field
                    .attr
                    .rename
                    .clone()
                    .unwrap_or_else(|| format!("\"{ident}\""));
                match_arms.push(format!(
                    "                    __Field::Field{idx} => {{\n                        if {ident}.is_some() {{\n                            return Err({serde_path}::de::Error::duplicate_field({key}));\n                        }}\n                        {ident} = Some(map.next_value()?);\n                    }},",
                    idx = idx,
                    ident = ident,
                    serde_path = serde_path,
                    key = key
                ));
            }
            match_arms.push(format!(
                "                    __Field::__Ignore => {{ let _ = map.next_value::<{serde_path}::de::IgnoredAny>()?; }},",
                serde_path = serde_path
            ));
            let mut inits = Vec::new();
            for field in fields {
                let name_ident = field.name.as_ref().unwrap();
                inits.push(format!(
                    "let {name_ident} = {name_ident}.ok_or_else(|| {serde_path}::de::Error::missing_field(\"{name_ident}\"))?;",
                    name_ident = name_ident,
                    serde_path = serde_path
                ));
            }
            let field_names = fields
                .iter()
                .map(|f| {
                    f.attr
                        .rename
                        .clone()
                        .unwrap_or_else(|| format!("\"{}\"", f.name.as_ref().unwrap()))
                })
                .collect::<Vec<_>>()
                .join(", ");
            let struct_init = fields
                .iter()
                .map(|f| f.name.as_ref().unwrap())
                .map(|name| name.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            let visitor_generics = visitor_generics(type_params);
            let visitor_where = visitor_where_clause(type_params, serde_path);
            let visitor_phantom = visitor_phantom(type_params);
            format!(
"__Variant::Variant{idx} => {{\n                    {field_enum}\n                    struct __StructVisitor{visitor_generics}({visitor_phantom});\n                    impl{visitor_generics} {serde_path}::de::Visitor<'de> for __StructVisitor{visitor_generics} {visitor_where} {{\n                        type Value = {name}{ty_generics};\n                        fn expecting(&self, formatter: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {{\n                            formatter.write_str(\"struct variant\")\n                        }}\n                        fn visit_map<A>(self, mut map: A) -> ::core::result::Result<Self::Value, A::Error>\n                        where\n                            A: {serde_path}::de::MapAccess<'de>,\n                        {{\n                            {map_lines}\n                            while let Some(__field) = map.next_key::<__Field>()? {{\n                                match __field {{\n{match_arms}\n                                }}\n                            }}\n                            {inits}\n                            Ok({name}::{variant} {{ {struct_init} }})\n                        }}\n                    }}\n                    <A::Variant as {serde_path}::de::VariantAccess>::struct_variant(access, &[{field_names}], __StructVisitor(::core::marker::PhantomData))\n                }},",
                idx = index,
                serde_path = serde_path,
                name = name,
                ty_generics = ty_generics,
                variant = variant.name,
                map_lines = map_lines.join("\n                            "),
                match_arms = match_arms.join("\n"),
                inits = inits.join("\n                            "),
                struct_init = struct_init,
                field_names = field_names,
                visitor_generics = visitor_generics,
                visitor_where = visitor_where,
                visitor_phantom = visitor_phantom,
                field_enum = field_enum
            )
        }
    }
}
