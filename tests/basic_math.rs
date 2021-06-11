#![feature(test)]
#![feature(bench_black_box)]
#![no_std]
#![no_main]
#![test_runner(common::test_case_runner)]
#![feature(custom_test_frameworks)]
#![reexport_test_harness_main = "test_main"]

/// Regarding `#[test]` and `#[test_case]` this comment explains the current implementation
/// https://github.com/rust-lang/rust/issues/50297#issuecomment-524180479
/// This is of course subject to change, since the whole feature is not stable
///
//extern crate hermit;
//extern crate x86_64;
#[macro_use]
extern crate float_cmp;

//use common::*;
use core::hint::black_box;

use common::exit;

// Either use black_box from core::hint or the value_fence definition
// core hint is a nop, but possibly only prevents dead code elimination
// value_fence has higher overhead but should be a bit safer regarding preventing optimizations
// pub fn black_box<T>(x: T) -> T {
// 	common::value_fence::<T>(x)
// }
mod common;

#[test_case]
fn add1() {
	let x = black_box(1) + black_box(2);
	assert_eq!(x, 3);
}

#[test_case]
fn subtest() {
	int_test::<u8>();
	int_test::<u16>();
	int_test::<u32>();
	int_test::<u64>();
	int_test::<u128>();
	int_test::<i8>();
	int_test::<i16>();
	int_test::<i32>();
	int_test::<i64>();
	int_test::<i128>();

	sint_test::<i8>();
	sint_test::<i16>();
	sint_test::<i32>();
	sint_test::<i64>();
	sint_test::<i128>();
}

fn int_test<T>()
where
	T: core::fmt::Debug,
	T: num_traits::int::PrimInt,
{
	let fifteen = T::from(15).unwrap();
	let ten = T::from(10).unwrap();
	let r_five: T = black_box(fifteen) - black_box(ten);
	let r_zero: T = black_box(fifteen) - black_box(fifteen);
	assert_eq!(r_five, T::from(5).unwrap());
	assert_eq!(r_zero, T::from(0).unwrap());

	let r_twentyfive: T = black_box(fifteen) + black_box(ten);
	let r_twentyfive2: T = black_box(r_zero) + black_box(r_twentyfive);
	assert_eq!(r_twentyfive, T::from(25).unwrap());
	assert_eq!(r_twentyfive2, r_twentyfive);

	let r_hundred: T = black_box(ten) * black_box(ten);
	let r_hundred2: T = black_box(ten).pow(2);
	let r_zero: T = black_box(r_hundred) * black_box(r_zero);
	assert_eq!(r_hundred, T::from(100).unwrap());
	assert_eq!(r_hundred, r_hundred2);
	assert_eq!(r_zero, T::from(0).unwrap());

	let r_ten: T = black_box(r_hundred) / black_box(ten);
	let r_one: T = black_box(r_ten) / black_box(ten);
	let r_zero: T = black_box(r_one) / black_box(ten);
	assert_eq!(r_ten, ten);
	assert_eq!(r_one, T::from(1).unwrap());
	assert_eq!(r_zero, T::from(0).unwrap());
}

fn sint_test<T>()
where
	T: core::fmt::Debug,
	T: num_traits::sign::Signed,
	T: num_traits::int::PrimInt,
{
	let fifteen = T::from(15).unwrap();
	let ten = T::from(10).unwrap();
	let r_minusfive: T = black_box(ten) - black_box(fifteen);
	assert_eq!(r_minusfive, T::from(-5).unwrap());
	let r_minusfifteen: T = black_box(r_minusfive) - black_box(ten);
	assert_eq!(r_minusfifteen, T::from(-15).unwrap());
	let r_fifteen: T = black_box(r_minusfifteen) + black_box(fifteen) + black_box(fifteen);
	assert_eq!(r_fifteen, fifteen);

	let r_minusseventyfive: T = black_box(r_minusfive) * black_box(fifteen);
	let r_zero: T = black_box(r_minusseventyfive) * black_box(T::from(0).unwrap());
	assert_eq!(r_minusseventyfive, T::from(-75).unwrap());
	assert_eq!(r_zero, T::from(0).unwrap());
	assert_eq!(r_zero, T::from(-0).unwrap());

	let r_minusseven: T = black_box(r_minusseventyfive) / black_box(ten);
	assert_eq!(r_minusseven, T::from(-7).unwrap());

	let r_fortynine: T = black_box(r_minusseven).pow(2);
	let r_fortynine2: T = black_box(r_minusseven) * black_box(r_minusseven);
	assert_eq!(r_fortynine, T::from(49).unwrap());
	assert_eq!(r_fortynine, r_fortynine2);
}

#[test_case]
fn test_f64_arithmetic() {
	let x = black_box::<f64>(65.2);
	let y = black_box::<f64>(89.123);
	let z = x * y;
	assert!(approx_eq!(f64, z, 5810.8196f64, ulps = 1));
	let z = z * y;
	assert!(approx_eq!(f64, z, 517877.6752108f64, ulps = 1));
	let z = z * y;
	assert!(approx_eq!(f64, z, 46_154_812.047_812_13_f64, ulps = 2));
	let z = z * y;
	assert!(approx_eq!(f64, z, 4_113_455_314.137_160_3_f64, ulps = 3));

	let z = black_box(z) / y;
	assert!(approx_eq!(f64, z, 46_154_812.047_812_13_f64, ulps = 2));
	assert!(!approx_eq!(f64, z, 46_154_812.047_812_13_f64, ulps = 1)); // If we haven't lost any precision, the something is fishy

	let z = black_box(z) / y;
	assert!(approx_eq!(f64, z, 517877.6752108f64, ulps = 2));
	assert!(!approx_eq!(f64, z, 517877.6752108f64, ulps = 1));

	// Division
	let x = black_box::<f64>(4.0);
	let y = black_box::<f64>(5.0);
	let z = x / y;
	assert!(approx_eq!(f64, z, 0.8f64, ulps = 0));
	let z = black_box(z) / y;
	assert!(approx_eq!(f64, z, 0.16f64, ulps = 0));
	// 0100011110101110000101000111101011100001010001111011 exp 01111111100
	let z = black_box(z) / y;
	assert!(approx_eq!(f64, z, 0.032f64, ulps = 0));
	let z = black_box(z) / y;
	assert!(approx_eq!(f64, z, 0.0064f64, ulps = 0));
	//1010001101101110001011101011000111000100001100101101 exp 01111110111
	let z = black_box(z) / y;
	assert!(approx_eq!(f64, z, 0.00128f64, ulps = 0));
	//0b0011111101010100111110001011010110001000111000110110100011110001

	let z = black_box(z) * black_box(y) * black_box(y) * black_box(y);
	assert!(approx_eq!(f64, z, 0.16f64, ulps = 0));
	let z = black_box(z * y);
	assert!(approx_eq!(f64, z, 0.8f64, ulps = 0));
}

#[no_mangle]
extern "C" fn runtime_entry(_argc: i32, _argv: *const *const u8, _env: *const *const u8) -> ! {
	test_main();
	exit(false);
}
