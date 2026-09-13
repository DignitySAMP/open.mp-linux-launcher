// Stand-in for samp.dll in the Wine test. Writes its module path to omp-tui-dll-loaded.txt
// in the working directory when loaded.

#![no_std]
#![allow(clippy::not_unsafe_ptr_arg_deref)]

#[path = "common.rs"]
mod common;

use common::*;

static mut MODPATH: [u16; 1024] = [0; 1024];

#[unsafe(no_mangle)]
pub extern "system" fn DllMain(module: HMODULE, reason: u32, _reserved: *mut core::ffi::c_void) -> BOOL {
    if reason == DLL_PROCESS_ATTACH {
        unsafe {
            let p = (&raw mut MODPATH).cast::<u16>();
            GetModuleFileNameW(module, p, 1000);
            write_marker("omp-tui-dll-loaded.txt", p);
        }
    }
    1
}
