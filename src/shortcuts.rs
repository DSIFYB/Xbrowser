use crate::{send, Action};
use windows_sys::Win32::{
    Foundation::*,
    System::Threading::GetCurrentThreadId,
    UI::{Input::KeyboardAndMouse::GetKeyState, WindowsAndMessaging::*},
};

pub struct Shortcuts(HHOOK);
impl Shortcuts {
    pub fn install() -> std::io::Result<Self> {
        let hook = unsafe {
            SetWindowsHookExW(
                WH_GETMESSAGE,
                Some(on_message),
                std::ptr::null_mut(),
                GetCurrentThreadId(),
            )
        };
        if hook.is_null() {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(Self(hook))
        }
    }
}
impl Drop for Shortcuts {
    fn drop(&mut self) {
        unsafe {
            UnhookWindowsHookEx(self.0);
        }
    }
}

// A thread-local message hook also sees keyboard input focused inside WebView2.
unsafe extern "system" fn on_message(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code >= 0 && wp == PM_REMOVE as usize {
        let msg = &mut *(lp as *mut MSG);
        if msg.message == WM_KEYDOWN && GetKeyState(0x11) < 0 {
            let action = match msg.wParam {
                0x54 => Some(Action::NewTab),
                0x57 => Some(Action::CloseTab),
                0x4c => Some(Action::FocusAddress),
                0x09 => Some(Action::NextTab(GetKeyState(0x10) < 0)),
                _ => None,
            };
            if let Some(action) = action {
                send(action);
                msg.message = WM_NULL;
            }
        }
    }
    CallNextHookEx(std::ptr::null_mut(), code, wp, lp)
}
