use crate::error::Error;
use proc_macro::{Delimiter, Group, TokenStream, TokenTree};

#[derive(Clone)]
pub struct TokenCursor {
    tokens: Vec<TokenTree>,
    pos: usize,
}

impl TokenCursor {
    pub fn new(stream: TokenStream) -> Self {
        Self {
            tokens: stream.into_iter().collect(),
            pos: 0,
        }
    }

    pub fn from_tokens(tokens: Vec<TokenTree>) -> Self {
        Self { tokens, pos: 0 }
    }

    pub fn is_end(&self) -> bool {
        self.pos >= self.tokens.len()
    }

    pub fn peek(&self) -> Option<&TokenTree> {
        self.tokens.get(self.pos)
    }

    pub fn next(&mut self) -> Option<TokenTree> {
        if self.is_end() {
            None
        } else {
            let token = self.tokens[self.pos].clone();
            self.pos += 1;
            Some(token)
        }
    }

    pub fn consume_punct(&mut self, ch: char) -> bool {
        match self.peek() {
            Some(TokenTree::Punct(p)) if p.as_char() == ch => {
                self.next();
                true
            }
            _ => false,
        }
    }

    #[allow(dead_code)]
    pub fn expect_ident(&mut self, expected: &str) -> Result<(), Error> {
        match self.next() {
            Some(TokenTree::Ident(ident)) if ident.to_string() == expected => Ok(()),
            _ => Err(Error::new(format!("expected identifier `{expected}`"))),
        }
    }

    pub fn expect_group(&mut self, delim: Delimiter) -> Result<Group, Error> {
        match self.next() {
            Some(TokenTree::Group(group)) if group.delimiter() == delim => Ok(group),
            _ => Err(Error::new("expected delimited group")),
        }
    }

    pub fn expect_punct(&mut self, ch: char) -> Result<(), Error> {
        match self.next() {
            Some(TokenTree::Punct(p)) if p.as_char() == ch => Ok(()),
            _ => Err(Error::new(format!("expected punctuation `{ch}`"))),
        }
    }
}

pub fn token_stream_to_string(tokens: &[TokenTree]) -> String {
    let mut stream = TokenStream::new();
    for token in tokens {
        stream.extend(Some(token.clone()));
    }
    stream.to_string()
}
