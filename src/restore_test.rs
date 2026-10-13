use crate::{send, Action, Browser, TabId};
use std::error::Error;
impl Browser {
    pub(super) fn check_restore(&self, id: TabId) -> Result<(), Box<dyn Error>> {
        let expression = match self.restore_step {
            0 => "document.body.innerText.includes('Rust')",
            1 | 6 => "document.title === 'Example Domain'",
            2 => "document.body.innerText.includes('История') && document.body.innerText.includes('rust-lang.org')",
            3 => "document.querySelector('.entry-title') !== null",
            4 => "document.querySelector('form.editor') !== null",
            5 => "document.body.innerText.includes('Проверенная закладка')",
            7 => "document.querySelector('form[action=\"/settings/save\"]') !== null",
            8 => "document.body.innerText.includes('Настройки сохранены')",
            9 => "document.querySelector('form').action.startsWith('https://duckduckgo.com/')",
            10 => "document.querySelector('form[action=\"/bookmarks/delete\"]') !== null",
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
                self.restore_script("const f=document.querySelector('form.editor'); f.elements.title.value='Проверенная закладка'; f.elements.url.value='https://example.com/'; f.submit();")?;
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
                self.restore_script("const f=document.querySelector('form'); f.elements.search_engine.value='duckduckgo'; f.elements.lazy_restore.value='false'; f.elements.confirm_close_multiple.value='true'; f.submit();")?;
            }
            8 => {
                let saved = self.store.as_ref().unwrap().settings()?;
                if saved != self.preferences
                    || saved.lazy_restore
                    || !saved.confirm_close_multiple
                    || saved.search_engine != crate::settings::SearchEngine::DuckDuckGo
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
                self.restore_script(
                    "document.querySelector('form[action=\"/bookmarks/delete\"]').submit();",
                )?;
            }
            _ => {
                let reopened = crate::storage::Store::open(&self.database)?;
                if reopened.bookmark(self.restore_bookmark)?.is_some()
                    || reopened.settings()? != self.preferences
                {
                    return Err("Reopened database differs".into());
                }
                self.restore_done = true;
                println!("CORE_VERIFIED four_visible_tabs selected_tab loading_mode history bookmark_edit_open_delete settings_post_persist homepage_provider");
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
