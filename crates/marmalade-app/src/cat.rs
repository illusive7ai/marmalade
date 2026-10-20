//! The cat: a click-through, always-on-top, per-pixel-alpha sprite window.
//!
//! Event-driven: the frame timer is armed from `SpritePlayer::wake_in()` and killed when nothing
//! will change. When the cat sleeps, the player frees every frame but one and the thread blocks
//! in `GetMessageW` with zero wakeups.

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
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, VK_SPACE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, KillTimer, PostQuitMessage, RegisterClassW, SetTimer,
    SetWindowPos, ShowWindow, SystemParametersInfoW, UpdateLayeredWindow, CS_HREDRAW, CS_VREDRAW,
    HWND_TOPMOST, SPI_GETWORKAREA, SWP_NOACTIVATE, SWP_NOSIZE, SW_SHOWNOACTIVATE,
    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, ULW_ALPHA, WM_DESTROY, WM_HOTKEY, WM_TIMER, WNDCLASSW,
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

pub const W: i32 = 256;
pub const H: i32 = 256;
const AWAKE_SECS: u64 = 15;
const TIMER_ID: usize = 1;
const HK_WAKE: i32 = 1;
const HK_QUIT: i32 = 2;
const HK_BAR: i32 = 3;
const WALK: &str = "walk";
const SIT: &str = "sit";

struct Cat {
    hwnd: HWND,
    mem_dc: HDC,
    bitmap: HBITMAP,
    bits: *mut u8,
    player: SpritePlayer,
    last_tick: Instant,
    awake_until: Instant,
    awake: bool,
    /// Keep awake regardless of the timeout (the chat bar is open).
    hold: bool,
}

thread_local! {
    static CAT: RefCell<Option<Cat>> = const { RefCell::new(None) };
}

fn with_cat<R>(f: impl FnOnce(&mut Cat) -> R) -> Option<R> {
    CAT.with(|c| c.borrow_mut().as_mut().map(f))
}

pub fn work_area() -> RECT {
    let mut work = RECT::default();
    // SAFETY: `work` is a valid RECT for SPI_GETWORKAREA.
    let _ = unsafe {
        SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            Some(&mut work as *mut RECT as *mut c_void),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };
    work
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

/// Create the cat window, register the global hotkeys, and start it awake in the bottom-right corner.
pub fn create(hinst: HINSTANCE) -> windows::core::Result<HWND> {
    // SAFETY: plain Win32 window/DIB setup on the UI thread; every handle is checked via `?`.
    unsafe {
        let class = w!("MarmaladeCat");
        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wndproc),
            hInstance: hinst,
            lpszClassName: class,
            ..Default::default()
        };
        RegisterClassW(&wc);

        let (x, y) = corner_position();
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
        let _ = player.play(WALK); // on failure the window stays transparent; nothing to draw

        let now = Instant::now();
        CAT.with(|c| {
            *c.borrow_mut() = Some(Cat {
                hwnd,
                mem_dc,
                bitmap,
                bits: bits as *mut u8,
                player,
                last_tick: now,
                awake_until: now + Duration::from_secs(AWAKE_SECS),
                awake: true,
                hold: false,
            });
        });

        // RegisterHotKey, not a keyboard hook: no keylogger-like behaviour, no per-keystroke cost.
        let m = MOD_CONTROL | MOD_ALT | MOD_NOREPEAT;
        let _ = RegisterHotKey(hwnd, HK_WAKE, m, 'M' as u32);
        let _ = RegisterHotKey(hwnd, HK_QUIT, m, 'Q' as u32);
        let _ = RegisterHotKey(hwnd, HK_BAR, m, VK_SPACE.0 as u32);

        render();
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        schedule(hwnd);
        Ok(hwnd)
    }
}

pub fn destroy() {
    CAT.with(|c| {
        if let Some(cat) = c.borrow_mut().take() {
            // SAFETY: handles were created in `create` and are released exactly once here.
            unsafe {
                let _ = DeleteObject(HGDIOBJ(cat.bitmap.0));
                let _ = DeleteDC(cat.mem_dc);
            }
        }
    });
}

fn corner_position() -> (i32, i32) {
    let work = work_area();
    (work.right - W - 16, work.bottom - H - 8)
}

/// Sit next to the chat bar and stay awake until `release()`.
pub fn attend(bar: RECT) {
    let x = bar.left + 24;
    let y = bar.top - H + 36; // overlap the bar's top edge so the cat appears to sit on it
    with_cat(|cat| {
        cat.hold = true;
        cat.awake = true;
        cat.last_tick = Instant::now();
        let _ = cat.player.play(SIT);
        // SAFETY: valid window handle owned by this thread.
        unsafe {
            let _ = SetWindowPos(
                cat.hwnd,
                HWND_TOPMOST,
                x,
                y,
                0,
                0,
                SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
    });
    render();
    reschedule();
}

/// Go back to the corner and let the normal sleep timeout run.
pub fn release() {
    let (x, y) = corner_position();
    with_cat(|cat| {
        cat.hold = false;
        cat.awake_until = Instant::now() + Duration::from_secs(AWAKE_SECS);
        cat.last_tick = Instant::now();
        let _ = cat.player.play(WALK);
        // SAFETY: valid window handle owned by this thread.
        unsafe {
            let _ = SetWindowPos(
                cat.hwnd,
                HWND_TOPMOST,
                x,
                y,
                0,
                0,
                SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
    });
    render();
    reschedule();
}

fn reschedule() {
    if let Some(hwnd) = with_cat(|c| c.hwnd) {
        // SAFETY: valid window handle owned by this thread.
        unsafe { schedule(hwnd) };
    }
}

/// Arm the single OS timer for the next frame, or kill it when nothing will change.
///
/// # Safety
/// `hwnd` must be the cat window, on the thread that owns it.
unsafe fn schedule(hwnd: HWND) {
    let next = with_cat(|cat| cat.player.wake_in()).flatten();
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
            let changed = with_cat(|cat| {
                let now = Instant::now();
                let dt = now - cat.last_tick;
                cat.last_tick = now;
                let mut changed = cat.player.tick(dt);
                if cat.awake && !cat.hold && now >= cat.awake_until {
                    cat.awake = false;
                    cat.player.sleep(); // frees every frame but the one on screen
                    changed = true;
                }
                changed
            })
            .unwrap_or(false);
            if changed {
                render();
            }
            schedule(hwnd);
            LRESULT(0)
        }
        WM_HOTKEY => {
            match wparam.0 as i32 {
                HK_WAKE => {
                    with_cat(|cat| {
                        let now = Instant::now();
                        cat.awake = true;
                        cat.awake_until = now + Duration::from_secs(AWAKE_SECS);
                        cat.last_tick = now;
                        let mirrored = cat.player.mirrored();
                        cat.player.set_mirrored(!mirrored);
                        let _ = cat.player.play(WALK);
                    });
                    render();
                    schedule(hwnd);
                }
                HK_BAR => crate::bar::toggle(),
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
    with_cat(|cat| {
        // SAFETY: `bits` points at the W*H*4-byte DIB section created in `create`.
        unsafe {
            let dst = std::slice::from_raw_parts_mut(cat.bits, (W * H * 4) as usize);
            dst.fill(0);
            cat.player.blit(dst, W as u32, H as u32, 0, 0);

            let size = SIZE { cx: W, cy: H };
            let zero = POINT { x: 0, y: 0 };
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            let _ = UpdateLayeredWindow(
                cat.hwnd,
                None,
                None,
                Some(&size),
                cat.mem_dc,
                Some(&zero),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
        }
    });
}
