use std::{io, mem, ptr};
use windows_sys::Win32::Storage::Xps::{PrintWindow, PW_CLIENTONLY};
use windows_sys::Win32::{Foundation::*, Graphics::Gdi::*, UI::WindowsAndMessaging::*};

// Capture only this application's client area for visual verification.
pub fn capture(hwnd: HWND) -> io::Result<()> {
    unsafe {
        let mut rect: RECT = mem::zeroed();
        if GetClientRect(hwnd, &mut rect) == 0 {
            return Err(io::Error::last_os_error());
        }
        RedrawWindow(
            hwnd,
            ptr::null(),
            ptr::null_mut(),
            RDW_INVALIDATE | RDW_UPDATENOW | RDW_ALLCHILDREN,
        );
        let source = GetDC(hwnd);
        let dc = CreateCompatibleDC(source);
        let bitmap = CreateCompatibleBitmap(source, rect.right, rect.bottom);
        if source.is_null() || dc.is_null() || bitmap.is_null() {
            if !bitmap.is_null() {
                DeleteObject(bitmap);
            }
            if !dc.is_null() {
                DeleteDC(dc);
            }
            if !source.is_null() {
                ReleaseDC(hwnd, source);
            }
            return Err(io::Error::last_os_error());
        }
        let old = SelectObject(dc, bitmap);
        let copied = PrintWindow(hwnd, dc, PW_CLIENTONLY | 2);
        SelectObject(dc, old);
        let mut info: BITMAPINFO = mem::zeroed();
        info.bmiHeader.biSize = mem::size_of::<BITMAPINFOHEADER>() as u32;
        info.bmiHeader.biWidth = rect.right;
        info.bmiHeader.biHeight = -rect.bottom;
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32;
        info.bmiHeader.biCompression = BI_RGB;
        let mut pixels = vec![0u8; rect.right as usize * rect.bottom as usize * 4];
        let rows = GetDIBits(
            dc,
            bitmap,
            0,
            rect.bottom as u32,
            pixels.as_mut_ptr().cast(),
            &mut info,
            DIB_RGB_COLORS,
        );
        DeleteObject(bitmap);
        DeleteDC(dc);
        ReleaseDC(hwnd, source);
        if copied == 0 || rows != rect.bottom {
            return Err(io::Error::last_os_error());
        }
        let mut header = Vec::with_capacity(54 + pixels.len());
        header.extend_from_slice(b"BM");
        header.extend_from_slice(&(54u32 + pixels.len() as u32).to_le_bytes());
        header.extend_from_slice(&[0; 4]);
        header.extend_from_slice(&54u32.to_le_bytes());
        header.extend_from_slice(&40u32.to_le_bytes());
        header.extend_from_slice(&rect.right.to_le_bytes());
        header.extend_from_slice(&(-rect.bottom).to_le_bytes());
        header.extend_from_slice(&1u16.to_le_bytes());
        header.extend_from_slice(&32u16.to_le_bytes());
        header.extend_from_slice(&0u32.to_le_bytes());
        header.extend_from_slice(&(pixels.len() as u32).to_le_bytes());
        header.extend_from_slice(&[0; 16]);
        header.extend_from_slice(&pixels);
        std::fs::create_dir_all("screenshots")?;
        std::fs::write("screenshots/axiom-0.1.2.bmp", header)?;
    }
    Ok(())
}
