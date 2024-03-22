use proc_macro::TokenStream;
use quote::ToTokens;
use syn::parse::Nothing;
use syn::parse_macro_input;

macro_rules! bail {
    ($span:expr, $($tt:tt)*) => {
        return Err(syn::Error::new_spanned($span, format!($($tt)*)))
    };
}

mod system;

// The structure of this implementation is inspired by Amanieu's excellent naked-function crate.
#[proc_macro_attribute]
pub fn system(attr: TokenStream, item: TokenStream) -> TokenStream {
	parse_macro_input!(attr as Nothing);
	match system::system_attribute(parse_macro_input!(item)) {
		Ok(item) => item.into_token_stream().into(),
		Err(e) => e.to_compile_error().into(),
	}
}
