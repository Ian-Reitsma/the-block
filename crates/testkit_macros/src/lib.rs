use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, ItemFn};

/// Attribute macro that enforces serial execution by locking a global mutex
/// before running the annotated test.
#[proc_macro_attribute]
pub fn tb_serial(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemFn);
    let attrs = input.attrs;
    let vis = input.vis;
    let sig = input.sig;
    let block = input.block;

    let output = quote! {
        #(#attrs)*
        #[test]
        #vis #sig {
            let _tb_serial_guard = ::testkit::serial::lock();
            let _ = &_tb_serial_guard;
            #block
        }
    };

    output.into()
}
