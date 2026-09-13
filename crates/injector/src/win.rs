// Win32 imports. With raw-dylib rustc generates the import stubs itself, so no import
// libraries or SDK are needed.

#![allow(dead_code, clippy::upper_case_acronyms)]

use core::ffi::c_void;

pub type HANDLE = *mut c_void;
pub type HMODULE = *mut c_void;
pub type BOOL = i32;
pub type FARPROC = *const c_void;

pub const STD_OUTPUT_HANDLE: u32 = 0xFFFF_FFF5; // (DWORD)-11
pub const CREATE_SUSPENDED: u32 = 0x0000_0004;
pub const MEM_COMMIT: u32 = 0x1000;
pub const MEM_RESERVE: u32 = 0x2000;
pub const MEM_RELEASE: u32 = 0x8000;
pub const PAGE_READWRITE: u32 = 0x04;
pub const INFINITE: u32 = 0xFFFF_FFFF;
pub const WAIT_OBJECT_0: u32 = 0;
pub const STILL_ACTIVE: u32 = 259;
pub const FORMAT_MESSAGE_FROM_SYSTEM: u32 = 0x1000;
pub const FORMAT_MESSAGE_IGNORE_INSERTS: u32 = 0x200;
pub const ERROR_MOD_NOT_FOUND: u32 = 126;
pub const ERROR_TIMEOUT: u32 = 1460;

#[repr(C)]
pub struct STARTUPINFOW {
    pub cb: u32,
    pub lpReserved: *mut u16,
    pub lpDesktop: *mut u16,
    pub lpTitle: *mut u16,
    pub dwX: u32,
    pub dwY: u32,
    pub dwXSize: u32,
    pub dwYSize: u32,
    pub dwXCountChars: u32,
    pub dwYCountChars: u32,
    pub dwFillAttribute: u32,
    pub dwFlags: u32,
    pub wShowWindow: u16,
    pub cbReserved2: u16,
    pub lpReserved2: *mut u8,
    pub hStdInput: HANDLE,
    pub hStdOutput: HANDLE,
    pub hStdError: HANDLE,
}

impl STARTUPINFOW {
    pub const fn zeroed() -> Self {
        unsafe { core::mem::zeroed() }
    }
}

#[repr(C)]
pub struct PROCESS_INFORMATION {
    pub hProcess: HANDLE,
    pub hThread: HANDLE,
    pub dwProcessId: u32,
    pub dwThreadId: u32,
}

impl PROCESS_INFORMATION {
    pub const fn zeroed() -> Self {
        unsafe { core::mem::zeroed() }
    }
}

#[repr(C)]
pub struct SECURITY_ATTRIBUTES {
    pub nLength: u32,
    pub lpSecurityDescriptor: *mut c_void,
    pub bInheritHandle: BOOL,
}

#[link(name = "kernel32", kind = "raw-dylib", import_name_type = "undecorated")]
unsafe extern "system" {
    pub fn GetCommandLineW() -> *const u16;
    pub fn GetStdHandle(nStdHandle: u32) -> HANDLE;
    pub fn WriteFile(h: HANDLE, buf: *const u8, len: u32, written: *mut u32, overlapped: *mut c_void) -> BOOL;
    pub fn CreateProcessW(
        app: *const u16,
        cmdline: *mut u16,
        proc_attr: *mut SECURITY_ATTRIBUTES,
        thread_attr: *mut SECURITY_ATTRIBUTES,
        inherit: BOOL,
        flags: u32,
        env: *mut c_void,
        cwd: *const u16,
        si: *const STARTUPINFOW,
        pi: *mut PROCESS_INFORMATION,
    ) -> BOOL;
    pub fn VirtualAllocEx(h: HANDLE, addr: *mut c_void, size: usize, alloc_type: u32, protect: u32) -> *mut c_void;
    pub fn VirtualFreeEx(h: HANDLE, addr: *mut c_void, size: usize, free_type: u32) -> BOOL;
    pub fn WriteProcessMemory(h: HANDLE, addr: *mut c_void, buf: *const u8, size: usize, written: *mut usize) -> BOOL;
    pub fn GetModuleHandleW(name: *const u16) -> HMODULE;
    pub fn GetProcAddress(module: HMODULE, name: *const u8) -> FARPROC;
    pub fn CreateRemoteThread(
        h: HANDLE,
        attr: *mut SECURITY_ATTRIBUTES,
        stack: usize,
        start: FARPROC,
        param: *mut c_void,
        flags: u32,
        thread_id: *mut u32,
    ) -> HANDLE;
    pub fn WaitForSingleObject(h: HANDLE, ms: u32) -> u32;
    pub fn GetExitCodeThread(h: HANDLE, code: *mut u32) -> BOOL;
    pub fn GetExitCodeProcess(h: HANDLE, code: *mut u32) -> BOOL;
    pub fn ResumeThread(h: HANDLE) -> u32;
    pub fn TerminateProcess(h: HANDLE, code: u32) -> BOOL;
    pub fn CloseHandle(h: HANDLE) -> BOOL;
    pub fn GetLastError() -> u32;
    pub fn Sleep(ms: u32);
    pub fn ExitProcess(code: u32) -> !;
    pub fn FormatMessageW(
        flags: u32,
        source: *const c_void,
        message_id: u32,
        language_id: u32,
        buffer: *mut u16,
        size: u32,
        args: *mut c_void,
    ) -> u32;
    pub fn K32EnumProcessModules(h: HANDLE, modules: *mut HMODULE, cb: u32, needed: *mut u32) -> BOOL;
    pub fn K32GetModuleBaseNameW(h: HANDLE, module: HMODULE, name: *mut u16, size: u32) -> u32;
}

#[link(name = "shell32", kind = "raw-dylib", import_name_type = "undecorated")]
unsafe extern "system" {
    pub fn CommandLineToArgvW(cmdline: *const u16, argc: *mut i32) -> *mut *mut u16;
}

// nul-terminated utf-16 literal
macro_rules! w {
    ($s:literal) => {{
        const S: &str = $s;
        const N: usize = S.len() + 1;
        const W: [u16; N] = {
            let mut out = [0u16; N];
            let b = S.as_bytes();
            let mut i = 0;
            while i < b.len() {
                out[i] = b[i] as u16;
                i += 1;
            }
            out
        };
        W
    }};
}
pub(crate) use w;
