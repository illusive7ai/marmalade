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

use crate::fetch::{self, Scene};
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
const MAX_SCENE_STEP: Duration = Duration::from_millis(100);
/// How long one clip dissolves into the next.
const FADE_SECS: f32 = 0.16;
const HK_WAKE: i32 = 1;
const HK_QUIT: i32 = 2;
const HK_BAR: i32 = 3;
const SIT: &str = "sit";
const SLEEP: &str = "sleep";

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
    /// The previous clip's last frame, dissolving into the one that just started.
    fade: Option<Fade>,
    /// Whole-cat opacity (the leap fades out into the folder).
    alpha: f32,
    /// Screen row of the window's top, and the screen row from which the cat is hidden behind the folder.
    win_y: i32,
    clip_y: Option<i32>,
}

struct Fade {
    from: Vec<u8>,
    started: Instant,
}

/// A running fetch scene and everything it created.
struct Active {
    scene: Scene,
    folder: Option<Prop>,
    folder_shown: bool,
    /// The file once it has left the cat's mouth.
    file: Option<Prop>,
    file_shown: bool,
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
    // The real cat first, the generated placeholder shapes as a fallback.
    let mut candidates = vec![
        PathBuf::from("assets/cat"),
        PathBuf::from("assets/placeholder"),
    ];
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for pack in ["cat", "placeholder"] {
                candidates.push(dir.join("../../assets").join(pack));
                candidates.push(dir.join("assets").join(pack));
            }
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
        let _ = player.play(SIT); // on failure the window stays transparent; nothing to draw

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
                fade: None,
                alpha: 1.0,
                win_y: y,
                clip_y: None,
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
        cat.fade = None;
        cat.alpha = 1.0;
        cat.clip_y = None;
        cat.win_y = y;
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

/// True while the fetch scene owns the cat.
pub fn busy() -> bool {
    with_cat(|c| c.scene.is_some()).unwrap_or(false)
}

/// Switch to clip `id`, dissolving from whatever is on screen now. Loading can take a while, so
/// the clip's clock starts afterwards.
fn switch_clip(cat: &mut Cat, id: &str) {
    if cat.player.current_id() == Some(id) {
        return;
    }
    // SAFETY: `bits` points at the W*H*4-byte DIB section created in `create`.
    let on_screen = unsafe { std::slice::from_raw_parts(cat.bits, (W * H * 4) as usize) }.to_vec();
    let _ = cat.player.play(id);
    cat.last_tick = Instant::now();
    cat.fade = Some(Fade {
        from: on_screen,
        started: Instant::now(),
    });
}

/// Play the fetch scene from wherever the cat is now, then run `on_done` when the file is
/// delivered. The cat walks back to its corner afterwards. Runs `on_done` at once if the cat
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

        // The jump arc is timed to the leap clip, so ask how long it is.
        let mut probe = SpritePlayer::new(assets_dir());
        let leap_secs = probe
            .play("leap")
            .ok()
            .and_then(|_| probe.duration())
            .map_or(1.0, |d| d.as_secs_f64());
        // Where the carried file sits against the cat's face on the way back (that clip faces
        // the other way from the leap).
        probe.set_mirrored(plan.dir >= 0);
        let mouth = probe
            .play("carry")
            .ok()
            .and_then(|_| probe.mouth())
            .unwrap_or((200, 150));
        let segs = fetch::build(plan, leap_secs, corner_position(), mouth);

        let was_mirrored = cat.player.mirrored();
        cat.player.set_mirrored(segs[0].mirrored);
        if let Some(clip) = segs[0].clip {
            switch_clip(cat, clip);
        }
        cat.hold = true;
        cat.awake = true;
        cat.last_tick = Instant::now();
        cat.alpha = 1.0;
        cat.clip_y = None;
        cat.scene = Some(Active {
            scene: Scene::new(segs, plan.folder),
            folder: create_prop(
                &props::folder_pixels(),
                fetch::FOLDER_W,
                fetch::FOLDER_H,
                plan.folder,
            ),
            folder_shown: true,
            file: {
                let file = create_prop(
                    &props::file_pixels(),
                    props::FILE_W,
                    props::FILE_H,
                    plan.folder,
                );
                if let Some(f) = &file {
                    // SAFETY: valid window created just above on this thread.
                    unsafe {
                        let _ = ShowWindow(f.hwnd, SW_HIDE);
                    }
                }
                file
            },
            file_shown: false,
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

/// One scene frame: advance the timeline, start the next clip when a segment changes, move the windows.
fn scene_tick() {
    let (finished, deliver) = with_cat(|cat| {
        // Taken out so `switch_clip` can borrow the whole cat; put back below.
        let Some(mut act) = cat.scene.take() else {
            return (false, None);
        };
        let now = Instant::now();
        // A stalled frame (e.g. decoding the next clip) must not fast-forward the scene.
        let dt = (now - cat.last_tick).min(MAX_SCENE_STEP);
        cat.last_tick = now;
        cat.player.tick(dt);

        if act.scene.tick(dt.as_secs_f64(), cat.player.is_finished()) {
            if let Some(clip) = act.scene.current().and_then(|s| s.clip) {
                switch_clip(cat, clip);
            }
        }
        let deliver = if act.scene.take_deliver() {
            act.on_done.take()
        } else {
            None
        };

        let pose = act.scene.pose();
        cat.player.set_mirrored(pose.mirrored);
        cat.carrying = pose.carrying;
        cat.alpha = pose.alpha;
        cat.clip_y = pose.clip_y;
        cat.win_y = pose.cat.1;
        if cat
            .fade
            .as_ref()
            .is_some_and(|f| f.started.elapsed().as_secs_f32() >= FADE_SECS)
        {
            cat.fade = None;
        }

        // SAFETY: valid windows owned by this thread.
        unsafe {
            if pose.visible != act.cat_shown {
                let cmd = if pose.visible {
                    SW_SHOWNOACTIVATE
                } else {
                    SW_HIDE
                };
                let _ = ShowWindow(cat.hwnd, cmd);
                act.cat_shown = pose.visible;
            }
            if pose.visible {
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
        if let Some(file) = &act.file {
            // SAFETY: valid window owned by this thread.
            unsafe {
                match pose.file {
                    Some(pos) => {
                        let _ = SetWindowPos(
                            file.hwnd,
                            None,
                            pos.0,
                            pos.1,
                            0,
                            0,
                            SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOZORDER,
                        );
                        if !act.file_shown {
                            let _ = ShowWindow(file.hwnd, SW_SHOWNOACTIVATE);
                            act.file_shown = true;
                        }
                    }
                    None if act.file_shown => {
                        let _ = ShowWindow(file.hwnd, SW_HIDE);
                        act.file_shown = false;
                    }
                    None => {}
                }
            }
        }
        let finished = act.scene.is_done();
        cat.scene = Some(act);
        (finished, deliver)
    })
    .unwrap_or((false, None));
    render();
    if let Some(f) = deliver {
        f();
    }
    if finished {
        end_scene();
    }
}

/// Tear the scene down and leave the cat sitting in its corner.
fn end_scene() {
    let leftover = with_cat(|cat| {
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
            if let Some(file) = act.file {
                destroy_prop(file);
            }
        }
        act.on_done
    })
    .flatten();
    release();
    // Only reached if the scene ended before the hand-over (it never should); never lose the action.
    if let Some(f) = leftover {
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
                if cat.fade.is_some() {
                    changed = true; // the dissolve needs a redraw every tick
                    if cat
                        .fade
                        .as_ref()
                        .is_some_and(|f| f.started.elapsed().as_secs_f32() >= FADE_SECS)
                    {
                        cat.fade = None;
                        if !cat.awake {
                            cat.player.sleep(); // frees every frame but the one on screen
                        }
                    }
                }
                if cat.awake && !cat.hold && now >= cat.awake_until {
                    cat.awake = false;
                    switch_clip(cat, SLEEP); // curl up, dissolving from the sitting pose
                    if cat.fade.is_none() {
                        cat.player.sleep();
                    }
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
                        switch_clip(cat, SIT);
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
            if cat.carrying {
                // Behind the cat, so its chin overlaps the paper: held in the mouth, not floating.
                if let Some((mx, my)) = cat.player.mouth() {
                    props::draw_file(dst, W, H, mx, my);
                }
            }
            cat.player.blit(dst, W as u32, H as u32, 0, 0);
            if let Some(f) = &cat.fade {
                // Cross-dissolve (premultiplied pixels, so a plain linear blend is correct).
                let k = (f.started.elapsed().as_secs_f32() / FADE_SECS).min(1.0);
                for (d, s) in dst.iter_mut().zip(&f.from) {
                    *d = (*d as f32 * k + *s as f32 * (1.0 - k)) as u8;
                }
            }
            if cat.alpha < 1.0 {
                let a = cat.alpha.max(0.0);
                for b in dst.iter_mut() {
                    *b = (*b as f32 * a) as u8;
                }
            }
            if let Some(cy) = cat.clip_y {
                // Behind the folder: nothing of the cat shows below the folder's bottom edge.
                let row = (cy - cat.win_y).clamp(0, H) as usize;
                dst[row * W as usize * 4..].fill(0);
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
