//! service macros
use proc_macro::TokenStream;
mod service;

#[proc_macro]
pub fn start_service_ctrl_dispatcher(item: TokenStream) -> TokenStream {
    service::expand_start_service_ctrl_dispatcher(item).map_or_else(
        |e| TokenStream::from(e.to_compile_error()),
        TokenStream::from,
    )
}

#[proc_macro_attribute]
pub fn service(attr: TokenStream, item: TokenStream) -> TokenStream {
    service::expand_service(attr, item).map_or_else(
        |e| TokenStream::from(e.to_compile_error()),
        TokenStream::from,
    )
}
