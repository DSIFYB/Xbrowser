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
            (104, "Восстановить закрытую вкладку\tCtrl+Shift+T"),
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
            104 => Some(Action::ReopenTab),
            _ => None,
        }
    }
}

pub fn show_tab(hwnd: HWND, id: crate::TabId) -> Option<Action> {
    unsafe {
        let menu = CreatePopupMenu();
        if menu.is_null() {
            return None;
        }
        for (command, label) in [
            (201, "Дублировать вкладку"),
            (202, "Переместить влево"),
            (203, "Переместить вправо"),
            (204, "Закрыть другие вкладки"),
            (205, "Закрыть вкладки справа"),
            (206, "Закрыть вкладку"),
            (207, "Восстановить закрытую вкладку\tCtrl+Shift+T"),
        ] {
            AppendMenuW(menu, MF_STRING, command, wide(label).as_ptr());
        }
        let mut point = POINT { x: 0, y: 0 };
        GetCursorPos(&mut point);
        let command = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_NONOTIFY,
            point.x,
            point.y,
            0,
            hwnd,
            std::ptr::null(),
        );
        DestroyMenu(menu);
        match command {
            201 => Some(Action::DuplicateTab(id)),
            202 => Some(Action::MoveTab(id, -1)),
            203 => Some(Action::MoveTab(id, 1)),
            204 => Some(Action::CloseOtherTabs(id, false)),
            205 => Some(Action::CloseOtherTabs(id, true)),
            206 => Some(Action::CloseTabId(id)),
            207 => Some(Action::ReopenTab),
            _ => None,
        }
    }
}
