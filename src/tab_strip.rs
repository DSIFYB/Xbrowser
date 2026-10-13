use std::{io, mem, ptr, sync::OnceLock};

use windows_sys::Win32::{
    Foundation::{GetLastError, HWND, LPARAM, RECT, WPARAM},
    Graphics::Gdi::{
        BeginPaint, CreateFontW, CreatePen, CreateSolidBrush, DeleteObject, DrawTextW, Ellipse,
        EndPaint, FillRect, InvalidateRect, LineTo, MoveToEx, RoundRect, SelectObject, SetBkMode,
        SetTextColor, CLEARTYPE_QUALITY, DEFAULT_CHARSET, DEFAULT_PITCH, DT_CENTER,
        DT_END_ELLIPSIS, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, FW_NORMAL, OUT_DEFAULT_PRECIS,
        PAINTSTRUCT, TRANSPARENT,
    },
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        Controls::WM_MOUSELEAVE,
        Input::KeyboardAndMouse::{TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT},
        WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, GetWindowLongPtrW, RegisterClassW, SetWindowLongPtrW,
            SetWindowPos, GWLP_USERDATA, SWP_NOACTIVATE, SWP_NOZORDER, WM_ERASEBKGND,
            WM_LBUTTONDOWN, WM_MBUTTONDOWN, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCCREATE, WM_NCDESTROY,
            WM_PAINT, WM_SIZE, WNDCLASSW, WS_CHILD, WS_VISIBLE,
        },
    },
};

const CLASS_NAME: &str = "AxiomBrowserTabStrip";
const STRIP_COLOR: u32 = rgb(222, 228, 239);
const ACTIVE_COLOR: u32 = rgb(255, 255, 255);
const TEXT_COLOR: u32 = rgb(35, 43, 58);
const MUTED_COLOR: u32 = rgb(110, 122, 143);
const DIVIDER_COLOR: u32 = rgb(190, 199, 214);

static CLASS_REGISTERED: OnceLock<Result<(), u32>> = OnceLock::new();

pub struct TabStrip {
    pub hwnd: HWND,
}

struct State {
    entries: Vec<(u64, String)>,
    active: Option<u64>,
    scroll_index: usize,
    width: i32,
    height: i32,
    scale: f64,
    hover: Option<Hit>,
    font: windows_sys::Win32::Graphics::Gdi::HFONT,
    window_owned: bool,
}
impl Drop for State {
    fn drop(&mut self) {
        unsafe {
            if !self.font.is_null() {
                DeleteObject(self.font);
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hit {
    Tab(usize),
    Close(usize),
    New,
    ScrollLeft,
    ScrollRight,
}

#[derive(Clone, Copy)]
struct TabBounds {
    index: usize,
    x: i32,
    width: i32,
}

struct Layout {
    tabs: Vec<TabBounds>,
    plus_x: i32,
    left_arrow_x: Option<i32>,
    right_arrow_x: Option<i32>,
}

impl TabStrip {
    pub fn new(parent: HWND) -> io::Result<Self> {
        register_class()?;
        let instance = unsafe { GetModuleHandleW(ptr::null()) };
        let font = unsafe {
            CreateFontW(
                -16,
                0,
                0,
                0,
                FW_NORMAL as i32,
                0,
                0,
                0,
                DEFAULT_CHARSET as u32,
                OUT_DEFAULT_PRECIS as u32,
                0,
                CLEARTYPE_QUALITY as u32,
                DEFAULT_PITCH as u32,
                wide("Segoe UI").as_ptr(),
            )
        };
        if font.is_null() {
            return Err(io::Error::last_os_error());
        }

        let state = Box::new(State {
            entries: Vec::new(),
            active: None,
            scroll_index: 0,
            width: 600,
            height: 44,
            scale: 1.0,
            hover: None,
            font,
            window_owned: false,
        });
        let state = Box::into_raw(state);
        let class_name = wide(CLASS_NAME);
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class_name.as_ptr(),
                wide("").as_ptr(),
                WS_CHILD | WS_VISIBLE,
                0,
                0,
                600,
                44,
                parent,
                ptr::null_mut(),
                instance,
                state.cast(),
            )
        };
        if hwnd.is_null() {
            // If creation failed before WM_NCCREATE attached the state, this is still ours.
            unsafe { drop(Box::from_raw(state)) };
            return Err(io::Error::last_os_error());
        }
        unsafe {
            (*state).window_owned = true;
        }

        Ok(Self { hwnd })
    }

    pub fn update(&self, entries: &[(u64, String)], active: Option<u64>) -> io::Result<()> {
        unsafe {
            let state = state_mut(self.hwnd).ok_or_else(io::Error::last_os_error)?;
            state.entries.clear();
            state.entries.extend(entries.iter().cloned());
            state.active = active;
            state.scroll_index = state
                .scroll_index
                .min(state.entries.len().saturating_sub(1));
            keep_active_visible(state);
            if InvalidateRect(self.hwnd, ptr::null(), 0) == 0 {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(())
    }

    pub fn resize(&self, width: u32, scale: f64) {
        let scale = if scale.is_finite() && scale > 0.0 {
            scale.clamp(0.5, 4.0)
        } else {
            1.0
        };
        let width = width.min(i32::MAX as u32) as i32;
        let height = scaled(44, scale);
        unsafe {
            if let Some(state) = state_mut(self.hwnd) {
                if state.scale != scale {
                    let font = CreateFontW(
                        -scaled(16, scale),
                        0,
                        0,
                        0,
                        400,
                        0,
                        0,
                        0,
                        DEFAULT_CHARSET as u32,
                        OUT_DEFAULT_PRECIS as u32,
                        0,
                        CLEARTYPE_QUALITY as u32,
                        DEFAULT_PITCH as u32,
                        wide("Segoe UI").as_ptr(),
                    );
                    if !font.is_null() {
                        DeleteObject(state.font);
                        state.font = font;
                    }
                }
                state.width = width;
                state.height = height;
                state.scale = scale;
                keep_active_visible(state);
            }
            SetWindowPos(
                self.hwnd,
                ptr::null_mut(),
                0,
                0,
                width,
                height,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
            InvalidateRect(self.hwnd, ptr::null(), 0);
        }
    }
}

fn register_class() -> io::Result<()> {
    let result = *CLASS_REGISTERED.get_or_init(|| unsafe {
        let instance = GetModuleHandleW(ptr::null());
        let class_name = wide(CLASS_NAME);
        let class = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(tab_strip_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: instance,
            hIcon: ptr::null_mut(),
            hCursor: ptr::null_mut(),
            hbrBackground: ptr::null_mut(),
            lpszMenuName: ptr::null(),
            lpszClassName: class_name.as_ptr(),
        };
        if RegisterClassW(&class) != 0 {
            Ok(())
        } else {
            let error = GetLastError();
            if error == 1410 {
                Ok(())
            } else {
                Err(error)
            }
        }
    });
    result.map_err(|error| io::Error::from_raw_os_error(error as i32))
}

unsafe extern "system" fn tab_strip_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> isize {
    if msg == WM_NCCREATE {
        let create = &*(lp as *const windows_sys::Win32::UI::WindowsAndMessaging::CREATESTRUCTW);
        let state = create.lpCreateParams as *mut State;
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, state as isize);
        return 1;
    }

    if msg == WM_NCDESTROY {
        let state = state_ptr(hwnd);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        if !state.is_null() && (*state).window_owned {
            drop(Box::from_raw(state));
        }
        return DefWindowProcW(hwnd, msg, wp, lp);
    }

    let Some(state) = state_mut(hwnd) else {
        return DefWindowProcW(hwnd, msg, wp, lp);
    };

    match msg {
        WM_ERASEBKGND => 1,
        WM_SIZE => {
            state.width = loword(lp as usize) as i32;
            state.height = hiword(lp as usize) as i32;
            keep_active_visible(state);
            InvalidateRect(hwnd, ptr::null(), 0);
            0
        }
        WM_PAINT => {
            paint(hwnd, state);
            0
        }
        WM_MOUSEMOVE => {
            let x = signed_loword(lp as usize);
            let y = signed_hiword(lp as usize);
            let hit = hit_test(state, x, y);
            if state.hover != hit {
                state.hover = hit;
                InvalidateRect(hwnd, ptr::null(), 0);
            }
            let mut tracking = TRACKMOUSEEVENT {
                cbSize: mem::size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: hwnd,
                dwHoverTime: 0,
            };
            TrackMouseEvent(&mut tracking);
            0
        }
        WM_MOUSELEAVE => {
            state.hover = None;
            InvalidateRect(hwnd, ptr::null(), 0);
            0
        }
        WM_LBUTTONDOWN => {
            match hit_test(
                state,
                signed_loword(lp as usize),
                signed_hiword(lp as usize),
            ) {
                Some(Hit::Tab(index)) => {
                    if let Some((id, _)) = state.entries.get(index) {
                        crate::send(crate::Action::SelectTabId(*id));
                    }
                }
                Some(Hit::Close(index)) => close_at(state, index),
                Some(Hit::New) => crate::send(crate::Action::NewTab),
                Some(Hit::ScrollLeft) => {
                    state.scroll_index = state.scroll_index.saturating_sub(1);
                    InvalidateRect(hwnd, ptr::null(), 0);
                }
                Some(Hit::ScrollRight) => {
                    state.scroll_index =
                        (state.scroll_index + 1).min(state.entries.len().saturating_sub(1));
                    InvalidateRect(hwnd, ptr::null(), 0);
                }
                None => {}
            }
            0
        }
        WM_MBUTTONDOWN => {
            if let Some(Hit::Tab(index) | Hit::Close(index)) = hit_test(
                state,
                signed_loword(lp as usize),
                signed_hiword(lp as usize),
            ) {
                close_at(state, index);
            }
            0
        }
        WM_MOUSEWHEEL => {
            let delta = (wp >> 16) as i16;
            if delta > 0 {
                state.scroll_index = state.scroll_index.saturating_sub(1);
            } else if delta < 0 {
                state.scroll_index =
                    (state.scroll_index + 1).min(state.entries.len().saturating_sub(1));
            }
            InvalidateRect(hwnd, ptr::null(), 0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

unsafe fn paint(hwnd: HWND, state: &State) {
    let mut paint = PAINTSTRUCT::default();
    let hdc = BeginPaint(hwnd, &mut paint);
    let strip_brush = CreateSolidBrush(STRIP_COLOR);
    let active_brush = CreateSolidBrush(ACTIVE_COLOR);
    let active_pen = CreatePen(0, 1, ACTIVE_COLOR);
    let divider_pen = CreatePen(0, 1, DIVIDER_COLOR);
    let hover_pen = CreatePen(0, 1, rgb(207, 216, 231));
    let dot_brush = CreateSolidBrush(rgb(76, 126, 210));
    let hover_brush = CreateSolidBrush(rgb(207, 216, 231));
    let client = RECT {
        left: 0,
        top: 0,
        right: state.width,
        bottom: state.height,
    };
    FillRect(hdc, &client, strip_brush);
    let old_font = SelectObject(hdc, state.font);
    let old_pen = SelectObject(hdc, divider_pen);
    SetBkMode(hdc, TRANSPARENT as i32);
    let layout = layout(state);

    for bounds in &layout.tabs {
        let (id, title) = &state.entries[bounds.index];
        let top = scaled(4, state.scale);
        let bottom = state.height - scaled(2, state.scale);
        let is_active = Some(*id) == state.active;
        let is_hovered = state.hover == Some(Hit::Tab(bounds.index));
        if is_active {
            let old_brush = SelectObject(hdc, active_brush);
            let old_pen = SelectObject(hdc, active_pen);
            RoundRect(
                hdc,
                bounds.x,
                top,
                bounds.x + bounds.width,
                bottom,
                scaled(12, state.scale),
                scaled(12, state.scale),
            );
            SelectObject(hdc, old_brush);
            SelectObject(hdc, old_pen);
        } else if is_hovered {
            let old_brush = SelectObject(hdc, hover_brush);
            let old_pen = SelectObject(hdc, hover_pen);
            RoundRect(
                hdc,
                bounds.x,
                top,
                bounds.x + bounds.width,
                bottom,
                scaled(10, state.scale),
                scaled(10, state.scale),
            );
            SelectObject(hdc, old_brush);
            SelectObject(hdc, old_pen);
        } else {
            MoveToEx(
                hdc,
                bounds.x + bounds.width,
                top + scaled(7, state.scale),
                ptr::null_mut(),
            );
            LineTo(
                hdc,
                bounds.x + bounds.width,
                bottom - scaled(7, state.scale),
            );
        }

        SetTextColor(hdc, TEXT_COLOR);
        let dot_x = bounds.x + scaled(14, state.scale);
        let dot_y = state.height / 2;
        let old_brush = SelectObject(hdc, dot_brush);
        Ellipse(
            hdc,
            dot_x - scaled(4, state.scale),
            dot_y - scaled(4, state.scale),
            dot_x + scaled(4, state.scale),
            dot_y + scaled(4, state.scale),
        );
        SelectObject(hdc, old_brush);

        let text_left = bounds.x + scaled(25, state.scale);
        let close_width = scaled(28, state.scale);
        let mut text_rect = RECT {
            left: text_left,
            top,
            right: bounds.x + bounds.width - close_width,
            bottom,
        };
        let title = wide(title);
        DrawTextW(
            hdc,
            title.as_ptr(),
            -1,
            &mut text_rect,
            DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | DT_NOPREFIX,
        );

        let close_left = bounds.x + bounds.width - scaled(27, state.scale);
        let mut close_rect = RECT {
            left: close_left,
            top,
            right: bounds.x + bounds.width - scaled(4, state.scale),
            bottom,
        };
        if state.hover == Some(Hit::Close(bounds.index)) {
            SetTextColor(hdc, rgb(170, 55, 62));
        } else {
            SetTextColor(hdc, MUTED_COLOR);
        }
        draw_text(hdc, "×", &mut close_rect);
    }

    if let Some(x) = layout.left_arrow_x {
        SetTextColor(hdc, MUTED_COLOR);
        let mut rect = button_rect(x, state.height, state.scale);
        draw_text(hdc, "‹", &mut rect);
    }
    if let Some(x) = layout.right_arrow_x {
        SetTextColor(hdc, MUTED_COLOR);
        let mut rect = button_rect(x, state.height, state.scale);
        draw_text(hdc, "›", &mut rect);
    }
    let plus_rect = button_rect(layout.plus_x, state.height, state.scale);
    if state.hover == Some(Hit::New) {
        FillRect(hdc, &plus_rect, hover_brush);
    }
    SetTextColor(hdc, MUTED_COLOR);
    let mut plus_rect = plus_rect;
    draw_text(hdc, "+", &mut plus_rect);

    SelectObject(hdc, old_font);
    SelectObject(hdc, old_pen);
    EndPaint(hwnd, &paint);
    DeleteObject(strip_brush);
    DeleteObject(active_brush);
    DeleteObject(active_pen);
    DeleteObject(divider_pen);
    DeleteObject(hover_pen);
    DeleteObject(dot_brush);
    DeleteObject(hover_brush);
}

unsafe fn draw_text(hdc: windows_sys::Win32::Graphics::Gdi::HDC, text: &str, rect: &mut RECT) {
    let text = wide(text);
    DrawTextW(
        hdc,
        text.as_ptr(),
        -1,
        rect,
        DT_SINGLELINE | DT_VCENTER | DT_CENTER | DT_END_ELLIPSIS | DT_NOPREFIX,
    );
}

unsafe fn hit_test(state: &State, x: i32, y: i32) -> Option<Hit> {
    if y < 0 || y >= state.height {
        return None;
    }
    let layout = layout(state);
    if let Some(left) = layout.left_arrow_x {
        if x >= left && x < left + button_width(state.scale) {
            return Some(Hit::ScrollLeft);
        }
    }
    if let Some(right) = layout.right_arrow_x {
        if x >= right && x < right + button_width(state.scale) {
            return Some(Hit::ScrollRight);
        }
    }
    for bounds in layout.tabs {
        if x >= bounds.x && x < bounds.x + bounds.width {
            if x >= bounds.x + bounds.width - scaled(32, state.scale) {
                return Some(Hit::Close(bounds.index));
            }
            return Some(Hit::Tab(bounds.index));
        }
    }
    if x >= layout.plus_x && x < layout.plus_x + button_width(state.scale) {
        return Some(Hit::New);
    }
    None
}

fn layout(state: &State) -> Layout {
    let scale = state.scale;
    let plus_width = button_width(scale);
    let arrow_width = button_width(scale);
    let total_width: i32 = state
        .entries
        .iter()
        .map(|(_, title)| tab_width(title, scale))
        .sum();
    let overflow = total_width > state.width - plus_width;
    let left_arrow_x = (overflow && state.scroll_index > 0).then_some(0);
    let viewport_left = if overflow { arrow_width } else { 0 };
    let viewport_right = if overflow {
        state.width - plus_width - arrow_width
    } else {
        state.width - plus_width
    }
    .max(viewport_left);
    let mut tabs = Vec::new();
    let mut x = viewport_left;
    for index in state.scroll_index..state.entries.len() {
        let width = tab_width(&state.entries[index].1, scale);
        if x + width > viewport_right {
            break;
        }
        tabs.push(TabBounds { index, x, width });
        x += width;
    }
    let right_arrow_x = (overflow
        && tabs
            .last()
            .is_some_and(|tab| tab.index + 1 < state.entries.len()))
    .then_some((state.width - plus_width - arrow_width).max(viewport_left));
    let plus_x = if overflow {
        state.width - plus_width
    } else {
        x.min(state.width - plus_width)
    }
    .max(0);

    Layout {
        tabs,
        plus_x,
        left_arrow_x,
        right_arrow_x,
    }
}

fn keep_active_visible(state: &mut State) {
    let Some(active_index) = state
        .entries
        .iter()
        .position(|(id, _)| Some(*id) == state.active)
    else {
        return;
    };
    if active_index < state.scroll_index {
        state.scroll_index = active_index;
        return;
    }
    let plus_width = button_width(state.scale);
    let total_width: i32 = state
        .entries
        .iter()
        .map(|(_, title)| tab_width(title, state.scale))
        .sum();
    let overflow = total_width > state.width - plus_width;
    let left = if overflow {
        button_width(state.scale)
    } else {
        0
    };
    let right = if overflow {
        state.width - plus_width - button_width(state.scale)
    } else {
        state.width - plus_width
    };
    let mut x = left;
    for index in state.scroll_index..=active_index {
        if index == active_index && x + tab_width(&state.entries[index].1, state.scale) > right {
            state.scroll_index = index;
            while state.scroll_index > 0 {
                let visible_width: i32 = state.entries[state.scroll_index - 1..=active_index]
                    .iter()
                    .map(|(_, title)| tab_width(title, state.scale))
                    .sum();
                if visible_width > right - left {
                    break;
                }
                state.scroll_index -= 1;
            }
            return;
        }
        x += tab_width(&state.entries[index].1, state.scale);
    }
}

fn close_at(state: &State, index: usize) {
    if let Some((id, _)) = state.entries.get(index) {
        crate::send(crate::Action::CloseTabId(*id));
    }
}

fn tab_width(title: &str, scale: f64) -> i32 {
    let chars = title.chars().count().min(64) as i32;
    (scaled(70, scale) + chars * scaled(7, scale)).clamp(scaled(160, scale), scaled(240, scale))
}

fn button_width(scale: f64) -> i32 {
    scaled(32, scale)
}

fn scaled(value: i32, scale: f64) -> i32 {
    ((value as f64 * scale).round() as i32).max(1)
}

fn button_rect(x: i32, height: i32, scale: f64) -> RECT {
    let width = button_width(scale);
    RECT {
        left: x,
        top: 0,
        right: x + width,
        bottom: height,
    }
}

unsafe fn state_ptr(hwnd: HWND) -> *mut State {
    GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut State
}

unsafe fn state_mut(hwnd: HWND) -> Option<&'static mut State> {
    state_ptr(hwnd).as_mut()
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

const fn rgb(red: u8, green: u8, blue: u8) -> u32 {
    red as u32 | ((green as u32) << 8) | ((blue as u32) << 16)
}

fn loword(value: usize) -> u16 {
    value as u16
}

fn hiword(value: usize) -> u16 {
    (value >> 16) as u16
}

fn signed_loword(value: usize) -> i32 {
    loword(value) as i16 as i32
}

fn signed_hiword(value: usize) -> i32 {
    hiword(value) as i16 as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    fn tabs(width: i32, scale: f64) -> State {
        State {
            entries: (0..20)
                .map(|id| (id, format!("A long page title {id}")))
                .collect(),
            active: Some(19),
            scroll_index: 0,
            width,
            height: scaled(44, scale),
            scale,
            hover: None,
            font: ptr::null_mut(),
            window_owned: false,
        }
    }
    #[test]
    fn overflow_keeps_active_tab_visible_at_multiple_scales() {
        for scale in [1.0, 1.5, 2.0] {
            let mut state = tabs(scaled(500, scale), scale);
            keep_active_visible(&mut state);
            let layout = layout(&state);
            assert!(layout.tabs.iter().any(|tab| tab.index == 19));
            assert_eq!(
                unsafe { hit_test(&state, layout.plus_x + scaled(16, scale), scaled(20, scale)) },
                Some(Hit::New)
            );
        }
    }
    #[test]
    fn scroll_arrow_and_new_tab_have_distinct_hit_targets() {
        let state = tabs(500, 1.0);
        let layout = layout(&state);
        let right = layout.right_arrow_x.unwrap();
        assert!(right + button_width(1.0) <= layout.plus_x);
        assert!(matches!(
            unsafe { hit_test(&state, right + 8, 20) },
            Some(Hit::ScrollRight)
        ));
        assert!(matches!(
            unsafe { hit_test(&state, layout.plus_x + 8, 20) },
            Some(Hit::New)
        ));
    }
}
