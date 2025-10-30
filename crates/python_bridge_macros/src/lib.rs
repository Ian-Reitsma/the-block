use proc_macro::TokenStream;

fn passthrough(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item
}

#[proc_macro_attribute]
pub fn new(attr: TokenStream, item: TokenStream) -> TokenStream {
    passthrough(attr, item)
}

#[proc_macro_attribute]
pub fn getter(attr: TokenStream, item: TokenStream) -> TokenStream {
    passthrough(attr, item)
}

#[proc_macro_attribute]
pub fn setter(attr: TokenStream, item: TokenStream) -> TokenStream {
    passthrough(attr, item)
}

#[proc_macro_attribute]
pub fn staticmethod(attr: TokenStream, item: TokenStream) -> TokenStream {
    passthrough(attr, item)
}
