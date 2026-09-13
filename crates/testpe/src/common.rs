// Shared by the bin and the cdylib through #[path], a bin cannot depend on a cdylib.

#![allow(dead_code, non_snake_case, non_upper_case_globals, clippy::upper_case_acronyms, clippy::not_unsafe_ptr_arg_deref)]

use core::ffi::c_void;
use core::panic::PanicInfo;

pub type HANDLE = *mut c_void;
pub type HMODULE = *mut c_void;
pub type BOOL = i32;

pub const GENERIC_WRITE: u32 = 0x4000_0000;
pub const CREATE_ALWAYS: u32 = 2;
pub const FILE_ATTRIBUTE_NORMAL: u32 = 0x80;
pub const INVALID_HANDLE_VALUE: HANDLE = -1isize as HANDLE;
pub const DLL_PROCESS_ATTACH: u32 = 1;

#[link(name = "kernel32", kind = "raw-dylib", import_name_type = "undecorated")]
unsafe extern "system" {
    pub fn GetCommandLineW() -> *const u16;
    pub fn GetCurrentDirectoryW(len: u32, buf: *mut u16) -> u32;
    pub fn GetModuleFileNameW(module: HMODULE, buf: *mut u16, len: u32) -> u32;
    pub fn CreateFileW(
        name: *const u16,
        access: u32,
        share: u32,
        sec: *mut c_void,
        disposition: u32,
        flags: u32,
        template: HANDLE,
    ) -> HANDLE;
    pub fn WriteFile(h: HANDLE, buf: *const u8, len: u32, written: *mut u32, overlapped: *mut c_void) -> BOOL;
    pub fn CloseHandle(h: HANDLE) -> BOOL;
    pub fn Sleep(ms: u32);
    pub fn ExitProcess(code: u32) -> !;
}

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    unsafe { ExitProcess(101) }
}

#[unsafe(no_mangle)]
pub static _fltused: i32 = 0;

#[unsafe(no_mangle)]
pub extern "C" fn __CxxFrameHandler3() -> ! {
    unsafe { ExitProcess(102) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memcpy(dst: *mut c_void, src: *const c_void, n: usize) -> *mut c_void {
    let (d, s) = (dst as *mut u8, src as *const u8);
    let mut i = 0;
    while i < n {
        unsafe { *d.add(i) = *s.add(i) };
        i += 1;
    }
    dst
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memmove(dst: *mut c_void, src: *const c_void, n: usize) -> *mut c_void {
    let (d, s) = (dst as *mut u8, src as *const u8);
    if (d as usize) < (s as usize) {
        let mut i = 0;
        while i < n {
            unsafe { *d.add(i) = *s.add(i) };
            i += 1;
        }
    } else {
        let mut i = n;
        while i > 0 {
            i -= 1;
            unsafe { *d.add(i) = *s.add(i) };
        }
    }
    dst
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memset(dst: *mut c_void, c: i32, n: usize) -> *mut c_void {
    let d = dst as *mut u8;
    let mut i = 0;
    while i < n {
        unsafe { *d.add(i) = c as u8 };
        i += 1;
    }
    dst
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memcmp(a: *const c_void, b: *const c_void, n: usize) -> i32 {
    let (a, b) = (a as *const u8, b as *const u8);
    let mut i = 0;
    while i < n {
        let (x, y) = unsafe { (*a.add(i), *b.add(i)) };
        if x != y {
            return x as i32 - y as i32;
        }
        i += 1;
    }
    0
}

pub unsafe fn wlen(p: *const u16) -> usize {
    let mut n = 0;
    while unsafe { *p.add(n) } != 0 {
        n += 1;
    }
    n
}

static mut PATH: [u16; 1024] = [0; 1024];
static mut BYTES: [u8; 4096] = [0; 4096];

pub unsafe fn write_marker(name: &str, wide: *const u16) -> bool {
    unsafe {
        let path = (&raw mut PATH).cast::<u16>();
        let n = GetCurrentDirectoryW(900, path) as usize;
        if n == 0 {
            return false;
        }
        let mut p = n;
        *path.add(p) = b'\\' as u16;
        p += 1;
        for &c in name.as_bytes() {
            *path.add(p) = c as u16;
            p += 1;
        }
        *path.add(p) = 0;
        let h = CreateFileW(
            path,
            GENERIC_WRITE,
            0,
            core::ptr::null_mut(),
            CREATE_ALWAYS,
            FILE_ATTRIBUTE_NORMAL,
            core::ptr::null_mut(),
        );
        if h == INVALID_HANDLE_VALUE {
            return false;
        }
        let bytes = (&raw mut BYTES).cast::<u8>();
        let len = wlen(wide).min(4000);
        let mut out = 0usize;
        for i in 0..len {
            let c = *wide.add(i);
            let mut buf = [0u8; 4];
            let s = char::from_u32(c as u32).unwrap_or('?').encode_utf8(&mut buf);
            for &b in s.as_bytes() {
                *bytes.add(out) = b;
                out += 1;
            }
        }
        let mut written = 0u32;
        let ok = WriteFile(h, bytes, out as u32, &mut written, core::ptr::null_mut());
        CloseHandle(h);
        ok != 0
    }
}
