//! The chat bar: a dark, rounded input window with a live preview of what the router understood.
//!
//! A native EDIT control does the text input (paste, selection, IME and accessibility for free).
//! Enter runs the action; actions with a real-world effect (printing) need Enter twice. Esc or
//! clicking away closes the bar. While it is open the cat sits on top of it.

use std::cell::RefCell;
use std::ffi::c_void;
use std::sync::atomic::{AtomicIsize, Ordering};

use marmalade_core::{route, Action, Routed};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreateRoundRectRgn, CreateSolidBrush, DrawTextW, EndPaint, FillRect,
    InvalidateRect, SelectObject, SetBkColor, SetBkMode, SetTextColor, SetWindowRgn,
    CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DT_END_ELLIPSIS, DT_LEFT, DT_NOPREFIX,
    DT_SINGLELINE, DT_VCENTER, HBRUSH, HDC, HFONT, HGDIOBJ, OUT_DEFAULT_PRECIS, PAINTSTRUCT,
    TRANSPARENT,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{SetFocus, VK_ESCAPE, VK_RETURN};
use windows::Win32::UI::WindowsAndMessaging::{
    CallWindowProcW, CreateWindowExW, DefWindowProcW, GetWindowTextW, LoadCursorW, RegisterClassW,
    SendMessageW, SetForegroundWindow, SetWindowLongPtrW, SetWindowPos, SetWindowTextW, ShowWindow,
    CS_DROPSHADOW, EN_CHANGE, ES_AUTOHSCROLL, GWLP_WNDPROC, HWND_TOPMOST, IDC_ARROW,
    SWP_SHOWWINDOW, SW_HIDE, WA_INACTIVE, WINDOW_STYLE, WM_ACTIVATE, WM_CHAR, WM_COMMAND,
    WM_CTLCOLOREDIT, WM_ERASEBKGND, WM_KEYDOWN, WM_PAINT, WM_SETFOCUS, WM_SETFONT, WNDCLASSW,
    WNDPROC, WS_CHILD, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP, WS_VISIBLE,
};

const BAR_W: i32 = 640;
const INPUT_H: i32 = 52;
const PREVIEW_H: i32 = 30;
const BAR_H: i32 = INPUT_H + PREVIEW_H;
const MAX_INPUT: usize = 300;

const BG: COLORREF = rgb(30, 30, 34);
const SEPARATOR: COLORREF = rgb(58, 58, 64);
const INPUT_TEXT: COLORREF = rgb(240, 240, 245);

/// Mirrors the state colours of the cat's pills: blue = ready, orange = needs you, green = done.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tone {
    Idle,
    Ready,
    Attention,
    Done,
}

impl Tone {
    fn color(self) -> COLORREF {
        match self {
            Tone::Idle => rgb(140, 140, 152),
            Tone::Ready => rgb(110, 170, 255),
            Tone::Attention => rgb(255, 170, 70),
            Tone::Done => rgb(110, 210, 140),
        }
    }
}

struct Bar {
    hwnd: HWND,
    edit: HWND,
    brush: HBRUSH,
    separator: HBRUSH,
    font_small: HFONT,
    tone: Tone,
    preview: String,
    /// An action waiting for a second Enter.
    pending: Option<Action>,
    visible: bool,
}

thread_local! {
    static BAR: RefCell<Option<Bar>> = const { RefCell::new(None) };
}

static OLD_EDIT_PROC: AtomicIsize = AtomicIsize::new(0);

const fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF(r as u32 | (g as u32) << 8 | (b as u32) << 16)
}

/// Run `f` on the bar state. Returns `None` if the state is missing or already borrowed, which
/// happens when a Win32 call made while borrowed re-enters the window procedure; never hold the
/// borrow across a call that can send messages.
fn with_bar<R>(f: impl FnOnce(&mut Bar) -> R) -> Option<R> {
    BAR.with(|b| b.try_borrow_mut().ok().and_then(|mut g| g.as_mut().map(f)))
}

pub fn create(hinst: HINSTANCE, _cat: HWND) -> windows::core::Result<()> {
    // SAFETY: plain Win32 window setup on the UI thread; handles are checked via `?`.
    unsafe {
        let class = w!("MarmaladeBar");
        let wc = WNDCLASSW {
            style: CS_DROPSHADOW,
            lpfnWndProc: Some(wndproc),
            hInstance: hinst,
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            lpszClassName: class,
            ..Default::default()
        };
        RegisterClassW(&wc);

        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            class,
            PCWSTR::null(),
            WS_POPUP,
            0,
            0,
            BAR_W,
            BAR_H,
            None,
            None,
            hinst,
            None,
        )?;

        // Rounded corners: native on Windows 11, a region elsewhere.
        let pref = DWMWCP_ROUND;
        let rounded = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &pref as *const _ as *const c_void,
            std::mem::size_of_val(&pref) as u32,
        );
        if rounded.is_err() {
            SetWindowRgn(
                hwnd,
                CreateRoundRectRgn(0, 0, BAR_W + 1, BAR_H + 1, 28, 28),
                true,
            );
        }

        let font_input = font(-20);
        let font_small = font(-15);

        let edit = CreateWindowExW(
            Default::default(),
            w!("EDIT"),
            PCWSTR::null(),
            WS_CHILD | WS_VISIBLE | WINDOW_STYLE_EDIT,
            22,
            13,
            BAR_W - 44,
            28,
            hwnd,
            None,
            hinst,
            None,
        )?;
        SendMessageW(edit, WM_SETFONT, WPARAM(font_input.0 as usize), LPARAM(1));
        SendMessageW(edit, EM_SETLIMITTEXT, WPARAM(MAX_INPUT), LPARAM(0));
        let old = SetWindowLongPtrW(edit, GWLP_WNDPROC, edit_proc as *const () as usize as isize);
        OLD_EDIT_PROC.store(old, Ordering::SeqCst);

        BAR.with(|b| {
            *b.borrow_mut() = Some(Bar {
                hwnd,
                edit,
                brush: CreateSolidBrush(BG),
                separator: CreateSolidBrush(SEPARATOR),
                font_small,
                tone: Tone::Idle,
                preview: IDLE_HINT.to_string(),
                pending: None,
                visible: false,
            });
        });
    }
    Ok(())
}

const WINDOW_STYLE_EDIT: WINDOW_STYLE = WINDOW_STYLE(ES_AUTOHSCROLL as u32);
/// Not exported by the `windows` crate's WindowsAndMessaging module (it lives in Controls).
const EM_SETLIMITTEXT: u32 = 0x00C5;
const IDLE_HINT: &str =
    "Ask me anything  \u{2022}  try \u{201c}find my cv\u{201d}  \u{2022}  Esc to close";

unsafe fn font(height: i32) -> HFONT {
    CreateFontW(
        height,
        0,
        0,
        0,
        400,
        0,
        0,
        0,
        DEFAULT_CHARSET.0 as u32,
        OUT_DEFAULT_PRECIS.0 as u32,
        CLIP_DEFAULT_PRECIS.0 as u32,
        CLEARTYPE_QUALITY.0 as u32,
        0,
        w!("Segoe UI"),
    )
}

pub fn toggle() {
    let visible = with_bar(|b| b.visible).unwrap_or(false);
    if visible {
        hide();
    } else {
        show();
    }
}

fn show() {
    let Some((hwnd, edit)) = with_bar(|b| {
        b.visible = true;
        b.pending = None;
        b.tone = Tone::Idle;
        b.preview = IDLE_HINT.to_string();
        (b.hwnd, b.edit)
    }) else {
        return;
    };
    let work = crate::cat::work_area();
    let x = work.left + (work.right - work.left - BAR_W) / 2;
    let y = work.bottom - BAR_H - 28;
    // SAFETY: valid windows owned by this thread; no state borrow is held across these calls.
    unsafe {
        let _ = SetWindowPos(hwnd, HWND_TOPMOST, x, y, BAR_W, BAR_H, SWP_SHOWWINDOW);
        let _ = SetWindowTextW(edit, w!(""));
        let _ = SetForegroundWindow(hwnd);
        let _ = SetFocus(edit);
        let _ = InvalidateRect(hwnd, None, true);
    }
    crate::cat::attend(RECT {
        left: x,
        top: y,
        right: x + BAR_W,
        bottom: y + BAR_H,
    });
}

fn hide() {
    let Some(hwnd) = with_bar(|b| {
        b.visible = false;
        b.pending = None;
        b.hwnd
    }) else {
        return;
    };
    // SAFETY: valid window owned by this thread.
    unsafe {
        let _ = ShowWindow(hwnd, SW_HIDE);
    }
    crate::cat::release();
}

fn set_preview(tone: Tone, text: String) {
    if let Some(hwnd) = with_bar(|b| {
        b.tone = tone;
        b.preview = text;
        b.hwnd
    }) {
        let r = RECT {
            left: 0,
            top: INPUT_H,
            right: BAR_W,
            bottom: BAR_H,
        };
        // SAFETY: valid window owned by this thread.
        unsafe {
            let _ = InvalidateRect(hwnd, Some(&r), false);
        }
    }
}

fn read_input() -> String {
    let Some(edit) = with_bar(|b| b.edit) else {
        return String::new();
    };
    let mut buf = [0u16; MAX_INPUT + 2];
    // SAFETY: `buf` is a valid mutable slice; `edit` is a valid window.
    let n = unsafe { GetWindowTextW(edit, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

/// Live preview while typing.
fn on_change() {
    with_bar(|b| b.pending = None);
    let text = read_input();
    if text.trim().is_empty() {
        set_preview(Tone::Idle, IDLE_HINT.to_string());
        return;
    }
    match route(&text) {
        Routed::Intent(a) => {
            let hint = if a.requires_confirmation() {
                "  (Enter twice)"
            } else {
                ""
            };
            set_preview(Tone::Ready, format!("\u{2192} {a}{hint}"));
        }
        Routed::NeedsMore(m) => set_preview(Tone::Attention, m),
        Routed::Unknown => set_preview(
            Tone::Idle,
            "I don't know that yet. Type \u{201c}help\u{201d} to see what I can do.".into(),
        ),
    }
}

fn submit() {
    let text = read_input();
    match route(&text) {
        Routed::Intent(action) => {
            let confirmed = with_bar(|b| b.pending.as_ref() == Some(&action)).unwrap_or(false);
            if action.requires_confirmation() && !confirmed {
                set_preview(
                    Tone::Attention,
                    format!("Press Enter again to confirm: {action}"),
                );
                with_bar(|b| b.pending = Some(action));
                return;
            }
            let status = crate::handler::execute(&action);
            clear_input();
            set_preview(Tone::Done, status);
        }
        Routed::NeedsMore(m) => set_preview(Tone::Attention, m),
        Routed::Unknown => set_preview(
            Tone::Idle,
            "I don't know that yet. Type \u{201c}help\u{201d} to see what I can do.".into(),
        ),
    }
}

fn clear_input() {
    if let Some(edit) = with_bar(|b| b.edit) {
        // SAFETY: valid window; this sends EN_CHANGE, so no state borrow may be held here.
        unsafe {
            let _ = SetWindowTextW(edit, w!(""));
        }
    }
}

fn escape() {
    let had_pending = with_bar(|b| b.pending.take().is_some()).unwrap_or(false);
    if had_pending {
        on_change();
    } else {
        hide();
    }
}

unsafe extern "system" fn edit_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_KEYDOWN if wparam.0 == VK_RETURN.0 as usize => {
            submit();
            return LRESULT(0);
        }
        WM_KEYDOWN if wparam.0 == VK_ESCAPE.0 as usize => {
            escape();
            return LRESULT(0);
        }
        // Swallow the character messages for Enter and Esc so the control does not beep.
        WM_CHAR if wparam.0 == 13 || wparam.0 == 27 => return LRESULT(0),
        _ => {}
    }
    let old: WNDPROC = std::mem::transmute::<isize, WNDPROC>(OLD_EDIT_PROC.load(Ordering::SeqCst));
    CallWindowProcW(old, hwnd, msg, wparam, lparam)
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_CTLCOLOREDIT => {
            let hdc = HDC(wparam.0 as *mut c_void);
            SetTextColor(hdc, INPUT_TEXT);
            SetBkColor(hdc, BG);
            let brush = with_bar(|b| b.brush).unwrap_or_default();
            LRESULT(brush.0 as isize)
        }
        WM_COMMAND if (wparam.0 >> 16) as u32 == EN_CHANGE => {
            on_change();
            LRESULT(0)
        }
        WM_ACTIVATE if (wparam.0 & 0xFFFF) as u32 == WA_INACTIVE => {
            if with_bar(|b| b.visible).unwrap_or(false) {
                hide();
            }
            LRESULT(0)
        }
        WM_SETFOCUS => {
            if let Some(edit) = with_bar(|b| b.edit) {
                let _ = SetFocus(edit);
            }
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            paint(hwnd);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn paint(hwnd: HWND) {
    let mut ps = PAINTSTRUCT::default();
    let hdc = BeginPaint(hwnd, &mut ps);
    if let Some((brush, sep, font_small, tone, preview)) = with_bar(|b| {
        (
            b.brush,
            b.separator,
            b.font_small,
            b.tone,
            b.preview.clone(),
        )
    }) {
        FillRect(hdc, &ps.rcPaint, brush);
        let line = RECT {
            left: 20,
            top: INPUT_H,
            right: BAR_W - 20,
            bottom: INPUT_H + 1,
        };
        FillRect(hdc, &line, sep);

        SelectObject(hdc, HGDIOBJ(font_small.0));
        SetBkMode(hdc, TRANSPARENT);
        SetTextColor(hdc, tone.color());
        let mut text: Vec<u16> = format!("\u{25CF}  {preview}").encode_utf16().collect();
        let mut r = RECT {
            left: 22,
            top: INPUT_H + 1,
            right: BAR_W - 22,
            bottom: BAR_H,
        };
        DrawTextW(
            hdc,
            &mut text,
            &mut r,
            DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
        );
    }
    let _ = EndPaint(hwnd, &ps);
}
