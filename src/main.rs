mod navigation;
mod preview;
mod shortcuts;
mod smoke;
mod tab_strip;
mod tabs;
mod toolbar;

use std::{error::Error, sync::OnceLock};
use tabs::{TabId, TabSet};
use windows_sys::Win32::{
    Foundation::*,
    UI::{
        Controls::EM_SETSEL,
        Input::KeyboardAndMouse::{GetFocus, SetFocus},
        WindowsAndMessaging::*,
    },
};
use winit::{
    application::ApplicationHandler,
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
    SelectTabId(TabId),
    CloseTabId(TabId),
    RestoreAddress,
    FocusAddress,
    NextTab(bool),
    Loaded(TabId, String),
    Title(TabId, String),
    Verified(TabId, String),
    Timeout,
    CapturePreview,
    FinishPreview,
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
    match msg {
        WM_PAINT => {
            toolbar::paint(hwnd);
            return 0;
        }
        WM_ERASEBKGND => return 1,
        WM_CTLCOLOREDIT => return toolbar::color_edit(wp as _),
        WM_DRAWITEM => {
            toolbar::draw_button(&*(lp as *const windows_sys::Win32::UI::Controls::DRAWITEMSTRUCT));
            return 1;
        }
        _ => {}
    }
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
    if msg == WM_COMMAND && (wp & 0xffff) == 4 {
        windows_sys::Win32::Graphics::Gdi::InvalidateRect(hwnd, std::ptr::null(), 0);
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
    if msg == WM_PAINT {
        let result = CallWindowProcW(
            std::mem::transmute::<isize, WNDPROC>(*EDIT_PROC.get().unwrap()),
            hwnd,
            msg,
            wp,
            lp,
        );
        toolbar::paint_placeholder(hwnd);
        return result;
    }
    if msg == WM_KEYDOWN && wp == 13 {
        send(Action::Navigate);
        return 0;
    }
    if msg == WM_KEYDOWN && wp == 27 {
        send(Action::RestoreAddress);
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
    toolbar: Option<toolbar::Toolbar>,
    smoke: bool,
    loads: usize,
    failed: bool,
    smoke_done: bool,
    preview: bool,
    preview_done: bool,
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
                .with_title("Axiom Browser")
                .with_inner_size(winit::dpi::LogicalSize::new(1100, 760))
                .with_min_inner_size(winit::dpi::LogicalSize::new(500, 300)),
        )?;
        let RawWindowHandle::Win32(handle) = window.window_handle()?.as_raw() else {
            return Err("Windows is required".into());
        };
        let hwnd = handle.hwnd.get() as HWND;
        self.strip = Some(tab_strip::TabStrip::new(hwnd)?);
        let toolbar = toolbar::Toolbar::new(hwnd)?;
        self.edit = toolbar.edit;
        self.toolbar = Some(toolbar);
        unsafe {
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
        let profile = if self.smoke || self.preview {
            std::env::current_dir()?.join(if self.smoke {
                ".smoke-profile"
            } else {
                ".preview-profile"
            })
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
        self.add_tab(if self.smoke {
            "https://example.com/"
        } else {
            "about:blank"
        })?;
        if self.preview {
            self.add_tab("https://example.com/")?;
            self.add_tab("https://rust-lang.org/")?;
            self.select_tab(0)?;
            std::thread::spawn(|| {
                std::thread::sleep(std::time::Duration::from_secs(2));
                send(Action::CapturePreview);
            });
        }
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
        .with_visible(false)
        .with_custom_protocol("axiom".into(), |_, request| {
            let allowed = request.uri().host() == Some("localhost") && request.uri().path() == "/";
            wry::http::Response::builder()
                .status(if allowed { 200 } else { 404 })
                .header("Content-Type", "text/html; charset=utf-8")
                .body(std::borrow::Cow::Borrowed(if allowed {
                    include_bytes!("../assets/new_tab.html").as_slice()
                } else {
                    b"Not found".as_slice()
                }))
                .expect("Static response is valid")
        })
        .with_bounds(Rect {
            position: wry::dpi::PhysicalPosition::new(0, 100).into(),
            size: wry::dpi::PhysicalSize::new(1100, 680).into(),
        })
        .with_navigation_handler(|url| {
            url == "about:blank"
                || url::Url::parse(&url).is_ok_and(|url| {
                    matches!(url.scheme(), "http" | "https") && url.host_str().is_some()
                })
        })
        .with_permission_handler(|_| wry::PermissionResponse::Deny)
        .with_new_window_req_handler(|_, _| wry::NewWindowResponse::Deny)
        .with_document_title_changed_handler(move |title| send(Action::Title(id, title)))
        .with_on_page_load_handler(move |event, url| {
            if matches!(event, wry::PageLoadEvent::Finished) {
                send(Action::Loaded(id, url));
            }
        });
        builder = if url == "about:blank" {
            builder.with_url("axiom://localhost/")
        } else {
            builder.with_url(url)
        };
        if let Some(environment) = environment {
            builder = builder.with_environment(environment);
        }
        let view = builder.build_as_child(self.window.as_ref().ok_or("Window is missing")?)?;
        shortcuts::attach_webview(&view)?;
        self.tabs.add(Tab {
            view,
            url: url.into(),
            title: if url == "about:blank" {
                "Новая вкладка".into()
            } else {
                "Загрузка…".into()
            },
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
                window.set_title(&format!("{} — Axiom Browser", tab.value.title));
            }
        }
        if let (Some(tab), Some(toolbar)) = (self.tabs.active(), &self.toolbar) {
            let mut back = Default::default();
            let mut forward = Default::default();
            unsafe {
                tab.value.view.webview().CanGoBack(&mut back)?;
                tab.value.view.webview().CanGoForward(&mut forward)?;
            }
            toolbar.set_history(back.as_bool(), forward.as_bool());
        }
        Ok(())
    }
    fn set_address(&self) {
        if let Some(tab) = self.tabs.active() {
            unsafe {
                SetWindowTextW(
                    self.edit,
                    wide(if tab.value.url == "about:blank" {
                        ""
                    } else {
                        &tab.value.url
                    })
                    .as_ptr(),
                );
            }
        }
    }
    fn resize(&mut self) -> wry::Result<()> {
        if let Some(window) = &self.window {
            let size = window.inner_size();
            let scale = window.scale_factor();
            let chrome_height = (toolbar::HEIGHT * scale).round() as u32;
            if let Some(toolbar) = &mut self.toolbar {
                toolbar.resize(size.width, scale);
            }
            if let Some(strip) = &self.strip {
                strip.resize(size.width, scale);
            }
            for tab in self.tabs.entries() {
                tab.value.view.set_bounds(Rect {
                    position: wry::dpi::PhysicalPosition::new(0, chrome_height as i32).into(),
                    size: wry::dpi::PhysicalSize::new(
                        size.width.max(1),
                        size.height.saturating_sub(chrome_height).max(1),
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
                let url = navigation::resolve_input(&input)?;
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
            Action::SelectTabId(id) => self.select_tab(id)?,
            Action::CloseTabId(id) => {
                let active = self.tabs.active().map(|tab| tab.id) == Some(id);
                if self.tabs.close(id).is_some() {
                    self.sync_tabs()?;
                    if active {
                        self.set_address();
                        if let Some(tab) = self.tabs.active() {
                            tab.value.view.focus()?;
                        }
                    }
                }
            }
            Action::RestoreAddress => {
                self.set_address();
                if let Some(tab) = self.tabs.active() {
                    tab.value.view.focus()?;
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
                        "Новая вкладка".into()
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
                tab.value.url = if url == "http://axiom.localhost/" || url == "axiom://localhost/" {
                    "about:blank".into()
                } else {
                    url.clone()
                };
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
            Action::CapturePreview => {
                self.tabs.active().ok_or("No preview tab")?.value.view.evaluate_script_with_callback("JSON.stringify({title:document.title,text:document.body.innerText.slice(0,350),form:!!document.querySelector('form[action=\"https://www.google.com/search\"]')})", |result| println!("NEW_TAB_PREVIEW {result}"))?;
                let RawWindowHandle::Win32(handle) = self
                    .window
                    .as_ref()
                    .ok_or("Window missing")?
                    .window_handle()?
                    .as_raw()
                else {
                    return Err("Windows required".into());
                };
                preview::capture(handle.hwnd.get() as HWND)?;
                std::thread::spawn(|| {
                    std::thread::sleep(std::time::Duration::from_millis(200));
                    send(Action::FinishPreview);
                });
            }
            Action::FinishPreview => self.preview_done = true,
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
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
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
        if self.tabs.len() == 0
            || self.preview_done
            || (self.smoke && (self.smoke_done || self.failed))
        {
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
    let preview = std::env::args().any(|arg| arg == "--ui-preview");
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
        toolbar: None,
        smoke,
        loads: 0,
        failed: false,
        smoke_done: false,
        preview,
        preview_done: false,
    };
    event_loop.run_app(&mut app)?;
    if app.failed || (app.smoke && !app.smoke_done) {
        std::process::exit(1);
    }
    Ok(())
}
