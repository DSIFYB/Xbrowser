mod navigation;

use std::{error::Error, sync::OnceLock};
use windows_sys::Win32::{
    Foundation::*, System::LibraryLoader::GetModuleHandleW, UI::WindowsAndMessaging::*,
};
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy},
    window::{Window, WindowId},
};
use wry::{
    raw_window_handle::{HasWindowHandle, RawWindowHandle},
    Rect, WebContext, WebView, WebViewBuilder, WebViewExtWindows,
};

#[derive(Debug)]
enum Action {
    Navigate,
    Back,
    Forward,
    Reload,
    Loaded(String),
    Verified(String),
    Timeout,
}
static PROXY: OnceLock<EventLoopProxy<Action>> = OnceLock::new();
static PARENT_PROC: OnceLock<isize> = OnceLock::new();
static EDIT_PROC: OnceLock<isize> = OnceLock::new();
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
fn send(action: Action) {
    if let Some(proxy) = PROXY.get() {
        let _ = proxy.send_event(action);
    }
}

// Subclasses run on the window thread; original procedures remain alive for the window lifetime.
unsafe extern "system" fn parent_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_COMMAND && (wp >> 16) == 0 {
        match wp & 0xffff {
            1 => send(Action::Back),
            2 => send(Action::Forward),
            3 => send(Action::Reload),
            5 => send(Action::Navigate),
            _ => {}
        }
    }
    CallWindowProcW(
        std::mem::transmute::<isize, WNDPROC>(*PARENT_PROC.get().unwrap()),
        hwnd,
        msg,
        wp,
        lp,
    )
}
unsafe extern "system" fn edit_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_KEYDOWN && wp == 13 {
        send(Action::Navigate);
        return 0;
    }
    if msg == WM_CHAR && wp == 13 {
        return 0;
    }
    CallWindowProcW(
        std::mem::transmute::<isize, WNDPROC>(*EDIT_PROC.get().unwrap()),
        hwnd,
        msg,
        wp,
        lp,
    )
}

struct Browser {
    view: Option<WebView>,
    context: Option<WebContext>,
    window: Option<Window>,
    edit: HWND,
    go: HWND,
    smoke: bool,
    loads: usize,
    failed: bool,
}
impl Browser {
    fn error(&mut self, message: impl std::fmt::Display) {
        eprintln!("{message}");
        if self.smoke {
            self.failed = true;
        } else {
            unsafe {
                MessageBoxW(
                    std::ptr::null_mut(),
                    wide(&message.to_string()).as_ptr(),
                    wide("Axiom Browser").as_ptr(),
                    MB_OK | MB_ICONERROR,
                );
            }
        }
    }
    fn init(&mut self, event_loop: &ActiveEventLoop) -> Result<(), Box<dyn Error>> {
        let window = event_loop.create_window(
            Window::default_attributes()
                .with_title("Axiom Browser 0.1")
                .with_inner_size(PhysicalSize::new(1100, 760))
                .with_min_inner_size(PhysicalSize::new(500, 300)),
        )?;
        let RawWindowHandle::Win32(handle) = window.window_handle()?.as_raw() else {
            return Err("Windows is required".into());
        };
        let hwnd = handle.hwnd.get() as HWND;
        unsafe {
            let instance = GetModuleHandleW(std::ptr::null());
            for (id, label, x) in [(1, "Back", 8), (2, "Forward", 80), (3, "Reload", 152)] {
                if CreateWindowExW(
                    0,
                    wide("BUTTON").as_ptr(),
                    wide(label).as_ptr(),
                    WS_CHILD | WS_VISIBLE | WS_TABSTOP,
                    x,
                    8,
                    68,
                    28,
                    hwnd,
                    id as HMENU,
                    instance,
                    std::ptr::null(),
                )
                .is_null()
                {
                    return Err(std::io::Error::last_os_error().into());
                }
            }
            self.edit = CreateWindowExW(
                WS_EX_CLIENTEDGE,
                wide("EDIT").as_ptr(),
                wide("https://example.com/").as_ptr(),
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | ES_AUTOHSCROLL as u32,
                228,
                8,
                770,
                28,
                hwnd,
                4 as HMENU,
                instance,
                std::ptr::null(),
            );
            self.go = CreateWindowExW(
                0,
                wide("BUTTON").as_ptr(),
                wide("Go").as_ptr(),
                WS_CHILD | WS_VISIBLE | WS_TABSTOP,
                1006,
                8,
                70,
                28,
                hwnd,
                5 as HMENU,
                instance,
                std::ptr::null(),
            );
            if self.edit.is_null() || self.go.is_null() {
                return Err(std::io::Error::last_os_error().into());
            }
            EDIT_PROC
                .set(SetWindowLongPtrW(
                    self.edit,
                    GWLP_WNDPROC,
                    edit_proc as *const () as isize,
                ))
                .map_err(|_| "Edit already initialized")?;
            PARENT_PROC
                .set(SetWindowLongPtrW(
                    hwnd,
                    GWLP_WNDPROC,
                    parent_proc as *const () as isize,
                ))
                .map_err(|_| "Window already initialized")?;
        }
        let profile = if self.smoke {
            std::env::current_dir()?.join(".smoke-profile")
        } else {
            std::path::PathBuf::from(
                std::env::var_os("LOCALAPPDATA").ok_or("LOCALAPPDATA is missing")?,
            )
            .join("AxiomBrowser")
            .join("Profile")
        };
        std::fs::create_dir_all(&profile)?;
        self.context = Some(WebContext::new(Some(profile)));
        let view = WebViewBuilder::new_with_web_context(self.context.as_mut().unwrap())
            .with_url("https://example.com/")
            .with_bounds(Rect {
                position: wry::dpi::PhysicalPosition::new(0, 44).into(),
                size: wry::dpi::PhysicalSize::new(1100, 716).into(),
            })
            .with_navigation_handler(|url| {
                url == "about:blank" || navigation::normalize_address(&url).is_ok()
            })
            .with_permission_handler(|_| wry::PermissionResponse::Deny)
            .with_new_window_req_handler(|_, _| wry::NewWindowResponse::Deny)
            .with_on_page_load_handler(|event, url| {
                if matches!(event, wry::PageLoadEvent::Finished) {
                    send(Action::Loaded(url));
                }
            })
            .build_as_child(&window)?;
        self.view = Some(view);
        self.window = Some(window);
        self.resize()?;
        Ok(())
    }
    fn resize(&self) -> wry::Result<()> {
        if let (Some(window), Some(view)) = (&self.window, &self.view) {
            let size = window.inner_size();
            unsafe {
                MoveWindow(self.edit, 228, 8, (size.width as i32 - 316).max(80), 28, 1);
                MoveWindow(self.go, size.width as i32 - 80, 8, 70, 28, 1);
            }
            view.set_bounds(Rect {
                position: wry::dpi::PhysicalPosition::new(0, 44).into(),
                size: wry::dpi::PhysicalSize::new(
                    size.width.max(1),
                    size.height.saturating_sub(44).max(1),
                )
                .into(),
            })?;
        }
        Ok(())
    }
    fn act(&mut self, action: Action) -> Result<(), Box<dyn Error>> {
        let Some(view) = &self.view else {
            return Ok(());
        };
        match action {
            Action::Navigate => {
                let input = unsafe {
                    let len = GetWindowTextLengthW(self.edit);
                    let mut text = vec![0u16; len as usize + 1];
                    let count = GetWindowTextW(self.edit, text.as_mut_ptr(), text.len() as i32);
                    String::from_utf16_lossy(&text[..count as usize])
                };
                let url = navigation::normalize_address(&input)?;
                view.load_url(&url)?;
                view.focus()?;
            }
            Action::Back => unsafe {
                view.webview().GoBack()?;
            },
            Action::Forward => unsafe {
                view.webview().GoForward()?;
            },
            Action::Reload => unsafe {
                view.webview().Reload()?;
            },
            Action::Loaded(url) => {
                unsafe {
                    SetWindowTextW(self.edit, wide(&url).as_ptr());
                }
                if self.smoke {
                    println!("PAGE_LOADED {url}");
                    view.evaluate_script_with_callback("JSON.stringify({title:document.title,url:location.href,text:document.body.innerText.slice(0,500)})", |result| println!("PAGE_DIAGNOSTIC {result}"))?;
                    let check = if self.loads == 0 {
                        "location.hostname === 'example.com' && document.title === 'Example Domain' && document.body.innerText.length > 100"
                    } else if self.loads == 2 {
                        "location.hostname === 'example.com' && document.title === 'Example Domain' && document.body.innerText.length > 100"
                    } else {
                        "location.hostname.endsWith('rust-lang.org') && document.body.innerText.includes('Rust')"
                    };
                    view.evaluate_script_with_callback(check, |result| {
                        send(Action::Verified(result))
                    })?;
                }
            }
            Action::Verified(result) => {
                if result != "true" {
                    return Err(format!("Page content verification failed: {result}").into());
                }
                self.loads += 1;
                println!("CONTENT_VERIFIED {}", self.loads);
                match self.loads {
                    1 => {
                        unsafe {
                            SetWindowTextW(self.edit, wide("https://www.rust-lang.org/").as_ptr());
                        }
                        self.act(Action::Navigate)?;
                    }
                    2 => {
                        if let Some(window) = &self.window {
                            let _ = window.request_inner_size(PhysicalSize::new(900, 600));
                        }
                        self.act(Action::Back)?;
                    }
                    3 => self.act(Action::Forward)?,
                    4 => self.act(Action::Reload)?,
                    _ => {}
                }
            }
            Action::Timeout => {}
        }
        Ok(())
    }
}
impl ApplicationHandler<Action> for Browser {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() {
            if let Err(err) = self.init(event_loop) {
                self.error(err);
                self.failed = true;
                event_loop.exit();
            }
        }
    }
    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(_) => {
                if let Err(err) = self.resize() {
                    self.error(err);
                }
            }
            _ => {}
        }
    }
    fn user_event(&mut self, event_loop: &ActiveEventLoop, action: Action) {
        if matches!(action, Action::Timeout) && self.smoke {
            self.error("Smoke test timed out");
            event_loop.exit();
            return;
        }
        if let Err(err) = self.act(action) {
            self.error(err);
        }
        if self.smoke && (self.loads >= 5 || self.failed) {
            event_loop.exit();
        }
    }
}
fn main() -> Result<(), Box<dyn Error>> {
    let event_loop = EventLoop::<Action>::with_user_event().build()?;
    PROXY
        .set(event_loop.create_proxy())
        .map_err(|_| "Event loop already initialized")?;
    let smoke = std::env::args().any(|arg| arg == "--smoke-test");
    if smoke {
        std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_secs(60));
            send(Action::Timeout);
        });
    }
    let mut app = Browser {
        view: None,
        context: None,
        window: None,
        edit: std::ptr::null_mut(),
        go: std::ptr::null_mut(),
        smoke,
        loads: 0,
        failed: false,
    };
    event_loop.run_app(&mut app)?;
    if app.failed {
        std::process::exit(1);
    }
    Ok(())
}
