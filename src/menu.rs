use crate::{wide, Action};
use windows_sys::Win32::{Foundation::*, UI::WindowsAndMessaging::*};
pub fn show(hwnd: HWND) -> Option<Action> {
    unsafe {
        let menu = CreatePopupMenu();
        if menu.is_null() {
            return None;
        }
        for (id, label) in [
            (100, "История\tCtrl+H"),
            (101, "Закладки\tCtrl+Shift+O"),
            (102, "Сохранить / изменить закладку\tCtrl+D"),
            (103, "Настройки"),
        ] {
            AppendMenuW(menu, MF_STRING, id, wide(label).as_ptr());
        }
        let mut point = POINT { x: 0, y: 0 };
        GetCursorPos(&mut point);
        let chosen = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTALIGN,
            point.x,
            point.y,
            0,
            hwnd,
            std::ptr::null(),
        );
        DestroyMenu(menu);
        match chosen {
            100 => Some(Action::History),
            101 => Some(Action::Bookmarks),
            102 => Some(Action::ToggleBookmark),
            103 => Some(Action::Settings),
            _ => None,
        }
    }
}
