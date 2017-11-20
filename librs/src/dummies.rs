#[no_mangle]
pub extern "C" fn __page_map() { panic!("__page_map"); }

#[no_mangle]
pub extern "C" fn cpu_detection() { panic!("cpu_detection"); }

#[no_mangle]
pub extern "C" fn fpu_init() { panic!("fpu_init"); }

#[no_mangle]
pub extern "C" fn gdt_install() { panic!("gdt_install"); }

#[no_mangle]
pub extern "C" fn idt_install() { panic!("idt_install"); }

#[no_mangle]
pub extern "C" fn page_init() { panic!("page_init"); }

#[no_mangle]
pub extern "C" fn page_unmap() { panic!("page_unmap"); }

#[no_mangle]
pub extern "C" fn restore_fpu_state() { panic!("restore_fpu_state"); }

#[no_mangle]
pub extern "C" fn save_fpu_state() { panic!("save_fpu_state"); }

#[no_mangle]
pub extern "C" fn set_tss() { panic!("set_tss"); }

#[no_mangle]
pub extern "C" fn virt_to_phys() { panic!("virt_to_phys"); }
