//! System error numbers.

use num_enum::{IntoPrimitive, TryFromPrimitive};
use thiserror::Error;

/// The error type for I/O operations and system calls.
///
/// The values of these error numbers are the same as in Linux.
/// See [`asm-generic/errno-base.h`] and [`asm-generic/errno.h`] for details.
///
/// [`asm-generic/errno-base.h`]: https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/tree/include/uapi/asm-generic/errno-base.h?h=v6.15
/// [`asm-generic/errno.h`]: https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/tree/include/uapi/asm-generic/errno.h?h=v6.15
#[derive(Error, TryFromPrimitive, IntoPrimitive, PartialEq, Eq, Clone, Copy, Debug)]
#[repr(i32)]
pub enum Errno {
	/// Operation not permitted
	#[doc(alias = "EPERM")]
	#[error("Operation not permitted")]
	Perm = 1,

	/// No such file or directory
	#[doc(alias = "ENOENT")]
	#[error("No such file or directory")]
	Noent = 2,

	/// No such process
	#[doc(alias = "ESRCH")]
	#[error("No such process")]
	Srch = 3,

	/// Interrupted system call
	#[doc(alias = "EINTR")]
	#[error("Interrupted system call")]
	Intr = 4,

	/// I/O error
	#[doc(alias = "EIO")]
	#[error("I/O error")]
	Io = 5,

	/// No such device or address
	#[doc(alias = "ENXIO")]
	#[error("No such device or address")]
	Nxio = 6,

	/// Argument list too long
	#[doc(alias = "E2BIG")]
	#[error("Argument list too long")]
	Toobig = 7,

	/// Exec format error
	#[doc(alias = "ENOEXEC")]
	#[error("Exec format error")]
	Noexec = 8,

	/// Bad file number
	#[doc(alias = "EBADF")]
	#[error("Bad file number")]
	Badf = 9,

	/// No child processes
	#[doc(alias = "ECHILD")]
	#[error("No child processes")]
	Child = 10,

	/// Try again
	#[doc(alias = "EAGAIN")]
	#[error("Try again")]
	Again = 11,

	/// Out of memory
	#[doc(alias = "ENOMEM")]
	#[error("Out of memory")]
	Nomem = 12,

	/// Permission denied
	#[doc(alias = "EACCES")]
	#[error("Permission denied")]
	Acces = 13,

	/// Bad address
	#[doc(alias = "EFAULT")]
	#[error("Bad address")]
	Fault = 14,

	/// Block device required
	#[doc(alias = "ENOTBLK")]
	#[error("Block device required")]
	Notblk = 15,

	/// Device or resource busy
	#[doc(alias = "EBUSY")]
	#[error("Device or resource busy")]
	Busy = 16,

	/// File exists
	#[doc(alias = "EEXIST")]
	#[error("File exists")]
	Exist = 17,

	/// Cross-device link
	#[doc(alias = "EXDEV")]
	#[error("Cross-device link")]
	Xdev = 18,

	/// No such device
	#[doc(alias = "ENODEV")]
	#[error("No such device")]
	Nodev = 19,

	/// Not a directory
	#[doc(alias = "ENOTDIR")]
	#[error("Not a directory")]
	Notdir = 20,

	/// Is a directory
	#[doc(alias = "EISDIR")]
	#[error("Is a directory")]
	Isdir = 21,

	/// Invalid argument
	#[doc(alias = "EINVAL")]
	#[error("Invalid argument")]
	Inval = 22,

	/// File table overflow
	#[doc(alias = "ENFILE")]
	#[error("File table overflow")]
	Nfile = 23,

	/// Too many open files
	#[doc(alias = "EMFILE")]
	#[error("Too many open files")]
	Mfile = 24,

	/// Not a typewriter
	#[doc(alias = "ENOTTY")]
	#[error("Not a typewriter")]
	Notty = 25,

	/// Text file busy
	#[doc(alias = "ETXTBSY")]
	#[error("Text file busy")]
	Txtbsy = 26,

	/// File too large
	#[doc(alias = "EFBIG")]
	#[error("File too large")]
	Fbig = 27,

	/// No space left on device
	#[doc(alias = "ENOSPC")]
	#[error("No space left on device")]
	Nospc = 28,

	/// Illegal seek
	#[doc(alias = "ESPIPE")]
	#[error("Illegal seek")]
	Spipe = 29,

	/// Read-only file system
	#[doc(alias = "EROFS")]
	#[error("Read-only file system")]
	Rofs = 30,

	/// Too many links
	#[doc(alias = "EMLINK")]
	#[error("Too many links")]
	Mlink = 31,

	/// Broken pipe
	#[doc(alias = "EPIPE")]
	#[error("Broken pipe")]
	Pipe = 32,

	/// Math argument out of domain of func
	#[doc(alias = "EDOM")]
	#[error("Math argument out of domain of func")]
	Dom = 33,

	/// Math result not representable
	#[doc(alias = "ERANGE")]
	#[error("Math result not representable")]
	Range = 34,

	/// Resource deadlock would occur
	#[doc(alias = "EDEADLK")]
	#[error("Resource deadlock would occur")]
	Deadlk = 35,

	/// File name too long
	#[doc(alias = "ENAMETOOLONG")]
	#[error("File name too long")]
	Nametoolong = 36,

	/// No record locks available
	#[doc(alias = "ENOLCK")]
	#[error("No record locks available")]
	Nolck = 37,

	/// Invalid system call number
	#[doc(alias = "ENOSYS")]
	#[error("Invalid system call number")]
	Nosys = 38,

	/// Directory not empty
	#[doc(alias = "ENOTEMPTY")]
	#[error("Directory not empty")]
	Notempty = 39,

	/// Too many symbolic links encountered
	#[doc(alias = "ELOOP")]
	#[error("Too many symbolic links encountered")]
	Loop = 40,

	/// No message of desired type
	#[doc(alias = "ENOMSG")]
	#[error("No message of desired type")]
	Nomsg = 42,

	/// Identifier removed
	#[doc(alias = "EIDRM")]
	#[error("Identifier removed")]
	Idrm = 43,

	/// Channel number out of range
	#[doc(alias = "ECHRNG")]
	#[error("Channel number out of range")]
	Chrng = 44,

	/// Level 2 not synchronized
	#[doc(alias = "EL2NSYNC")]
	#[error("Level 2 not synchronized")]
	L2nsync = 45,

	/// Level 3 halted
	#[doc(alias = "EL3HLT")]
	#[error("Level 3 halted")]
	L3hlt = 46,

	/// Level 3 reset
	#[doc(alias = "EL3RST")]
	#[error("Level 3 reset")]
	L3rst = 47,

	/// Link number out of range
	#[doc(alias = "ELNRNG")]
	#[error("Link number out of range")]
	Lnrng = 48,

	/// Protocol driver not attached
	#[doc(alias = "EUNATCH")]
	#[error("Protocol driver not attached")]
	Unatch = 49,

	/// No CSI structure available
	#[doc(alias = "ENOCSI")]
	#[error("No CSI structure available")]
	Nocsi = 50,

	/// Level 2 halted
	#[doc(alias = "EL2HLT")]
	#[error("Level 2 halted")]
	L2hlt = 51,

	/// Invalid exchange
	#[doc(alias = "EBADE")]
	#[error("Invalid exchange")]
	Bade = 52,

	/// Invalid request descriptor
	#[doc(alias = "EBADR")]
	#[error("Invalid request descriptor")]
	Badr = 53,

	/// Exchange full
	#[doc(alias = "EXFULL")]
	#[error("Exchange full")]
	Xfull = 54,

	/// No anode
	#[doc(alias = "ENOANO")]
	#[error("No anode")]
	Noano = 55,

	/// Invalid request code
	#[doc(alias = "EBADRQC")]
	#[error("Invalid request code")]
	Badrqc = 56,

	/// Invalid slot
	#[doc(alias = "EBADSLT")]
	#[error("Invalid slot")]
	Badslt = 57,

	/// Bad font file format
	#[doc(alias = "EBFONT")]
	#[error("Bad font file format")]
	Bfont = 59,

	/// Device not a stream
	#[doc(alias = "ENOSTR")]
	#[error("Device not a stream")]
	Nostr = 60,

	/// No data available
	#[doc(alias = "ENODATA")]
	#[error("No data available")]
	Nodata = 61,

	/// Timer expired
	#[doc(alias = "ETIME")]
	#[error("Timer expired")]
	Time = 62,

	/// Out of streams resources
	#[doc(alias = "ENOSR")]
	#[error("Out of streams resources")]
	Nosr = 63,

	/// Machine is not on the network
	#[doc(alias = "ENONET")]
	#[error("Machine is not on the network")]
	Nonet = 64,

	/// Package not installed
	#[doc(alias = "ENOPKG")]
	#[error("Package not installed")]
	Nopkg = 65,

	/// Object is remote
	#[doc(alias = "EREMOTE")]
	#[error("Object is remote")]
	Remote = 66,

	/// Link has been severed
	#[doc(alias = "ENOLINK")]
	#[error("Link has been severed")]
	Nolink = 67,

	/// Advertise error
	#[doc(alias = "EADV")]
	#[error("Advertise error")]
	Adv = 68,

	/// Srmount error
	#[doc(alias = "ESRMNT")]
	#[error("Srmount error")]
	Srmnt = 69,

	/// Communication error on send
	#[doc(alias = "ECOMM")]
	#[error("Communication error on send")]
	Comm = 70,

	/// Protocol error
	#[doc(alias = "EPROTO")]
	#[error("Protocol error")]
	Proto = 71,

	/// Multihop attempted
	#[doc(alias = "EMULTIHOP")]
	#[error("Multihop attempted")]
	Multihop = 72,

	/// RFS specific error
	#[doc(alias = "EDOTDOT")]
	#[error("RFS specific error")]
	Dotdot = 73,

	/// Not a data message
	#[doc(alias = "EBADMSG")]
	#[error("Not a data message")]
	Badmsg = 74,

	/// Value too large for defined data type
	#[doc(alias = "EOVERFLOW")]
	#[error("Value too large for defined data type")]
	Overflow = 75,

	/// Name not unique on network
	#[doc(alias = "ENOTUNIQ")]
	#[error("Name not unique on network")]
	Notuniq = 76,

	/// File descriptor in bad state
	#[doc(alias = "EBADFD")]
	#[error("File descriptor in bad state")]
	Badfd = 77,

	/// Remote address changed
	#[doc(alias = "EREMCHG")]
	#[error("Remote address changed")]
	Remchg = 78,

	/// Can not access a needed shared library
	#[doc(alias = "ELIBACC")]
	#[error("Can not access a needed shared library")]
	Libacc = 79,

	/// Accessing a corrupted shared library
	#[doc(alias = "ELIBBAD")]
	#[error("Accessing a corrupted shared library")]
	Libbad = 80,

	/// .lib section in a.out corrupted
	#[doc(alias = "ELIBSCN")]
	#[error(".lib section in a.out corrupted")]
	Libscn = 81,

	/// Attempting to link in too many shared libraries
	#[doc(alias = "ELIBMAX")]
	#[error("Attempting to link in too many shared libraries")]
	Libmax = 82,

	/// Cannot exec a shared library directly
	#[doc(alias = "ELIBEXEC")]
	#[error("Cannot exec a shared library directly")]
	Libexec = 83,

	/// Illegal byte sequence
	#[doc(alias = "EILSEQ")]
	#[error("Illegal byte sequence")]
	Ilseq = 84,

	/// Interrupted system call should be restarted
	#[doc(alias = "ERESTART")]
	#[error("Interrupted system call should be restarted")]
	Restart = 85,

	/// Streams pipe error
	#[doc(alias = "ESTRPIPE")]
	#[error("Streams pipe error")]
	Strpipe = 86,

	/// Too many users
	#[doc(alias = "EUSERS")]
	#[error("Too many users")]
	Users = 87,

	/// Socket operation on non-socket
	#[doc(alias = "ENOTSOCK")]
	#[error("Socket operation on non-socket")]
	Notsock = 88,

	/// Destination address required
	#[doc(alias = "EDESTADDRREQ")]
	#[error("Destination address required")]
	Destaddrreq = 89,

	/// Message too long
	#[doc(alias = "EMSGSIZE")]
	#[error("Message too long")]
	Msgsize = 90,

	/// Protocol wrong type for socket
	#[doc(alias = "EPROTOTYPE")]
	#[error("Protocol wrong type for socket")]
	Prototype = 91,

	/// Protocol not available
	#[doc(alias = "ENOPROTOOPT")]
	#[error("Protocol not available")]
	Noprotoopt = 92,

	/// Protocol not supported
	#[doc(alias = "EPROTONOSUPPORT")]
	#[error("Protocol not supported")]
	Protonosupport = 93,

	/// Socket type not supported
	#[doc(alias = "ESOCKTNOSUPPORT")]
	#[error("Socket type not supported")]
	Socktnosupport = 94,

	/// Operation not supported on transport endpoint
	#[doc(alias = "EOPNOTSUPP")]
	#[error("Operation not supported on transport endpoint")]
	Opnotsupp = 95,

	/// Protocol family not supported
	#[doc(alias = "EPFNOSUPPORT")]
	#[error("Protocol family not supported")]
	Pfnosupport = 96,

	/// Address family not supported by protocol
	#[doc(alias = "EAFNOSUPPORT")]
	#[error("Address family not supported by protocol")]
	Afnosupport = 97,

	/// Address already in use
	#[doc(alias = "EADDRINUSE")]
	#[error("Address already in use")]
	Addrinuse = 98,

	/// Cannot assign requested address
	#[doc(alias = "EADDRNOTAVAIL")]
	#[error("Cannot assign requested address")]
	Addrnotavail = 99,

	/// Network is down
	#[doc(alias = "ENETDOWN")]
	#[error("Network is down")]
	Netdown = 100,

	/// Network is unreachable
	#[doc(alias = "ENETUNREACH")]
	#[error("Network is unreachable")]
	Netunreach = 101,

	/// Network dropped connection because of reset
	#[doc(alias = "ENETRESET")]
	#[error("Network dropped connection because of reset")]
	Netreset = 102,

	/// Software caused connection abort
	#[doc(alias = "ECONNABORTED")]
	#[error("Software caused connection abort")]
	Connaborted = 103,

	/// Connection reset by peer
	#[doc(alias = "ECONNRESET")]
	#[error("Connection reset by peer")]
	Connreset = 104,

	/// No buffer space available
	#[doc(alias = "ENOBUFS")]
	#[error("No buffer space available")]
	Nobufs = 105,

	/// Transport endpoint is already connected
	#[doc(alias = "EISCONN")]
	#[error("Transport endpoint is already connected")]
	Isconn = 106,

	/// Transport endpoint is not connected
	#[doc(alias = "ENOTCONN")]
	#[error("Transport endpoint is not connected")]
	Notconn = 107,

	/// Cannot send after transport endpoint shutdown
	#[doc(alias = "ESHUTDOWN")]
	#[error("Cannot send after transport endpoint shutdown")]
	Shutdown = 108,

	/// Too many references: cannot splice
	#[doc(alias = "ETOOMANYREFS")]
	#[error("Too many references: cannot splice")]
	Toomanyrefs = 109,

	/// Connection timed out
	#[doc(alias = "ETIMEDOUT")]
	#[error("Connection timed out")]
	Timedout = 110,

	/// Connection refused
	#[doc(alias = "ECONNREFUSED")]
	#[error("Connection refused")]
	Connrefused = 111,

	/// Host is down
	#[doc(alias = "EHOSTDOWN")]
	#[error("Host is down")]
	Hostdown = 112,

	/// No route to host
	#[doc(alias = "EHOSTUNREACH")]
	#[error("No route to host")]
	Hostunreach = 113,

	/// Operation already in progress
	#[doc(alias = "EALREADY")]
	#[error("Operation already in progress")]
	Already = 114,

	/// Operation now in progress
	#[doc(alias = "EINPROGRESS")]
	#[error("Operation now in progress")]
	Inprogress = 115,

	/// Stale file handle
	#[doc(alias = "ESTALE")]
	#[error("Stale file handle")]
	Stale = 116,

	/// Structure needs cleaning
	#[doc(alias = "EUCLEAN")]
	#[error("Structure needs cleaning")]
	Uclean = 117,

	/// Not a XENIX named type file
	#[doc(alias = "ENOTNAM")]
	#[error("Not a XENIX named type file")]
	Notnam = 118,

	/// No XENIX semaphores available
	#[doc(alias = "ENAVAIL")]
	#[error("No XENIX semaphores available")]
	Navail = 119,

	/// Is a named type file
	#[doc(alias = "EISNAM")]
	#[error("Is a named type file")]
	Isnam = 120,

	/// Remote I/O error
	#[doc(alias = "EREMOTEIO")]
	#[error("Remote I/O error")]
	Remoteio = 121,

	/// Quota exceeded
	#[doc(alias = "EDQUOT")]
	#[error("Quota exceeded")]
	Dquot = 122,

	/// No medium found
	#[doc(alias = "ENOMEDIUM")]
	#[error("No medium found")]
	Nomedium = 123,

	/// Wrong medium type
	#[doc(alias = "EMEDIUMTYPE")]
	#[error("Wrong medium type")]
	Mediumtype = 124,

	/// Operation Canceled
	#[doc(alias = "ECANCELED")]
	#[error("Operation Canceled")]
	Canceled = 125,

	/// Required key not available
	#[doc(alias = "ENOKEY")]
	#[error("Required key not available")]
	Nokey = 126,

	/// Key has expired
	#[doc(alias = "EKEYEXPIRED")]
	#[error("Key has expired")]
	Keyexpired = 127,

	/// Key has been revoked
	#[doc(alias = "EKEYREVOKED")]
	#[error("Key has been revoked")]
	Keyrevoked = 128,

	/// Key was rejected by service
	#[doc(alias = "EKEYREJECTED")]
	#[error("Key was rejected by service")]
	Keyrejected = 129,

	/// Owner died
	#[doc(alias = "EOWNERDEAD")]
	#[error("Owner died")]
	Ownerdead = 130,

	/// State not recoverable
	#[doc(alias = "ENOTRECOVERABLE")]
	#[error("State not recoverable")]
	Notrecoverable = 131,

	/// Operation not possible due to RF-kill
	#[doc(alias = "ERFKILL")]
	#[error("Operation not possible due to RF-kill")]
	Rfkill = 132,

	/// Memory page has hardware error
	#[doc(alias = "EHWPOISON")]
	#[error("Memory page has hardware error")]
	Hwpoison = 133,
}

/// Returns the pointer to `errno`.
#[cfg(all(
	not(any(feature = "common-os", feature = "nostd")),
	not(target_arch = "riscv64"),
))]
#[unsafe(no_mangle)]
#[linkage = "weak"]
pub extern "C" fn sys_errno_location() -> *mut i32 {
	use core::cell::UnsafeCell;

	#[thread_local]
	static ERRNO: UnsafeCell<i32> = UnsafeCell::new(0);

	ERRNO.get()
}

/// Get the error number from the thread local storage
///
/// Soft-deprecated in favor of using `sys_errno_location`.
#[cfg(not(feature = "nostd"))]
#[unsafe(no_mangle)]
pub extern "C" fn sys_get_errno() -> i32 {
	sys_errno()
}

/// Get the error number from the thread local storage
///
/// Soft-deprecated in favor of using `sys_errno_location`.
#[cfg(not(feature = "nostd"))]
#[unsafe(no_mangle)]
pub extern "C" fn sys_errno() -> i32 {
	cfg_if::cfg_if! {
		if #[cfg(any(feature = "common-os", target_arch = "riscv64"))] {
			0
		} else {
			unsafe { *sys_errno_location() }
		}
	}
}

pub(crate) trait ToErrno {
	fn to_errno(&self) -> Option<i32> {
		None
	}

	fn set_errno(self) -> Self
	where
		Self: Sized,
	{
		if let Some(errno) = self.to_errno() {
			cfg_if::cfg_if! {
				if #[cfg(any(feature = "common-os", feature = "nostd", target_arch = "riscv64"))] {
					let _ = errno;
				} else {
					unsafe {
						*sys_errno_location() = errno;
					}
				}
			}
		}
		self
	}
}

impl ToErrno for i32 {
	fn to_errno(&self) -> Option<i32> {
		(*self < 0).then_some(-self)
	}
}

impl ToErrno for i64 {
	fn to_errno(&self) -> Option<i32> {
		(*self < 0).then(|| i32::try_from(-self).unwrap())
	}
}

impl ToErrno for isize {
	fn to_errno(&self) -> Option<i32> {
		(*self < 0).then(|| i32::try_from(-self).unwrap())
	}
}

impl ToErrno for Errno {
	fn to_errno(&self) -> Option<i32> {
		Some(i32::from(*self))
	}
}

impl ToErrno for u8 {}
impl ToErrno for u16 {}
impl ToErrno for u32 {}
impl ToErrno for usize {}
impl ToErrno for *mut u8 {}
impl ToErrno for () {}
impl ToErrno for ! {}
