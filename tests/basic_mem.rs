#![feature(test)]
#![no_std]
#![no_main]
#![test_runner(common::test_case_runner)]
#![feature(custom_test_frameworks)]
#![reexport_test_harness_main = "test_main"]

extern crate alloc;

use alloc::vec::Vec;
use core::mem::size_of;

//no-std otherwise std::mem::size_of
mod common;

const PATTERN: u8 = 0xAB;

/// Mainly test if memcpy works as expected. Also somewhat tests memcmp
/// Works with u8, u16, u32, u64, u128, i16, i32, i64 and i128
/// Probably not a super good test
fn mem<T>()
where
	T: core::fmt::Debug,
	T: num_traits::int::PrimInt,
{
	extern "C" {
		fn memcpy(dest: *mut u8, src: *const u8, n: usize) -> *mut u8;
		fn memcmp(s1: *const u8, s2: *const u8, n: usize) -> i32;
	}
	let vec_size: u32 = 10000;
	let pre_dest_vec_size: u32 = 1;
	let post_dest_vec_size: u32 = 1;
	let t_base_pattern: T = T::from(PATTERN).unwrap();
	let mut pattern: T = t_base_pattern;
	// Fill pattern of type T with size_of<T> times the byte pattern
	// The "pre" and "post part of the destination vector are later filled with this pattern
	for _i in 1..size_of::<T>() {
		pattern = pattern.shl(8) + t_base_pattern;
	}
	let pattern = pattern; // remove mut

	let a: Vec<T> = {
		//Vec containing 0..min(vec_size, T::max_value()) as pattern for vec_size elements
		let mut a: Vec<T> = Vec::with_capacity(vec_size as usize);
		let max = {
			// the max value in a is the minimum of (vec_size -1) and T::max
			let tmax = T::max_value();
			if T::from(vec_size).is_none() {
				tmax.to_u64().unwrap() // If vec_size can't be represented in T, then tmax must fit in u64
			} else {
				(vec_size - 1) as u64
			}
		};
		// ToDo - This loop should be rewritten in a nicer way
		while a.len() < vec_size as usize {
			for i in 0..=max {
				a.push(T::from(i).unwrap());
				if a.len() == vec_size as usize {
					break;
				};
			}
		}
		a
	};
	assert_eq!(a.len(), vec_size as usize);

	let mut b: Vec<T> =
		Vec::with_capacity((vec_size + pre_dest_vec_size + post_dest_vec_size) as usize);
	// Manually set length, since we will be manually filling the vector
	unsafe {
		b.set_len((vec_size + pre_dest_vec_size + post_dest_vec_size) as usize);
	}
	// Fill pre and post section with `pattern`
	for i in 0..pre_dest_vec_size {
		b[i as usize] = pattern;
	}
	for i in 0..post_dest_vec_size {
		b[(pre_dest_vec_size + vec_size + i) as usize] = pattern;
	}
	// Copy the actual vector
	unsafe {
		memcpy(
			b.as_mut_ptr().offset(pre_dest_vec_size as isize) as *mut u8,
			a.as_ptr() as *const u8,
			((size_of::<T>() as u32) * vec_size) as usize,
		);
	}
	// Assert that `pattern` in pre section was not changed by memcpy
	for i in 0..pre_dest_vec_size {
		assert_eq!(b[i as usize], pattern);
	}
	// Assert that `a` was correctly copied to `b`
	{
		let mut i = 0; // a[i] should match b[pre_dest_vec_size + i]
		let mut j: T = T::from(0).unwrap();
		while i < vec_size {
			assert_eq!(b[(pre_dest_vec_size + i) as usize], j);
			i += 1;
			j = if j == T::max_value() {
				T::from(0).unwrap()
			} else {
				j + T::from(1).unwrap()
			}
		}
	}
	// Assert that `pattern` in post section was not changed
	for i in 0..post_dest_vec_size {
		assert_eq!(b[(pre_dest_vec_size + vec_size + i) as usize], pattern);
	}
	// Do the assertions again, but this time using `memcmp`
	unsafe {
		assert_eq!(
			memcmp(
				b.as_ptr().offset(pre_dest_vec_size as isize) as *const u8,
				a.as_ptr() as *const u8,
				(size_of::<T>() as usize) * vec_size as usize,
			),
			0
		);
		// pattern is larger, a[0] is 0
		assert!(memcmp(b.as_ptr() as *const u8, a.as_ptr() as *const u8, 1) > 0);
		assert!(memcmp(a.as_ptr() as *const u8, b.as_ptr() as *const u8, 1) < 0);
		assert!(
			memcmp(
				b.as_ptr().offset((vec_size + pre_dest_vec_size) as isize) as *const u8,
				a.as_ptr() as *const u8,
				1,
			) > 0
		);
	}
}

#[test_case]
fn test_mem() {
	mem::<u8>();
	mem::<u16>();
	mem::<u32>();
	mem::<u64>();
	mem::<u128>();
	mem::<usize>();
}

#[no_mangle]
extern "C" fn runtime_entry(_argc: i32, _argv: *const *const u8, _env: *const *const u8) -> ! {
	test_main();
	common::exit(false)
}
