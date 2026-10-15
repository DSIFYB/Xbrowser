use std::{cell::Cell, rc::Rc};
use webview2_com::{
    FaviconChangedEventHandler, GetFaviconCompletedHandler,
    Microsoft::Web::WebView2::Win32::{ICoreWebView2_15, COREWEBVIEW2_FAVICON_IMAGE_FORMAT_PNG},
    NavigationStartingEventHandler,
};
use windows::core::Interface;
use windows_sys::Win32::UI::WindowsAndMessaging::{CreateIconFromResourceEx, DestroyIcon, HICON};
use wry::WebViewExtWindows;

#[derive(Debug)]
pub struct Icon(pub HICON);
impl Drop for Icon {
    fn drop(&mut self) {
        unsafe {
            DestroyIcon(self.0);
        }
    }
}
impl Icon {
    pub fn from_png(mut bytes: Vec<u8>) -> Option<Self> {
        if bytes.len() < 24
            || !bytes.starts_with(b"\x89PNG\r\n\x1a\n")
            || bytes.len() > 1024 * 1024
            || &bytes[12..16] != b"IHDR"
        {
            return None;
        }
        let width = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
        let height = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
        if width == 0 || height == 0 || width > 1024 || height > 1024 {
            return None;
        }
        let icon = unsafe {
            CreateIconFromResourceEx(
                bytes.as_mut_ptr(),
                bytes.len() as u32,
                1,
                0x30000,
                16,
                16,
                0,
            )
        };
        (!icon.is_null()).then_some(Self(icon))
    }
}

pub fn attach(view: &wry::WebView, id: crate::TabId) -> Result<(), Box<dyn std::error::Error>> {
    let generation = Rc::new(Cell::new(0u64));
    let started = generation.clone();
    let mut token = 0;
    unsafe {
        view.webview().add_NavigationStarting(
            &NavigationStartingEventHandler::create(Box::new(move |_, _| {
                let next = started.get().saturating_add(1);
                started.set(next);
                crate::send(crate::Action::Started(id, next));
                Ok(())
            })),
            &mut token,
        )?;
        // Older runtimes still show the neutral icon and retain navigation controls.
        if let Ok(modern) = view.webview().cast::<ICoreWebView2_15>() {
            modern.add_FaviconChanged(
                &FaviconChangedEventHandler::create(Box::new(move |sender, _| {
                    if let Some(sender) = sender {
                        let modern = sender.cast::<ICoreWebView2_15>()?;
                        let current = generation.get();
                        modern.GetFavicon(
                            COREWEBVIEW2_FAVICON_IMAGE_FORMAT_PNG,
                            &GetFaviconCompletedHandler::create(Box::new(move |status, stream| {
                                if status.is_ok() {
                                    if let Some(stream) = stream {
                                        let mut stat = Default::default();
                                        if stream
                                            .Stat(
                                                &mut stat,
                                                windows::Win32::System::Com::STATFLAG_NONAME,
                                            )
                                            .is_ok()
                                            && stat.cbSize > 0
                                            && stat.cbSize <= 1024 * 1024
                                        {
                                            let mut bytes = vec![0u8; stat.cbSize as usize];
                                            let mut read = 0;
                                            if stream
                                                .Read(
                                                    bytes.as_mut_ptr().cast(),
                                                    bytes.len() as u32,
                                                    Some(&mut read),
                                                )
                                                .is_ok()
                                                && read as usize == bytes.len()
                                            {
                                                crate::send(crate::Action::Favicon(
                                                    id, current, bytes,
                                                ));
                                            }
                                        }
                                    }
                                }
                                Ok(())
                            })),
                        )?;
                    }
                    Ok(())
                })),
                &mut token,
            )?;
        }
    }
    Ok(())
}
