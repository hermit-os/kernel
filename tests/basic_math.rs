#![no_std]
#![no_main]

extern crate hermit;
use hermit::{print, println};

// Workaround since the "real" runtime_entry function (defined in libstd) is not available,
// since the target-os is hermit-kernel and not hermit
#[no_mangle]
extern "C"
fn runtime_entry(argc: i32, argv: *const *const u8, _env: *const *const u8) -> ! {
    let res = main(argc as isize, argv);
    match res {
        Ok(_) => hermit::sys_exit(0),
        Err(_) => hermit::sys_exit(1),  //ToDo: sys_exit exitcode doesn't seem to get passed to qemu
        // sys_exit argument doesn't actually get used, gets silently dropped!
        // Maybe this is not possible on QEMU?
        // https://os.phil-opp.com/testing/#exiting-qemu device needed?
    }
}




/*
/// assert_eq but returns Result<(),&str> instead of panicking
/// no error message possible
/// adapted from libcore assert_eq macro
macro_rules! equals {
    ($left:expr, $right:expr) => ({
        match (&$left, &$right) {
            (left_val, right_val) => {
                if !(*left_val == *right_val) {
                    return Err(r#"assertion failed: `(left == right)`
  left: `{:?}`,
 right: `{:?}`"# &*left_val, &*right_val);
                }
                else { return Ok(()); }
            }
        }
    });
    ($left:expr, $right:expr,) => ({
        $crate::assert_eq!($left, $right)
    });
}

macro_rules! n_equals {
    ($left:expr, $right:expr) => ({
        match (&$left, &$right) {
            (left_val, right_val) => {
                if *left_val == *right_val {
                    // The reborrows below are intentional. Without them, the stack slot for the
                    // borrow is initialized even before the values are compared, leading to a
                    // noticeable slow down.
                    return Err(r#"assertion failed: `(left == right)`
  left: `{:?}`,
 right: `{:?}`"#, &*left_val, &*right_val);
                }
                else return Ok(());
            }
        }
    });
    ($left:expr, $right:expr,) => {
        $crate::assert_ne!($left, $right)
    };
}
*/

//ToDo - add a testrunner so we can group multiple similar tests

//ToDo - Idea: pass some values into main - compute and print result to stdout
//ToDo - add some kind of assert like macro that returns a result instead of panicking, Err contains line number etc to pinpoint the issue
pub fn main(_argc: isize, _argv: *const *const u8) -> Result<(), ()>{
    let x = 25;
    let y = 310;
    let z = x * y;
    println!("25 * 310 = {}", z);
    assert_eq!(z, 7750);
    Ok(())
}
