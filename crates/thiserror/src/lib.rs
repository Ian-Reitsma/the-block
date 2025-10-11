#![forbid(unsafe_code)]

use proc_macro::{Delimiter, Group, TokenStream, TokenTree};
use std::collections::BTreeSet;
use std::str::FromStr;

#[proc_macro_derive(Error, attributes(error, from, source))]
pub fn derive_error(input: TokenStream) -> TokenStream {
    match EnumDef::parse(input) {
        Ok(def) => def.render(),
        Err(err) => compile_error(err),
    }
}

fn compile_error(message: String) -> TokenStream {
    let escaped = message.replace('"', "\\\"");
    TokenStream::from_str(&format!("compile_error!(\"{}\");", escaped))
        .expect("compile_error token")
}

#[derive(Debug)]
struct EnumDef {
    name: String,
    generics: String,
    where_clause: String,
    variants: Vec<Variant>,
}

#[derive(Debug)]
struct Variant {
    name: String,
    kind: VariantKind,
    format: VariantFormat,
    fields: Vec<Field>,
    placeholders: Placeholders,
}

#[derive(Debug, Clone, Copy)]
enum VariantKind {
    Unit,
    Tuple,
    Struct,
}

#[derive(Debug)]
enum VariantFormat {
    Message(String),
    Transparent,
}

#[derive(Debug)]
struct Field {
    name: Option<String>,
    ty: String,
    is_from: bool,
    is_source: bool,
}

#[derive(Debug, Default)]
struct Placeholders {
    numeric: BTreeSet<usize>,
    named: BTreeSet<String>,
}

impl EnumDef {
    fn parse(input: TokenStream) -> Result<Self, String> {
        let tokens: Vec<TokenTree> = input.into_iter().collect();
        let mut idx = 0;
        while matches!(tokens.get(idx), Some(TokenTree::Punct(p)) if p.as_char() == '#') {
            idx += 1;
            if matches!(tokens.get(idx), Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Bracket)
            {
                idx += 1;
            }
        }
        // Skip optional visibility tokens.
        if matches!(tokens.get(idx), Some(TokenTree::Ident(ident)) if ident.to_string() == "pub") {
            idx += 1;
            if matches!(tokens.get(idx), Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Parenthesis)
            {
                idx += 1;
            }
        }
        if !matches!(tokens.get(idx), Some(TokenTree::Ident(ident)) if ident.to_string() == "enum")
        {
            return Err("expected `enum`".into());
        }
        idx += 1;
        let name = match tokens.get(idx) {
            Some(TokenTree::Ident(ident)) => {
                idx += 1;
                ident.to_string()
            }
            _ => return Err("expected enum name".into()),
        };
        let mut generics_tokens = Vec::new();
        let mut where_tokens = Vec::new();
        let mut body = None;
        let mut generics_captured = false;
        while let Some(token) = tokens.get(idx) {
            match token {
                TokenTree::Group(group) if group.delimiter() == Delimiter::Brace => {
                    body = Some(group.clone());
                    break;
                }
                TokenTree::Punct(p) if p.as_char() == '<' && !generics_captured => {
                    let mut depth = 0i32;
                    while let Some(tok) = tokens.get(idx) {
                        if let TokenTree::Punct(p) = tok {
                            match p.as_char() {
                                '<' => depth += 1,
                                '>' => depth -= 1,
                                _ => {}
                            }
                        }
                        generics_tokens.push(tok.clone());
                        idx += 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    generics_captured = true;
                    continue;
                }
                TokenTree::Ident(ident) if ident.to_string() == "where" => {
                    while let Some(tok) = tokens.get(idx) {
                        if let TokenTree::Group(group) = tok {
                            if group.delimiter() == Delimiter::Brace {
                                break;
                            }
                        }
                        where_tokens.push(tok.clone());
                        idx += 1;
                    }
                    continue;
                }
                _ => {
                    idx += 1;
                }
            }
        }
        let body = body.ok_or_else(|| "expected enum body".to_string())?;
        let generics = tokens_to_string(&generics_tokens);
        let where_clause = tokens_to_string(&where_tokens);
        let variants = Variant::parse_many(body.stream())?;
        Ok(Self {
            name,
            generics,
            where_clause,
            variants,
        })
    }

    fn render(&self) -> TokenStream {
        let mut output = String::new();
        output.push_str(&self.render_display());
        output.push_str(&self.render_error());
        for impl_from in self.render_from_impls() {
            output.push_str(&impl_from);
        }
        TokenStream::from_str(&output).expect("generated tokens")
    }

    fn render_display(&self) -> String {
        let mut arms = Vec::new();
        for variant in &self.variants {
            arms.push(variant.render_display_arm(&self.name));
        }
        let impl_generics = self.generics.clone();
        let type_generics = self.generics.clone();
        let where_clause = if self.where_clause.is_empty() {
            String::new()
        } else {
            format!(" {}", self.where_clause)
        };
        format!(
            "impl{impl_generics} core::fmt::Display for {name}{type_generics} {where_clause} {{ fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {{ match self {{ {arms} }} }} }}",
            impl_generics = impl_generics,
            name = self.name,
            type_generics = type_generics,
            where_clause = where_clause,
            arms = arms.join(" ")
        )
    }

    fn render_error(&self) -> String {
        let mut arms = Vec::new();
        for variant in &self.variants {
            arms.push(variant.render_source_arm(&self.name));
        }
        let impl_generics = self.generics.clone();
        let type_generics = self.generics.clone();
        let where_clause = if self.where_clause.is_empty() {
            String::new()
        } else {
            format!(" {}", self.where_clause)
        };
        format!(
            "impl{impl_generics} std::error::Error for {name}{type_generics} {where_clause} {{ fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {{ match self {{ {arms} }} }} }}",
            impl_generics = impl_generics,
            name = self.name,
            type_generics = type_generics,
            where_clause = where_clause,
            arms = arms.join(" ")
        )
    }

    fn render_from_impls(&self) -> Vec<String> {
        let mut impls = Vec::new();
        for variant in &self.variants {
            if let Some((ty, body)) = variant.render_from(&self.name) {
                let impl_generics = self.generics.clone();
                let type_generics = self.generics.clone();
                let where_clause = if self.where_clause.is_empty() {
                    String::new()
                } else {
                    format!(" {}", self.where_clause)
                };
                impls.push(format!(
                    "impl{impl_generics} From<{ty}> for {name}{type_generics} {where_clause} {{ fn from(value: {ty}) -> Self {{ {body} }} }}",
                    impl_generics = impl_generics,
                    ty = ty,
                    name = self.name,
                    type_generics = type_generics,
                    where_clause = where_clause,
                    body = body
                ));
            }
        }
        impls
    }
}

impl Variant {
    fn parse_many(stream: TokenStream) -> Result<Vec<Self>, String> {
        let mut variants = Vec::new();
        let mut iter = stream.into_iter().peekable();
        let mut attrs = Vec::new();
        while let Some(token) = iter.next() {
            match token {
                TokenTree::Punct(p) if p.as_char() == '#' => {
                    if let Some(TokenTree::Group(group)) = iter.next() {
                        attrs.push(group);
                    } else {
                        return Err("expected attribute group".into());
                    }
                }
                TokenTree::Ident(ident) => {
                    let name = ident.to_string();
                    let (kind, fields_tokens) = if let Some(TokenTree::Group(group)) = iter.peek() {
                        match group.delimiter() {
                            Delimiter::Parenthesis => {
                                let group = group.clone();
                                iter.next();
                                (VariantKind::Tuple, Some(group.stream()))
                            }
                            Delimiter::Brace => {
                                let group = group.clone();
                                iter.next();
                                (VariantKind::Struct, Some(group.stream()))
                            }
                            _ => (VariantKind::Unit, None),
                        }
                    } else {
                        (VariantKind::Unit, None)
                    };
                    let format = parse_variant_format(&attrs)?;
                    let fields = match (kind, fields_tokens) {
                        (VariantKind::Unit, _) => Vec::new(),
                        (VariantKind::Tuple, Some(tokens)) => parse_tuple_fields(tokens)?,
                        (VariantKind::Struct, Some(tokens)) => parse_struct_fields(tokens)?,
                        _ => Vec::new(),
                    };
                    let placeholders = match &format {
                        VariantFormat::Message(template) => extract_placeholders(template),
                        VariantFormat::Transparent => Placeholders::default(),
                    };
                    let variant = Variant {
                        name,
                        kind,
                        format,
                        fields,
                        placeholders,
                    };
                    variant.validate()?;
                    variants.push(variant);
                    attrs.clear();
                    // consume trailing comma if present
                    if matches!(iter.peek(), Some(TokenTree::Punct(p)) if p.as_char() == ',') {
                        iter.next();
                    }
                }
                TokenTree::Punct(p) if p.as_char() == ',' => {
                    continue;
                }
                other => return Err(format!("unexpected token in enum body: {other}")),
            }
        }
        Ok(variants)
    }

    fn validate(&self) -> Result<(), String> {
        if let VariantFormat::Transparent = self.format {
            match self.kind {
                VariantKind::Unit => {
                    return Err(format!(
                        "#[error(transparent)] is not allowed on unit variant `{}`",
                        self.name
                    ));
                }
                VariantKind::Tuple | VariantKind::Struct => {
                    if self.fields.len() != 1 {
                        return Err(format!(
                            "variant `{}` with #[error(transparent)] must contain exactly one field",
                            self.name
                        ));
                    }
                    let source_count = self
                        .fields
                        .iter()
                        .filter(|field| field.is_source || field.is_from)
                        .count();
                    if source_count == 0 {
                        return Err(format!(
                            "variant `{}` with #[error(transparent)] requires a field marked with #[from] or #[source]",
                            self.name
                        ));
                    }
                    if source_count > 1 {
                        return Err(format!(
                            "variant `{}` cannot have multiple #[from] or #[source] fields when using #[error(transparent)]",
                            self.name
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    fn render_display_arm(&self, _enum_name: &str) -> String {
        match &self.kind {
            VariantKind::Unit => match &self.format {
                VariantFormat::Message(message) => format!(
                    "Self::{variant} => write!(f, \"{msg}\"),",
                    variant = self.name,
                    msg = escape_literal(message)
                ),
                VariantFormat::Transparent => format!(
                    "Self::{variant} => write!(f, \"{variant}\"),",
                    variant = self.name
                ),
            },
            VariantKind::Tuple => self.render_display_tuple(),
            VariantKind::Struct => self.render_display_struct(),
        }
    }

    fn render_display_tuple(&self) -> String {
        let bindings: Vec<String> = (0..self.fields.len())
            .map(|idx| format!("f{idx}"))
            .collect();
        let pattern = format!(
            "Self::{variant}({args})",
            variant = self.name,
            args = bindings
                .iter()
                .map(|b| format!("{b}"))
                .collect::<Vec<_>>()
                .join(",")
        );
        match &self.format {
            VariantFormat::Message(message) => {
                let mut args = String::new();
                for binding in &bindings {
                    args.push_str(&format!(", {binding}", binding = binding));
                }
                format!(
                    "{pattern} => write!(f, \"{msg}\"{args}),",
                    pattern = pattern,
                    msg = escape_literal(message),
                    args = args
                )
            }
            VariantFormat::Transparent => {
                let binding = bindings.get(0).cloned().unwrap_or_else(|| "_".to_string());
                format!(
                    "{pattern} => write!(f, \"{{}}\", {binding}),",
                    pattern = pattern,
                    binding = binding
                )
            }
        }
    }

    fn render_display_struct(&self) -> String {
        let mut binding_fields = self.placeholders.named.clone();
        let display_fields = self.placeholders.named.clone();
        for field in &self.fields {
            if let Some(name) = &field.name {
                if field.is_from || field.is_source {
                    binding_fields.insert(name.clone());
                }
            }
        }
        let mut bindings = Vec::new();
        for name in &binding_fields {
            bindings.push(name.clone());
        }
        let pattern = if bindings.is_empty() {
            format!("Self::{variant} {{ .. }}", variant = self.name)
        } else {
            format!(
                "Self::{variant} {{ {bindings}, .. }}",
                variant = self.name,
                bindings = bindings.join(",")
            )
        };
        match &self.format {
            VariantFormat::Message(message) => {
                let mut args = String::new();
                for name in display_fields.iter() {
                    args.push_str(&format!(", {name} = {name}", name = name));
                }
                format!(
                    "{pattern} => write!(f, \"{msg}\"{args}),",
                    pattern = pattern,
                    msg = escape_literal(message),
                    args = args
                )
            }
            VariantFormat::Transparent => {
                let source = binding_fields
                    .iter()
                    .next()
                    .cloned()
                    .unwrap_or_else(|| "_".to_string());
                format!(
                    "{pattern} => write!(f, \"{{}}\", {source}),",
                    pattern = pattern,
                    source = source
                )
            }
        }
    }

    fn render_source_arm(&self, _enum_name: &str) -> String {
        match &self.kind {
            VariantKind::Unit => format!("Self::{variant} => None,", variant = self.name),
            VariantKind::Tuple => {
                let bindings: Vec<String> = (0..self.fields.len())
                    .map(|idx| format!("f{idx}"))
                    .collect();
                let pattern = format!(
                    "Self::{variant}({args})",
                    variant = self.name,
                    args = bindings.join(",")
                );
                let mut sources: Vec<String> = self
                    .fields
                    .iter()
                    .enumerate()
                    .filter(|(_, field)| field.is_source || field.is_from)
                    .map(|(idx, field)| source_expression(&bindings[idx], &field.ty))
                    .collect();
                if let Some(expr) = fold_sources(&mut sources) {
                    format!("{pattern} => {expr},", pattern = pattern, expr = expr)
                } else {
                    format!("{pattern} => None,", pattern = pattern)
                }
            }
            VariantKind::Struct => {
                let sources: Vec<(String, String)> = self
                    .fields
                    .iter()
                    .filter(|field| field.is_source || field.is_from)
                    .map(|field| {
                        let field_name = field
                            .name
                            .as_ref()
                            .expect("struct fields always have a name")
                            .clone();
                        let expr = source_expression(&field_name, &field.ty);
                        (field_name, expr)
                    })
                    .collect();
                if sources.is_empty() {
                    format!("Self::{variant} {{ .. }} => None,", variant = self.name)
                } else {
                    let bindings: Vec<String> =
                        sources.iter().map(|(name, _)| name.clone()).collect();
                    let pattern = format!(
                        "Self::{variant} {{ {fields}, .. }}",
                        variant = self.name,
                        fields = bindings.join(",")
                    );
                    let mut exprs: Vec<String> =
                        sources.into_iter().map(|(_, expr)| expr).collect();
                    let expr = fold_sources(&mut exprs).expect("non-empty sources");
                    format!("{pattern} => {expr},", pattern = pattern, expr = expr)
                }
            }
        }
    }

    fn render_from(&self, enum_name: &str) -> Option<(String, String)> {
        match self.kind {
            VariantKind::Tuple => {
                if self.fields.len() == 1 && self.fields[0].is_from {
                    Some((
                        self.fields[0].ty.clone(),
                        format!(
                            "{enum_name}::{variant}(value)",
                            enum_name = enum_name,
                            variant = self.name
                        ),
                    ))
                } else {
                    None
                }
            }
            VariantKind::Struct => {
                if self.fields.len() == 1 && self.fields[0].is_from {
                    let field = self.fields[0].name.clone().unwrap();
                    Some((
                        self.fields[0].ty.clone(),
                        format!(
                            "{enum_name}::{variant} {{ {field}: value }}",
                            enum_name = enum_name,
                            variant = self.name,
                            field = field
                        ),
                    ))
                } else {
                    None
                }
            }
            VariantKind::Unit => None,
        }
    }
}

fn parse_variant_format(attrs: &[Group]) -> Result<VariantFormat, String> {
    for attr in attrs.iter().rev() {
        let mut tokens = attr.stream().into_iter();
        if let Some(TokenTree::Ident(ident)) = tokens.next() {
            if ident.to_string() != "error" {
                continue;
            }
            match tokens.next() {
                Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Parenthesis => {
                    let inner: Vec<TokenTree> = group.stream().into_iter().collect();
                    if inner.len() == 1 {
                        match &inner[0] {
                            TokenTree::Literal(lit) => {
                                let text = lit.to_string();
                                let trimmed = text.trim_matches('"').to_string();
                                return Ok(VariantFormat::Message(trimmed));
                            }
                            TokenTree::Ident(ident) if ident.to_string() == "transparent" => {
                                return Ok(VariantFormat::Transparent);
                            }
                            other => return Err(format!("unsupported #[error] value: {other}")),
                        }
                    }
                }
                _ => {}
            }
        }
    }
    Err("missing #[error(..)] attribute".into())
}

fn parse_tuple_fields(stream: TokenStream) -> Result<Vec<Field>, String> {
    split_fields(stream)
        .into_iter()
        .map(parse_tuple_field)
        .collect()
}

fn parse_struct_fields(stream: TokenStream) -> Result<Vec<Field>, String> {
    split_fields(stream)
        .into_iter()
        .map(parse_struct_field)
        .collect()
}

fn split_fields(stream: TokenStream) -> Vec<Vec<TokenTree>> {
    let mut fields = Vec::new();
    let mut current = Vec::new();
    let mut depth = 0;
    for token in stream.into_iter() {
        match &token {
            TokenTree::Punct(p) if p.as_char() == ',' && depth == 0 => {
                if !current.is_empty() {
                    fields.push(current);
                    current = Vec::new();
                }
            }
            TokenTree::Group(group) => {
                depth += 1;
                current.push(TokenTree::Group(group.clone()));
                depth -= 1;
            }
            _ => current.push(token),
        }
    }
    if !current.is_empty() {
        fields.push(current);
    }
    fields
}

fn parse_tuple_field(tokens: Vec<TokenTree>) -> Result<Field, String> {
    let (attrs, rest) = split_attrs(tokens);
    let ty = tokens_to_string(&rest);
    if ty.is_empty() {
        return Err("missing tuple field type".into());
    }
    let (is_from, is_source) = parse_field_flags(attrs);
    Ok(Field {
        name: None,
        ty,
        is_from,
        is_source,
    })
}

fn parse_struct_field(tokens: Vec<TokenTree>) -> Result<Field, String> {
    let (attrs, rest) = split_attrs(tokens);
    let mut iter = rest.into_iter();
    let name = match iter.next() {
        Some(TokenTree::Ident(ident)) => ident.to_string(),
        other => return Err(format!("expected field name, found {other:?}")),
    };
    match iter.next() {
        Some(TokenTree::Punct(p)) if p.as_char() == ':' => {}
        _ => return Err("expected ':' after field name".into()),
    }
    let ty_tokens: Vec<TokenTree> = iter.collect();
    let ty = tokens_to_string(&ty_tokens);
    if ty.is_empty() {
        return Err("missing struct field type".into());
    }
    let (is_from, is_source) = parse_field_flags(attrs);
    Ok(Field {
        name: Some(name),
        ty,
        is_from,
        is_source,
    })
}

fn split_attrs(tokens: Vec<TokenTree>) -> (Vec<Group>, Vec<TokenTree>) {
    let mut attrs = Vec::new();
    let mut rest = Vec::new();
    let mut iter = tokens.into_iter().peekable();
    while let Some(token) = iter.peek() {
        match token {
            TokenTree::Punct(p) if p.as_char() == '#' => {
                iter.next();
                if let Some(TokenTree::Group(group)) = iter.next() {
                    attrs.push(group);
                }
            }
            _ => {
                rest.extend(iter);
                break;
            }
        }
    }
    (attrs, rest)
}

fn parse_field_flags(attrs: Vec<Group>) -> (bool, bool) {
    let mut is_from = false;
    let mut is_source = false;
    for attr in attrs {
        let mut tokens = attr.stream().into_iter();
        if let Some(TokenTree::Ident(ident)) = tokens.next() {
            match ident.to_string().as_str() {
                "from" => is_from = true,
                "source" => is_source = true,
                _ => {}
            }
        }
    }
    if is_from {
        is_source = true;
    }
    (is_from, is_source)
}

fn tokens_to_string(tokens: &[TokenTree]) -> String {
    fn helper(tokens: &[TokenTree], out: &mut String) {
        for token in tokens {
            match token {
                TokenTree::Group(group) => {
                    if group.delimiter() != Delimiter::None {
                        let open = match group.delimiter() {
                            Delimiter::Brace => '{',
                            Delimiter::Bracket => '[',
                            Delimiter::Parenthesis => '(',
                            Delimiter::None => unreachable!(),
                        };
                        out.push(open);
                    }
                    let inner: Vec<TokenTree> = group.stream().into_iter().collect();
                    helper(&inner, out);
                    if group.delimiter() != Delimiter::None {
                        let close = match group.delimiter() {
                            Delimiter::Brace => '}',
                            Delimiter::Bracket => ']',
                            Delimiter::Parenthesis => ')',
                            Delimiter::None => unreachable!(),
                        };
                        out.push(close);
                    }
                }
                TokenTree::Ident(ident) => out.push_str(&ident.to_string()),
                TokenTree::Literal(lit) => out.push_str(&lit.to_string()),
                TokenTree::Punct(punct) => out.push(punct.as_char()),
            }
        }
    }

    let mut out = String::new();
    helper(tokens, &mut out);
    out
}

fn escape_literal(literal: &str) -> String {
    literal.replace('\\', "\\\\").replace('"', "\\\"")
}

fn extract_placeholders(template: &str) -> Placeholders {
    let mut placeholders = Placeholders::default();
    let chars: Vec<char> = template.chars().collect();
    let mut idx = 0;
    while idx < chars.len() {
        if chars[idx] == '{' {
            if idx + 1 < chars.len() && chars[idx + 1] == '{' {
                idx += 2;
                continue;
            }
            idx += 1;
            let start = idx;
            while idx < chars.len() && chars[idx] != '}' {
                idx += 1;
            }
            if idx < chars.len() {
                let placeholder: String = chars[start..idx].iter().collect();
                let key = placeholder.split(':').next().unwrap_or("").trim();
                if !key.is_empty() {
                    if let Ok(index) = key.parse::<usize>() {
                        placeholders.numeric.insert(index);
                    } else {
                        placeholders.named.insert(key.to_string());
                    }
                }
                idx += 1;
            }
        } else {
            idx += 1;
        }
    }
    placeholders
}

fn source_expression(binding: &str, ty: &str) -> String {
    let trimmed = ty.trim();
    let normalized = normalize_type(trimmed);
    if let Some(inner) = strip_generic_argument(&normalized, &OPTION_PREFIXES) {
        let inner_expr = source_expression("value", &inner);
        return format!(
            "{binding}.as_ref().and_then(|value| {inner_expr})",
            binding = binding,
            inner_expr = inner_expr
        );
    }
    if strip_generic_argument(&normalized, &BOX_PREFIXES).is_some()
        || strip_generic_argument(&normalized, &ARC_PREFIXES).is_some()
        || strip_generic_argument(&normalized, &RC_PREFIXES).is_some()
    {
        return format!(
            "Some(({binding}.as_ref()) as &(dyn std::error::Error + 'static))",
            binding = binding
        );
    }
    format!(
        "Some(({binding}) as &(dyn std::error::Error + 'static))",
        binding = binding
    )
}

fn fold_sources(exprs: &mut Vec<String>) -> Option<String> {
    let mut iter = exprs.drain(..);
    let mut current = iter.next()?;
    for next in iter {
        current = format!(
            "match {expr} {{ Some(value) => Some(value), None => {next} }}",
            expr = current,
            next = next
        );
    }
    Some(current)
}

const OPTION_PREFIXES: [&str; 3] = ["Option", "std::option::Option", "core::option::Option"];
const BOX_PREFIXES: [&str; 3] = ["Box", "std::boxed::Box", "alloc::boxed::Box"];
const ARC_PREFIXES: [&str; 3] = ["Arc", "std::sync::Arc", "alloc::sync::Arc"];
const RC_PREFIXES: [&str; 3] = ["Rc", "std::rc::Rc", "alloc::rc::Rc"];

fn strip_generic_argument(ty: &str, prefixes: &[&str]) -> Option<String> {
    for prefix in prefixes {
        if let Some(arg) = strip_generic_argument_for_prefix(ty, prefix) {
            return Some(arg);
        }
    }
    None
}

fn strip_generic_argument_for_prefix(ty: &str, prefix: &str) -> Option<String> {
    if !ty.starts_with(prefix) {
        return None;
    }
    let mut chars = ty[prefix.len()..].chars().peekable();
    if chars.next()? != '<' {
        return None;
    }
    let mut depth = 0;
    let mut result = String::new();
    while let Some(ch) = chars.next() {
        match ch {
            '<' => {
                depth += 1;
                result.push(ch);
            }
            '>' => {
                if depth == 0 {
                    let remaining: String = chars.collect();
                    if remaining.trim().is_empty() {
                        return Some(result.trim().to_string());
                    } else {
                        return None;
                    }
                } else {
                    depth -= 1;
                    result.push(ch);
                }
            }
            _ => result.push(ch),
        }
    }
    None
}

fn normalize_type(input: &str) -> String {
    input.chars().filter(|ch| !ch.is_whitespace()).collect()
}
