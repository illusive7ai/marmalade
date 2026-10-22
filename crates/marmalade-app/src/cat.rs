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

use crate::fetch::{self, Phase, Scene};
use crate::props;
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
    RegisterHotKey, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, VK_SPACE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetWindowRect, KillTimer, PostQuitMessage,
    RegisterClassW, SetTimer, SetWindowPos, ShowWindow, SystemParametersInfoW, UpdateLayeredWindow,
    CS_HREDRAW, CS_VREDRAW, HWND_TOPMOST, SPI_GETWORKAREA, SWP_NOACTIVATE, SWP_NOSIZE,
    SWP_NOZORDER, SW_HIDE, SW_SHOWNOACTIVATE, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, ULW_ALPHA,
    WM_DESTROY, WM_HOTKEY, WM_TIMER, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

pub const W: i32 = 256;
pub const H: i32 = 256;
const AWAKE_SECS: u64 = 15;
const TIMER_ID: usize = 1;
/// Drives the fetch scene at ~60 fps; only armed while the scene runs.
const SCENE_TIMER: usize = 2;
const SCENE_FRAME_MS: u32 = 16;
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
    /// Draw the carried file at the cat's mouth.
    carrying: bool,
    scene: Option<Active>,
}

/// A running fetch scene and everything it created.
struct Active {
    scene: Scene,
    folder: Option<Prop>,
    folder_shown: bool,
    cat_shown: bool,
    was_mirrored: bool,
    on_done: Option<Box<dyn FnOnce()>>,
}

/// A small click-through window showing a fixed bitmap (the folder).
struct Prop {
    hwnd: HWND,
    dc: HDC,
    bitmap: HBITMAP,
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
                carrying: false,
                scene: None,
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

/// True while the fetch scene owns the cat.
pub fn busy() -> bool {
    with_cat(|c| c.scene.is_some()).unwrap_or(false)
}

/// Play the fetch scene from wherever the cat is now, then run `on_done` when the file is
/// delivered. The cat goes back to its corner afterwards. Runs `on_done` at once if the cat
/// cannot perform (no window), so the caller never loses the action.
pub fn start_fetch(on_done: Box<dyn FnOnce()>) {
    let mut callback = Some(on_done);
    let timer_hwnd = with_cat(|cat| {
        let mut r = RECT::default();
        // SAFETY: valid window handle owned by this thread; `r` is a valid RECT.
        unsafe {
            let _ = GetWindowRect(cat.hwnd, &mut r);
        }
        let work = work_area();
        let plan = fetch::plan((r.left, r.top), work.left, work.right);

        let was_mirrored = cat.player.mirrored();
        cat.player.set_mirrored(plan.dir < 0);
        let _ = cat.player.play("leap");
        let leap_secs = cat.player.duration().map_or(1.0, |d| d.as_secs_f64());

        cat.hold = true;
        cat.awake = true;
        cat.last_tick = Instant::now();
        cat.scene = Some(Active {
            scene: Scene::new(plan, leap_secs),
            folder: create_prop(
                &props::folder_pixels(),
                fetch::FOLDER_W,
                fetch::FOLDER_H,
                plan.folder,
            ),
            folder_shown: true,
            cat_shown: true,
            was_mirrored,
            on_done: callback.take(),
        });
        cat.hwnd
    });
    let Some(hwnd) = timer_hwnd else {
        if let Some(f) = callback {
            f();
        }
        return;
    };
    render();
    // SAFETY: valid window handle owned by this thread.
    unsafe {
        let _ = KillTimer(hwnd, TIMER_ID);
        SetTimer(hwnd, SCENE_TIMER, SCENE_FRAME_MS, None);
    }
}

/// One scene frame: advance the timeline, switch clips on phase changes, move the windows.
fn scene_tick() {
    let finished = with_cat(|cat| {
        let Some(act) = cat.scene.as_mut() else {
            return false;
        };
        let now = Instant::now();
        let dt = now - cat.last_tick;
        cat.last_tick = now;
        cat.player.tick(dt);

        let before = act.scene.phase();
        act.scene.tick(dt.as_secs_f64(), cat.player.is_finished());
        let pose = act.scene.pose();
        if act.scene.phase() != before {
            if let Some(clip) = act.scene.clip() {
                let _ = cat.player.play(clip);
            }
        }
        cat.player.set_mirrored(pose.mirrored);
        cat.carrying = pose.carrying;

        // SAFETY: valid windows owned by this thread.
        unsafe {
            if pose.cat_visible != act.cat_shown {
                let cmd = if pose.cat_visible {
                    SW_SHOWNOACTIVATE
                } else {
                    SW_HIDE
                };
                let _ = ShowWindow(cat.hwnd, cmd);
                act.cat_shown = pose.cat_visible;
            }
            if pose.cat_visible {
                let _ = SetWindowPos(
                    cat.hwnd,
                    None,
                    pose.cat.0,
                    pose.cat.1,
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOZORDER,
                );
            }
            if let Some(folder) = &act.folder {
                if pose.folder_visible != act.folder_shown {
                    let cmd = if pose.folder_visible {
                        SW_SHOWNOACTIVATE
                    } else {
                        SW_HIDE
                    };
                    let _ = ShowWindow(folder.hwnd, cmd);
                    act.folder_shown = pose.folder_visible;
                }
                if pose.folder_visible {
                    let _ = SetWindowPos(
                        folder.hwnd,
                        None,
                        pose.folder.0,
                        pose.folder.1,
                        0,
                        0,
                        SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOZORDER,
                    );
                }
            }
        }
        act.scene.phase() == Phase::Done
    })
    .unwrap_or(false);
    render();
    if finished {
        end_scene();
    }
}

/// Tear the scene down, send the cat home, then hand the file over.
fn end_scene() {
    let done = with_cat(|cat| {
        let act = cat.scene.take()?;
        cat.carrying = false;
        cat.player.set_mirrored(act.was_mirrored);
        // SAFETY: valid windows/handles owned by this thread, released exactly once.
        unsafe {
            let _ = KillTimer(cat.hwnd, SCENE_TIMER);
            let _ = ShowWindow(cat.hwnd, SW_SHOWNOACTIVATE);
            if let Some(folder) = act.folder {
                destroy_prop(folder);
            }
        }
        act.on_done
    })
    .flatten();
    release();
    if let Some(f) = done {
        f();
    }
}

/// Create a click-through, always-on-top window showing `pixels` (premultiplied BGRA) at `pos`.
fn create_prop(pixels: &[u8], w: i32, h: i32, pos: (i32, i32)) -> Option<Prop> {
    // SAFETY: plain Win32 window/DIB setup on the UI thread; every handle is checked and any
    // partial creation is released before returning `None`.
    unsafe {
        let hinst: HINSTANCE = GetModuleHandleW(None).ok()?.into();
        let class = w!("MarmaladeProp");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(prop_proc),
            hInstance: hinst,
            lpszClassName: class,
            ..Default::default()
        };
        RegisterClassW(&wc); // fails harmlessly when already registered
        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            class,
            PCWSTR::null(),
            WS_POPUP,
            pos.0,
            pos.1,
            w,
            h,
            None,
            None,
            hinst,
            None,
        )
        .ok()?;

        let screen = GetDC(None);
        let dc = CreateCompatibleDC(screen);
        ReleaseDC(None, screen);
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut c_void = std::ptr::null_mut();
        let Ok(bitmap) = CreateDIBSection(dc, &bmi, DIB_RGB_COLORS, &mut bits, None, 0) else {
            let _ = DeleteDC(dc);
            let _ = DestroyWindow(hwnd);
            return None;
        };
        if bits.is_null() || pixels.len() != (w * h * 4) as usize {
            let _ = DeleteObject(HGDIOBJ(bitmap.0));
            let _ = DeleteDC(dc);
            let _ = DestroyWindow(hwnd);
            return None;
        }
        // `bits` is a w*h*4-byte DIB section and the source length was checked above.
        std::ptr::copy_nonoverlapping(pixels.as_ptr(), bits as *mut u8, pixels.len());
        SelectObject(dc, HGDIOBJ(bitmap.0));

        let size = SIZE { cx: w, cy: h };
        let zero = POINT { x: 0, y: 0 };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let _ = UpdateLayeredWindow(
            hwnd,
            None,
            None,
            Some(&size),
            dc,
            Some(&zero),
            COLORREF(0),
            Some(&blend),
            ULW_ALPHA,
        );
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        Some(Prop { hwnd, dc, bitmap })
    }
}

unsafe extern "system" fn prop_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

/// # Safety
/// `prop` must come from `create_prop` and not have been destroyed.
unsafe fn destroy_prop(prop: Prop) {
    let _ = DestroyWindow(prop.hwnd);
    let _ = DeleteObject(HGDIOBJ(prop.bitmap.0));
    let _ = DeleteDC(prop.dc);
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
    if busy() {
        let _ = KillTimer(hwnd, TIMER_ID); // the scene timer drives the player meanwhile
        return;
    }
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
        WM_TIMER if wparam.0 == SCENE_TIMER => {
            scene_tick();
            LRESULT(0)
        }
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
                HK_WAKE if !busy() => {
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
            if cat.carrying {
                if let Some((mx, my)) = cat.player.mouth() {
                    props::draw_file(dst, W, H, mx, my);
                }
            }

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
