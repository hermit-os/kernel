#![allow(dead_code)]

use core::convert::{From, Into};
use core::hash::{Hash, Hasher};
use core::{fmt, ops};

/// Align address downwards.
///
/// Returns the greatest x with alignment `align` so that x <= addr.
/// The alignment must be a power of 2.
#[inline(always)]
fn align_down(addr: u64, align: u64) -> u64 {
	addr & !(align - 1)
}

/// Align address upwards.
///
/// Returns the smallest x with alignment `align` so that x >= addr.
/// The alignment must be a power of 2.
#[inline(always)]
fn align_up(addr: u64, align: u64) -> u64 {
	let align_mask = align - 1;
	if addr & align_mask == 0 {
		addr
	} else {
		(addr | align_mask) + 1
	}
}

/// A wrapper for a physical address, which is in principle
/// derived from the crate x86.
#[repr(transparent)]
#[derive(Copy, Clone, Eq, Ord, PartialEq, PartialOrd)]
pub struct PhysAddr(pub u64);

impl PhysAddr {
	/// Convert to `u64`
	pub fn as_u64(self) -> u64 {
		self.0
	}

	/// Convert to `usize`
	pub fn as_usize(self) -> usize {
		self.0 as usize
	}

	/// Physical Address zero.
	pub const fn zero() -> Self {
		PhysAddr(0)
	}

	/// Is zero?
	pub fn is_zero(self) -> bool {
		self == PhysAddr::zero()
	}

	fn align_up<U>(self, align: U) -> Self
	where
		U: Into<u64>,
	{
		PhysAddr(align_up(self.0, align.into()))
	}

	fn align_down<U>(self, align: U) -> Self
	where
		U: Into<u64>,
	{
		PhysAddr(align_down(self.0, align.into()))
	}

	/// Is this address aligned to `align`?
	///
	/// # Note
	/// `align` must be a power of two.
	pub fn is_aligned<U>(self, align: U) -> bool
	where
		U: Into<u64> + Copy,
	{
		if !align.into().is_power_of_two() {
			return false;
		}

		self.align_down(align) == self
	}
}

impl From<u64> for PhysAddr {
	fn from(num: u64) -> Self {
		PhysAddr(num)
	}
}

impl From<usize> for PhysAddr {
	fn from(num: usize) -> Self {
		PhysAddr(num as u64)
	}
}

impl From<i32> for PhysAddr {
	fn from(num: i32) -> Self {
		PhysAddr(num as u64)
	}
}

impl From<PhysAddr> for u64 {
	fn from(value: PhysAddr) -> Self {
		value.0
	}
}

impl From<PhysAddr> for usize {
	fn from(value: PhysAddr) -> Self {
		value.0 as usize
	}
}

impl ops::Add for PhysAddr {
	type Output = PhysAddr;

	fn add(self, rhs: PhysAddr) -> Self::Output {
		PhysAddr(self.0 + rhs.0)
	}
}

impl ops::Add<u64> for PhysAddr {
	type Output = PhysAddr;

	fn add(self, rhs: u64) -> Self::Output {
		PhysAddr::from(self.0 + rhs)
	}
}

impl ops::Add<usize> for PhysAddr {
	type Output = PhysAddr;

	fn add(self, rhs: usize) -> Self::Output {
		PhysAddr::from(self.0 + rhs as u64)
	}
}

impl ops::AddAssign for PhysAddr {
	fn add_assign(&mut self, other: PhysAddr) {
		*self = PhysAddr::from(self.0 + other.0);
	}
}

impl ops::AddAssign<u64> for PhysAddr {
	fn add_assign(&mut self, offset: u64) {
		*self = PhysAddr::from(self.0 + offset);
	}
}

impl ops::Sub for PhysAddr {
	type Output = PhysAddr;

	fn sub(self, rhs: PhysAddr) -> Self::Output {
		PhysAddr::from(self.0 - rhs.0)
	}
}

impl ops::Sub<u64> for PhysAddr {
	type Output = PhysAddr;

	fn sub(self, rhs: u64) -> Self::Output {
		PhysAddr::from(self.0 - rhs)
	}
}

impl ops::Sub<usize> for PhysAddr {
	type Output = PhysAddr;

	fn sub(self, rhs: usize) -> Self::Output {
		PhysAddr::from(self.0 - rhs as u64)
	}
}

impl ops::Rem for PhysAddr {
	type Output = PhysAddr;

	fn rem(self, rhs: PhysAddr) -> Self::Output {
		PhysAddr(self.0 % rhs.0)
	}
}

impl ops::Rem<u64> for PhysAddr {
	type Output = u64;

	fn rem(self, rhs: u64) -> Self::Output {
		self.0 % rhs
	}
}

impl ops::Rem<usize> for PhysAddr {
	type Output = u64;

	fn rem(self, rhs: usize) -> Self::Output {
		self.0 % (rhs as u64)
	}
}

impl ops::BitAnd for PhysAddr {
	type Output = Self;

	fn bitand(self, rhs: Self) -> Self {
		PhysAddr(self.0 & rhs.0)
	}
}

impl ops::BitAnd<u64> for PhysAddr {
	type Output = u64;

	fn bitand(self, rhs: u64) -> Self::Output {
		Into::<u64>::into(self) & rhs
	}
}

impl ops::BitOr for PhysAddr {
	type Output = PhysAddr;

	fn bitor(self, rhs: PhysAddr) -> Self::Output {
		PhysAddr(self.0 | rhs.0)
	}
}

impl ops::BitOr<u64> for PhysAddr {
	type Output = u64;

	fn bitor(self, rhs: u64) -> Self::Output {
		self.0 | rhs
	}
}

impl ops::Shr<u64> for PhysAddr {
	type Output = u64;

	fn shr(self, rhs: u64) -> Self::Output {
		self.0 >> rhs
	}
}

impl fmt::Binary for PhysAddr {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		self.0.fmt(f)
	}
}

impl fmt::Display for PhysAddr {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		self.0.fmt(f)
	}
}

impl fmt::Debug for PhysAddr {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "{:#x}", self.0)
	}
}

impl fmt::LowerHex for PhysAddr {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		self.0.fmt(f)
	}
}

impl fmt::Octal for PhysAddr {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		self.0.fmt(f)
	}
}

impl fmt::UpperHex for PhysAddr {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		self.0.fmt(f)
	}
}

impl fmt::Pointer for PhysAddr {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		use core::fmt::LowerHex;
		self.0.fmt(f)
	}
}

impl Hash for PhysAddr {
	fn hash<H: Hasher>(&self, state: &mut H) {
		self.0.hash(state);
	}
}

/// A wrapper for a virtual address, which is in principle
/// derived from the crate x86.
#[repr(transparent)]
#[derive(Copy, Clone, Eq, Ord, PartialEq, PartialOrd)]
pub struct VirtAddr(pub u64);

impl VirtAddr {
	/// Convert from `u64`
	pub const fn from_u64(v: u64) -> Self {
		VirtAddr(v)
	}

	/// Convert from `usize`
	pub const fn from_usize(v: usize) -> Self {
		VirtAddr(v as u64)
	}

	/// Convert to `u64`
	pub const fn as_u64(self) -> u64 {
		self.0
	}

	/// Convert to `usize`
	pub const fn as_usize(self) -> usize {
		self.0 as usize
	}

	/// Convert to mutable pointer.
	pub fn as_mut_ptr<T>(self) -> *mut T {
		self.0 as *mut T
	}

	/// Convert to pointer.
	pub fn as_ptr<T>(self) -> *const T {
		self.0 as *const T
	}

	/// Physical Address zero.
	pub const fn zero() -> Self {
		VirtAddr(0)
	}

	/// Is zero?
	pub fn is_zero(self) -> bool {
		self == VirtAddr::zero()
	}

	fn align_up<U>(self, align: U) -> Self
	where
		U: Into<u64>,
	{
		VirtAddr(align_up(self.0, align.into()))
	}

	fn align_down<U>(self, align: U) -> Self
	where
		U: Into<u64>,
	{
		VirtAddr(align_down(self.0, align.into()))
	}

	/// Offset within the 4 KiB page.
	pub fn base_page_offset(self) -> u64 {
		self.0 & (BASE_PAGE_SIZE as u64 - 1)
	}

	/// Offset within the 2 MiB page.
	pub fn large_page_offset(self) -> u64 {
		self.0 & (MEGA_PAGE_SIZE as u64 - 1)
	}

	/// Offset within the 1 GiB page.
	pub fn giga_page_offset(self) -> u64 {
		self.0 & (GIGA_PAGE_SIZE as u64 - 1)
	}

	/// Offset within the 512 GiB page.
	pub fn tera_page_offset(self) -> u64 {
		self.0 & (TERA_PAGE_SIZE as u64 - 1)
	}

	/// Return address of nearest 4 KiB page (lower or equal than self).
	pub fn align_down_to_base_page(self) -> Self {
		self.align_down(BASE_PAGE_SIZE as u64)
	}

	/// Return address of nearest 2 MiB page (lower or equal than self).
	pub fn align_down_to_large_page(self) -> Self {
		self.align_down(MEGA_PAGE_SIZE as u64)
	}

	/// Return address of nearest 1 GiB page (lower or equal than self).
	pub fn align_down_to_giga_page(self) -> Self {
		self.align_down(GIGA_PAGE_SIZE as u64)
	}

	/// Return address of nearest 512 GiB page (lower or equal than self).
	pub fn align_down_to_tera_page(self) -> Self {
		self.align_down(TERA_PAGE_SIZE as u64)
	}

	/// Return address of nearest 4 KiB page (higher or equal than self).
	pub fn align_up_to_base_page(self) -> Self {
		self.align_up(BASE_PAGE_SIZE as u64)
	}

	/// Return address of nearest 2 MiB page (higher or equal than self).
	pub fn align_up_to_large_page(self) -> Self {
		self.align_up(MEGA_PAGE_SIZE as u64)
	}

	/// Return address of nearest 1 GiB page (higher or equal than self).
	pub fn align_up_to_giga_page(self) -> Self {
		self.align_up(GIGA_PAGE_SIZE as u64)
	}

	/// Return address of nearest 1 GiB page (higher or equal than self).
	pub fn align_up_to_huge_page(self) -> Self {
		self.align_up(GIGA_PAGE_SIZE as u64)
	}

	/// Return address of nearest 512 GiB page (higher or equal than self).
	pub fn align_up_to_tera_page(self) -> Self {
		self.align_up(TERA_PAGE_SIZE as u64)
	}

	/// Is this address aligned to a 4 KiB page?
	pub fn is_base_page_aligned(self) -> bool {
		self.align_down(BASE_PAGE_SIZE as u64) == self
	}

	/// Is this address aligned to a 2 MiB page?
	pub fn is_large_page_aligned(self) -> bool {
		self.align_down(MEGA_PAGE_SIZE as u64) == self
	}

	/// Is this address aligned to a 1 GiB page?
	pub fn is_giga_page_aligned(self) -> bool {
		self.align_down(GIGA_PAGE_SIZE as u64) == self
	}

	/// Is this address aligned to a 512 GiB page?
	pub fn is_tera_page_aligned(self) -> bool {
		self.align_down(TERA_PAGE_SIZE as u64) == self
	}

	/// Is this address aligned to `align`?
	///
	/// # Note
	/// `align` must be a power of two.
	pub fn is_aligned<U>(self, align: U) -> bool
	where
		U: Into<u64> + Copy,
	{
		if !align.into().is_power_of_two() {
			return false;
		}

		self.align_down(align) == self
	}
}

impl From<u64> for VirtAddr {
	fn from(num: u64) -> Self {
		VirtAddr(num)
	}
}

impl From<i32> for VirtAddr {
	fn from(num: i32) -> Self {
		VirtAddr(num as u64)
	}
}

impl From<VirtAddr> for u64 {
	fn from(value: VirtAddr) -> Self {
		value.0
	}
}

impl From<usize> for VirtAddr {
	fn from(num: usize) -> Self {
		VirtAddr(num as u64)
	}
}

impl From<VirtAddr> for usize {
	fn from(value: VirtAddr) -> Self {
		value.0 as usize
	}
}

impl ops::Add for VirtAddr {
	type Output = VirtAddr;

	fn add(self, rhs: VirtAddr) -> Self::Output {
		VirtAddr(self.0 + rhs.0)
	}
}

impl ops::Add<u64> for VirtAddr {
	type Output = VirtAddr;

	fn add(self, rhs: u64) -> Self::Output {
		VirtAddr(self.0 + rhs)
	}
}

impl ops::Add<usize> for VirtAddr {
	type Output = VirtAddr;

	fn add(self, rhs: usize) -> Self::Output {
		VirtAddr::from(self.0 + rhs as u64)
	}
}

impl ops::AddAssign for VirtAddr {
	fn add_assign(&mut self, other: VirtAddr) {
		*self = VirtAddr::from(self.0 + other.0);
	}
}

impl ops::AddAssign<u64> for VirtAddr {
	fn add_assign(&mut self, offset: u64) {
		*self = VirtAddr::from(self.0 + offset);
	}
}

impl ops::AddAssign<usize> for VirtAddr {
	fn add_assign(&mut self, offset: usize) {
		*self = VirtAddr::from(self.0 + offset as u64);
	}
}

impl ops::Sub for VirtAddr {
	type Output = VirtAddr;

	fn sub(self, rhs: VirtAddr) -> Self::Output {
		VirtAddr::from(self.0 - rhs.0)
	}
}

impl ops::Sub<u64> for VirtAddr {
	type Output = VirtAddr;

	fn sub(self, rhs: u64) -> Self::Output {
		VirtAddr::from(self.0 - rhs)
	}
}

impl ops::Sub<usize> for VirtAddr {
	type Output = VirtAddr;

	fn sub(self, rhs: usize) -> Self::Output {
		VirtAddr::from(self.0 - rhs as u64)
	}
}

impl ops::Rem for VirtAddr {
	type Output = VirtAddr;

	fn rem(self, rhs: VirtAddr) -> Self::Output {
		VirtAddr(self.0 % rhs.0)
	}
}

impl ops::Rem<u64> for VirtAddr {
	type Output = u64;

	fn rem(self, rhs: Self::Output) -> Self::Output {
		self.0 % rhs
	}
}

impl ops::Rem<usize> for VirtAddr {
	type Output = usize;

	fn rem(self, rhs: Self::Output) -> Self::Output {
		self.as_usize() % rhs
	}
}

impl ops::BitAnd for VirtAddr {
	type Output = Self;

	fn bitand(self, rhs: Self) -> Self::Output {
		VirtAddr(self.0 & rhs.0)
	}
}

impl ops::BitAnd<u64> for VirtAddr {
	type Output = VirtAddr;

	fn bitand(self, rhs: u64) -> Self::Output {
		VirtAddr(self.0 & rhs)
	}
}

impl ops::BitAnd<usize> for VirtAddr {
	type Output = VirtAddr;

	fn bitand(self, rhs: usize) -> Self::Output {
		VirtAddr(self.0 & rhs as u64)
	}
}

impl ops::BitAnd<i32> for VirtAddr {
	type Output = VirtAddr;

	fn bitand(self, rhs: i32) -> Self::Output {
		VirtAddr(self.0 & rhs as u64)
	}
}

impl ops::BitOr for VirtAddr {
	type Output = VirtAddr;

	fn bitor(self, rhs: VirtAddr) -> VirtAddr {
		VirtAddr(self.0 | rhs.0)
	}
}

impl ops::BitOr<u64> for VirtAddr {
	type Output = VirtAddr;

	fn bitor(self, rhs: u64) -> Self::Output {
		VirtAddr(self.0 | rhs)
	}
}

impl ops::BitOr<usize> for VirtAddr {
	type Output = VirtAddr;

	fn bitor(self, rhs: usize) -> Self::Output {
		VirtAddr(self.0 | rhs as u64)
	}
}

impl ops::Shr<u64> for VirtAddr {
	type Output = u64;

	fn shr(self, rhs: u64) -> Self::Output {
		self.0 >> rhs
	}
}

impl ops::Shr<usize> for VirtAddr {
	type Output = u64;

	fn shr(self, rhs: usize) -> Self::Output {
		self.0 >> rhs as u64
	}
}

impl ops::Shr<i32> for VirtAddr {
	type Output = u64;

	fn shr(self, rhs: i32) -> Self::Output {
		self.0 >> rhs as u64
	}
}

impl fmt::Binary for VirtAddr {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		self.0.fmt(f)
	}
}

impl fmt::Display for VirtAddr {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "{:#x}", self.0)
	}
}

impl fmt::Debug for VirtAddr {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "{:#x}", self.0)
	}
}

impl fmt::LowerHex for VirtAddr {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		self.0.fmt(f)
	}
}

impl fmt::Octal for VirtAddr {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		self.0.fmt(f)
	}
}

impl fmt::UpperHex for VirtAddr {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		self.0.fmt(f)
	}
}

impl fmt::Pointer for VirtAddr {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		use core::fmt::LowerHex;
		self.0.fmt(f)
	}
}

impl Hash for VirtAddr {
	fn hash<H: Hasher>(&self, state: &mut H) {
		self.0.hash(state);
	}
}

/// Log2 of base page size (12 bits).
pub const BASE_PAGE_SHIFT: usize = 12;

/// Size of a base page (4 KiB)
pub const BASE_PAGE_SIZE: usize = 4096;

/// Size of a mega page (2 MiB)
pub const MEGA_PAGE_SIZE: usize = 1024 * 1024 * 2;

/// Size of a giga page (1 GiB)
pub const GIGA_PAGE_SIZE: usize = 1024 * 1024 * 1024;

/// Size of a tera page (512 GiB)
pub const TERA_PAGE_SIZE: usize = 1024 * 1024 * 1024 * 512;
