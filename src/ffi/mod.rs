#![warn(unsafe_op_in_unsafe_fn)]

#[allow(non_camel_case_types)]
pub type c_char = i8;

mod c_str;

pub use c_str::CStr;
