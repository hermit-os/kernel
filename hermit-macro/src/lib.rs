use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::ToTokens;
use syn::{Ident, parse_macro_input};

macro_rules! bail {
    ($span:expr, $($tt:tt)*) => {
        return Err(syn::Error::new_spanned($span, format!($($tt)*)))
    };
}

mod system;

// The structure of this implementation is inspired by Amanieu's excellent naked-function crate.
#[proc_macro_attribute]
pub fn system(_attr: TokenStream, item: TokenStream) -> TokenStream {
	let attr = Some(Ident::new("errno", Span::call_site()));
	match system::system_attribute(attr, parse_macro_input!(item)) {
		Ok(item) => item.into_token_stream().into(),
		Err(e) => e.to_compile_error().into(),
	}
}
