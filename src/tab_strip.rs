use std::{io, ptr};

use windows_sys::Win32::{
    Foundation::HWND,
    UI::WindowsAndMessaging::{
        CreateWindowExW, SendMessageW, SetWindowPos, CBS_DROPDOWNLIST, CB_ADDSTRING, CB_GETCURSEL,
        CB_GETITEMDATA, CB_RESETCONTENT, CB_SETCURSEL, CB_SETITEMDATA, HMENU, SWP_NOZORDER,
        WS_CHILD, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
    },
};

const NEW_BUTTON_ID: usize = 6;
const CLOSE_BUTTON_ID: usize = 7;
const COMBO_BOX_ID: usize = 8;

pub struct TabStrip {
    pub select: HWND,
    new_button: HWND,
    close_button: HWND,
}

impl TabStrip {
    pub fn new(parent: HWND) -> io::Result<Self> {
        let instance =
            unsafe { windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(ptr::null()) };
        let new_button = create_control(
            parent,
            instance,
            "BUTTON",
            "New tab",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            NEW_BUTTON_ID,
            8,
            44,
            90,
            28,
        )?;
        let close_button = create_control(
            parent,
            instance,
            "BUTTON",
            "Close tab",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            CLOSE_BUTTON_ID,
            104,
            44,
            90,
            28,
        )?;
        let select = create_control(
            parent,
            instance,
            "COMBOBOX",
            "",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_VSCROLL | CBS_DROPDOWNLIST as u32,
            COMBO_BOX_ID,
            202,
            44,
            400,
            300,
        )?;

        Ok(Self {
            select,
            new_button,
            close_button,
        })
    }

    pub fn update(&self, entries: &[(u64, String)], active: Option<u64>) -> io::Result<()> {
        unsafe {
            SendMessageW(self.select, CB_RESETCONTENT, 0, 0);

            let mut active_index = None;
            for (id, title) in entries {
                let title = to_wide(title);
                let added = SendMessageW(self.select, CB_ADDSTRING, 0, title.as_ptr() as isize);
                if added < 0 {
                    return Err(io::Error::other("Could not add a tab to the selector"));
                }

                if SendMessageW(self.select, CB_SETITEMDATA, added as usize, *id as isize) == -1 {
                    return Err(io::Error::other("Could not store a tab ID"));
                }
                if Some(*id) == active {
                    active_index = Some(added as usize);
                }
            }

            if let Some(index) = active_index {
                SendMessageW(self.select, CB_SETCURSEL, index, 0);
            } else {
                SendMessageW(self.select, CB_SETCURSEL, usize::MAX, 0);
            }
        }
        Ok(())
    }

    pub fn selected_id(&self) -> Option<u64> {
        unsafe {
            let index = SendMessageW(self.select, CB_GETCURSEL, 0, 0);
            if index < 0 {
                return None;
            }

            let id = SendMessageW(self.select, CB_GETITEMDATA, index as usize, 0);
            (id != -1).then_some(id as u64)
        }
    }

    pub fn resize(&self, width: u32) {
        let combo_width = width.saturating_sub(210).min(i32::MAX as u32) as i32;
        unsafe {
            SetWindowPos(
                self.new_button,
                ptr::null_mut(),
                8,
                44,
                90,
                28,
                SWP_NOZORDER,
            );
            SetWindowPos(
                self.close_button,
                ptr::null_mut(),
                104,
                44,
                90,
                28,
                SWP_NOZORDER,
            );
            SetWindowPos(
                self.select,
                ptr::null_mut(),
                202,
                44,
                combo_width,
                300,
                SWP_NOZORDER,
            );
        }
    }
}

fn create_control(
    parent: HWND,
    instance: windows_sys::Win32::Foundation::HINSTANCE,
    class_name: &str,
    text: &str,
    style: u32,
    id: usize,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) -> io::Result<HWND> {
    let class_name = to_wide(class_name);
    let text = to_wide(text);
    let hwnd = unsafe {
        CreateWindowExW(
            0,
            class_name.as_ptr(),
            text.as_ptr(),
            style,
            x,
            y,
            width,
            height,
            parent,
            id as HMENU,
            instance,
            ptr::null(),
        )
    };
    if hwnd.is_null() {
        Err(io::Error::last_os_error())
    } else {
        Ok(hwnd)
    }
}

fn to_wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
