use core::ffi::c_void;
use core::fmt;
use core::ptr::NonNull;

use one_shot_mutex::OneShotMutex;
use uefi::boot::{EventType, Tpl};
use uefi::Event;

use crate::arch;

pub enum Console {
	None,
	BootServices,
	Native { console: arch::Console },
}

impl Console {
	const fn new() -> Self {
		Self::None
	}

	fn exit_boot_services(&mut self) {
		assert!(matches!(self, Self::BootServices { .. }));
		*self = Self::Native {
			console: arch::Console::default(),
		};
	}

	fn init(&mut self) {
		assert!(matches!(self, Console::None));
		unsafe {
			uefi::boot::create_event(
				EventType::SIGNAL_EXIT_BOOT_SERVICES,
				Tpl::NOTIFY,
				Some(exit_boot_services),
				None,
			)
			.unwrap();
		}
		*self = Console::BootServices;
	}
}

impl fmt::Write for Console {
	fn write_str(&mut self, s: &str) -> fmt::Result {
		match self {
			Console::None => {
				self.init();
				self.write_str(s)?;
			}
			Console::BootServices => uefi::system::with_stdout(|stdout| stdout.write_str(s))?,
			Console::Native { console } => console.write_bytes(s.as_bytes()),
		}
		Ok(())
	}
}

unsafe extern "efiapi" fn exit_boot_services(_e: Event, _ctx: Option<NonNull<c_void>>) {
	CONSOLE.lock().exit_boot_services();
}

pub static CONSOLE: OneShotMutex<Console> = OneShotMutex::new(Console::new());
