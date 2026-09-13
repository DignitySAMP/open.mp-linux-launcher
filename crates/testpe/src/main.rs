// Stand-in for gta_sa.exe in the Wine test. Writes its command line to omp-tui-game-args.txt
// and exits with code 7.

#![no_std]
#![no_main]

#[path = "common.rs"]
mod common;

use common::*;

#[unsafe(no_mangle)]
pub extern "C" fn mainCRTStartup() -> ! {
    unsafe {
        let ok = write_marker("omp-tui-game-args.txt", GetCommandLineW());
        Sleep(1500);
        ExitProcess(if ok { 7 } else { 9 })
    }
}
