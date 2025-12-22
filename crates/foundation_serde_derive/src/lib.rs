#![forbid(unsafe_code)]

use proc_macro::TokenStream;

mod ast;
mod attr;
mod de;
mod error;
mod generics;
mod parser;
mod ser;
mod tokens;

use error::Error;

fn render_result(result: Result<String, Error>) -> TokenStream {
    match result {
        Ok(code) => code.parse().expect("generated derive"),
        Err(err) => err.into_compile_error(),
    }
}

#[proc_macro_derive(Serialize, attributes(serde))]
pub fn derive_serialize(input: TokenStream) -> TokenStream {
    let parsed = match parser::parse_input(input) {
        Ok(item) => item,
        Err(err) => return err.into_compile_error(),
    };
    render_result(ser::expand(&parsed))
}

#[proc_macro_derive(Deserialize, attributes(serde))]
pub fn derive_deserialize(input: TokenStream) -> TokenStream {
    let parsed = match parser::parse_input(input) {
        Ok(item) => item,
        Err(err) => return err.into_compile_error(),
    };
    render_result(de::expand(&parsed))
}
