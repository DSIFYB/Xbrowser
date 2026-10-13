use crate::{send, Action, Browser, TabId};
use std::error::Error;
impl Browser {
    pub(super) fn check_restore(&self, id: TabId) -> Result<(), Box<dyn Error>> {
        let expression=match self.restore_step {0=>"document.body.innerText.includes('Rust')",1=>"document.title === 'Example Domain'",_=>"document.body.innerText.includes('История') && document.body.innerText.includes('rust-lang.org')"};
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
            return Err(format!("Restore verification failed at {}", self.restore_step).into());
        }
        let loaded = self
            .tabs
            .entries()
            .iter()
            .filter(|tab| tab.value.view.is_some())
            .count();
        match self.restore_step {
            0 => {
                if loaded != 1 || self.tabs.len() != 3 || id != 1 {
                    return Err("Restoration did not load only the selected tab".into());
                }
                self.act(Action::ToggleBookmark)?;
                self.restore_step = 1;
                self.select_tab(0)?;
            }
            1 => {
                if loaded != 2 || self.tabs.get_mut(2).unwrap().value.view.is_some() {
                    return Err("Background tab was eagerly loaded".into());
                }
                let store = self.store.as_ref().unwrap();
                if !store.is_bookmarked("https://rust-lang.org/")?
                    || !store
                        .history()?
                        .iter()
                        .any(|entry| entry.url == "https://rust-lang.org/")
                {
                    return Err("History/bookmark not recorded".into());
                }
                self.restore_step = 2;
                self.act(Action::History)?;
            }
            _ => {
                self.restore_done = true;
                println!("RESTORE_VERIFIED lazy_views=1 then=2 selected_tab history bookmark library_page");
            }
        }
        Ok(())
    }
}
