//! Marmalade overlay spike.
//!
//! A click-through, always-on-top, per-pixel-alpha sprite window. Purpose: measure idle RAM / CPU /
//! wakeups of the native approach, now with real sprite frames from `marmalade-sprites`.
//!
//! Behaviour:
//!   * starts awake, playing the `walk` placeholder, for AWAKE_SECS, then sleeps: the player frees
//!     all frames but one, the timer is killed, and the thread blocks in GetMessageW (zero wakeups).
//!   * the frame timer is armed from `SpritePlayer::wake_in()`, i.e. only when a frame is due.
//!   * Ctrl+Alt+M wakes the cat (and flips its facing), Ctrl+Alt+Q quits. (RegisterHotKey, no hook.)
//!
//! Usage: overlay-spike.exe [assets_dir]   (default: assets/placeholder, found relative to cwd or exe)
#![windows_subsystem = "windows"]

use std::cell::RefCell;
use std::ffi::c_void;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use marmalade_sprites::SpritePlayer;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{
    COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC, SelectObject,
    AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS,
    HBITMAP, HDC, HGDIOBJ,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, KillTimer, PostQuitMessage,
    RegisterClassW, SetTimer, ShowWindow, SystemParametersInfoW, TranslateMessage,
    UpdateLayeredWindow, CS_HREDRAW, CS_VREDRAW, MSG, SPI_GETWORKAREA, SW_SHOWNOACTIVATE,
    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, ULW_ALPHA, WM_DESTROY, WM_HOTKEY, WM_TIMER, WNDCLASSW,
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

const W: i32 = 256;
const H: i32 = 256;
const AWAKE_SECS: u64 = 15;
const TIMER_ID: usize = 1;
const HK_WAKE: i32 = 1;
const HK_QUIT: i32 = 2;
const ANIM: &str = "walk";

struct App {
    hwnd: HWND,
    mem_dc: HDC,
    bitmap: HBITMAP,
    bits: *mut u8,
    player: SpritePlayer,
    last_tick: Instant,
    awake_until: Instant,
    awake: bool,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

fn assets_dir() -> PathBuf {
    if let Some(arg) = std::env::args().nth(1) {
        return arg.into();
    }
    let mut candidates = vec![PathBuf::from("assets/placeholder")];
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("../../assets/placeholder"));
            candidates.push(dir.join("assets/placeholder"));
        }
    }
    candidates
        .into_iter()
        .find(|p| p.is_dir())
        .unwrap_or_else(|| "assets/placeholder".into())
}

fn main() -> windows::core::Result<()> {
    unsafe {
        let hinst: HINSTANCE = GetModuleHandleW(None)?.into();
        let class = w!("MarmaladeSpike");
        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wndproc),
            hInstance: hinst,
            lpszClassName: class,
            ..Default::default()
        };
        RegisterClassW(&wc);

        let mut work = RECT::default();
        SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            Some(&mut work as *mut RECT as *mut c_void),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )?;
        let x = work.right - W - 16;
        let y = work.bottom - H - 8;

        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            class,
            PCWSTR::null(),
            WS_POPUP,
            x,
            y,
            W,
            H,
            None,
            None,
            hinst,
            None,
        )?;

        // One persistent top-down 32-bit DIB; this is the only per-frame surface.
        let screen = GetDC(None);
        let mem_dc = CreateCompatibleDC(screen);
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: W,
                biHeight: -H,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut c_void = std::ptr::null_mut();
        let bitmap = CreateDIBSection(mem_dc, &bmi, DIB_RGB_COLORS, &mut bits, None, 0)?;
        SelectObject(mem_dc, HGDIOBJ(bitmap.0));
        ReleaseDC(None, screen);

        let mut player = SpritePlayer::new(assets_dir());
        let _ = player.play(ANIM); // on failure the window stays transparent; nothing to draw

        let now = Instant::now();
        let app = App {
            hwnd,
            mem_dc,
            bitmap,
            bits: bits as *mut u8,
            player,
            last_tick: now,
            awake_until: now + Duration::from_secs(AWAKE_SECS),
            awake: true,
        };
        APP.with(|a| *a.borrow_mut() = Some(app));

        let _ = RegisterHotKey(
            hwnd,
            HK_WAKE,
            MOD_CONTROL | MOD_ALT | MOD_NOREPEAT,
            'M' as u32,
        );
        let _ = RegisterHotKey(
            hwnd,
            HK_QUIT,
            MOD_CONTROL | MOD_ALT | MOD_NOREPEAT,
            'Q' as u32,
        );

        render();
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        schedule(hwnd);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        APP.with(|a| {
            if let Some(app) = a.borrow_mut().take() {
                let _ = DeleteObject(HGDIOBJ(app.bitmap.0));
                let _ = DeleteDC(app.mem_dc);
            }
        });
    }
    Ok(())
}

/// Arm the single OS timer for the next frame, or kill it when nothing will change.
unsafe fn schedule(hwnd: HWND) {
    let next = APP.with(|a| a.borrow().as_ref().and_then(|app| app.player.wake_in()));
    match next {
        Some(d) => {
            SetTimer(hwnd, TIMER_ID, (d.as_millis() as u32).max(10), None);
        }
        None => {
            let _ = KillTimer(hwnd, TIMER_ID);
        }
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_TIMER => {
            let changed = APP.with(|a| {
                let mut a = a.borrow_mut();
                let app = a.as_mut().unwrap();
                let now = Instant::now();
                let dt = now - app.last_tick;
                app.last_tick = now;
                let mut changed = app.player.tick(dt);
                if app.awake && now >= app.awake_until {
                    app.awake = false;
                    app.player.sleep(); // frees every frame but the one on screen
                    changed = true;
                }
                changed
            });
            if changed {
                render();
            }
            schedule(hwnd);
            LRESULT(0)
        }
        WM_HOTKEY => {
            match wparam.0 as i32 {
                HK_WAKE => {
                    APP.with(|a| {
                        let mut a = a.borrow_mut();
                        let app = a.as_mut().unwrap();
                        let now = Instant::now();
                        app.awake = true;
                        app.awake_until = now + Duration::from_secs(AWAKE_SECS);
                        app.last_tick = now;
                        let mirrored = app.player.mirrored();
                        app.player.set_mirrored(!mirrored);
                        let _ = app.player.play(ANIM);
                    });
                    render();
                    schedule(hwnd);
                }
                HK_QUIT => PostQuitMessage(0),
                _ => {}
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// Composite the current sprite frame into the DIB and push it to the layered window.
fn render() {
    APP.with(|a| {
        let mut a = a.borrow_mut();
        let app = a.as_mut().unwrap();
        unsafe {
            let dst = std::slice::from_raw_parts_mut(app.bits, (W * H * 4) as usize);
            dst.fill(0);
            app.player.blit(dst, W as u32, H as u32, 0, 0);

            let size = SIZE { cx: W, cy: H };
            let zero = POINT { x: 0, y: 0 };
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            let _ = UpdateLayeredWindow(
                app.hwnd,
                None,
                None,
                Some(&size),
                app.mem_dc,
                Some(&zero),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
        }
    });
}
