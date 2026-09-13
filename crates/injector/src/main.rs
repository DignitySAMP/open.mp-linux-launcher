// Starts a program and injects DLLs into it.
//
//   omp-injector.exe --exe <path> --cwd <dir> [--dll <path>]... [--suspended] [--wait]
//                    [--wait-module <name>] [--retries N] [--delay MS] -- <args...>
//
// Progress is printed as one JSON object per line. Exit codes: 2 bad arguments, 3 the process
// could not be started, 4 injection failed. Built without std or a C runtime, so rustc and
// lld-link are enough.

#![no_std]
#![no_main]
#![allow(non_snake_case, non_upper_case_globals, clippy::missing_safety_doc, clippy::upper_case_acronyms)]

mod rt;
mod win;

use core::ptr::{null, null_mut};
use win::*;

const MAX_DLLS: usize = 8;
const MAX_ARGS: usize = 64;
const CMDLINE_CAP: usize = 8192;
const OUT_CAP: usize = 4096;

struct Opts {
    exe: *const u16,
    cwd: *const u16,
    dlls: [*const u16; MAX_DLLS],
    ndlls: usize,
    suspended: bool,
    wait: bool,
    wait_module: *const u16,
    retries: u32,
    delay_ms: u32,
    prog_args: [*const u16; MAX_ARGS],
    nprog_args: usize,
}

const MSG_CAP: usize = 512;
const MODNAME_CAP: usize = 260;
const MODULES_CAP: usize = 1024;

static mut CMDLINE: [u16; CMDLINE_CAP] = [0; CMDLINE_CAP];
static mut OUT: [u8; OUT_CAP] = [0; OUT_CAP];
static mut MSG: [u16; MSG_CAP] = [0; MSG_CAP];
static mut MODNAME: [u16; MODNAME_CAP] = [0; MODNAME_CAP];
static mut MODULES: [HMODULE; MODULES_CAP] = [null_mut(); MODULES_CAP];

// Edition 2024 does not allow references to static mut, so these hand out raw pointers.
fn cmdline_ptr() -> *mut u16 {
    (&raw mut CMDLINE).cast()
}
fn out_ptr() -> *mut u8 {
    (&raw mut OUT).cast()
}
fn msg_ptr() -> *mut u16 {
    (&raw mut MSG).cast()
}
fn modname_ptr() -> *mut u16 {
    (&raw mut MODNAME).cast()
}
fn modules_ptr() -> *mut HMODULE {
    (&raw mut MODULES).cast()
}

unsafe fn wlen(p: *const u16) -> usize {
    let mut n = 0;
    while unsafe { *p.add(n) } != 0 {
        n += 1;
    }
    n
}

fn weq_ascii(w: *const u16, s: &str) -> bool {
    let b = s.as_bytes();
    for (i, &c) in b.iter().enumerate() {
        if unsafe { *w.add(i) } != c as u16 {
            return false;
        }
    }
    unsafe { *w.add(b.len()) == 0 }
}

fn weq_nocase(a: *const u16, b: *const u16) -> bool {
    let mut i = 0;
    loop {
        let (x, y) = unsafe { (*a.add(i), *b.add(i)) };
        let lx = if (b'A' as u16..=b'Z' as u16).contains(&x) { x + 32 } else { x };
        let ly = if (b'A' as u16..=b'Z' as u16).contains(&y) { y + 32 } else { y };
        if lx != ly {
            return false;
        }
        if x == 0 {
            return true;
        }
        i += 1;
    }
}

fn parse_u32(w: *const u16) -> Option<u32> {
    let n = unsafe { wlen(w) };
    if n == 0 {
        return None;
    }
    let mut v: u32 = 0;
    for i in 0..n {
        let c = unsafe { *w.add(i) };
        if !(b'0' as u16..=b'9' as u16).contains(&c) {
            return None;
        }
        v = v.checked_mul(10)?.checked_add((c - b'0' as u16) as u32)?;
    }
    Some(v)
}

struct Out {
    len: usize,
}

impl Out {
    fn begin(event: &str) -> Out {
        let mut o = Out { len: 0 };
        o.raw(b"{\"event\":\"");
        o.raw(event.as_bytes());
        o.raw(b"\"");
        o
    }

    fn raw(&mut self, b: &[u8]) {
        for &c in b {
            if self.len < OUT_CAP - 2 {
                unsafe { *out_ptr().add(self.len) = c };
                self.len += 1;
            }
        }
    }

    fn esc_byte(&mut self, c: u8) {
        match c {
            b'"' => self.raw(b"\\\""),
            b'\\' => self.raw(b"\\\\"),
            b'\n' => self.raw(b"\\n"),
            b'\r' => self.raw(b"\\r"),
            b'\t' => self.raw(b"\\t"),
            0..=0x1f => {
                self.raw(b"\\u00");
                let h = b"0123456789abcdef";
                self.raw(&[h[(c >> 4) as usize], h[(c & 15) as usize]]);
            }
            _ => self.raw(&[c]),
        }
    }

    fn wstr(&mut self, key: &str, w: *const u16) -> &mut Self {
        self.raw(b",\"");
        self.raw(key.as_bytes());
        self.raw(b"\":\"");
        if !w.is_null() {
            let n = unsafe { wlen(w) };
            let mut i = 0;
            while i < n {
                let u = unsafe { *w.add(i) } as u32;
                let cp = if (0xD800..0xDC00).contains(&u) && i + 1 < n {
                    let lo = unsafe { *w.add(i + 1) } as u32;
                    if (0xDC00..0xE000).contains(&lo) {
                        i += 1;
                        0x10000 + ((u - 0xD800) << 10) + (lo - 0xDC00)
                    } else {
                        0xFFFD
                    }
                } else {
                    u
                };
                let mut buf = [0u8; 4];
                let s = char::from_u32(cp).unwrap_or('\u{FFFD}').encode_utf8(&mut buf);
                for &b in s.as_bytes() {
                    self.esc_byte(b);
                }
                i += 1;
            }
        }
        self.raw(b"\"");
        self
    }

    fn str(&mut self, key: &str, s: &str) -> &mut Self {
        self.raw(b",\"");
        self.raw(key.as_bytes());
        self.raw(b"\":\"");
        for &b in s.as_bytes() {
            self.esc_byte(b);
        }
        self.raw(b"\"");
        self
    }

    fn num(&mut self, key: &str, mut v: u32) -> &mut Self {
        self.raw(b",\"");
        self.raw(key.as_bytes());
        self.raw(b"\":");
        let mut digits = [0u8; 10];
        let mut n = 0;
        loop {
            digits[n] = b'0' + (v % 10) as u8;
            n += 1;
            v /= 10;
            if v == 0 {
                break;
            }
        }
        while n > 0 {
            n -= 1;
            self.raw(&[digits[n]]);
        }
        self
    }

    fn end(&mut self) {
        self.raw(b"}\n");
        unsafe {
            let h = GetStdHandle(STD_OUTPUT_HANDLE);
            let mut written: u32 = 0;
            WriteFile(h, out_ptr(), self.len as u32, &mut written, null_mut());
        }
    }
}

unsafe fn error_message(code: u32) -> *const u16 {
    unsafe {
        let n = FormatMessageW(
            FORMAT_MESSAGE_FROM_SYSTEM | FORMAT_MESSAGE_IGNORE_INSERTS,
            null(),
            code,
            0,
            msg_ptr(),
            (MSG_CAP - 1) as u32,
            null_mut(),
        ) as usize;
        let mut end = n;
        let m = msg_ptr();
        while end > 0 && matches!(*m.add(end - 1), 10 | 13 | 32 | 9) {
            end -= 1;
        }
        *m.add(end) = 0;
        if end == 0 {
            let s = b"unknown error";
            for (i, &c) in s.iter().enumerate() {
                *m.add(i) = c as u16;
            }
            *m.add(s.len()) = 0;
        }
        m
    }
}

fn emit_error(stage: &str, code: u32) {
    let msg = unsafe { error_message(code) };
    Out::begin("error").str("stage", stage).num("code", code).wstr("message", msg).end();
}

unsafe fn parse_opts() -> Result<Opts, &'static str> {
    let mut o = Opts {
        exe: null(),
        cwd: null(),
        dlls: [null(); MAX_DLLS],
        ndlls: 0,
        suspended: false,
        wait: false,
        wait_module: null(),
        retries: 5,
        delay_ms: 500,
        prog_args: [null(); MAX_ARGS],
        nprog_args: 0,
    };
    let mut argc: i32 = 0;
    let argv = unsafe { CommandLineToArgvW(GetCommandLineW(), &mut argc) };
    if argv.is_null() {
        return Err("CommandLineToArgvW failed");
    }
    let argc = argc as usize;
    let arg = |i: usize| unsafe { *argv.add(i) };
    let mut i = 1;
    while i < argc {
        let a = arg(i);
        let next = |i: &mut usize| -> Result<*const u16, &'static str> {
            *i += 1;
            if *i >= argc { Err("missing value after option") } else { Ok(arg(*i)) }
        };
        if weq_ascii(a, "--") {
            i += 1;
            while i < argc {
                if o.nprog_args >= MAX_ARGS {
                    return Err("too many program arguments");
                }
                o.prog_args[o.nprog_args] = arg(i);
                o.nprog_args += 1;
                i += 1;
            }
            break;
        } else if weq_ascii(a, "--exe") {
            o.exe = next(&mut i)?;
        } else if weq_ascii(a, "--cwd") {
            o.cwd = next(&mut i)?;
        } else if weq_ascii(a, "--dll") {
            if o.ndlls >= MAX_DLLS {
                return Err("too many --dll options");
            }
            o.dlls[o.ndlls] = next(&mut i)?;
            o.ndlls += 1;
        } else if weq_ascii(a, "--suspended") {
            o.suspended = true;
        } else if weq_ascii(a, "--no-suspend") {
            o.suspended = false;
        } else if weq_ascii(a, "--wait") {
            o.wait = true;
        } else if weq_ascii(a, "--wait-module") {
            o.wait_module = next(&mut i)?;
        } else if weq_ascii(a, "--retries") {
            o.retries = parse_u32(next(&mut i)?).ok_or("bad --retries")?;
        } else if weq_ascii(a, "--delay") {
            o.delay_ms = parse_u32(next(&mut i)?).ok_or("bad --delay")?;
        } else {
            return Err("unknown option");
        }
        i += 1;
    }
    if o.exe.is_null() {
        return Err("--exe is required");
    }
    if o.retries == 0 {
        o.retries = 1;
    }
    Ok(o)
}

// Quoting rules: https://learn.microsoft.com/en-us/cpp/c-language/parsing-c-command-line-arguments
unsafe fn cmdline_push(pos: &mut usize, arg: *const u16) -> bool {
    let n = unsafe { wlen(arg) };
    let mut needs_quotes = n == 0;
    for i in 0..n {
        let c = unsafe { *arg.add(i) };
        if c == b' ' as u16 || c == b'\t' as u16 || c == b'"' as u16 {
            needs_quotes = true;
        }
    }
    let push = |pos: &mut usize, c: u16| -> bool {
        if *pos + 1 >= CMDLINE_CAP {
            return false;
        }
        unsafe { *cmdline_ptr().add(*pos) = c };
        *pos += 1;
        true
    };
    if *pos > 0 && !push(pos, b' ' as u16) {
        return false;
    }
    if !needs_quotes {
        for i in 0..n {
            if !push(pos, unsafe { *arg.add(i) }) {
                return false;
            }
        }
        return true;
    }
    if !push(pos, b'"' as u16) {
        return false;
    }
    let mut i = 0;
    while i < n {
        let mut backslashes = 0;
        while i < n && unsafe { *arg.add(i) } == b'\\' as u16 {
            backslashes += 1;
            i += 1;
        }
        if i == n {
            for _ in 0..backslashes * 2 {
                if !push(pos, b'\\' as u16) {
                    return false;
                }
            }
            break;
        }
        let c = unsafe { *arg.add(i) };
        if c == b'"' as u16 {
            for _ in 0..backslashes * 2 + 1 {
                if !push(pos, b'\\' as u16) {
                    return false;
                }
            }
        } else {
            for _ in 0..backslashes {
                if !push(pos, b'\\' as u16) {
                    return false;
                }
            }
        }
        if !push(pos, c) {
            return false;
        }
        i += 1;
    }
    push(pos, b'"' as u16)
}

// Loads the DLL with LoadLibraryW in a remote thread, the same technique samp.exe uses.
// Err carries the Win32 error code.
unsafe fn inject(process: HANDLE, dll: *const u16) -> Result<u32, u32> {
    unsafe {
        let bytes = (wlen(dll) + 1) * 2;
        let mem = VirtualAllocEx(process, null_mut(), bytes, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE);
        if mem.is_null() {
            return Err(GetLastError());
        }
        let ok = WriteProcessMemory(process, mem, dll as *const u8, bytes, null_mut());
        if ok == 0 {
            let e = GetLastError();
            VirtualFreeEx(process, mem, 0, MEM_RELEASE);
            return Err(e);
        }
        let k32 = GetModuleHandleW(w!("kernel32.dll").as_ptr());
        let load = GetProcAddress(k32, c"LoadLibraryW".as_ptr() as *const u8);
        if load.is_null() {
            let e = GetLastError();
            VirtualFreeEx(process, mem, 0, MEM_RELEASE);
            return Err(e);
        }
        let thread = CreateRemoteThread(process, null_mut(), 0, load, mem, 0, null_mut());
        if thread.is_null() {
            let e = GetLastError();
            VirtualFreeEx(process, mem, 0, MEM_RELEASE);
            return Err(e);
        }
        let waited = WaitForSingleObject(thread, 15_000);
        let mut code: u32 = 0;
        let result = if waited != WAIT_OBJECT_0 {
            Err(ERROR_TIMEOUT)
        } else if GetExitCodeThread(thread, &mut code) == 0 {
            Err(GetLastError())
        } else if code == 0 {
            // LoadLibraryW returned NULL inside the target process.
            Err(ERROR_MOD_NOT_FOUND)
        } else {
            Ok(code)
        };
        CloseHandle(thread);
        VirtualFreeEx(process, mem, 0, MEM_RELEASE);
        result
    }
}

unsafe fn module_loaded(process: HANDLE, name: *const u16) -> bool {
    unsafe {
        let mut needed: u32 = 0;
        let cap = (MODULES_CAP * size_of::<HMODULE>()) as u32;
        if K32EnumProcessModules(process, modules_ptr(), cap, &mut needed) == 0 {
            return false;
        }
        let count = (needed as usize / size_of::<HMODULE>()).min(MODULES_CAP);
        for i in 0..count {
            let n = K32GetModuleBaseNameW(process, *modules_ptr().add(i), modname_ptr(), MODNAME_CAP as u32);
            if n > 0 && weq_nocase(modname_ptr(), name) {
                return true;
            }
        }
        false
    }
}

unsafe fn run() -> u32 {
    let opts = match unsafe { parse_opts() } {
        Ok(o) => o,
        Err(e) => {
            Out::begin("error").str("stage", "args").num("code", 0).str("message", e).end();
            return 2;
        }
    };

    let mut pos = 0usize;
    if !unsafe { cmdline_push(&mut pos, opts.exe) } {
        Out::begin("error").str("stage", "args").num("code", 0).str("message", "command line too long").end();
        return 2;
    }
    for i in 0..opts.nprog_args {
        if !unsafe { cmdline_push(&mut pos, opts.prog_args[i]) } {
            Out::begin("error").str("stage", "args").num("code", 0).str("message", "command line too long").end();
            return 2;
        }
    }
    unsafe { *cmdline_ptr().add(pos) = 0 };

    let mut si = STARTUPINFOW::zeroed();
    si.cb = size_of::<STARTUPINFOW>() as u32;
    let mut pi = PROCESS_INFORMATION::zeroed();
    let flags = if opts.suspended { CREATE_SUSPENDED } else { 0 };
    let ok = unsafe {
        CreateProcessW(
            opts.exe,
            cmdline_ptr(),
            null_mut(),
            null_mut(),
            0,
            flags,
            null_mut(),
            if opts.cwd.is_null() { null() } else { opts.cwd },
            &si,
            &mut pi,
        )
    };
    if ok == 0 {
        emit_error("spawn", unsafe { GetLastError() });
        return 3;
    }
    Out::begin("spawned").num("pid", pi.dwProcessId).end();

    if !opts.suspended && !opts.wait_module.is_null() {
        Out::begin("waiting").wstr("module", opts.wait_module).end();
        let mut waited_ms = 0u32;
        while waited_ms < 60_000 && !unsafe { module_loaded(pi.hProcess, opts.wait_module) } {
            unsafe { Sleep(100) };
            waited_ms += 100;
            let mut code = STILL_ACTIVE;
            if unsafe { GetExitCodeProcess(pi.hProcess, &mut code) } != 0 && code != STILL_ACTIVE {
                Out::begin("error")
                    .str("stage", "wait-module")
                    .num("code", code)
                    .str("message", "process exited before the module loaded")
                    .end();
                return 4;
            }
        }
    }

    for d in 0..opts.ndlls {
        let dll = opts.dlls[d];
        let mut attempt = 1u32;
        loop {
            match unsafe { inject(pi.hProcess, dll) } {
                Ok(_) => {
                    Out::begin("injected").wstr("dll", dll).num("attempt", attempt).end();
                    break;
                }
                Err(code) => {
                    if attempt >= opts.retries {
                        let msg = unsafe { error_message(code) };
                        Out::begin("error")
                            .str("stage", "inject")
                            .num("code", code)
                            .wstr("message", msg)
                            .wstr("dll", dll)
                            .end();
                        unsafe {
                            TerminateProcess(pi.hProcess, 1);
                        }
                        return 4;
                    }
                    let msg = unsafe { error_message(code) };
                    Out::begin("retry")
                        .wstr("dll", dll)
                        .num("attempt", attempt)
                        .num("code", code)
                        .wstr("message", msg)
                        .end();
                    attempt += 1;
                    unsafe { Sleep(opts.delay_ms) };
                }
            }
        }
    }

    if opts.suspended {
        unsafe { ResumeThread(pi.hThread) };
        Out::begin("resumed").end();
    }

    let mut exit = 0u32;
    if opts.wait {
        unsafe {
            WaitForSingleObject(pi.hProcess, INFINITE);
            let mut code = 0u32;
            if GetExitCodeProcess(pi.hProcess, &mut code) != 0 {
                exit = code;
            }
        }
        Out::begin("exit").num("code", exit).end();
    }
    unsafe {
        CloseHandle(pi.hThread);
        CloseHandle(pi.hProcess);
    }
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn mainCRTStartup() -> ! {
    let code = unsafe { run() };
    unsafe { ExitProcess(code) }
}
