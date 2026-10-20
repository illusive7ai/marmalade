//! Marmalade day-one spike.
//!
//! A click-through, always-on-top, per-pixel-alpha cat drawn in code. Purpose: measure
//! idle RAM / CPU / wakeups of the native approach before committing to a stack.
//!
//! Behaviour:
//!   * starts awake (30 fps breathing + blinking) for AWAKE_SECS, then sleeps:
//!     the frame timer is killed, one static frame is drawn, and the thread blocks in
//!     GetMessageW with zero wakeups.
//!   * Ctrl+Alt+M wakes the cat, Ctrl+Alt+Q quits. (RegisterHotKey, no keyboard hook.)
#![windows_subsystem = "windows"]

use std::cell::RefCell;
use std::ffi::c_void;
use std::time::{Duration, Instant};

use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Rect, Stroke, Transform};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC, SelectObject,
    AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS,
    HBITMAP, HDC, HGDIOBJ,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, KillTimer, PostQuitMessage,
    RegisterClassW, SetTimer, ShowWindow, SystemParametersInfoW, TranslateMessage, UpdateLayeredWindow,
    CS_HREDRAW, CS_VREDRAW, MSG, SPI_GETWORKAREA, SW_SHOWNOACTIVATE, ULW_ALPHA,
    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, WM_DESTROY, WM_HOTKEY, WM_TIMER, WNDCLASSW, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

const W: i32 = 160;
const H: i32 = 160;
const FRAME_MS: u32 = 33; // ~30 fps; never raise the system timer resolution
const AWAKE_SECS: u64 = 15;
const TIMER_ID: usize = 1;
const HK_WAKE: i32 = 1;
const HK_QUIT: i32 = 2;

struct App {
    hwnd: HWND,
    mem_dc: HDC,
    bitmap: HBITMAP,
    bits: *mut u8,
    pixmap: Pixmap,
    origin: Instant,
    awake_until: Instant,
    awake: bool,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
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

        let now = Instant::now();
        let app = App {
            hwnd,
            mem_dc,
            bitmap,
            bits: bits as *mut u8,
            pixmap: Pixmap::new(W as u32, H as u32).expect("pixmap"),
            origin: now,
            awake_until: now + Duration::from_secs(AWAKE_SECS),
            awake: true,
        };
        APP.with(|a| *a.borrow_mut() = Some(app));

        let _ = RegisterHotKey(hwnd, HK_WAKE, MOD_CONTROL | MOD_ALT | MOD_NOREPEAT, 'M' as u32);
        let _ = RegisterHotKey(hwnd, HK_QUIT, MOD_CONTROL | MOD_ALT | MOD_NOREPEAT, 'Q' as u32);

        render();
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        SetTimer(hwnd, TIMER_ID, FRAME_MS, None);

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

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_TIMER => {
            let sleep_now = APP.with(|a| {
                let mut a = a.borrow_mut();
                let app = a.as_mut().unwrap();
                app.awake && Instant::now() >= app.awake_until
            });
            if sleep_now {
                APP.with(|a| a.borrow_mut().as_mut().unwrap().awake = false);
                let _ = KillTimer(hwnd, TIMER_ID); // asleep = zero wakeups
            }
            render();
            LRESULT(0)
        }
        WM_HOTKEY => {
            match wparam.0 as i32 {
                HK_WAKE => {
                    APP.with(|a| {
                        let mut a = a.borrow_mut();
                        let app = a.as_mut().unwrap();
                        app.awake = true;
                        app.awake_until = Instant::now() + Duration::from_secs(AWAKE_SECS);
                    });
                    SetTimer(hwnd, TIMER_ID, FRAME_MS, None);
                    render();
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

/// Draw the cat into the pixmap, copy it into the DIB (RGBA -> BGRA, both premultiplied),
/// and push it to the layered window.
fn render() {
    APP.with(|a| {
        let mut a = a.borrow_mut();
        let app = a.as_mut().unwrap();
        let t = app.origin.elapsed().as_secs_f32();
        draw_cat(&mut app.pixmap, t, app.awake);

        let src = app.pixmap.data();
        unsafe {
            let dst = std::slice::from_raw_parts_mut(app.bits, src.len());
            for (d, s) in dst.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
                d[0] = s[2];
                d[1] = s[1];
                d[2] = s[0];
                d[3] = s[3];
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

fn paint(r: u8, g: u8, b: u8, a: u8) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color_rgba8(r, g, b, a);
    p.anti_alias = true;
    p
}

/// Stylized orange cat: squircle body, two ears, eyes that blink while awake and close when asleep.
fn draw_cat(pm: &mut Pixmap, t: f32, awake: bool) {
    pm.fill(tiny_skia::Color::TRANSPARENT);
    let cx = W as f32 / 2.0;
    let breathe = if awake { 1.0 + 0.025 * (t * 2.2).sin() } else { 1.0 };
    let (bw, bh) = (92.0 * breathe, 84.0 * breathe);
    let cy = 96.0;

    // ears
    let ear = paint(0xF2, 0x8C, 0x28, 255);
    for s in [-1.0f32, 1.0] {
        let mut pb = PathBuilder::new();
        pb.move_to(cx + s * 14.0, cy - bh / 2.0 + 8.0);
        pb.line_to(cx + s * 40.0, cy - bh / 2.0 - 22.0);
        pb.line_to(cx + s * 44.0, cy - bh / 2.0 + 16.0);
        pb.close();
        if let Some(p) = pb.finish() {
            pm.fill_path(&p, &ear, FillRule::Winding, Transform::identity(), None);
        }
    }

    // body (rounded rect used as squircle)
    let mut pb = PathBuilder::new();
    if let Some(rect) = Rect::from_xywh(cx - bw / 2.0, cy - bh / 2.0, bw, bh) {
        let r = 34.0;
        let (l, tp, rt, bt) = (rect.left(), rect.top(), rect.right(), rect.bottom());
        pb.move_to(l + r, tp);
        pb.line_to(rt - r, tp);
        pb.quad_to(rt, tp, rt, tp + r);
        pb.line_to(rt, bt - r);
        pb.quad_to(rt, bt, rt - r, bt);
        pb.line_to(l + r, bt);
        pb.quad_to(l, bt, l, bt - r);
        pb.line_to(l, tp + r);
        pb.quad_to(l, tp, l + r, tp);
        pb.close();
        if let Some(p) = pb.finish() {
            pm.fill_path(&p, &paint(0xF7, 0x9A, 0x3A, 255), FillRule::Winding, Transform::identity(), None);
        }
    }

    // eyes
    let ink = paint(0x2B, 0x1D, 0x14, 255);
    let blinking = awake && (t % 4.0) > 3.85;
    for s in [-1.0f32, 1.0] {
        let ex = cx + s * 20.0;
        let ey = cy - 4.0;
        if awake && !blinking {
            let mut pb = PathBuilder::new();
            pb.push_oval(Rect::from_xywh(ex - 5.0, ey - 8.0, 10.0, 16.0).unwrap());
            if let Some(p) = pb.finish() {
                pm.fill_path(&p, &ink, FillRule::Winding, Transform::identity(), None);
            }
        } else {
            let mut pb = PathBuilder::new();
            pb.move_to(ex - 7.0, ey);
            pb.quad_to(ex, ey + 6.0, ex + 7.0, ey);
            if let Some(p) = pb.finish() {
                let stroke = Stroke { width: 3.0, ..Default::default() };
                pm.stroke_path(&p, &ink, &stroke, Transform::identity(), None);
            }
        }
    }

    // nose
    let mut pb = PathBuilder::new();
    pb.move_to(cx - 4.0, cy + 10.0);
    pb.line_to(cx + 4.0, cy + 10.0);
    pb.line_to(cx, cy + 15.0);
    pb.close();
    if let Some(p) = pb.finish() {
        pm.fill_path(&p, &paint(0xC2, 0x5A, 0x4A, 255), FillRule::Winding, Transform::identity(), None);
    }
}
