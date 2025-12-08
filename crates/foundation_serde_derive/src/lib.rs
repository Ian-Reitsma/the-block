#![forbid(unsafe_code)]

//! First-party derive macros for Serialize and Deserialize.
//!
//! This crate provides proc macros that generate serialization code for structs and enums,
//! compatible with the foundation_serde trait ecosystem.

extern crate proc_macro;

use proc_macro::TokenStream;
use syn::{parse_macro_input, DeriveInput};

mod de;
mod ser;

/// Derive macro for the `Serialize` trait.
///
/// # Example
///
/// ```ignore
/// #[derive(Serialize)]
/// struct Point {
///     x: i32,
///     y: i32,
/// }
/// ```
#[proc_macro_derive(Serialize, attributes(serde))]
pub fn derive_serialize(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    ser::expand_derive_serialize(&input)
        .unwrap_or_else(|err| err.to_compile_error())
        .into()
}

/// Derive macro for the `Deserialize` trait.
///
/// # Example
///
/// ```ignore
/// #[derive(Deserialize)]
/// struct Point {
///     x: i32,
///     y: i32,
/// }
/// ```
#[proc_macro_derive(Deserialize, attributes(serde))]
pub fn derive_deserialize(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    de::expand_derive_deserialize(&input)
        .unwrap_or_else(|err| err.to_compile_error())
        .into()
}
