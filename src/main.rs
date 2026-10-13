mod instance;
mod internal;
mod library;
mod menu;
mod navigation;
mod preview;
mod restore_test;
mod settings;
mod shortcuts;
mod smoke;
mod storage;
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
    History,
    Bookmarks,
    ToggleBookmark,
    Menu,
    Completed(TabId, String, bool),
    RestoreChecked(TabId, String),
    Settings,
    SettingsChanged,
    BookmarksChanged,
    OpenSaved(String),
    OpenAllBookmarks,
    ReopenTab,
    DuplicateTab(TabId),
    TabMenu(TabId),
    MoveTab(TabId, isize),
    CloseOtherTabs(TabId, bool),
    HistoryCleared,
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
            9 => send(Action::Menu),
            10 => send(Action::ToggleBookmark),
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
    view: Option<WebView>,
    url: String,
    title: String,
    pinned: bool,
    loaded: bool,
}
impl Tab {
    fn live(&self) -> Result<&WebView, Box<dyn Error>> {
        self.view.as_ref().ok_or_else(|| "Tab is not loaded".into())
    }
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
    store: Option<storage::Store>,
    database: std::path::PathBuf,
    environment: Option<webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Environment>,
    restore_test: bool,
    restore_step: usize,
    restore_done: bool,
    preferences: settings::Settings,
    form_token: String,
    restore_bookmark: i64,
    closed_tabs: Vec<storage::SavedTab>,
}
impl Browser {
    fn error(&mut self, message: impl std::fmt::Display) {
        eprintln!("{message}");
        if self.smoke || self.restore_test {
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
        unsafe {
            let dark = 1i32;
            // Windows 10/11 draw the standard title bar in the application's dark palette.
            windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute(
                hwnd,
                20,
                (&dark as *const i32).cast(),
                4,
            );
        }
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
        let profile = if self.smoke || self.preview || self.restore_test {
            std::env::current_dir()?.join(if self.smoke {
                ".smoke-profile"
            } else if self.restore_test {
                ".restore-profile"
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
        self.database = profile.join("browser.sqlite3");
        self.store = Some(storage::Store::open(&self.database)?);
        self.preferences = self.store.as_ref().unwrap().settings()?;
        let session = self.store.as_ref().unwrap().load_session()?;
        self.context = Some(WebContext::new(Some(profile)));
        self.window = Some(window);
        self.shortcuts = Some(shortcuts::Shortcuts::install()?);
        if self.preferences.restore_session
            && !self.smoke
            && !self.preview
            && !session.tabs.is_empty()
        {
            for saved in session.tabs {
                self.tabs.add(Tab {
                    view: None,
                    url: saved.url,
                    title: saved.title,
                    pinned: saved.pinned,
                    loaded: false,
                });
            }
            let id = self.tabs.entries()[session.active.min(self.tabs.len() - 1)].id;
            if !self.preferences.lazy_restore {
                let ids = self
                    .tabs
                    .entries()
                    .iter()
                    .map(|tab| tab.id)
                    .collect::<Vec<_>>();
                for id in ids {
                    self.ensure_view(id)?;
                }
            }
            self.select_tab(id)?;
            self.resize()?;
        } else {
            let startup = if self.smoke {
                "https://example.com/".to_string()
            } else {
                self.preferences.home_url.clone()
            };
            self.add_tab(&startup)?;
        }
        if self.preview {
            if std::env::args().any(|arg| arg == "--preview-settings") {
                self.act(Action::Settings)?;
            } else if std::env::args().any(|arg| arg == "--preview-bookmarks") {
                let store = self.store.as_ref().unwrap();
                if !store.is_bookmarked("https://example.com/")? {
                    store.toggle_bookmark("https://example.com/", "Example Domain")?;
                }
                if !store.is_bookmarked("https://rust-lang.org/")? {
                    store.toggle_bookmark(
                        "https://rust-lang.org/",
                        "Rust — язык программирования",
                    )?;
                }
                self.act(Action::Bookmarks)?;
            } else {
                self.add_tab("https://example.com/")?;
                self.add_tab("https://rust-lang.org/")?;
                self.select_tab(0)?;
            }
            std::thread::spawn(|| {
                std::thread::sleep(std::time::Duration::from_secs(2));
                send(Action::CapturePreview);
            });
        }
        Ok(())
    }
    fn add_tab(&mut self, url: &str) -> Result<(), Box<dyn Error>> {
        let id = self.tabs.next_id();
        let view = self.create_view(id, url)?;
        self.tabs.add(Tab {
            view: Some(view),
            url: url.into(),
            title: if url == "about:blank" {
                "Новая вкладка".into()
            } else {
                "Загрузка…".into()
            },
            pinned: false,
            loaded: false,
        });
        self.sync_tabs()?;
        self.set_address();
        self.resize()?;
        self.persist()?;
        Ok(())
    }
    fn create_view(&mut self, id: TabId, url: &str) -> Result<WebView, Box<dyn Error>> {
        // All tabs explicitly reuse the first tab's COM environment and default profile.
        let environment = self.environment.clone();
        let database = self.database.clone();
        let token = self.form_token.clone();
        let navigation_token = token.clone();
        let mut builder = WebViewBuilder::new_with_web_context(
            self.context.as_mut().ok_or("Profile is missing")?,
        )
        .with_visible(false)
        .with_custom_protocol("axiom".into(), move |_, request| {
            internal::respond(&database, &token, request)
        })
        .with_bounds(Rect {
            position: wry::dpi::PhysicalPosition::new(0, 100).into(),
            size: wry::dpi::PhysicalSize::new(1100, 680).into(),
        })
        .with_navigation_handler(move |url| {
            if let Some(action) = internal::navigation_action(&url, &navigation_token) {
                send(action);
                return false;
            }
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
        unsafe {
            view.controller()
                .SetZoomFactor(self.preferences.page_zoom as f64 / 100.0)?;
        }
        shortcuts::attach_webview(&view)?;
        let mut token = 0;
        unsafe {
            view.webview().add_NavigationCompleted(
                &webview2_com::NavigationCompletedEventHandler::create(Box::new(
                    move |webview, args| {
                        if let (Some(webview), Some(args)) = (webview, args) {
                            let mut success = Default::default();
                            let mut url = Default::default();
                            args.IsSuccess(&mut success)?;
                            webview.Source(&mut url)?;
                            send(Action::Completed(
                                id,
                                webview2_com::take_pwstr(url),
                                success.as_bool(),
                            ));
                        }
                        Ok(())
                    },
                )),
                &mut token,
            )?;
        }
        if self.environment.is_none() {
            self.environment = Some(view.environment());
        }
        Ok(view)
    }
    fn ensure_view(&mut self, id: TabId) -> Result<(), Box<dyn Error>> {
        let Some(tab) = self.tabs.get_mut(id) else {
            return Ok(());
        };
        if tab.value.view.is_none() {
            let url = tab.value.url.clone();
            let view = self.create_view(id, &url)?;
            self.tabs.get_mut(id).unwrap().value.view = Some(view);
        }
        Ok(())
    }
    fn persist(&mut self) -> Result<(), Box<dyn Error>> {
        let active = self
            .tabs
            .entries()
            .iter()
            .position(|tab| Some(tab.id) == self.tabs.active().map(|tab| tab.id))
            .unwrap_or(0);
        let tabs = self
            .tabs
            .entries()
            .iter()
            .map(|tab| storage::SavedTab {
                url: tab.value.url.clone(),
                title: tab.value.title.clone(),
                pinned: tab.value.pinned,
            })
            .collect();
        if let Some(store) = &mut self.store {
            store.save_session(&storage::Session { tabs, active })?;
        }
        Ok(())
    }
    fn sync_tabs(&self) -> Result<(), Box<dyn Error>> {
        let active = self.tabs.active().map(|tab| tab.id);
        for tab in self.tabs.entries() {
            if let Some(view) = &tab.value.view {
                view.set_visible(Some(tab.id) == active)?;
            }
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
                tab.value.live()?.webview().CanGoBack(&mut back)?;
                tab.value.live()?.webview().CanGoForward(&mut forward)?;
            }
            toolbar.set_history(back.as_bool(), forward.as_bool());
            if let Some(store) = &self.store {
                toolbar.set_bookmarked(store.is_bookmarked(&tab.value.url)?);
            }
        }
        Ok(())
    }
    fn set_address(&self) {
        if let Some(tab) = self.tabs.active() {
            unsafe {
                SetWindowTextW(
                    self.edit,
                    wide(if tab.value.url == "http://axiom.localhost/history" {
                        "axiom://history"
                    } else if tab.value.url == "http://axiom.localhost/bookmarks" {
                        "axiom://bookmarks"
                    } else if tab.value.url == "http://axiom.localhost/settings" {
                        "axiom://settings"
                    } else if tab.value.url == "about:blank" {
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
                if let Some(view) = &tab.value.view {
                    view.set_bounds(Rect {
                        position: wry::dpi::PhysicalPosition::new(0, chrome_height as i32).into(),
                        size: wry::dpi::PhysicalSize::new(
                            size.width.max(1),
                            size.height.saturating_sub(chrome_height).max(1),
                        )
                        .into(),
                    })?;
                }
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
                let url = match input.trim() {
                    "axiom://history" => "http://axiom.localhost/history".into(),
                    "axiom://bookmarks" => "http://axiom.localhost/bookmarks".into(),
                    "axiom://settings" => "http://axiom.localhost/settings".into(),
                    _ => {
                        if self.preferences.search_engine == settings::SearchEngine::Google {
                            navigation::resolve_input(&input)?
                        } else {
                            navigation::resolve_with_engine(&input, self.preferences.search_engine)?
                        }
                    }
                };
                let tab = self.tabs.active_mut().ok_or("No active tab")?;
                tab.value.live()?.load_url(&url)?;
                tab.value.url = url;
                tab.value.live()?.focus()?;
            }
            Action::Back => unsafe {
                let view = &self.tabs.active().ok_or("No active tab")?.value.live()?;
                let mut allowed = Default::default();
                view.webview().CanGoBack(&mut allowed)?;
                if allowed.as_bool() {
                    view.webview().GoBack()?;
                }
            },
            Action::Forward => unsafe {
                let view = &self.tabs.active().ok_or("No active tab")?.value.live()?;
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
                    .live()?
                    .webview()
                    .Reload()?;
            },
            Action::NewTab => {
                self.add_tab(&self.preferences.home_url.clone())?;
                self.act(Action::FocusAddress)?;
            }
            Action::CloseTab => {
                if let Some(id) = self.tabs.active().map(|tab| tab.id) {
                    self.remember_closed(id);
                    if let Some(next) = self.tabs.active().map(|tab| tab.id) {
                        self.ensure_view(next)?;
                    }
                    self.sync_tabs()?;
                    self.set_address();
                }
                if let Some(tab) = self.tabs.active() {
                    tab.value.live()?.focus()?;
                }
            }
            Action::SelectTabId(id) => self.select_tab(id)?,
            Action::CloseTabId(id) => {
                let active = self.tabs.active().map(|tab| tab.id) == Some(id);
                if self.remember_closed(id) {
                    if let Some(next) = self.tabs.active().map(|tab| tab.id) {
                        self.ensure_view(next)?;
                    }
                    self.sync_tabs()?;
                    if active {
                        self.set_address();
                        if let Some(tab) = self.tabs.active() {
                            tab.value.live()?.focus()?;
                        }
                    }
                }
            }
            Action::RestoreAddress => {
                self.set_address();
                if let Some(tab) = self.tabs.active() {
                    tab.value.live()?.focus()?;
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
                tab.value.loaded = true;
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
                if self.restore_test && self.tabs.active().map(|tab| tab.id) == Some(id) {
                    self.check_restore(id)?;
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
                self.tabs.active().ok_or("No preview tab")?.value.live()?.evaluate_script_with_callback("JSON.stringify({title:document.title,text:document.body.innerText.slice(0,350),form:!!document.querySelector('form[action=\"https://www.google.com/search\"]')})", |result| println!("NEW_TAB_PREVIEW {result}"))?;
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
            Action::History => self.add_tab("http://axiom.localhost/history")?,
            Action::Bookmarks => self.add_tab("http://axiom.localhost/bookmarks")?,
            Action::ToggleBookmark => {
                if let (Some(store), Some(tab)) = (&self.store, self.tabs.active()) {
                    if storage::is_external(&tab.value.url) {
                        let existing = store
                            .bookmarks()?
                            .into_iter()
                            .find(|entry| entry.url == tab.value.url);
                        if let Some(entry) = existing {
                            self.add_tab(&format!(
                                "http://axiom.localhost/bookmarks?edit={}",
                                entry.id
                            ))?;
                        } else {
                            store.toggle_bookmark(&tab.value.url, &tab.value.title)?;
                            self.sync_tabs()?;
                        }
                    }
                }
            }
            Action::Menu => {
                let RawWindowHandle::Win32(handle) = self
                    .window
                    .as_ref()
                    .ok_or("No window")?
                    .window_handle()?
                    .as_raw()
                else {
                    return Ok(());
                };
                if let Some(action) = menu::show(handle.hwnd.get() as HWND) {
                    self.act(action)?;
                }
            }
            Action::TabMenu(id) => {
                let RawWindowHandle::Win32(handle) = self
                    .window
                    .as_ref()
                    .ok_or("No window")?
                    .window_handle()?
                    .as_raw()
                else {
                    return Ok(());
                };
                if let Some(action) = menu::show_tab(handle.hwnd.get() as HWND, id) {
                    self.act(action)?;
                }
            }
            Action::ReopenTab => {
                if let Some(saved) = self.closed_tabs.pop() {
                    self.add_tab(&saved.url)?;
                }
            }
            Action::DuplicateTab(id) => {
                if let Some(url) = self
                    .tabs
                    .entries()
                    .iter()
                    .find(|t| t.id == id)
                    .map(|t| t.value.url.clone())
                {
                    self.add_tab(&url)?;
                }
            }
            Action::MoveTab(id, offset) => {
                self.tabs.move_by(id, offset);
                self.sync_tabs()?;
            }
            Action::CloseOtherTabs(id, right_only) => {
                if let Some(index) = self.tabs.entries().iter().position(|t| t.id == id) {
                    let ids = self
                        .tabs
                        .entries()
                        .iter()
                        .enumerate()
                        .filter(|(i, t)| t.id != id && (!right_only || *i > index))
                        .map(|(_, t)| t.id)
                        .collect::<Vec<_>>();
                    for other in ids {
                        self.remember_closed(other);
                    }
                    if let Some(active) = self.tabs.active().map(|t| t.id) {
                        self.ensure_view(active)?;
                    }
                    self.sync_tabs()?;
                    self.set_address();
                }
            }
            Action::Completed(id, url, success) => {
                if success && self.preferences.remember_history && storage::is_external(&url) {
                    if let (Some(store), Some(tab)) = (
                        &self.store,
                        self.tabs.entries().iter().find(|tab| tab.id == id),
                    ) {
                        store.add_visit(&url, &tab.value.title)?;
                    }
                }
            }
            Action::RestoreChecked(id, result) => self.advance_restore(id, result)?,
            Action::Settings => self.add_tab("http://axiom.localhost/settings")?,
            Action::SettingsChanged => {
                self.preferences = self.store.as_ref().ok_or("No store")?.settings()?;
                for tab in self.tabs.entries() {
                    if let Some(view) = &tab.value.view {
                        unsafe {
                            view.controller()
                                .SetZoomFactor(self.preferences.page_zoom as f64 / 100.0)?;
                        }
                    }
                    if tab.value.url == "about:blank" {
                        if let Some(view) = &tab.value.view {
                            unsafe {
                                view.webview().Reload()?;
                            }
                        }
                    }
                }
            }
            Action::BookmarksChanged => self.sync_tabs()?,
            Action::HistoryCleared => {
                for tab in self.tabs.entries() {
                    if tab.value.url.starts_with("http://axiom.localhost/history") {
                        if let Some(view) = &tab.value.view {
                            unsafe {
                                view.webview().Reload()?;
                            }
                        }
                    }
                }
            }
            Action::OpenSaved(url) => self.add_tab(&url)?,
            Action::OpenAllBookmarks => {
                let entries = self.store.as_ref().ok_or("No store")?.bookmarks()?;
                for entry in entries {
                    self.add_tab(&entry.url)?;
                }
            }
        }
        self.persist()?;
        Ok(())
    }
    fn remember_closed(&mut self, id: TabId) -> bool {
        if let Some(tab) = self.tabs.close(id) {
            self.closed_tabs.push(storage::SavedTab {
                url: tab.url,
                title: tab.title,
                pinned: tab.pinned,
            });
            if self.closed_tabs.len() > 20 {
                self.closed_tabs.remove(0);
            }
            true
        } else {
            false
        }
    }
    fn select_tab(&mut self, id: TabId) -> Result<(), Box<dyn Error>> {
        self.ensure_view(id)?;
        if self.tabs.select(id) {
            self.sync_tabs()?;
            self.set_address();
            self.tabs.active().unwrap().value.live()?.focus()?;
            self.persist()?;
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
            WindowEvent::CloseRequested => {
                if self.preferences.confirm_close_multiple
                    && self.tabs.len() > 1
                    && !self.smoke
                    && !self.restore_test
                    && !self.preview
                {
                    let accepted = unsafe {
                        MessageBoxW(
                            std::ptr::null_mut(),
                            wide(&format!(
                                "Закрыть окно с {} вкладками? Открытые страницы будут сохранены.",
                                self.tabs.len()
                            ))
                            .as_ptr(),
                            wide("Axiom Browser").as_ptr(),
                            MB_YESNO | MB_ICONQUESTION,
                        )
                    };
                    if accepted != IDYES {
                        return;
                    }
                }
                if let Err(err) = self.persist() {
                    self.error(err);
                }
                event_loop.exit();
            }
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                if let Err(err) = self.resize() {
                    self.error(err);
                }
            }
            _ => {}
        }
    }
    fn user_event(&mut self, event_loop: &ActiveEventLoop, action: Action) {
        if matches!(action, Action::Timeout) && (self.smoke || self.restore_test) {
            self.error("Smoke test timed out");
            event_loop.exit();
            return;
        }
        if let Err(err) = self.act(action) {
            self.error(err);
        }
        if self.tabs.len() == 0
            || self.preview_done
            || self.restore_done
            || (self.restore_test && self.failed)
            || (self.smoke && (self.smoke_done || self.failed))
        {
            event_loop.exit();
        }
    }
}
fn main() -> Result<(), Box<dyn Error>> {
    if std::env::args().any(|arg| arg == "--seed-session-test") {
        let root = std::env::current_dir()?.join(".restore-profile");
        std::fs::create_dir_all(&root)?;
        let mut store = storage::Store::open(&root.join("browser.sqlite3"))?;
        store.clear_history()?;
        for entry in store.bookmarks()? {
            store.remove_bookmark(entry.id)?;
        }
        store.save_session(&storage::Session {
            active: 3,
            tabs: [
                (
                    "https://example.com/",
                    "Saved first page with a long descriptive title",
                ),
                (
                    "https://example.org/",
                    "Saved second page with a long descriptive title",
                ),
                (
                    "https://example.com/",
                    "Saved third page with a long descriptive title",
                ),
                (
                    "https://rust-lang.org/",
                    "Saved fourth page with a long descriptive title",
                ),
            ]
            .into_iter()
            .map(|(url, title)| storage::SavedTab {
                url: url.into(),
                title: title.into(),
                pinned: false,
            })
            .collect(),
        })?;
        store.save_settings(&settings::Settings {
            lazy_restore: !std::env::args().any(|arg| arg == "--restore-all"),
            ..Default::default()
        })?;
        println!("SESSION_SEEDED");
        return Ok(());
    }
    let event_loop = EventLoop::<Action>::with_user_event().build()?;
    PROXY
        .set(event_loop.create_proxy())
        .map_err(|_| "Event loop already initialized")?;
    let smoke = std::env::args().any(|arg| arg == "--smoke-test");
    let preview = std::env::args().any(|arg| arg == "--ui-preview");
    let restore_test = std::env::args().any(|arg| arg == "--restore-test");
    let _instance = if !smoke && !preview && !restore_test {
        match instance::Instance::acquire("Local\\AxiomBrowserNormal")? {
            Some(instance) => Some(instance),
            None => {
                unsafe {
                    MessageBoxW(
                        std::ptr::null_mut(),
                        wide("Браузер уже запущен. Используйте открытое окно.").as_ptr(),
                        wide("Axiom Browser").as_ptr(),
                        MB_OK | MB_ICONINFORMATION,
                    );
                }
                return Ok(());
            }
        }
    } else {
        None
    };
    if smoke || restore_test {
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
        store: None,
        database: std::path::PathBuf::new(),
        environment: None,
        restore_test,
        restore_step: 0,
        restore_done: false,
        preferences: Default::default(),
        form_token: internal::nonce()?,
        restore_bookmark: 0,
        closed_tabs: Vec::new(),
    };
    event_loop.run_app(&mut app)?;
    if app.failed || (app.smoke && !app.smoke_done) {
        std::process::exit(1);
    }
    Ok(())
}
