//! Marmalade: a desktop cat that does your repetitive tasks when you ask.
//!
//! Hotkeys (global, via RegisterHotKey):
//!   Ctrl+Alt+Space  open or close the chat bar
//!   Ctrl+Alt+M      wake the cat (and flip its facing)
//!   Ctrl+Alt+Q      quit
//!
//! Usage: marmalade.exe [assets_dir]   (default: assets/placeholder, found relative to cwd or exe)
#![windows_subsystem = "windows"]

mod bar;
mod cat;
mod fetch;
mod handler;
mod props;

use windows::Win32::Foundation::HINSTANCE;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, TranslateMessage, MSG,
};

fn main() -> windows::core::Result<()> {
    // SAFETY: standard single-threaded Win32 message loop; all windows live on this thread.
    unsafe {
        let hinst: HINSTANCE = GetModuleHandleW(None)?.into();
        let cat_hwnd = cat::create(hinst)?;
        bar::create(hinst, cat_hwnd)?;

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    cat::destroy();
    Ok(())
}
