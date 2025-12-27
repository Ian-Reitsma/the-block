use crate::ast::{Data, DeriveInput, Field, Fields, Struct, Variant};
use crate::attr::{ContainerAttr, FieldAttr, FieldDefault, StructAttr, VariantAttr};
use crate::error::Error;
use crate::generics::{GenericParam, Generics, ParamKind};
use crate::tokens::{token_stream_to_string, TokenCursor};
use proc_macro::{Delimiter, Group, TokenStream, TokenTree};

pub fn parse_input(stream: TokenStream) -> Result<DeriveInput, Error> {
    let mut cursor = TokenCursor::new(stream);
    let metas = parse_serde_attributes(&mut cursor)?;
    let container_attr = apply_container_attrs(&metas);
    skip_visibility(&mut cursor);
    let item_kind = match cursor.next() {
        Some(TokenTree::Ident(ident)) => ident.to_string(),
        _ => return Err(Error::new("expected `struct` or `enum` keyword")),
    };
    let name = match cursor.next() {
        Some(TokenTree::Ident(ident)) => ident.to_string(),
        _ => return Err(Error::new("expected type name")),
    };
    let generics = parse_generics(&mut cursor)?;
    let data = match item_kind.as_str() {
        "struct" => {
            let fields = parse_struct_fields(&mut cursor)?;
            Data::Struct(fields)
        }
        "enum" => {
            let variants = parse_enum_variants(&mut cursor)?;
            Data::Enum(variants)
        }
        _ => return Err(Error::new("unsupported item for derive")),
    };
    Ok(DeriveInput {
        name,
        generics,
        data,
        container_attr,
    })
}

#[derive(Debug)]
enum SerdeMeta {
    Rename(String),
    Default,
    DefaultFunction(String),
    Crate(String),
    #[allow(dead_code)]
    RenameAll(String),
}

fn parse_serde_attributes(cursor: &mut TokenCursor) -> Result<Vec<SerdeMeta>, Error> {
    let mut metas = Vec::new();
    loop {
        match cursor.peek() {
            Some(TokenTree::Punct(p)) if p.as_char() == '#' => {
                cursor.next();
                let group = cursor.expect_group(Delimiter::Bracket)?;
                if let Some(attr) = parse_single_attribute(group)? {
                    metas.extend(attr);
                }
            }
            _ => break,
        }
    }
    Ok(metas)
}

fn parse_single_attribute(group: Group) -> Result<Option<Vec<SerdeMeta>>, Error> {
    let mut inner = TokenCursor::new(group.stream());
    let path = match inner.next() {
        Some(TokenTree::Ident(ident)) => ident.to_string(),
        _ => return Ok(None),
    };
    if path != "serde" {
        return Ok(None);
    }
    let args = match inner.next() {
        Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Parenthesis => group,
        _ => return Err(Error::new("expected serde attribute arguments")),
    };
    let tokens: Vec<TokenTree> = args.stream().into_iter().collect();
    let items = split_comma(tokens);
    let mut metas = Vec::new();
    for item in items {
        if let Some(meta) = parse_meta_item(&item)? {
            metas.push(meta);
        }
    }
    Ok(Some(metas))
}

fn parse_meta_item(tokens: &[TokenTree]) -> Result<Option<SerdeMeta>, Error> {
    if tokens.is_empty() {
        return Ok(None);
    }
    if let Some(TokenTree::Ident(ident)) = tokens.first() {
        let key = ident.to_string();
        match key.as_str() {
            "rename" => {
                if tokens.len() < 3 {
                    return Err(Error::new("expected string literal for `rename`"));
                }
                let value = literal_as_string(&tokens[2])?;
                return Ok(Some(SerdeMeta::Rename(value)));
            }
            "rename_all" => {
                if tokens.len() < 3 {
                    return Err(Error::new("expected string literal for `rename_all`"));
                }
                let value = literal_as_string(&tokens[2])?;
                return Ok(Some(SerdeMeta::RenameAll(value)));
            }
            "default" => {
                if tokens.len() == 1 {
                    return Ok(Some(SerdeMeta::Default));
                }
                if tokens.len() >= 3 {
                    let func = literal_without_quotes(&tokens[2])?;
                    return Ok(Some(SerdeMeta::DefaultFunction(func)));
                }
            }
            "crate" => {
                if tokens.len() < 3 {
                    return Err(Error::new("expected path for `crate`"));
                }
                let path = literal_without_quotes(&tokens[2])?;
                return Ok(Some(SerdeMeta::Crate(path)));
            }
            _ => return Ok(None),
        }
    }
    Ok(None)
}

fn literal_as_string(token: &TokenTree) -> Result<String, Error> {
    match token {
        TokenTree::Literal(lit) => Ok(lit.to_string()),
        _ => Err(Error::new("expected string literal")),
    }
}

fn literal_without_quotes(token: &TokenTree) -> Result<String, Error> {
    let literal = literal_as_string(token)?;
    Ok(literal.trim_matches('"').to_string())
}

fn apply_container_attrs(metas: &[SerdeMeta]) -> ContainerAttr {
    let mut attr = ContainerAttr::default();
    for meta in metas {
        if let SerdeMeta::Crate(path) = meta {
            attr.crate_path = Some(path.clone());
        }
    }
    attr
}

fn apply_field_attrs(metas: &[SerdeMeta]) -> FieldAttr {
    let mut attr = FieldAttr::default();
    for meta in metas {
        match meta {
            SerdeMeta::Rename(value) => attr.rename = Some(value.clone()),
            SerdeMeta::Default => attr.default = FieldDefault::Default,
            SerdeMeta::DefaultFunction(path) => attr.default = FieldDefault::Function(path.clone()),
            _ => {}
        }
    }
    attr
}

fn apply_variant_attrs(metas: &[SerdeMeta]) -> VariantAttr {
    let mut attr = VariantAttr::default();
    for meta in metas {
        if let SerdeMeta::Rename(value) = meta {
            attr.rename = Some(value.clone());
        }
    }
    attr
}

fn parse_generics(cursor: &mut TokenCursor) -> Result<Generics, Error> {
    let mut params = Vec::new();
    if cursor.consume_punct('<') {
        let mut depth = 1;
        let mut body = Vec::new();
        while depth > 0 {
            let token = cursor
                .next()
                .ok_or_else(|| Error::new("unterminated generics"))?;
            match &token {
                TokenTree::Punct(p) if p.as_char() == '<' => {
                    depth += 1;
                    body.push(token);
                }
                TokenTree::Punct(p) if p.as_char() == '>' => {
                    depth -= 1;
                    if depth > 0 {
                        body.push(token);
                    }
                }
                _ => body.push(token),
            }
        }
        let parts = split_comma(body);
        for part in parts {
            let decl = token_stream_to_string(&part);
            if decl.trim().is_empty() {
                continue;
            }
            let (name, kind) = extract_param_name(&decl);
            params.push(GenericParam {
                declaration: decl,
                name,
                kind,
            });
        }
    }
    let where_clause = parse_where_clause(cursor);
    Ok(Generics {
        params,
        where_clause,
    })
}

fn extract_param_name(param: &str) -> (String, ParamKind) {
    let trimmed = param.trim();
    if let Some(rest) = trimmed.strip_prefix("const ") {
        let name = rest
            .split(|c: char| c == ':' || c == '=' || c.is_whitespace())
            .next()
            .unwrap_or("")
            .to_string();
        return (name, ParamKind::Const);
    }
    if trimmed.starts_with('\'') {
        let name = trimmed
            .split(|c: char| c == ':' || c == ',' || c.is_whitespace())
            .next()
            .unwrap_or("")
            .to_string();
        return (name, ParamKind::Lifetime);
    }
    let name = trimmed
        .split(|c: char| c == ':' || c == '=' || c.is_whitespace())
        .next()
        .unwrap_or("")
        .to_string();
    (name, ParamKind::Type)
}

fn parse_where_clause(cursor: &mut TokenCursor) -> String {
    match cursor.peek() {
        Some(TokenTree::Ident(ident)) if ident.to_string() == "where" => {
            let mut tokens = Vec::new();
            tokens.push(cursor.next().unwrap());
            while let Some(token) = cursor.peek() {
                match token {
                    TokenTree::Group(group)
                        if matches!(
                            group.delimiter(),
                            Delimiter::Brace | Delimiter::Parenthesis
                        ) =>
                    {
                        break;
                    }
                    TokenTree::Punct(p) if p.as_char() == ';' => break,
                    _ => tokens.push(cursor.next().unwrap()),
                }
            }
            token_stream_to_string(&tokens)
        }
        _ => String::new(),
    }
}

fn parse_struct_fields(cursor: &mut TokenCursor) -> Result<Struct, Error> {
    match cursor.peek() {
        Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Brace => {
            let group = match cursor.next() {
                Some(TokenTree::Group(group)) => group,
                _ => unreachable!(),
            };
            let fields = parse_named_fields(group)?;
            Ok(Struct {
                fields: Fields::Named(fields),
                attrs: StructAttr::default(),
            })
        }
        Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Parenthesis => {
            let group = match cursor.next() {
                Some(TokenTree::Group(group)) => group,
                _ => unreachable!(),
            };
            let fields = parse_tuple_fields(group)?;
            cursor.consume_punct(';');
            Ok(Struct {
                fields: Fields::Unnamed(fields),
                attrs: StructAttr::default(),
            })
        }
        Some(TokenTree::Punct(p)) if p.as_char() == ';' => {
            cursor.next();
            Ok(Struct {
                fields: Fields::Unit,
                attrs: StructAttr::default(),
            })
        }
        _ => Err(Error::new("expected struct body")),
    }
}

fn parse_enum_variants(cursor: &mut TokenCursor) -> Result<Vec<Variant>, Error> {
    let group = match cursor.next() {
        Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Brace => group,
        _ => return Err(Error::new("expected enum body")),
    };
    let mut inner = TokenCursor::from_tokens(group.stream().into_iter().collect());
    let mut variants = Vec::new();
    while !inner.is_end() {
        if inner.consume_punct(',') {
            continue;
        }
        if inner.is_end() {
            break;
        }
        let metas = parse_serde_attributes(&mut inner)?;
        skip_visibility(&mut inner);
        let name = match inner.next() {
            Some(TokenTree::Ident(ident)) => ident.to_string(),
            _ => return Err(Error::new("expected variant name")),
        };
        let fields = match inner.peek() {
            Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Parenthesis => {
                let subtree = match inner.next() {
                    Some(TokenTree::Group(g)) => g,
                    _ => unreachable!(),
                };
                Fields::Unnamed(parse_tuple_fields(subtree)?)
            }
            Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Brace => {
                let subtree = match inner.next() {
                    Some(TokenTree::Group(g)) => g,
                    _ => unreachable!(),
                };
                Fields::Named(parse_named_fields(subtree)?)
            }
            _ => Fields::Unit,
        };
        if inner.consume_punct('=') {
            skip_discriminant(&mut inner);
        }
        let attr = apply_variant_attrs(&metas);
        variants.push(Variant { name, fields, attr });
        inner.consume_punct(',');
    }
    Ok(variants)
}

fn skip_discriminant(cursor: &mut TokenCursor) {
    while let Some(token) = cursor.peek() {
        match token {
            TokenTree::Punct(p) if p.as_char() == ',' => break,
            TokenTree::Group(_) => {
                cursor.next();
            }
            _ => {
                cursor.next();
            }
        }
    }
}

fn parse_named_fields(group: Group) -> Result<Vec<Field>, Error> {
    let mut inner = TokenCursor::from_tokens(group.stream().into_iter().collect());
    let mut fields = Vec::new();
    while !inner.is_end() {
        if inner.consume_punct(',') {
            continue;
        }
        if inner.is_end() {
            break;
        }
        let metas = parse_serde_attributes(&mut inner)?;
        skip_visibility(&mut inner);
        let name = match inner.next() {
            Some(TokenTree::Ident(ident)) => ident.to_string(),
            _ => return Err(Error::new("expected field name")),
        };
        inner.expect_punct(':')?;
        let ty_tokens = collect_until_comma(&mut inner);
        let ty = token_stream_to_string(&ty_tokens);
        let attr = apply_field_attrs(&metas);
        fields.push(Field {
            name: Some(name),
            ty,
            attr,
        });
        inner.consume_punct(',');
    }
    Ok(fields)
}

fn parse_tuple_fields(group: Group) -> Result<Vec<Field>, Error> {
    let mut inner = TokenCursor::from_tokens(group.stream().into_iter().collect());
    let mut fields = Vec::new();
    while !inner.is_end() {
        if inner.consume_punct(',') {
            continue;
        }
        if inner.is_end() {
            break;
        }
        let metas = parse_serde_attributes(&mut inner)?;
        skip_visibility(&mut inner);
        let ty_tokens = collect_until_comma(&mut inner);
        let ty = token_stream_to_string(&ty_tokens);
        let attr = apply_field_attrs(&metas);
        fields.push(Field {
            name: None,
            ty,
            attr,
        });
        inner.consume_punct(',');
    }
    Ok(fields)
}

fn collect_until_comma(cursor: &mut TokenCursor) -> Vec<TokenTree> {
    let mut tokens = Vec::new();
    let mut angle_depth = 0;
    while let Some(token) = cursor.peek() {
        match token {
            TokenTree::Punct(p) if p.as_char() == '<' => {
                angle_depth += 1;
                tokens.push(cursor.next().unwrap());
            }
            TokenTree::Punct(p) if p.as_char() == '>' => {
                if angle_depth > 0 {
                    angle_depth -= 1;
                }
                tokens.push(cursor.next().unwrap());
            }
            TokenTree::Punct(p) if p.as_char() == ',' && angle_depth == 0 => break,
            _ => tokens.push(cursor.next().unwrap()),
        }
    }
    tokens
}

fn skip_visibility(cursor: &mut TokenCursor) {
    if matches!(cursor.peek(), Some(TokenTree::Ident(ident)) if ident.to_string() == "pub") {
        cursor.next();
        if matches!(
            cursor.peek(),
            Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Parenthesis
        ) {
            cursor.next();
        }
    }
}

fn split_comma(tokens: Vec<TokenTree>) -> Vec<Vec<TokenTree>> {
    let mut parts = Vec::new();
    let mut current = Vec::new();
    let mut depth = 0;
    for token in tokens {
        match &token {
            TokenTree::Punct(p) if p.as_char() == '<' => {
                depth += 1;
                current.push(token);
            }
            TokenTree::Punct(p) if p.as_char() == '>' => {
                if depth > 0 {
                    depth -= 1;
                }
                current.push(token);
            }
            TokenTree::Punct(p) if p.as_char() == ',' && depth == 0 => {
                if !current.is_empty() {
                    parts.push(current);
                    current = Vec::new();
                }
            }
            _ => current.push(token),
        }
    }
    if !current.is_empty() {
        parts.push(current);
    }
    parts
}
