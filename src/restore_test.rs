use crate::{send, Action, Browser, TabId};
use std::error::Error;
use wry::WebViewExtWindows;
impl Browser {
    pub(super) fn check_restore(&self, id: TabId) -> Result<(), Box<dyn Error>> {
        let expression = if std::env::args().any(|arg| arg == "--restore-pinned-test") {
            "document.body.innerText.includes('Rust')"
        } else {
            match self.restore_step {
            0 => "document.body.innerText.includes('Rust')",
            1 | 6 => "document.title === 'Example Domain'",
            2 => "document.body.innerText.includes('История') && document.body.innerText.includes('rust-lang.org')",
            3 => "document.querySelector('.entry-title') !== null",
            4 => "document.querySelector('form[action=\"/bookmarks/save\"]') !== null",
            5 => "document.body.innerText.includes('Проверенная закладка')",
            7 => "document.querySelector('form[action=\"/settings/save\"]') !== null",
            8 => "document.body.innerText.includes('Настройки сохранены')",
            9 => "document.querySelector('form').action.startsWith('https://duckduckgo.com/')",
            10 => "document.querySelector('.entry-actions a.danger') !== null",
            11 => "document.querySelector('form[action=\"/bookmarks/delete\"]') !== null",
            12 => "document.querySelector('.entry-title') === null",
            13 => "document.querySelector('form[action=\"/history/clear\"]') !== null",
            14 | 15 | 16 => "document.body.innerText.includes('История')",
            17 | 22 | 23 => "document.title === 'Example Domain'",
            24 => "true",
            25 => "document.body.innerText.includes('Rust')",
            18 => "document.querySelector('form[action=\"/bookmarks/add\"]') !== null",
            19 | 20 => "document.body.innerText.includes('Добавленная закладка')",
            21 => "document.querySelectorAll('.entry-title').length === 2",
            _ => "document.querySelector('.entry-title') === null",
        }
        };
        let step = self.restore_step;
        self.tabs
            .active()
            .ok_or("No tab")?
            .value
            .live()?
            .evaluate_script_with_callback(expression, move |result| {
                send(Action::RestoreChecked(id, step, result))
            })?;
        Ok(())
    }
    pub(super) fn advance_restore(
        &mut self,
        id: TabId,
        result: String,
    ) -> Result<(), Box<dyn Error>> {
        if self.tabs.active().map(|tab| tab.id) != Some(id) {
            return Ok(());
        }
        if result != "true" {
            return Err(format!(
                "Core verification failed at {}: {result}",
                self.restore_step
            )
            .into());
        }
        let loaded = self
            .tabs
            .entries()
            .iter()
            .filter(|tab| tab.value.view.is_some())
            .count();
        if std::env::args().any(|arg| arg == "--restore-pinned-test") {
            if self.tabs.len() != 6
                || !self.tabs.entries()[..2].iter().all(|tab| tab.value.pinned)
                || self.tabs.entries()[2..].iter().any(|tab| tab.value.pinned)
                || self.tabs.active().unwrap().value.url != "https://rust-lang.org/"
                || self.strip.as_ref().unwrap().visible_ids().len() != 6
            {
                return Err("Pinned restoration changed order, selection or visibility".into());
            }
            let selected = self.tabs.active().unwrap().id;
            self.act(Action::CloseOtherTabs(selected, false))?;
            if self.tabs.len() != 3 || !self.tabs.entries()[..2].iter().all(|tab| tab.value.pinned)
            {
                return Err("Close others removed pinned tabs".into());
            }
            self.restore_done = true;
            println!("PINNED_RESTART_VERIFIED two_pins six_visible selected_page close_others_preserves_pins");
            return Ok(());
        }
        match self.restore_step {
            0 => {
                let expected = if self.preferences.lazy_restore { 1 } else { 4 };
                if loaded != expected
                    || self.tabs.len() != 4
                    || id != 3
                    || self.strip.as_ref().ok_or("No strip")?.visible_ids() != vec![0, 1, 2, 3]
                {
                    return Err(format!("Restored tabs mismatch: views={loaded}/{expected} count={} active={id} visible={:?}",self.tabs.len(),self.strip.as_ref().unwrap().visible_ids()).into());
                }
                self.act(Action::ToggleBookmark)?;
                let editor = self
                    .bookmark_editor
                    .as_ref()
                    .ok_or("Star did not open compact editor")?;
                if editor.values().0 != "https://rust-lang.org/" {
                    return Err("Compact editor targets the wrong page".into());
                }
                self.act(Action::DismissBookmarkEditor)?;
                if self
                    .store
                    .as_ref()
                    .unwrap()
                    .is_bookmarked("https://rust-lang.org/")?
                {
                    return Err("Cancel unexpectedly saved a bookmark".into());
                }
                self.act(Action::ToggleBookmark)?;
                self.act(Action::SaveBookmarkEditor)?;
                self.restore_step = 1;
                self.select_tab(0)?;
                if self.tabs.active().unwrap().value.loaded {
                    self.check_restore(0)?;
                }
            }
            1 => {
                if loaded != if self.preferences.lazy_restore { 2 } else { 4 } {
                    return Err("Incorrect background loading".into());
                }
                let store = self.store.as_ref().unwrap();
                if !store.is_bookmarked("https://rust-lang.org/")?
                    || !store
                        .history()?
                        .iter()
                        .any(|entry| entry.url == "https://rust-lang.org/")
                {
                    return Err("Visit/bookmark was not saved".into());
                }
                self.restore_step = 2;
                self.act(Action::History)?;
            }
            2 => {
                self.restore_step = 3;
                self.act(Action::Bookmarks)?;
            }
            3 => {
                self.restore_bookmark = self.store.as_ref().unwrap().bookmarks()?[0].id;
                self.restore_step = 4;
                self.add_tab(&format!(
                    "http://axiom.localhost/bookmarks?edit={}",
                    self.restore_bookmark
                ))?;
            }
            4 => {
                self.restore_step = 5;
                self.restore_script("const f=document.querySelector('form[action=\"/bookmarks/save\"]'); f.elements.title.value='Проверенная закладка'; f.elements.url.value='https://example.com/'; f.submit();")?;
            }
            5 => {
                let saved = self
                    .store
                    .as_ref()
                    .unwrap()
                    .bookmark(self.restore_bookmark)?
                    .ok_or("Missing bookmark")?;
                if saved.url != "https://example.com/" || saved.title != "Проверенная закладка"
                {
                    return Err("Bookmark edit was not persisted".into());
                }
                self.restore_step = 6;
                self.restore_script("document.querySelector('.entry-title').click();")?;
            }
            6 => {
                self.restore_step = 7;
                self.act(Action::Settings)?;
            }
            7 => {
                self.restore_step = 8;
                self.restore_script("const f=document.querySelector('form[action=\"/settings/save\"]'); f.elements.search_engine.value='duckduckgo'; f.elements.lazy_restore.value='false'; f.elements.confirm_close_multiple.value='true'; f.elements.page_zoom.value='125'; f.elements.remember_history.value='false'; f.submit();")?;
            }
            8 => {
                let saved = self.store.as_ref().unwrap().settings()?;
                if saved != self.preferences
                    || saved.lazy_restore
                    || !saved.confirm_close_multiple
                    || saved.search_engine != crate::settings::SearchEngine::DuckDuckGo
                    || saved.page_zoom != 125
                    || saved.remember_history
                {
                    return Err("Settings were not applied/persisted".into());
                }
                self.restore_step = 9;
                self.act(Action::NewTab)?;
            }
            9 => {
                self.restore_step = 10;
                self.act(Action::Bookmarks)?;
            }
            10 => {
                self.restore_step = 11;
                self.restore_script("document.querySelector('.entry-actions a.danger').click();")?;
            }
            11 => {
                self.restore_step = 12;
                self.restore_script(
                    "document.querySelector('form[action=\"/bookmarks/delete\"]').submit();",
                )?;
            }
            12 => {
                let reopened = crate::storage::Store::open(&self.database)?;
                if reopened.bookmark(self.restore_bookmark)?.is_some()
                    || reopened.settings()? != self.preferences
                {
                    return Err("Reopened database differs".into());
                }
                self.restore_step = 13;
                self.act(Action::History)?;
            }
            13 => {
                self.restore_step = 14;
                self.restore_script(
                    "document.querySelector('form[action=\"/history/clear\"]').submit();",
                )?;
            }
            14 => {
                if !self.store.as_ref().unwrap().history()?.is_empty() {
                    return Err("History clear failed".into());
                }
                self.restore_step = 15;
                self.act(Action::DuplicateTab(id))?;
            }
            15 => {
                let before = self.tabs.entries().iter().position(|t| t.id == id).unwrap();
                self.act(Action::MoveTab(id, -1))?;
                if self.tabs.entries()[before - 1].id != id || self.tabs.active().unwrap().id != id
                {
                    return Err("Move changed selection/order incorrectly".into());
                }
                self.act(Action::CloseTabId(id))?;
                self.restore_step = 16;
                self.act(Action::ReopenTab)?;
            }
            16 => {
                self.act(Action::CloseOtherTabs(id, false))?;
                if self.tabs.len() != 1 || self.tabs.active().unwrap().id != id {
                    return Err("Close others did not preserve target".into());
                }
                let mut zoom = 0.0;
                unsafe {
                    self.tabs
                        .active()
                        .unwrap()
                        .value
                        .live()?
                        .controller()
                        .ZoomFactor(&mut zoom)?;
                }
                if (zoom - 1.25f64).abs() > 0.001 {
                    return Err("Zoom was not applied".into());
                }
                self.restore_step = 17;
                self.add_tab("https://example.com/")?;
            }
            17 => {
                if !self.store.as_ref().unwrap().history()?.is_empty() {
                    return Err("Disabled history still records visits".into());
                }
                self.restore_step = 18;
                self.act(Action::Bookmarks)?;
            }
            18 => {
                self.restore_step = 19;
                self.restore_script("const f=document.querySelector('form[action=\"/bookmarks/add\"]'); f.elements.title.value='Добавленная закладка'; f.elements.url.value='https://example.com/'; f.submit();")?;
            }
            19 => {
                self.restore_step = 20;
                self.restore_script("const f=document.querySelector('form[action=\"/bookmarks/add\"]'); f.elements.title.value='Добавленная закладка'; f.elements.url.value='https://example.com/'; f.submit();")?;
            }
            20 => {
                if self.store.as_ref().unwrap().bookmarks()?.len() != 1 {
                    return Err("Repeated bookmark add duplicated/deleted the record".into());
                }
                self.restore_step = 21;
                self.restore_script("const f=document.querySelector('form[action=\"/bookmarks/add\"]'); f.elements.title.value='Вторая закладка'; f.elements.url.value='https://example.org/'; f.submit();")?;
            }
            21 => {
                self.restore_step = 22;
                self.restore_script("document.querySelector('.open-all').click();")?;
            }
            22 => {
                if self.tabs.len() != 5 || self.store.as_ref().unwrap().bookmarks()?.len() != 2 {
                    return Err("Open all did not open both bookmarks".into());
                }
                let count = self.tabs.len();
                self.act(Action::ToggleBookmark)?;
                self.bookmark_editor
                    .as_ref()
                    .ok_or("No compact editor")?
                    .set_title("Компактная закладка");
                self.act(Action::SaveBookmarkEditor)?;
                if self.tabs.len() != count
                    || !self
                        .store
                        .as_ref()
                        .unwrap()
                        .bookmarks()?
                        .iter()
                        .any(|entry| entry.title == "Компактная закладка")
                {
                    return Err("Compact editor opened an extra tab or did not save".into());
                }
                self.act(Action::PinTab(id))?;
                let saved = self.store.as_ref().unwrap().load_session()?;
                if !saved.tabs[0].pinned || self.tabs.entries()[0].id != id {
                    return Err("Pin/order was not persisted".into());
                }
                self.act(Action::CloseTabId(id))?;
                self.restore_step = 23;
                self.act(Action::ReopenTab)?;
            }
            23 => {
                if !self.tabs.active().unwrap().value.pinned || self.tabs.entries()[0].id != id {
                    return Err("Reopen lost pinned state".into());
                }
                let second = self.tabs.entries()[1].id;
                self.act(Action::PinTab(second))?;
                let source = self.tabs.entries()[2].id;
                let target = self.tabs.entries()[3].id;
                self.restore_step = 24;
                if !self.strip.as_ref().unwrap().exercise_drag(source, target) {
                    return Err("Drag targets are not visible".into());
                }
                send(Action::RestoreChecked(source, 24, "true".into()));
            }
            24 => {
                if self.tabs.entries()[3].id != id
                    || !self.tabs.entries()[0].value.pinned
                    || !self.tabs.entries()[1].value.pinned
                {
                    return Err("Native drag did not reorder tabs correctly".into());
                }
                let pinned = self.tabs.entries()[0].id;
                self.act(Action::DropTab(id, pinned))?;
                if self.tabs.entries()[2].id != id {
                    return Err("Drag crossed pinned boundary".into());
                }
                self.restore_step = 25;
                self.add_tab("https://rust-lang.org/")?;
            }
            _ => {
                if self.tabs.active().unwrap().value.icon.is_none() {
                    return Ok(());
                }
                if self.tabs.active().unwrap().value.generation == 0
                    || !self.tabs.active().unwrap().value.loaded
                {
                    return Err("Navigation loading lifecycle was not observed".into());
                }
                let generation = self.tabs.active().unwrap().value.generation;
                self.act(Action::Favicon(id, generation - 1, Vec::new()))?;
                if self.tabs.active().unwrap().value.icon.is_none() {
                    return Err("Stale favicon replaced the current icon".into());
                }
                self.restore_done = true;
                println!("CORE_VERIFIED compact_bookmark_save_edit native_drag pinned_order_persist_reopen_boundary real_favicon loading_state legacy_core");
            }
        }
        Ok(())
    }
    fn restore_script(&self, script: &str) -> Result<(), Box<dyn Error>> {
        self.tabs
            .active()
            .ok_or("No tab")?
            .value
            .live()?
            .evaluate_script(script)?;
        Ok(())
    }
}
