mod navigation;
mod shortcuts;
mod smoke;
mod tab_strip;
mod tabs;

use std::{error::Error, sync::OnceLock};
use tabs::{TabId, TabSet};
use windows_sys::Win32::{
    Foundation::*,
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        Controls::EM_SETSEL,
        Input::KeyboardAndMouse::{GetFocus, SetFocus},
        WindowsAndMessaging::*,
    },
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
    Rect, WebContext, WebView, WebViewBuilder, WebViewBuilderExtWindows, WebViewExtWindows,
};

#[derive(Debug)]
enum Action {
    Navigate,
    Back,
    Forward,
    Reload,
    NewTab,
    CloseTab,
    SelectTab,
    FocusAddress,
    NextTab(bool),
    Loaded(TabId, String),
    Title(TabId, String),
    Verified(TabId, String),
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
            6 => send(Action::NewTab),
            7 => send(Action::CloseTab),
            _ => {}
        }
    }
    if msg == WM_COMMAND && (wp & 0xffff) == 8 && (wp >> 16) == CBN_SELCHANGE as usize {
        send(Action::SelectTab);
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

struct Tab {
    view: WebView,
    url: String,
    title: String,
}
struct Browser {
    tabs: TabSet<Tab>,
    shortcuts: Option<shortcuts::Shortcuts>,
    context: Option<WebContext>,
    window: Option<Window>,
    strip: Option<tab_strip::TabStrip>,
    edit: HWND,
    go: HWND,
    smoke: bool,
    loads: usize,
    failed: bool,
    smoke_done: bool,
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
                .with_title("Axiom Browser 0.1.1")
                .with_inner_size(PhysicalSize::new(1100, 760))
                .with_min_inner_size(PhysicalSize::new(500, 300)),
        )?;
        let RawWindowHandle::Win32(handle) = window.window_handle()?.as_raw() else {
            return Err("Windows is required".into());
        };
        let hwnd = handle.hwnd.get() as HWND;
        self.strip = Some(tab_strip::TabStrip::new(hwnd)?);
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
        self.window = Some(window);
        self.shortcuts = Some(shortcuts::Shortcuts::install()?);
        self.add_tab("https://example.com/")?;
        Ok(())
    }
    fn add_tab(&mut self, url: &str) -> Result<(), Box<dyn Error>> {
        let id = self.tabs.next_id();
        // All tabs explicitly reuse the first tab's COM environment and default profile.
        let environment = self
            .tabs
            .entries()
            .first()
            .map(|tab| tab.value.view.environment());
        let mut builder = WebViewBuilder::new_with_web_context(
            self.context.as_mut().ok_or("Profile is missing")?,
        )
        .with_url(url)
        .with_visible(false)
        .with_bounds(Rect {
            position: wry::dpi::PhysicalPosition::new(0, 80).into(),
            size: wry::dpi::PhysicalSize::new(1100, 680).into(),
        })
        .with_navigation_handler(|url| {
            url == "about:blank" || navigation::normalize_address(&url).is_ok()
        })
        .with_permission_handler(|_| wry::PermissionResponse::Deny)
        .with_new_window_req_handler(|_, _| wry::NewWindowResponse::Deny)
        .with_document_title_changed_handler(move |title| send(Action::Title(id, title)))
        .with_on_page_load_handler(move |event, url| {
            if matches!(event, wry::PageLoadEvent::Finished) {
                send(Action::Loaded(id, url));
            }
        });
        if let Some(environment) = environment {
            builder = builder.with_environment(environment);
        }
        let view = builder.build_as_child(self.window.as_ref().ok_or("Window is missing")?)?;
        self.tabs.add(Tab {
            view,
            url: url.into(),
            title: "Loading…".into(),
        });
        self.sync_tabs()?;
        self.set_address();
        self.resize()?;
        Ok(())
    }
    fn sync_tabs(&self) -> Result<(), Box<dyn Error>> {
        let active = self.tabs.active().map(|tab| tab.id);
        for tab in self.tabs.entries() {
            tab.value.view.set_visible(Some(tab.id) == active)?;
        }
        if let Some(strip) = &self.strip {
            let labels = self
                .tabs
                .entries()
                .iter()
                .map(|tab| (tab.id, tab.value.title.clone()))
                .collect::<Vec<_>>();
            strip.update(&labels, active)?;
        }
        if let Some(tab) = self.tabs.active() {
            if let Some(window) = &self.window {
                window.set_title(&format!("{} — Axiom Browser 0.1.1", tab.value.title));
            }
        }
        Ok(())
    }
    fn set_address(&self) {
        if let Some(tab) = self.tabs.active() {
            unsafe {
                SetWindowTextW(self.edit, wide(&tab.value.url).as_ptr());
            }
        }
    }
    fn resize(&self) -> wry::Result<()> {
        if let Some(window) = &self.window {
            let size = window.inner_size();
            unsafe {
                MoveWindow(self.edit, 228, 8, (size.width as i32 - 316).max(80), 28, 1);
                MoveWindow(self.go, size.width as i32 - 80, 8, 70, 28, 1);
            }
            if let Some(strip) = &self.strip {
                strip.resize(size.width);
            }
            for tab in self.tabs.entries() {
                tab.value.view.set_bounds(Rect {
                    position: wry::dpi::PhysicalPosition::new(0, 80).into(),
                    size: wry::dpi::PhysicalSize::new(
                        size.width.max(1),
                        size.height.saturating_sub(80).max(1),
                    )
                    .into(),
                })?;
            }
        }
        Ok(())
    }
    fn act(&mut self, action: Action) -> Result<(), Box<dyn Error>> {
        match action {
            Action::Navigate => {
                let input = unsafe {
                    let len = GetWindowTextLengthW(self.edit);
                    let mut text = vec![0u16; len as usize + 1];
                    let count = GetWindowTextW(self.edit, text.as_mut_ptr(), text.len() as i32);
                    String::from_utf16_lossy(&text[..count as usize])
                };
                let url = navigation::normalize_address(&input)?;
                let tab = self.tabs.active_mut().ok_or("No active tab")?;
                tab.value.view.load_url(&url)?;
                tab.value.url = url;
                tab.value.view.focus()?;
            }
            Action::Back => unsafe {
                let view = &self.tabs.active().ok_or("No active tab")?.value.view;
                let mut allowed = Default::default();
                view.webview().CanGoBack(&mut allowed)?;
                if allowed.as_bool() {
                    view.webview().GoBack()?;
                }
            },
            Action::Forward => unsafe {
                let view = &self.tabs.active().ok_or("No active tab")?.value.view;
                let mut allowed = Default::default();
                view.webview().CanGoForward(&mut allowed)?;
                if allowed.as_bool() {
                    view.webview().GoForward()?;
                }
            },
            Action::Reload => unsafe {
                self.tabs
                    .active()
                    .ok_or("No active tab")?
                    .value
                    .view
                    .webview()
                    .Reload()?;
            },
            Action::NewTab => {
                self.add_tab("about:blank")?;
                self.act(Action::FocusAddress)?;
            }
            Action::CloseTab => {
                if let Some(id) = self.tabs.active().map(|tab| tab.id) {
                    self.tabs.close(id);
                    self.sync_tabs()?;
                    self.set_address();
                }
                if let Some(tab) = self.tabs.active() {
                    tab.value.view.focus()?;
                }
            }
            Action::SelectTab => {
                if let Some(id) = self.strip.as_ref().and_then(|strip| strip.selected_id()) {
                    self.select_tab(id)?;
                }
            }
            Action::NextTab(reverse) => {
                if let Some(index) = self
                    .tabs
                    .entries()
                    .iter()
                    .position(|tab| Some(tab.id) == self.tabs.active().map(|tab| tab.id))
                {
                    let count = self.tabs.len();
                    let next = if reverse {
                        (index + count - 1) % count
                    } else {
                        (index + 1) % count
                    };
                    self.select_tab(self.tabs.entries()[next].id)?;
                }
            }
            Action::FocusAddress => unsafe {
                SetFocus(self.edit);
                SendMessageW(self.edit, EM_SETSEL, 0, -1);
            },
            Action::Title(id, title) => {
                if let Some(tab) = self.tabs.get_mut(id) {
                    tab.value.title = if title.trim().is_empty() {
                        "New tab".into()
                    } else {
                        title
                    };
                    self.sync_tabs()?;
                }
            }
            Action::Loaded(id, url) => {
                let Some(tab) = self.tabs.get_mut(id) else {
                    return Ok(());
                };
                tab.value.url = url.clone();
                self.sync_tabs()?;
                if self.tabs.active().map(|tab| tab.id) == Some(id)
                    && unsafe { GetFocus() } != self.edit
                {
                    self.set_address();
                }
                if self.smoke && self.tabs.active().map(|tab| tab.id) == Some(id) {
                    println!("PAGE_LOADED tab={id} {url}");
                    self.check_smoke_page(id)?;
                }
            }
            Action::Verified(id, result) => {
                if self.tabs.active().map(|tab| tab.id) != Some(id) {
                    return Ok(());
                }
                if result != "true" {
                    return Err(format!(
                        "Page verification failed at step {}: {result}",
                        self.loads
                    )
                    .into());
                }
                self.loads += 1;
                println!("CONTENT_VERIFIED step={} tab={id}", self.loads);
                self.advance_smoke()?;
            }
            Action::Timeout => {}
        }
        Ok(())
    }
    fn select_tab(&mut self, id: TabId) -> Result<(), Box<dyn Error>> {
        if self.tabs.select(id) {
            self.sync_tabs()?;
            self.set_address();
            self.tabs.active().unwrap().value.view.focus()?;
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
        if self.tabs.len() == 0 || (self.smoke && (self.smoke_done || self.failed)) {
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
        tabs: TabSet::new(),
        shortcuts: None,
        context: None,
        window: None,
        strip: None,
        edit: std::ptr::null_mut(),
        go: std::ptr::null_mut(),
        smoke,
        loads: 0,
        failed: false,
        smoke_done: false,
    };
    event_loop.run_app(&mut app)?;
    if app.failed || (app.smoke && !app.smoke_done) {
        std::process::exit(1);
    }
    Ok(())
}
