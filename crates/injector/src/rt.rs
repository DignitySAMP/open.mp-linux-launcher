use core::ffi::c_void;
use core::panic::PanicInfo;

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    unsafe { crate::win::ExitProcess(101) }
}

// MSVC codegen references this symbol whenever floats are used.
#[unsafe(no_mangle)]
pub static _fltused: i32 = 0;

// The prebuilt core has landing pads that reference the MSVC personality routine. With
// panic=abort it is never called.
#[unsafe(no_mangle)]
pub extern "C" fn __CxxFrameHandler3() -> ! {
    unsafe { crate::win::ExitProcess(102) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memcpy(dst: *mut c_void, src: *const c_void, n: usize) -> *mut c_void {
    let (dst, src) = (dst as *mut u8, src as *const u8);
    let mut i = 0;
    while i < n {
        unsafe { *dst.add(i) = *src.add(i) };
        i += 1;
    }
    dst as *mut c_void
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memmove(dst: *mut c_void, src: *const c_void, n: usize) -> *mut c_void {
    let (dst, src) = (dst as *mut u8, src as *const u8);
    if (dst as usize) < (src as usize) {
        let mut i = 0;
        while i < n {
            unsafe { *dst.add(i) = *src.add(i) };
            i += 1;
        }
    } else {
        let mut i = n;
        while i > 0 {
            i -= 1;
            unsafe { *dst.add(i) = *src.add(i) };
        }
    }
    dst as *mut c_void
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memset(dst: *mut c_void, c: i32, n: usize) -> *mut c_void {
    let dst = dst as *mut u8;
    let mut i = 0;
    while i < n {
        unsafe { *dst.add(i) = c as u8 };
        i += 1;
    }
    dst as *mut c_void
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
