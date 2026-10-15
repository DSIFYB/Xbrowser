use crate::{send, wide, Action};
use std::{io, ptr, sync::OnceLock};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        Controls::{DRAWITEMSTRUCT, EM_SETSEL},
        HiDpi::GetDpiForWindow,
        Input::KeyboardAndMouse::{GetKeyState, SetFocus},
        Shell::{DefSubclassProc, SetWindowSubclass},
        WindowsAndMessaging::*,
    },
};
static CLASS: OnceLock<()> = OnceLock::new();
fn rgb(r: u32, g: u32, b: u32) -> u32 {
    r | g << 8 | b << 16
}
struct State {
    font: HFONT,
    brush: HBRUSH,
    title: HWND,
    url: HWND,
}
impl Drop for State {
    fn drop(&mut self) {
        unsafe {
            DeleteObject(self.font);
            DeleteObject(self.brush);
        }
    }
}
pub struct Editor {
    pub hwnd: HWND,
    pub id: Option<i64>,
    state: Box<State>,
}
impl Drop for Editor {
    fn drop(&mut self) {
        unsafe {
            if IsWindow(self.hwnd) != 0 {
                DestroyWindow(self.hwnd);
            }
        }
    }
}
impl Editor {
    pub fn open(owner: HWND, id: Option<i64>, url: &str, title: &str) -> io::Result<Self> {
        unsafe {
            let instance = GetModuleHandleW(ptr::null());
            CLASS.get_or_init(|| {
                let class_name = wide("AxiomBookmarkEditor");
                let class = WNDCLASSW {
                    lpfnWndProc: Some(window_proc),
                    hInstance: instance,
                    lpszClassName: class_name.as_ptr(),
                    hCursor: LoadCursorW(ptr::null_mut(), IDC_ARROW),
                    ..std::mem::zeroed()
                };
                RegisterClassW(&class);
            });
            let scale = GetDpiForWindow(owner).max(96) as f64 / 96.0;
            let px = |v: i32| (v as f64 * scale).round() as i32;
            let mut rect = RECT::default();
            GetClientRect(owner, &mut rect);
            let mut position = POINT {
                x: rect.right - px(454),
                y: px(92),
            };
            ClientToScreen(owner, &mut position);
            let mut work = RECT::default();
            SystemParametersInfoW(SPI_GETWORKAREA, 0, (&mut work as *mut RECT).cast(), 0);
            position.x = position
                .x
                .max(work.left)
                .min((work.right - px(438)).max(work.left));
            position.y = position
                .y
                .max(work.top)
                .min((work.bottom - px(308)).max(work.top));
            let font = CreateFontW(
                -px(14),
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
            let mut state = Box::new(State {
                font,
                brush: CreateSolidBrush(rgb(24, 28, 37)),
                title: ptr::null_mut(),
                url: ptr::null_mut(),
            });
            let hwnd = CreateWindowExW(
                WS_EX_TOOLWINDOW,
                wide("AxiomBookmarkEditor").as_ptr(),
                wide(if id.is_some() {
                    "Изменить закладку"
                } else {
                    "Сохранить закладку"
                })
                .as_ptr(),
                WS_POPUP | WS_CAPTION | WS_SYSMENU,
                position.x,
                position.y,
                px(438),
                px(308),
                owner,
                ptr::null_mut(),
                instance,
                ptr::null(),
            );
            if hwnd.is_null() {
                return Err(io::Error::last_os_error());
            }
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, (&mut *state as *mut State) as isize);
            let dark = 1i32;
            windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute(
                hwnd,
                20,
                (&dark as *const i32).cast(),
                4,
            );
            let child =
                |class: &str, text: &str, style: u32, id: usize, x: i32, y: i32, w: i32, h: i32| {
                    let control = CreateWindowExW(
                        0,
                        wide(class).as_ptr(),
                        wide(text).as_ptr(),
                        WS_CHILD | WS_VISIBLE | style,
                        px(x),
                        px(y),
                        px(w),
                        px(h),
                        hwnd,
                        id as HMENU,
                        instance,
                        ptr::null(),
                    );
                    SendMessageW(control, WM_SETFONT, font as usize, 1);
                    if style & WS_TABSTOP != 0 {
                        SetWindowSubclass(control, Some(control_proc), 1, 0);
                    }
                    control
                };
            child("STATIC", "Название", 0, 0, 20, 16, 392, 20);
            state.title = child(
                "EDIT",
                title,
                WS_TABSTOP | ES_AUTOHSCROLL as u32 | WS_BORDER,
                101,
                20,
                42,
                392,
                30,
            );
            child("STATIC", "Адрес", 0, 0, 20, 88, 392, 20);
            state.url = child(
                "EDIT",
                url,
                WS_TABSTOP | ES_AUTOHSCROLL as u32 | WS_BORDER,
                102,
                20,
                114,
                392,
                30,
            );
            child(
                "BUTTON",
                "Сохранить",
                WS_TABSTOP | BS_OWNERDRAW as u32,
                1,
                284,
                176,
                128,
                36,
            );
            child(
                "BUTTON",
                "Отмена",
                WS_TABSTOP | BS_OWNERDRAW as u32,
                2,
                176,
                176,
                96,
                36,
            );
            if id.is_some() {
                child(
                    "BUTTON",
                    "Удалить",
                    WS_TABSTOP | BS_OWNERDRAW as u32,
                    3,
                    20,
                    176,
                    100,
                    36,
                );
            }
            child(
                "STATIC",
                "Изменения применятся после сохранения.",
                0,
                0,
                20,
                236,
                392,
                20,
            );
            ShowWindow(hwnd, SW_SHOW);
            SetForegroundWindow(hwnd);
            SetFocus(state.title);
            SendMessageW(state.title, EM_SETSEL, 0, -1);
            Ok(Self { hwnd, id, state })
        }
    }
    pub fn values(&self) -> (String, String) {
        unsafe { (text(self.state.url), text(self.state.title)) }
    }
    pub fn set_title(&self, title: &str) {
        unsafe {
            SetWindowTextW(self.state.title, wide(title).as_ptr());
        }
    }
}
unsafe fn text(hwnd: HWND) -> String {
    let length = GetWindowTextLengthW(hwnd).clamp(0, 8192);
    let mut text = vec![0u16; length as usize + 1];
    let count = GetWindowTextW(hwnd, text.as_mut_ptr(), text.len() as i32);
    String::from_utf16_lossy(&text[..count as usize])
}
unsafe extern "system" fn control_proc(
    hwnd: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
    _: usize,
    _: usize,
) -> LRESULT {
    if msg == WM_KEYDOWN {
        match wp {
            0x1b => {
                send(Action::DismissBookmarkEditor);
                return 0;
            }
            0x0d => {
                let id = GetDlgCtrlID(hwnd);
                if id < 100 {
                    SendMessageW(GetParent(hwnd), WM_COMMAND, id as usize, hwnd as isize);
                } else {
                    send(Action::SaveBookmarkEditor);
                }
                return 0;
            }
            0x09 => {
                let next = GetNextDlgTabItem(GetParent(hwnd), hwnd, (GetKeyState(0x10) < 0) as i32);
                if !next.is_null() {
                    SetFocus(next);
                }
                return 0;
            }
            _ => {}
        }
    }
    if msg == WM_CHAR && matches!(wp, 9 | 13 | 27) {
        return 0;
    }
    DefSubclassProc(hwnd, msg, wp, lp)
}
unsafe extern "system" fn window_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_CLOSE => {
            send(Action::DismissBookmarkEditor);
            return 0;
        }
        WM_COMMAND if (wp >> 16) == 0 => {
            match wp & 0xffff {
                1 => send(Action::SaveBookmarkEditor),
                2 => send(Action::DismissBookmarkEditor),
                3 => send(Action::DeleteBookmarkEditor),
                _ => {}
            }
            return 0;
        }
        WM_NCDESTROY => {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        }
        _ => {}
    }
    let state = (GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const State).as_ref();
    if let Some(state) = state {
        match msg {
            WM_ERASEBKGND => {
                let mut rect = RECT::default();
                GetClientRect(hwnd, &mut rect);
                FillRect(wp as HDC, &rect, state.brush);
                return 1;
            }
            WM_CTLCOLORSTATIC | WM_CTLCOLOREDIT => {
                let dc = wp as HDC;
                SetTextColor(dc, rgb(237, 240, 247));
                SetBkColor(
                    dc,
                    if msg == WM_CTLCOLOREDIT {
                        rgb(35, 41, 56)
                    } else {
                        rgb(24, 28, 37)
                    },
                );
                SetDCBrushColor(
                    dc,
                    if msg == WM_CTLCOLOREDIT {
                        rgb(35, 41, 56)
                    } else {
                        rgb(24, 28, 37)
                    },
                );
                return GetStockObject(DC_BRUSH) as isize;
            }
            WM_DRAWITEM => {
                let item = &*(lp as *const DRAWITEMSTRUCT);
                let dc = item.hDC;
                let saved = SaveDC(dc);
                let fill = CreateSolidBrush(if item.CtlID == 1 {
                    rgb(182, 173, 255)
                } else {
                    rgb(35, 41, 56)
                });
                FillRect(dc, &item.rcItem, fill);
                DeleteObject(fill);
                SelectObject(dc, state.font);
                SetBkMode(dc, TRANSPARENT as i32);
                SetTextColor(
                    dc,
                    if item.CtlID == 1 {
                        rgb(23, 19, 38)
                    } else if item.CtlID == 3 {
                        rgb(255, 156, 170)
                    } else {
                        rgb(237, 240, 247)
                    },
                );
                let mut rect = item.rcItem;
                DrawTextW(
                    dc,
                    wide(&text(item.hwndItem)).as_ptr(),
                    -1,
                    &mut rect,
                    DT_CENTER | DT_VCENTER | DT_SINGLELINE,
                );
                if item.itemState & 0x10 != 0 {
                    DrawFocusRect(dc, &rect);
                }
                RestoreDC(dc, saved);
                return 1;
            }
            _ => {}
        }
    }
    DefWindowProcW(hwnd, msg, wp, lp)
}
