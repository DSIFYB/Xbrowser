use crate::wide;
use std::{io, ptr};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        Controls::{
            DRAWITEMSTRUCT, EM_SETCUEBANNER, EM_SETMARGINS, ODS_DISABLED, ODS_SELECTED,
            WM_MOUSELEAVE,
        },
        HiDpi::GetDpiForWindow,
        Input::KeyboardAndMouse::{
            EnableWindow, GetFocus, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT,
        },
        Shell::{DefSubclassProc, SetWindowSubclass},
        WindowsAndMessaging::*,
    },
};

pub const HEIGHT: f64 = 100.0;
pub struct Toolbar {
    pub edit: HWND,
    buttons: [HWND; 6],
    font: HFONT,
}
fn rgb(r: u32, g: u32, b: u32) -> u32 {
    r | g << 8 | b << 16
}
fn px(value: f64, scale: f64) -> i32 {
    (value * scale).round() as i32
}

impl Toolbar {
    pub fn new(parent: HWND) -> io::Result<Self> {
        let mut buttons = [ptr::null_mut(); 6];
        unsafe {
            let instance = GetModuleHandleW(ptr::null());
            for (index, id) in [1, 2, 3, 5, 10, 9].into_iter().enumerate() {
                buttons[index] = CreateWindowExW(
                    0,
                    wide("BUTTON").as_ptr(),
                    wide(["Назад", "Вперёд", "Обновить", "Перейти", "☆", "Меню"][index]).as_ptr(),
                    WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_OWNERDRAW as u32,
                    0,
                    0,
                    36,
                    36,
                    parent,
                    id as HMENU,
                    instance,
                    ptr::null(),
                );
                if buttons[index].is_null() {
                    return Err(io::Error::last_os_error());
                }
                if SetWindowSubclass(buttons[index], Some(button_proc), 1, 0) == 0 {
                    return Err(io::Error::last_os_error());
                }
            }
            let edit = CreateWindowExW(
                0,
                wide("EDIT").as_ptr(),
                wide("").as_ptr(),
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | ES_AUTOHSCROLL as u32,
                0,
                0,
                400,
                28,
                parent,
                4 as HMENU,
                instance,
                ptr::null(),
            );
            if edit.is_null() {
                return Err(io::Error::last_os_error());
            }
            SendMessageW(
                edit,
                EM_SETCUEBANNER,
                1,
                wide("Введите адрес или поисковый запрос").as_ptr() as isize,
            );
            SendMessageW(
                edit,
                EM_SETMARGINS,
                (EC_LEFTMARGIN | EC_RIGHTMARGIN) as usize,
                0,
            );
            Ok(Self {
                edit,
                buttons,
                font: ptr::null_mut(),
            })
        }
    }
    pub fn resize(&mut self, width: u32, scale: f64) {
        unsafe {
            for (index, x) in [12.0, 54.0, 96.0].into_iter().enumerate() {
                MoveWindow(
                    self.buttons[index],
                    px(x, scale),
                    px(54.0, scale),
                    px(36.0, scale),
                    px(36.0, scale),
                    1,
                );
            }
            for (index, offset) in [(3, 96.0), (4, 52.0), (5, 8.0)] {
                MoveWindow(
                    self.buttons[index],
                    width as i32 - px(offset + 36.0, scale),
                    px(54.0, scale),
                    px(36.0, scale),
                    px(36.0, scale),
                    1,
                );
            }
            MoveWindow(
                self.edit,
                px(180.0, scale),
                px(60.0, scale),
                (width as i32 - px(346.0, scale)).max(60),
                px(26.0, scale),
                1,
            );
            let font = CreateFontW(
                -px(15.0, scale),
                0,
                0,
                0,
                400,
                0,
                0,
                0,
                DEFAULT_CHARSET as u32,
                OUT_DEFAULT_PRECIS as u32,
                CLIP_DEFAULT_PRECIS as u32,
                CLEARTYPE_QUALITY as u32,
                DEFAULT_PITCH as u32,
                wide("Segoe UI").as_ptr(),
            );
            if !font.is_null() {
                SendMessageW(self.edit, WM_SETFONT, font as usize, 1);
                if !self.font.is_null() {
                    DeleteObject(self.font);
                }
                self.font = font;
            }
            InvalidateRect(GetParent(self.edit), ptr::null(), 0);
        }
    }
    pub fn set_history(&self, back: bool, forward: bool) {
        unsafe {
            EnableWindow(self.buttons[0], back as i32);
            EnableWindow(self.buttons[1], forward as i32);
        }
    }
    pub fn set_bookmarked(&self, saved: bool) {
        unsafe {
            SetWindowTextW(
                self.buttons[4],
                wide(if saved { "★" } else { "☆" }).as_ptr(),
            );
            InvalidateRect(self.buttons[4], ptr::null(), 0);
        }
    }
}
impl Drop for Toolbar {
    fn drop(&mut self) {
        unsafe {
            if !self.font.is_null() {
                DeleteObject(self.font);
            }
        }
    }
}

pub unsafe fn paint(hwnd: HWND) {
    let mut ps: PAINTSTRUCT = std::mem::zeroed();
    let dc = BeginPaint(hwnd, &mut ps);
    let mut client: RECT = std::mem::zeroed();
    GetClientRect(hwnd, &mut client);
    let scale = GetDpiForWindow(hwnd).max(96) as f64 / 96.0;
    FillRect(dc, &client, GetStockObject(WHITE_BRUSH) as HBRUSH);
    let bar = RECT {
        left: 0,
        top: px(44.0, scale),
        right: client.right,
        bottom: px(HEIGHT, scale),
    };
    let bg = CreateSolidBrush(rgb(248, 250, 253));
    FillRect(dc, &bar, bg);
    DeleteObject(bg);
    let focused = GetFocus() == GetDlgItem(hwnd, 4);
    let pen = CreatePen(
        PS_SOLID,
        px(1.0, scale),
        if focused {
            rgb(108, 149, 235)
        } else {
            rgb(226, 231, 240)
        },
    );
    let old_pen = SelectObject(dc, pen);
    let old_brush = SelectObject(dc, GetStockObject(WHITE_BRUSH));
    RoundRect(
        dc,
        px(146.0, scale),
        px(54.0, scale),
        client.right - px(154.0, scale),
        px(90.0, scale),
        px(36.0, scale),
        px(36.0, scale),
    );
    SelectObject(dc, old_pen);
    SelectObject(dc, old_brush);
    DeleteObject(pen);
    let pen = CreatePen(PS_SOLID, px(1.5, scale), rgb(131, 144, 164));
    let old = SelectObject(dc, pen);
    let brush = SelectObject(dc, GetStockObject(NULL_BRUSH));
    Ellipse(
        dc,
        px(159.0, scale),
        px(64.0, scale),
        px(171.0, scale),
        px(76.0, scale),
    );
    MoveToEx(dc, px(169.0, scale), px(74.0, scale), ptr::null_mut());
    LineTo(dc, px(174.0, scale), px(79.0, scale));
    SelectObject(dc, old);
    SelectObject(dc, brush);
    DeleteObject(pen);
    let pen = CreatePen(PS_SOLID, 1, rgb(230, 234, 241));
    let old = SelectObject(dc, pen);
    MoveToEx(dc, 0, bar.bottom - 1, ptr::null_mut());
    LineTo(dc, client.right, bar.bottom - 1);
    SelectObject(dc, old);
    DeleteObject(pen);
    EndPaint(hwnd, &ps);
}
pub unsafe fn color_edit(dc: HDC) -> LRESULT {
    SetTextColor(dc, rgb(42, 54, 76));
    SetBkColor(dc, rgb(255, 255, 255));
    GetStockObject(WHITE_BRUSH) as LRESULT
}
pub unsafe fn paint_placeholder(hwnd: HWND) {
    if GetWindowTextLengthW(hwnd) != 0 || GetFocus() == hwnd {
        return;
    }
    let dc = GetDC(hwnd);
    let mut rect: RECT = std::mem::zeroed();
    GetClientRect(hwnd, &mut rect);
    let font = SendMessageW(hwnd, WM_GETFONT, 0, 0);
    let old = SelectObject(dc, font as _);
    SetBkMode(dc, TRANSPARENT as i32);
    SetTextColor(dc, rgb(142, 151, 167));
    DrawTextW(
        dc,
        wide("Введите адрес или поисковый запрос").as_ptr(),
        -1,
        &mut rect,
        DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | DT_NOPREFIX,
    );
    SelectObject(dc, old);
    ReleaseDC(hwnd, dc);
}
pub unsafe fn draw_button(item: &DRAWITEMSTRUCT) {
    let scale = GetDpiForWindow(item.hwndItem).max(96) as f64 / 96.0;
    let dc = item.hDC;
    let rect = item.rcItem;
    let mut cursor = POINT { x: 0, y: 0 };
    GetCursorPos(&mut cursor);
    ScreenToClient(item.hwndItem, &mut cursor);
    let hover = cursor.x >= 0 && cursor.y >= 0 && cursor.x < rect.right && cursor.y < rect.bottom;
    let bg = CreateSolidBrush(rgb(248, 250, 253));
    FillRect(dc, &rect, bg);
    DeleteObject(bg);
    if hover || item.itemState & ODS_SELECTED != 0 {
        let bg = CreateSolidBrush(if item.itemState & ODS_SELECTED != 0 {
            rgb(217, 226, 240)
        } else {
            rgb(232, 237, 245)
        });
        let old = SelectObject(dc, bg);
        let oldp = SelectObject(dc, GetStockObject(NULL_PEN));
        RoundRect(
            dc,
            rect.left,
            rect.top,
            rect.right,
            rect.bottom,
            px(18.0, scale),
            px(18.0, scale),
        );
        SelectObject(dc, old);
        SelectObject(dc, oldp);
        DeleteObject(bg);
    }
    let color = if item.itemState & ODS_DISABLED != 0 {
        rgb(185, 193, 206)
    } else {
        rgb(76, 91, 114)
    };
    let pen = CreatePen(PS_SOLID, px(1.7, scale), color);
    let oldp = SelectObject(dc, pen);
    let oldb = SelectObject(dc, GetStockObject(NULL_BRUSH));
    let x = (rect.left + rect.right) / 2;
    let y = (rect.top + rect.bottom) / 2;
    let d = px(6.0, scale);
    match item.CtlID {
        1 | 2 | 5 => {
            let direction = if item.CtlID == 1 { -1 } else { 1 };
            MoveToEx(dc, x - direction * d, y - d, ptr::null_mut());
            LineTo(dc, x + direction * d, y);
            LineTo(dc, x - direction * d, y + d);
            MoveToEx(dc, x - direction * d, y, ptr::null_mut());
            LineTo(dc, x + direction * d, y);
        }
        3 => {
            Arc(dc, x - d, y - d, x + d, y + d, x, y - d, x + d, y);
            MoveToEx(dc, x + d / 2, y - d / 2, ptr::null_mut());
            LineTo(dc, x + d, y);
            LineTo(dc, x + d + d / 2, y - d / 2);
        }
        10 => {
            let mut text = [0u16; 3];
            GetWindowTextW(item.hwndItem, text.as_mut_ptr(), 3);
            SetTextColor(
                dc,
                if text[0] == 0x2605 {
                    rgb(55, 104, 218)
                } else {
                    color
                },
            );
            SetBkMode(dc, TRANSPARENT as i32);
            let font = CreateFontW(
                -px(24.0, scale),
                0,
                0,
                0,
                400,
                0,
                0,
                0,
                DEFAULT_CHARSET as u32,
                0,
                0,
                CLEARTYPE_QUALITY as u32,
                0,
                wide("Segoe UI").as_ptr(),
            );
            let old = SelectObject(dc, font);
            let mut rect = item.rcItem;
            DrawTextW(
                dc,
                text.as_ptr(),
                -1,
                &mut rect,
                DT_SINGLELINE | DT_CENTER | DT_VCENTER,
            );
            SelectObject(dc, old);
            DeleteObject(font);
        }
        9 => {
            let brush = CreateSolidBrush(color);
            let old = SelectObject(dc, brush);
            for offset in [-5.0, 0.0, 5.0] {
                let y = y + px(offset, scale);
                Ellipse(
                    dc,
                    x - px(1.2, scale),
                    y - px(1.2, scale),
                    x + px(1.2, scale),
                    y + px(1.2, scale),
                );
            }
            SelectObject(dc, old);
            DeleteObject(brush);
        }
        _ => {}
    }
    SelectObject(dc, oldp);
    SelectObject(dc, oldb);
    DeleteObject(pen);
}
unsafe extern "system" fn button_proc(
    hwnd: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
    _: usize,
    _: usize,
) -> LRESULT {
    if msg == WM_MOUSEMOVE {
        let mut track = TRACKMOUSEEVENT {
            cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
            dwFlags: TME_LEAVE,
            hwndTrack: hwnd,
            dwHoverTime: 0,
        };
        TrackMouseEvent(&mut track);
        InvalidateRect(hwnd, ptr::null(), 0);
    } else if msg == WM_MOUSELEAVE {
        InvalidateRect(hwnd, ptr::null(), 0);
    }
    DefSubclassProc(hwnd, msg, wp, lp)
}
