#![forbid(unsafe_code)]

use proc_macro::{Delimiter, TokenStream, TokenTree};
use std::str::FromStr;

#[derive(Debug)]
struct TypeDef {
    name: String,
    generics: String,
    where_predicates: Option<String>,
}

#[proc_macro_derive(Serialize, attributes(serde))]
pub fn derive_serialize(input: TokenStream) -> TokenStream {
    match TypeDef::parse(input) {
        Ok(def) => def.render_serialize(),
        Err(err) => compile_error(err),
    }
}

#[proc_macro_derive(Deserialize, attributes(serde))]
pub fn derive_deserialize(input: TokenStream) -> TokenStream {
    match TypeDef::parse(input) {
        Ok(def) => def.render_deserialize(),
        Err(err) => compile_error(err),
    }
}

fn compile_error(message: String) -> TokenStream {
    let escaped = escape_literal(&message);
    TokenStream::from_str(&format!("compile_error!(\"{}\");", escaped))
        .expect("compile_error token")
}

impl TypeDef {
    fn parse(input: TokenStream) -> Result<Self, String> {
        let tokens: Vec<TokenTree> = input.into_iter().collect();
        let mut idx = 0usize;

        while matches!(tokens.get(idx), Some(TokenTree::Punct(p)) if p.as_char() == '#') {
            idx += 1;
            if matches!(tokens.get(idx), Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Bracket)
            {
                idx += 1;
                continue;
            }
            return Err("expected attribute body".into());
        }

        if matches!(tokens.get(idx), Some(TokenTree::Ident(ident)) if ident.to_string() == "pub") {
            idx += 1;
            if matches!(tokens.get(idx), Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Parenthesis)
            {
                idx += 1;
            }
        }

        match tokens.get(idx) {
            Some(TokenTree::Ident(ident))
                if matches!(ident.to_string().as_str(), "struct" | "enum" | "union") =>
            {
                idx += 1;
            }
            _ => return Err("expected `struct`, `enum`, or `union`".into()),
        }

        let name = match tokens.get(idx) {
            Some(TokenTree::Ident(ident)) => {
                idx += 1;
                ident.to_string()
            }
            _ => return Err("expected type name".into()),
        };

        let mut generics_tokens = Vec::new();
        let mut generics_captured = false;
        let mut where_tokens = Vec::new();
        let mut has_where = false;

        while let Some(token) = tokens.get(idx) {
            match token {
                TokenTree::Punct(punct) if punct.as_char() == '<' && !generics_captured => {
                    let mut depth: i32 = 0;
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
                    has_where = true;
                    idx += 1;
                    while let Some(tok) = tokens.get(idx) {
                        match tok {
                            TokenTree::Group(group)
                                if matches!(
                                    group.delimiter(),
                                    Delimiter::Brace | Delimiter::Parenthesis | Delimiter::Bracket
                                ) =>
                            {
                                break
                            }
                            TokenTree::Punct(p) if p.as_char() == ';' => break,
                            _ => {
                                where_tokens.push(tok.clone());
                                idx += 1;
                            }
                        }
                    }
                    continue;
                }
                TokenTree::Group(group)
                    if matches!(
                        group.delimiter(),
                        Delimiter::Brace | Delimiter::Parenthesis | Delimiter::Bracket
                    ) =>
                {
                    break
                }
                TokenTree::Punct(p) if p.as_char() == ';' => break,
                _ => {
                    idx += 1;
                }
            }
        }

        let generics = tokens_to_string(&generics_tokens);
        let where_predicates = if has_where {
            Some(tokens_to_string(&where_tokens).trim().to_string())
        } else {
            None
        };

        Ok(Self {
            name,
            generics,
            where_predicates,
        })
    }

    fn render_serialize(&self) -> TokenStream {
        let msg = format!("foundation_serde stub serialize invoked for {}", self.name);
        let impl_generics = self.generics.clone();
        let type_generics = self.generics.clone();
        let where_clause = self.format_where_clause();
        let msg_literal = escape_literal(&msg);
        let source = format!(
            "impl{impl_generics} ::serde::ser::Serialize for {name}{type_generics}{where_clause} {{ fn serialize<S>(&self, _serializer: S) -> ::core::result::Result<S::Ok, S::Error> where S: ::serde::ser::Serializer {{ Err(<S::Error as ::serde::ser::Error>::custom(\"{msg}\")) }} }}",
            impl_generics = impl_generics,
            name = self.name,
            type_generics = type_generics,
            where_clause = where_clause,
            msg = msg_literal,
        );
        TokenStream::from_str(&source).expect("generated serialize tokens")
    }

    fn render_deserialize(&self) -> TokenStream {
        let msg = format!(
            "foundation_serde stub deserialize invoked for {}",
            self.name
        );
        let impl_generics = self.deserialize_impl_generics();
        let type_generics = self.generics.clone();
        let where_clause = self.format_where_clause();
        let msg_literal = escape_literal(&msg);
        let source = format!(
            "impl{impl_generics} ::serde::de::Deserialize<'de> for {name}{type_generics}{where_clause} {{ fn deserialize<D>(deserializer: D) -> ::core::result::Result<Self, D::Error> where D: ::serde::de::Deserializer<'de> {{ Err(<D::Error as ::serde::de::Error>::custom(\"{msg}\")) }} }}",
            impl_generics = impl_generics,
            name = self.name,
            type_generics = type_generics,
            where_clause = where_clause,
            msg = msg_literal,
        );
        TokenStream::from_str(&source).expect("generated deserialize tokens")
    }

    fn deserialize_impl_generics(&self) -> String {
        if self.generics.is_empty() {
            "<'de>".to_string()
        } else {
            let trimmed = self.generics.trim();
            if trimmed.starts_with('<') && trimmed.ends_with('>') {
                let inner = trimmed[1..trimmed.len() - 1].trim();
                if inner.is_empty() {
                    "<'de>".to_string()
                } else {
                    format!("<'de, {}>", inner)
                }
            } else {
                format!("<'de, {}>", trimmed)
            }
        }
    }

    fn format_where_clause(&self) -> String {
        match &self.where_predicates {
            None => String::new(),
            Some(predicates) if predicates.is_empty() => " where".to_string(),
            Some(predicates) => format!(" where {}", predicates),
        }
    }
}

fn tokens_to_string(tokens: &[TokenTree]) -> String {
    fn helper(tokens: &[TokenTree], out: &mut String) {
        for token in tokens {
            match token {
                TokenTree::Group(group) => {
                    let delimiter = group.delimiter();
                    if delimiter != Delimiter::None {
                        let open = match delimiter {
                            Delimiter::Brace => '{',
                            Delimiter::Bracket => '[',
                            Delimiter::Parenthesis => '(',
                            Delimiter::None => unreachable!(),
                        };
                        out.push(open);
                    }
                    let inner: Vec<TokenTree> = group.stream().into_iter().collect();
                    helper(&inner, out);
                    if delimiter != Delimiter::None {
                        let close = match delimiter {
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
