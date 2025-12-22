use crate::ast::{Data, DeriveInput, Field, Fields, Variant};
use crate::error::Error;
use crate::generics::Generics;

pub fn expand(input: &DeriveInput) -> Result<String, Error> {
    let serde_path = input
        .container_attr
        .crate_path
        .clone()
        .unwrap_or_else(|| "foundation_serialization::serde".to_string());
    let body = match &input.data {
        Data::Struct(struct_data) => serialize_struct(&input.name, struct_data, &serde_path),
        Data::Enum(variants) => serialize_enum(&input.name, variants, &serde_path),
    };
    build_impl(&input.name, &input.generics, &serde_path, &body)
}

fn build_impl(
    name: &str,
    generics: &Generics,
    serde_path: &str,
    body: &str,
) -> Result<String, Error> {
    let (impl_generics, ty_generics, mut where_clause) = generics.split_for_impl();
    let mut bounds = Vec::new();
    for param in generics.type_params() {
        bounds.push(format!("{}: {}::Serialize", param.name, serde_path));
    }
    if !bounds.is_empty() {
        if where_clause.is_empty() {
            where_clause = format!(" where {}", bounds.join(", "));
        } else {
            where_clause.push_str(", ");
            where_clause.push_str(&bounds.join(", "));
        }
    }
    let impl_block = format!(
        "#[automatically_derived]\nimpl{impl_generics} {serde_path}::Serialize for {name}{ty_generics} {where_clause} {{\n    fn serialize<S>(&self, serializer: S) -> ::core::result::Result<S::Ok, S::Error>\n    where\n        S: {serde_path}::Serializer,\n    {{\n        {body}\n    }}\n}}",
    );
    Ok(impl_block)
}

fn serialize_struct(name: &str, data: &crate::ast::Struct, serde_path: &str) -> String {
    match &data.fields {
        Fields::Named(fields) => serialize_struct_named(name, fields, serde_path),
        Fields::Unnamed(fields) => serialize_struct_tuple(name, fields, serde_path),
        Fields::Unit => format!("serializer.serialize_unit_struct(stringify!({name}))"),
    }
}

fn serialize_struct_named(name: &str, fields: &[Field], serde_path: &str) -> String {
    let mut lines = Vec::new();
    for field in fields {
        let field_name = field.name.as_ref().expect("named field");
        let key = field
            .attr
            .rename
            .clone()
            .unwrap_or_else(|| format!("\"{field_name}\""));
        lines.push(format!(
            "state.serialize_field({key}, &self.{field_name})?;"
        ));
    }
    format!(
        "use {serde_path}::ser::SerializeStruct;\nlet mut state = serializer.serialize_struct(stringify!({name}), {len})?;\n{lines}\nstate.end()",
        serde_path = serde_path,
        name = name,
        len = fields.len(),
        lines = lines.join("\n")
    )
}

fn serialize_struct_tuple(name: &str, fields: &[Field], serde_path: &str) -> String {
    let mut lines = Vec::new();
    for (idx, _) in fields.iter().enumerate() {
        lines.push(format!("state.serialize_field(&self.{idx})?;", idx = idx));
    }
    format!(
        "use {serde_path}::ser::SerializeTupleStruct;\nlet mut state = serializer.serialize_tuple_struct(stringify!({name}), {len})?;\n{lines}\nstate.end()",
        serde_path = serde_path,
        name = name,
        len = fields.len(),
        lines = lines.join("\n")
    )
}

fn serialize_enum(name: &str, variants: &[Variant], serde_path: &str) -> String {
    let mut arms = Vec::new();
    for (idx, variant) in variants.iter().enumerate() {
        arms.push(serialize_variant(name, idx, variant, serde_path));
    }
    format!("match self {{\n{}\n}}", arms.join("\n"))
}

fn serialize_variant(name: &str, index: usize, variant: &Variant, serde_path: &str) -> String {
    let variant_key = variant
        .attr
        .rename
        .clone()
        .unwrap_or_else(|| format!("\"{}\"", variant.name));
    match &variant.fields {
        Fields::Unit => format!(
            "{name}::{variant} => serializer.serialize_unit_variant(stringify!({name}), {index}, {variant_key}),",
            name = name,
            variant = variant.name,
            index = index,
            variant_key = variant_key
        ),
        Fields::Unnamed(fields) => {
            let bindings: Vec<String> = (0..fields.len())
                .map(|i| format!("__field{i}"))
                .collect();
            let binding_pattern = bindings
                .iter()
                .map(|b| format!("ref {b}"))
                .collect::<Vec<_>>()
                .join(", ");
            let mut body = Vec::new();
            for binding in &bindings {
                body.push(format!("state.serialize_field({binding})?;"));
            }
            format!(
                "{name}::{variant}({binding_pattern}) => {{\n    use {serde_path}::ser::SerializeTupleVariant;\n    let mut state = serializer.serialize_tuple_variant(stringify!({name}), {index}, {variant_key}, {len})?;\n    {body}\n    state.end()\n}},",
                name = name,
                variant = variant.name,
                binding_pattern = binding_pattern,
                serde_path = serde_path,
                index = index,
                variant_key = variant_key,
                len = fields.len(),
                body = body.join("\n    ")
            )
        }
        Fields::Named(fields) => {
            let field_names: Vec<String> = fields
                .iter()
                .map(|f| f.name.clone().expect("variant field"))
                .collect();
            let pattern = field_names
                .iter()
                .map(|name| format!("ref {name}"))
                .collect::<Vec<_>>()
                .join(", ");
            let mut body = Vec::new();
            for field in fields {
                let name = field.name.as_ref().unwrap();
                let key = field
                    .attr
                    .rename
                    .clone()
                    .unwrap_or_else(|| format!("\"{name}\""));
                body.push(format!("state.serialize_field({key}, {name})?;"));
            }
            format!(
                "{name}::{variant} {{ {pattern} }} => {{\n    use {serde_path}::ser::SerializeStructVariant;\n    let mut state = serializer.serialize_struct_variant(stringify!({name}), {index}, {variant_key}, {len})?;\n    {body}\n    state.end()\n}},",
                name = name,
                variant = variant.name,
                pattern = pattern,
                serde_path = serde_path,
                index = index,
                variant_key = variant_key,
                len = fields.len(),
                body = body.join("\n    ")
            )
        }
    }
}
