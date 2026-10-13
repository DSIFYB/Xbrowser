use crate::{send, Action, Browser, TabId};
use std::error::Error;
use wry::WebViewExtWindows;
impl Browser {
    pub(super) fn check_restore(&self, id: TabId) -> Result<(), Box<dyn Error>> {
        let expression = match self.restore_step {
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
            17 | 22 => "document.title === 'Example Domain'",
            18 => "document.querySelector('form[action=\"/bookmarks/add\"]') !== null",
            19 | 20 => "document.body.innerText.includes('Добавленная закладка')",
            21 => "document.querySelectorAll('.entry-title').length === 2",
            _ => "document.querySelector('.entry-title') === null",
        };
        self.tabs
            .active()
            .ok_or("No tab")?
            .value
            .live()?
            .evaluate_script_with_callback(expression, move |result| {
                send(Action::RestoreChecked(id, result))
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
        match self.restore_step {
            0 => {
                let expected = if self.preferences.lazy_restore { 1 } else { 4 };
                if loaded != expected
                    || self.tabs.len() != 4
                    || id != 3
                    || self.strip.as_ref().ok_or("No strip")?.visible_ids() != vec![0, 1, 2, 3]
                {
                    return Err("Restored tabs are missing, hidden or loaded incorrectly".into());
                }
                self.act(Action::ToggleBookmark)?;
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
            _ => {
                if self.tabs.len() != 5 || self.store.as_ref().unwrap().bookmarks()?.len() != 2 {
                    return Err("Open all did not open both bookmarks".into());
                }
                self.restore_done = true;
                println!("CORE_VERIFIED four_visible_tabs lazy_eager bookmarks_add_dedup_edit_open_all_confirm_delete settings_persist zoom125 clear_disable_history duplicate_move_close_reopen");
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
