use proc_macro2::{Ident, Span};
use quote::quote;
use syn::{Abi, Attribute, FnArg, Item, ItemFn, Pat, Result, Signature, Visibility, parse_quote};

fn validate_vis(vis: &Visibility) -> Result<()> {
	if !matches!(vis, Visibility::Public(_)) {
		bail!(vis, "#[system] functions must be public");
	}

	Ok(())
}

struct ParsedSig {
	args: Vec<Ident>,
}

fn parse_sig(sig: &Signature) -> Result<ParsedSig> {
	if let Some(constness) = sig.constness {
		bail!(constness, "#[system] is not supported on const functions");
	}
	if let Some(asyncness) = sig.asyncness {
		bail!(asyncness, "#[system] is not supported on async functions");
	}
	match &sig.abi {
		Some(Abi {
			extern_token: _,
			name: Some(name),
		}) if matches!(&*name.value(), "C" | "C-unwind") => {}
		_ => bail!(
			&sig.abi,
			"#[system] functions must be `extern \"C\"` or `extern \"C-unwind\"`"
		),
	}
	if !sig.generics.params.is_empty() {
		bail!(
			&sig.generics,
			"#[system] cannot be used with generic functions"
		);
	}
	if !sig.ident.to_string().starts_with("sys_") {
		bail!(&sig.ident, "#[system] functions must start with `sys_`");
	}

	let mut args = vec![];

	for arg in &sig.inputs {
		let pat = match arg {
			syn::FnArg::Receiver(_) => bail!(arg, "#[system] functions cannot take `self`"),
			syn::FnArg::Typed(pat) => pat,
		};
		if let Pat::Ident(pat) = &*pat.pat {
			args.push(pat.ident.clone());
		} else {
			bail!(pat, "unsupported pattern in #[system] function argument");
		}
	}

	Ok(ParsedSig { args })
}

fn validate_attrs(attrs: &[Attribute]) -> Result<()> {
	let mut no_mangle_found = false;
	for attr in attrs {
		if attr.path().is_ident("unsafe")
			&& let Ok(ident) = attr.parse_args::<Ident>()
			&& ident == "no_mangle"
		{
			no_mangle_found = true;
			continue;
		}

		if !attr.path().is_ident("cfg") && !attr.path().is_ident("doc") {
			bail!(
				attr,
				"#[system] functions may only have `#[doc]`, `#[unsafe(no_mangle)]` and `#[cfg]` attributes"
			);
		}
	}

	if !no_mangle_found {
		bail!(
			attrs.first(),
			"#[system] functions must have `#[unsafe(no_mangle)]` attribute"
		);
	}

	Ok(())
}

fn emit_func(mut func: ItemFn, sig: &ParsedSig) -> Result<ItemFn> {
	let args = &sig.args;
	let attrs = func.attrs.clone();
	let vis = func.vis.clone();
	let sig = func.sig.clone();

	let ident = Ident::new(&format!("__{}", func.sig.ident), Span::call_site());
	func.sig.ident = ident.clone();
	func.vis = Visibility::Inherited;
	func.attrs.clear();

	let input_idents = sig
		.inputs
		.iter()
		.map(|fn_arg| match fn_arg {
			FnArg::Typed(pat_type) => match &*pat_type.pat {
				Pat::Ident(pat_ident) => &pat_ident.ident,
				_ => unreachable!(),
			},
			_ => unreachable!(),
		})
		.collect::<Vec<_>>();
	#[allow(clippy::literal_string_with_formatting_args)]
	let input_format = input_idents
		.iter()
		.map(|ident| format!("{ident} = {{:?}}"))
		.collect::<Vec<_>>()
		.join(", ");
	let strace_format = format!("{}({input_format} ", sig.ident);

	let block = func.block;
	func.block = parse_quote! {{
		#[allow(unreachable_code)]
		#[allow(clippy::diverging_sub_expression)]
		{
			#[cfg(feature = "strace")]
			print!(#strace_format, #(#input_idents),*);
			let ret = #block;
			#[cfg(feature = "strace")]
			println!(") = {ret:?}");
			ret
		}
	}};

	let func_call = quote! {
		kernel_function!(#ident(#(#args),*))
	};

	let func_call = if func.sig.unsafety.is_some() {
		quote! {
			unsafe { #func_call }
		}
	} else {
		func_call
	};

	let func = parse_quote! {
		#(#attrs)*
		#vis #sig {
			#func

			#func_call
		}
	};

	Ok(func)
}

pub fn system_attribute(func: ItemFn) -> Result<Item> {
	validate_vis(&func.vis)?;
	let sig = parse_sig(&func.sig)?;
	validate_attrs(&func.attrs)?;
	let func = emit_func(func, &sig)?;
	Ok(Item::Fn(func))
}

#[cfg(test)]
mod tests {
	use quote::ToTokens;

	use super::*;

	#[test]
	fn test_safe() -> Result<()> {
		let input = parse_quote! {
			/// Adds two numbers together.
			///
			/// This is very important.
			#[cfg(target_os = "none")]
			#[unsafe(no_mangle)]
			pub extern "C" fn sys_test(a: i8, b: i16) -> i32 {
				let c = i16::from(a) + b;
				i32::from(c)
			}
		};

		let expected = quote! {
			/// Adds two numbers together.
			///
			/// This is very important.
			#[cfg(target_os = "none")]
			#[unsafe(no_mangle)]
			pub extern "C" fn sys_test(a: i8, b: i16) -> i32 {
				extern "C" fn __sys_test(a: i8, b: i16) -> i32 {
					#[allow(unreachable_code)]
					#[allow(clippy::diverging_sub_expression)]
					{
						#[cfg(feature = "strace")]
						print!("sys_test(a = {:?}, b = {:?} ", a, b);
						let ret = {
							let c = i16::from(a) + b;
							i32::from(c)
						};
						#[cfg(feature = "strace")]
						println!(") = {ret:?}");
						ret
					}
				}

				kernel_function!(__sys_test(a, b))
			}
		};

		let result = system_attribute(input)?.into_token_stream();

		assert_eq!(expected.to_string(), result.to_string());

		Ok(())
	}

	#[test]
	fn test_unsafe() -> Result<()> {
		let input = parse_quote! {
			/// Adds two numbers together.
			///
			/// This is very important.
			#[cfg(target_os = "none")]
			#[unsafe(no_mangle)]
			pub unsafe extern "C" fn sys_test(a: i8, b: i16) -> i32 {
				let c = i16::from(a) + b;
				i32::from(c)
			}
		};

		let expected = quote! {
			/// Adds two numbers together.
			///
			/// This is very important.
			#[cfg(target_os = "none")]
			#[unsafe(no_mangle)]
			pub unsafe extern "C" fn sys_test(a: i8, b: i16) -> i32 {
				unsafe extern "C" fn __sys_test(a: i8, b: i16) -> i32 {
					#[allow(unreachable_code)]
					#[allow(clippy::diverging_sub_expression)]
					{
						#[cfg(feature = "strace")]
						print!("sys_test(a = {:?}, b = {:?} ", a, b);
						let ret = {
							let c = i16::from(a) + b;
							i32::from(c)
						};
						#[cfg(feature = "strace")]
						println!(") = {ret:?}");
						ret
					}
				}

				unsafe { kernel_function!(__sys_test(a, b)) }
			}
		};

		let result = system_attribute(input)?.into_token_stream();

		assert_eq!(expected.to_string(), result.to_string());

		Ok(())
	}
}
