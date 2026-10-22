//! The chat bar: a dark, rounded input window with a live preview of what the router understood,
//! and a result list that grows upward from the input.
//!
//! A native EDIT control does the text input (paste, selection, IME and accessibility for free).
//! Enter runs the action; actions with a real-world effect (printing) need Enter twice. Searches
//! run on a worker thread and post their results back, so typing and the cat never stall.
//! Esc steps back one level (confirmation, then results, then the bar); clicking away closes it.

use std::cell::RefCell;
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::Mutex;

use marmalade_core::{paths, route, Action, FindSpec, Routed};
use marmalade_search::{search, Hit, Outcome, Request, Source};
use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreateRoundRectRgn, CreateSolidBrush, DrawTextW, EndPaint, FillRect,
    InvalidateRect, SelectObject, SetBkColor, SetBkMode, SetTextColor, SetWindowRgn,
    CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DT_END_ELLIPSIS, DT_LEFT, DT_NOPREFIX,
    DT_RIGHT, DT_SINGLELINE, DT_VCENTER, HBRUSH, HDC, HFONT, HGDIOBJ, OUT_DEFAULT_PRECIS,
    PAINTSTRUCT, TRANSPARENT,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, SetFocus, VK_CONTROL, VK_DOWN, VK_ESCAPE, VK_RETURN, VK_UP,
};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    CallWindowProcW, CreateWindowExW, DefWindowProcW, GetWindowTextW, LoadCursorW, PostMessageW,
    RegisterClassW, SendMessageW, SetForegroundWindow, SetWindowLongPtrW, SetWindowPos,
    SetWindowTextW, ShowWindow, CS_DROPSHADOW, EN_CHANGE, ES_AUTOHSCROLL, GWLP_WNDPROC,
    HWND_TOPMOST, IDC_ARROW, SWP_NOACTIVATE, SWP_NOZORDER, SWP_SHOWWINDOW, SW_HIDE, SW_SHOWNORMAL,
    WA_INACTIVE, WINDOW_STYLE, WM_ACTIVATE, WM_APP, WM_CHAR, WM_COMMAND, WM_CTLCOLOREDIT,
    WM_ERASEBKGND, WM_KEYDOWN, WM_PAINT, WM_SETFOCUS, WM_SETFONT, WNDCLASSW, WNDPROC, WS_CHILD,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP, WS_VISIBLE,
};

const BAR_W: i32 = 640;
const INPUT_H: i32 = 52;
const PREVIEW_H: i32 = 30;
const BASE_H: i32 = INPUT_H + PREVIEW_H;
const ROW_H: i32 = 34;
const LIST_PAD: i32 = 8;
const MAX_ROWS: usize = 6;
const MAX_INPUT: usize = 300;

/// Posted by the search worker when results are ready.
const WM_APP_RESULTS: u32 = WM_APP + 1;

const BG: COLORREF = rgb(30, 30, 34);
const SEPARATOR: COLORREF = rgb(58, 58, 64);
const SELECTED: COLORREF = rgb(46, 52, 70);
const INPUT_TEXT: COLORREF = rgb(240, 240, 245);
const DIM_TEXT: COLORREF = rgb(140, 140, 152);

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
            Tone::Idle => DIM_TEXT,
            Tone::Ready => rgb(110, 170, 255),
            Tone::Attention => rgb(255, 170, 70),
            Tone::Done => rgb(110, 210, 140),
        }
    }
}

struct Bar {
    hwnd: HWND,
    edit: HWND,
    /// Left edge and bottom edge of the window. The bottom stays put; the list grows upward.
    x: i32,
    bottom: i32,
    brush: HBRUSH,
    separator: HBRUSH,
    selected_brush: HBRUSH,
    font_row: HFONT,
    font_small: HFONT,
    tone: Tone,
    preview: String,
    /// An action waiting for a second Enter.
    pending: Option<Action>,
    results: Vec<Hit>,
    selected: usize,
    /// The input text the results belong to; Enter acts on a result only while it is unchanged.
    results_for: String,
    /// Bumped to invalidate any search still running.
    search_gen: u64,
    visible: bool,
}

/// A finished search handed from the worker thread to the UI thread.
struct Delivery {
    gen: u64,
    text: String,
    outcome: Outcome,
}

thread_local! {
    static BAR: RefCell<Option<Bar>> = const { RefCell::new(None) };
}

static OLD_EDIT_PROC: AtomicIsize = AtomicIsize::new(0);
static DELIVERY: Mutex<Option<Delivery>> = Mutex::new(None);

const fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF(r as u32 | (g as u32) << 8 | (b as u32) << 16)
}

/// Run `f` on the bar state. Returns `None` if the state is missing or already borrowed, which
/// happens when a Win32 call made while borrowed re-enters the window procedure; never hold the
/// borrow across a call that can send messages.
fn with_bar<R>(f: impl FnOnce(&mut Bar) -> R) -> Option<R> {
    BAR.with(|b| b.try_borrow_mut().ok().and_then(|mut g| g.as_mut().map(f)))
}

fn list_height(rows: usize) -> i32 {
    if rows == 0 {
        0
    } else {
        rows as i32 * ROW_H + LIST_PAD
    }
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
            BASE_H,
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
                CreateRoundRectRgn(0, 0, BAR_W + 1, BASE_H + 1, 28, 28),
                true,
            );
        }

        let font_input = font(-20);
        let font_row = font(-17);
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
                x: 0,
                bottom: 0,
                brush: CreateSolidBrush(BG),
                separator: CreateSolidBrush(SEPARATOR),
                selected_brush: CreateSolidBrush(SELECTED),
                font_row,
                font_small,
                tone: Tone::Idle,
                preview: IDLE_HINT.to_string(),
                pending: None,
                results: Vec::new(),
                selected: 0,
                results_for: String::new(),
                search_gen: 0,
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
    let work = crate::cat::work_area();
    let x = work.left + (work.right - work.left - BAR_W) / 2;
    let bottom = work.bottom - 28;
    let Some(edit) = with_bar(|b| {
        b.visible = true;
        b.pending = None;
        b.results.clear();
        b.search_gen += 1;
        b.x = x;
        b.bottom = bottom;
        b.tone = Tone::Idle;
        b.preview = IDLE_HINT.to_string();
        b.edit
    }) else {
        return;
    };
    apply_layout(true);
    // SAFETY: valid windows owned by this thread; no state borrow is held across these calls.
    unsafe {
        let _ = SetWindowTextW(edit, w!(""));
        if let Some(hwnd) = with_bar(|b| b.hwnd) {
            let _ = SetForegroundWindow(hwnd);
        }
        let _ = SetFocus(edit);
    }
    // The cat sits at the bar's left, bottom-aligned, clear of the result list.
    crate::cat::attend(RECT {
        left: x,
        top: bottom - BASE_H,
        right: x + BAR_W,
        bottom,
    });
}

fn hide() {
    let Some(hwnd) = with_bar(|b| {
        b.visible = false;
        b.pending = None;
        b.results.clear();
        b.search_gen += 1;
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

/// Size and place the window for the current number of results, keeping the input where it is.
fn apply_layout(show: bool) {
    let Some((hwnd, edit, x, bottom, rows)) =
        with_bar(|b| (b.hwnd, b.edit, b.x, b.bottom, b.results.len().min(MAX_ROWS)))
    else {
        return;
    };
    let list_h = list_height(rows);
    let h = list_h + BASE_H;
    let flags = if show { SWP_SHOWWINDOW } else { SWP_NOACTIVATE };
    // SAFETY: valid windows owned by this thread; no state borrow is held across these calls.
    unsafe {
        let _ = SetWindowPos(hwnd, HWND_TOPMOST, x, bottom - h, BAR_W, h, flags);
        let _ = SetWindowPos(
            edit,
            None,
            22,
            list_h + 13,
            BAR_W - 44,
            28,
            SWP_NOZORDER | SWP_NOACTIVATE,
        );
        let _ = InvalidateRect(hwnd, None, true);
    }
}

fn set_preview(tone: Tone, text: String) {
    if let Some(hwnd) = with_bar(|b| {
        b.tone = tone;
        b.preview = text;
        b.hwnd
    }) {
        // SAFETY: valid window owned by this thread.
        unsafe {
            let _ = InvalidateRect(hwnd, None, false);
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

const UNKNOWN_HINT: &str = "I don't know that yet. Type \u{201c}help\u{201d} to see what I can do.";

/// Live preview while typing. Any edit also drops stale results and cancels a pending confirmation.
fn on_change() {
    let had_results = with_bar(|b| {
        b.pending = None;
        b.search_gen += 1;
        let had = !b.results.is_empty();
        b.results.clear();
        had
    })
    .unwrap_or(false);
    if had_results {
        apply_layout(false);
    }

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
        Routed::Unknown => set_preview(Tone::Idle, UNKNOWN_HINT.into()),
    }
}

/// Enter (or Ctrl+Enter when `open_it`).
fn submit(open_it: bool) {
    let text = read_input();

    // A result list is showing for this exact input: act on the selected result.
    let target = with_bar(|b| {
        if !b.results.is_empty() && b.results_for == text {
            b.results.get(b.selected).map(|h| h.path.clone())
        } else {
            None
        }
    })
    .flatten();
    if let Some(path) = target {
        activate(&path, open_it);
        return;
    }

    match route(&text) {
        Routed::Intent(Action::Find(spec)) => start_find(spec, text),
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
        Routed::Unknown => set_preview(Tone::Idle, UNKNOWN_HINT.into()),
    }
}

/// Search on a worker thread; the result comes back as `WM_APP_RESULTS`.
fn start_find(spec: FindSpec, text: String) {
    let Some((gen, hwnd)) = with_bar(|b| {
        b.search_gen += 1;
        b.results.clear();
        (b.search_gen, b.hwnd)
    }) else {
        return;
    };
    apply_layout(false);
    set_preview(Tone::Ready, "Searching\u{2026}".into());

    let req = Request::from_spec(&spec, MAX_ROWS);
    let hwnd_raw = hwnd.0 as isize; // HWND is not Send; the raw value is just a handle
    std::thread::spawn(move || {
        let outcome = search(&req);
        if let Ok(mut slot) = DELIVERY.lock() {
            *slot = Some(Delivery { gen, text, outcome });
        }
        // SAFETY: PostMessageW is safe to call from any thread; the window may be gone, which is fine.
        unsafe {
            let _ = PostMessageW(
                HWND(hwnd_raw as *mut c_void),
                WM_APP_RESULTS,
                WPARAM(0),
                LPARAM(0),
            );
        }
    });
}

fn on_results() {
    let Some(d) = DELIVERY.lock().ok().and_then(|mut s| s.take()) else {
        return;
    };
    let current = with_bar(|b| b.visible && b.search_gen == d.gen).unwrap_or(false);
    if !current {
        return; // the user typed again or closed the bar: stale
    }
    let n = d.outcome.hits.len();
    let ms = d.outcome.elapsed.as_millis();
    let basic = if d.outcome.source == Source::Walker {
        "  \u{2022}  basic search"
    } else {
        ""
    };
    with_bar(|b| {
        b.results = d.outcome.hits;
        b.selected = 0;
        b.results_for = d.text.clone();
    });
    apply_layout(false);
    if n == 0 {
        set_preview(
            Tone::Attention,
            format!(
                "No files found for \u{201c}{}\u{201d}{basic}",
                d.text.trim()
            ),
        );
    } else {
        set_preview(
            Tone::Done,
            format!(
                "{n} found in {ms} ms  \u{2022}  \u{2191}\u{2193} choose  \u{2022}  Enter: show in folder  \u{2022}  Ctrl+Enter: open{basic}"
            ),
        );
    }
}

fn move_selection(delta: i32) -> bool {
    let moved = with_bar(|b| {
        if b.results.is_empty() {
            return None;
        }
        let n = b.results.len() as i32;
        b.selected = (b.selected as i32 + delta).rem_euclid(n) as usize;
        Some(b.hwnd)
    })
    .flatten();
    match moved {
        Some(hwnd) => {
            // SAFETY: valid window owned by this thread.
            unsafe {
                let _ = InvalidateRect(hwnd, None, false);
            }
            true
        }
        None => false,
    }
}

/// Show a result in Explorer, or open it if asked and the type is on the safe list.
fn activate(path: &Path, open_it: bool) {
    let text = path.to_string_lossy().into_owned();
    // Index results are absolute local paths; refuse anything else rather than hand it to the shell.
    if paths::check(&text).is_err() || !path.is_absolute() {
        set_preview(Tone::Attention, "I won't open that location.".into());
        return;
    }
    // SAFETY: ShellExecuteW with literal verbs and a validated path, on the UI thread.
    unsafe {
        if open_it && paths::is_safe_to_open(&text) {
            ShellExecuteW(
                None,
                w!("open"),
                &HSTRING::from(text.as_str()),
                PCWSTR::null(),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            );
        } else {
            ShellExecuteW(
                None,
                w!("open"),
                w!("explorer.exe"),
                &HSTRING::from(format!("/select,\"{text}\"")),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            );
        }
    }
    hide();
}

fn clear_input() {
    if let Some(edit) = with_bar(|b| b.edit) {
        // SAFETY: valid window; this sends EN_CHANGE, so no state borrow may be held here.
        unsafe {
            let _ = SetWindowTextW(edit, w!(""));
        }
    }
}

/// Esc steps back one level: a pending confirmation, then the result list, then the bar.
fn escape() {
    enum Next {
        Preview,
        Results,
        Close,
    }
    let next = with_bar(|b| {
        if b.pending.take().is_some() {
            Next::Preview
        } else if !b.results.is_empty() {
            b.results.clear();
            b.search_gen += 1;
            Next::Results
        } else {
            Next::Close
        }
    })
    .unwrap_or(Next::Close);
    match next {
        Next::Preview => on_change(),
        Next::Results => {
            apply_layout(false);
            on_change();
        }
        Next::Close => hide(),
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
            let ctrl = GetKeyState(VK_CONTROL.0 as i32) < 0;
            submit(ctrl);
            return LRESULT(0);
        }
        WM_KEYDOWN if wparam.0 == VK_ESCAPE.0 as usize => {
            escape();
            return LRESULT(0);
        }
        WM_KEYDOWN if wparam.0 == VK_UP.0 as usize && move_selection(-1) => return LRESULT(0),
        WM_KEYDOWN if wparam.0 == VK_DOWN.0 as usize && move_selection(1) => return LRESULT(0),
        // Swallow the character messages for Enter, Ctrl+Enter and Esc so the control does not beep.
        WM_CHAR if matches!(wparam.0, 10 | 13 | 27) => return LRESULT(0),
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
        WM_APP_RESULTS => {
            on_results();
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

/// `C:\Users\me\Documents` -> `~\Documents`, to keep rows short and private-looking.
fn display_folder(path: &Path) -> String {
    let parent: PathBuf = path.parent().map(Path::to_path_buf).unwrap_or_default();
    let shown = parent.to_string_lossy().into_owned();
    match std::env::var("USERPROFILE") {
        Ok(home) if !home.is_empty() && shown.to_lowercase().starts_with(&home.to_lowercase()) => {
            format!("~{}", &shown[home.len()..])
        }
        _ => shown,
    }
}

struct PaintData {
    brush: HBRUSH,
    separator: HBRUSH,
    selected_brush: HBRUSH,
    font_row: HFONT,
    font_small: HFONT,
    tone: Tone,
    preview: String,
    rows: Vec<(String, String)>,
    selected: usize,
}

unsafe fn draw(
    hdc: HDC,
    text: &str,
    rect: &mut RECT,
    flags: windows::Win32::Graphics::Gdi::DRAW_TEXT_FORMAT,
) {
    let mut wide: Vec<u16> = text.encode_utf16().collect();
    DrawTextW(
        hdc,
        &mut wide,
        rect,
        flags | DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | DT_NOPREFIX,
    );
}

unsafe fn paint(hwnd: HWND) {
    let mut ps = PAINTSTRUCT::default();
    let hdc = BeginPaint(hwnd, &mut ps);
    let data = with_bar(|b| PaintData {
        brush: b.brush,
        separator: b.separator,
        selected_brush: b.selected_brush,
        font_row: b.font_row,
        font_small: b.font_small,
        tone: b.tone,
        preview: b.preview.clone(),
        rows: b
            .results
            .iter()
            .take(MAX_ROWS)
            .map(|h| {
                let name = h
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                (name, display_folder(&h.path))
            })
            .collect(),
        selected: b.selected,
    });
    if let Some(d) = data {
        FillRect(hdc, &ps.rcPaint, d.brush);
        SetBkMode(hdc, TRANSPARENT);

        let list_h = list_height(d.rows.len());
        for (i, (name, folder)) in d.rows.iter().enumerate() {
            let top = LIST_PAD / 2 + i as i32 * ROW_H;
            if i == d.selected {
                let r = RECT {
                    left: 10,
                    top,
                    right: BAR_W - 10,
                    bottom: top + ROW_H - 2,
                };
                FillRect(hdc, &r, d.selected_brush);
            }
            SelectObject(hdc, HGDIOBJ(d.font_row.0));
            SetTextColor(hdc, INPUT_TEXT);
            let mut r = RECT {
                left: 24,
                top,
                right: 24 + 330,
                bottom: top + ROW_H - 2,
            };
            draw(hdc, name, &mut r, DT_LEFT);

            SelectObject(hdc, HGDIOBJ(d.font_small.0));
            SetTextColor(hdc, DIM_TEXT);
            let mut r = RECT {
                left: 24 + 340,
                top,
                right: BAR_W - 24,
                bottom: top + ROW_H - 2,
            };
            draw(hdc, folder, &mut r, DT_RIGHT);
        }
        if list_h > 0 {
            let line = RECT {
                left: 20,
                top: list_h - 1,
                right: BAR_W - 20,
                bottom: list_h,
            };
            FillRect(hdc, &line, d.separator);
        }

        let line = RECT {
            left: 20,
            top: list_h + INPUT_H,
            right: BAR_W - 20,
            bottom: list_h + INPUT_H + 1,
        };
        FillRect(hdc, &line, d.separator);

        SelectObject(hdc, HGDIOBJ(d.font_small.0));
        SetTextColor(hdc, d.tone.color());
        let mut r = RECT {
            left: 22,
            top: list_h + INPUT_H + 1,
            right: BAR_W - 22,
            bottom: list_h + BASE_H,
        };
        draw(hdc, &format!("\u{25CF}  {}", d.preview), &mut r, DT_LEFT);
    }
    let _ = EndPaint(hwnd, &ps);
}
