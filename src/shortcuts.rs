use crate::{send, Action};
use webview2_com::{
    AcceleratorKeyPressedEventHandler,
    Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN, COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN,
    },
};
use windows_sys::Win32::{
    Foundation::*,
    System::Threading::GetCurrentThreadId,
    UI::{Input::KeyboardAndMouse::GetKeyState, WindowsAndMessaging::*},
};
use wry::WebViewExtWindows;

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

pub fn attach_webview(view: &wry::WebView) -> Result<(), Box<dyn std::error::Error>> {
    let handler = AcceleratorKeyPressedEventHandler::create(Box::new(|_, args| {
        if let Some(args) = args {
            unsafe {
                let mut kind = Default::default();
                let mut key = 0;
                args.KeyEventKind(&mut kind)?;
                args.VirtualKey(&mut key)?;
                if kind == COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN
                    || kind == COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN
                {
                    if let Some(action) = shortcut(key as usize) {
                        args.SetHandled(true)?;
                        send(action);
                    }
                }
            }
        }
        Ok(())
    }));
    let mut token = 0;
    // The controller retains its callback and releases it when the tab is closed.
    unsafe {
        view.controller()
            .add_AcceleratorKeyPressed(&handler, &mut token)?;
    }
    Ok(())
}

unsafe fn shortcut(key: usize) -> Option<Action> {
    if GetKeyState(0x11) >= 0 || GetKeyState(0x12) < 0 {
        return None;
    }
    match key {
        0x48 => Some(Action::History),
        0x4f if GetKeyState(0x10) < 0 => Some(Action::Bookmarks),
        0x44 => Some(Action::ToggleBookmark),
        0x54 => Some(Action::NewTab),
        0x57 => Some(Action::CloseTab),
        0x4c => Some(Action::FocusAddress),
        0x09 => Some(Action::NextTab(GetKeyState(0x10) < 0)),
        _ => None,
    }
}

// Native toolbar controls use the window thread's normal message queue.
unsafe extern "system" fn on_message(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code >= 0 && wp == PM_REMOVE as usize {
        let msg = &mut *(lp as *mut MSG);
        if msg.message == WM_KEYDOWN {
            if let Some(action) = shortcut(msg.wParam) {
                send(action);
                msg.message = WM_NULL;
            }
        }
    }
    CallNextHookEx(std::ptr::null_mut(), code, wp, lp)
}
