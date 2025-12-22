#![allow(clippy::while_let_on_iterator)]
#![forbid(unsafe_code)]

use proc_macro::{Delimiter, Group, TokenStream, TokenTree};
use std::iter::Peekable;
use std::str::FromStr;

fn compile_error(message: &str) -> TokenStream {
    TokenStream::from_str(&format!("compile_error!(\"{message}\");")).expect("compile_error tokens")
}

fn take_attribute<I>(tokens: &mut Peekable<I>) -> Result<Option<TokenStream>, TokenStream>
where
    I: Iterator<Item = TokenTree>,
{
    match tokens.peek() {
        Some(TokenTree::Punct(punct)) if punct.as_char() == '#' => {
            let mut attr = TokenStream::new();
            let pound = tokens.next().expect("attribute pound");
            attr.extend(Some(pound));
            match tokens.next() {
                Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Bracket => {
                    attr.extend(Some(TokenTree::Group(group)));
                    Ok(Some(attr))
                }
                _ => Err(compile_error("expected attribute group after `#`")),
            }
        }
        _ => Ok(None),
    }
}

#[proc_macro_attribute]
pub fn tb_serial(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut tokens = item.into_iter().peekable();
    let mut attributes = Vec::new();
    loop {
        match take_attribute(&mut tokens) {
            Ok(Some(attr)) => attributes.push(attr),
            Ok(None) => break,
            Err(err) => return err,
        }
    }

    let mut signature = Vec::new();
    let mut body = None;
    while let Some(token) = tokens.next() {
        match &token {
            TokenTree::Group(group) if group.delimiter() == Delimiter::Brace => {
                body = Some(group.clone());
                break;
            }
            _ => signature.push(token),
        }
    }

    let body_group = match body {
        Some(group) => group,
        None => return compile_error("`tb_serial` expects a function item"),
    };

    if signature.is_empty() {
        return compile_error("`tb_serial` requires a function signature");
    }

    let mut output = TokenStream::new();
    for attr in attributes {
        output.extend(attr);
    }
    output.extend(TokenStream::from_str("#[test]").expect("test attribute"));
    for token in signature {
        output.extend(Some(token));
    }

    let mut guarded = TokenStream::new();
    guarded.extend(
        TokenStream::from_str("let _tb_serial_guard = ::testkit::serial::lock();")
            .expect("guard acquisition"),
    );
    guarded.extend(TokenStream::from_str("let _ = &_tb_serial_guard;").expect("guard usage"));
    guarded.extend(body_group.stream());

    let mut new_body = Group::new(Delimiter::Brace, guarded);
    new_body.set_span(body_group.span());
    output.extend(Some(TokenTree::Group(new_body)));

    for token in tokens {
        output.extend(Some(token));
    }

    output
}
