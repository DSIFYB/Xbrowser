use crate::{send, wide, Action, Browser, TabId};
use std::error::Error;
use windows_sys::Win32::UI::WindowsAndMessaging::*;
use winit::dpi::PhysicalSize;
use wry::WebViewExtWindows;

impl Browser {
    fn check_visibility(&self) -> Result<(), Box<dyn Error>> {
        let active = self.tabs.active().map(|tab| tab.id);
        let environment = self
            .tabs
            .entries()
            .first()
            .ok_or("No tabs")?
            .value
            .view
            .environment();
        for tab in self.tabs.entries() {
            let mut visible = Default::default();
            unsafe {
                tab.value.view.controller().IsVisible(&mut visible)?;
            }
            if visible.as_bool() != (Some(tab.id) == active) {
                return Err("Wrong tab visibility".into());
            }
            if tab.value.view.environment() != environment {
                return Err("Tabs do not share WebView2 environment".into());
            }
        }
        Ok(())
    }
    pub(super) fn check_smoke_page(&self, id: TabId) -> Result<(), Box<dyn Error>> {
        let tab = self
            .tabs
            .entries()
            .iter()
            .find(|tab| tab.id == id)
            .ok_or("Missing test tab")?;
        let example_check = "location.hostname === 'example.com' && document.title === 'Example Domain' && document.body.innerText.length > 100";
        let rust_check =
            "location.hostname === 'rust-lang.org' && document.body.innerText.includes('Rust')";
        let check = match self.loads {
            0 | 2 | 6..=8 => example_check.to_string(),
            4 => format!("(() => {{ if (!({rust_check})) return false; window.axiomSmokeMarker = 'retained'; document.cookie = 'axiom_smoke={}; SameSite=Lax; Secure; path=/'; return true; }})()", std::process::id()),
            5 => format!("({rust_check}) && document.cookie.split('; ').includes('axiom_smoke={}')", std::process::id()),
            _ => rust_check.to_string(),
        };
        tab.value
            .view
            .evaluate_script_with_callback(&check, move |result| {
                send(Action::Verified(id, result))
            })?;
        Ok(())
    }
    pub(super) fn advance_smoke(&mut self) -> Result<(), Box<dyn Error>> {
        self.check_visibility()?;
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
            5 => self.add_tab("https://rust-lang.org/")?,
            6..=8 => self.add_tab("https://example.com/")?,
            9 => {
                if self.tabs.len() != 5 {
                    return Err("Expected five loaded tabs".into());
                }
                let strip = self.strip.as_ref().ok_or("Missing tab selector")?;
                unsafe {
                    SendMessageW(strip.select, CB_SETCURSEL, 0, 0);
                }
                self.act(Action::SelectTab)?;
                self.act(Action::NextTab(true))?;
                if self.tabs.active().map(|tab| tab.id) != Some(4) {
                    return Err("Reverse tab cycle did not wrap".into());
                }
                self.act(Action::NextTab(false))?;
                if self.tabs.active().map(|tab| tab.id) != Some(0) {
                    return Err("Tab cycle did not wrap".into());
                }
                self.check_visibility()?;
                self.tabs
                    .active()
                    .unwrap()
                    .value
                    .view
                    .evaluate_script_with_callback(
                        "window.axiomSmokeMarker === 'retained'",
                        |result| send(Action::Verified(0, result)),
                    )?;
            }
            10 => {
                self.act(Action::FocusAddress)?;
                unsafe {
                    SetWindowTextW(self.edit, wide("unsubmitted.test").as_ptr());
                }
                self.act(Action::Title(2, "Background title".into()))?;
                self.act(Action::Loaded(2, "https://example.com/".into()))?;
                let mut address = [0u16; 128];
                let len = unsafe {
                    GetWindowTextW(self.edit, address.as_mut_ptr(), address.len() as i32)
                };
                if String::from_utf16_lossy(&address[..len as usize]) != "unsubmitted.test" {
                    return Err("Background event overwrote address input".into());
                }
                self.tabs.close(1);
                self.sync_tabs()?;
                self.act(Action::Loaded(1, "https://invalid.example/".into()))?;
                self.act(Action::Title(1, "Closed tab".into()))?;
                if self.tabs.len() != 4 || self.tabs.active().map(|tab| tab.id) != Some(0) {
                    return Err("Closing background tab changed active tab".into());
                }
                self.select_tab(4)?;
                self.act(Action::CloseTab)?;
                if self.tabs.active().map(|tab| tab.id) != Some(3) {
                    return Err("Active close did not choose neighbor".into());
                }
                self.check_visibility()?;
                self.act(Action::NewTab)?;
                if self.tabs.len() != 4 || self.tabs.active().map(|tab| tab.id) != Some(5) {
                    return Err("New tab did not get a fresh ID".into());
                }
                self.act(Action::CloseTab)?;
                while self.tabs.len() > 0 {
                    self.act(Action::CloseTab)?;
                }
                self.smoke_done = true;
                println!("TABS_VERIFIED count=5 shared_environment shared_cookie retained_state selector cycle input_preserved new_tab background_close active_close last_close stale_events");
            }
            _ => {}
        }
        Ok(())
    }
}
