pub type TabId = u64;

#[derive(Debug, PartialEq, Eq)]
pub struct TabEntry<T> {
    pub id: TabId,
    pub value: T,
}

#[derive(Debug)]
pub struct TabSet<T> {
    entries: Vec<TabEntry<T>>,
    active_id: Option<TabId>,
    next_id: TabId,
}

impl<T> Default for TabSet<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> TabSet<T> {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            active_id: None,
            next_id: 0,
        }
    }

    pub fn add(&mut self, value: T) -> TabId {
        let id = self.next_id;
        self.next_id = id.checked_add(1).expect("TabId space exhausted");
        self.entries.push(TabEntry { id, value });
        self.active_id = Some(id);
        id
    }

    /// Returns the ID that the next call to `add` will assign.
    pub fn next_id(&self) -> TabId {
        self.next_id
    }

    pub fn select(&mut self, id: TabId) -> bool {
        if self.entries.iter().any(|entry| entry.id == id) {
            self.active_id = Some(id);
            true
        } else {
            false
        }
    }

    pub fn close(&mut self, id: TabId) -> Option<T> {
        let index = self.entries.iter().position(|entry| entry.id == id)?;
        let was_active = self.active_id == Some(id);
        let entry = self.entries.remove(index);

        if was_active {
            self.active_id = if self.entries.is_empty() {
                None
            } else {
                Some(self.entries[index.min(self.entries.len() - 1)].id)
            };
        }

        Some(entry.value)
    }

    pub fn active(&self) -> Option<&TabEntry<T>> {
        self.active_id
            .and_then(|id| self.entries.iter().find(|entry| entry.id == id))
    }

    pub fn active_mut(&mut self) -> Option<&mut TabEntry<T>> {
        let id = self.active_id?;
        self.entries.iter_mut().find(|entry| entry.id == id)
    }

    pub fn entries(&self) -> &[TabEntry<T>] {
        &self.entries
    }

    pub fn get_mut(&mut self, id: TabId) -> Option<&mut TabEntry<T>> {
        self.entries.iter_mut().find(|entry| entry.id == id)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::TabSet;

    #[test]
    fn add_activates_and_select_switches_tabs() {
        let mut tabs = TabSet::new();
        let first = tabs.add("first");
        let second = tabs.add("second");

        assert_eq!(tabs.active().unwrap().id, second);
        assert!(tabs.select(first));
        assert_eq!(tabs.active().unwrap().value, "first");
        assert!(!tabs.select(99));
    }

    #[test]
    fn closing_active_selects_right_neighbor_or_last_left_neighbor() {
        let mut tabs = TabSet::new();
        let first = tabs.add(1);
        let second = tabs.add(2);
        let third = tabs.add(3);

        assert!(tabs.select(second));
        assert_eq!(tabs.close(second), Some(2));
        assert_eq!(tabs.active().unwrap().id, third);
        assert_eq!(tabs.close(third), Some(3));
        assert_eq!(tabs.active().unwrap().id, first);
    }

    #[test]
    fn closing_background_keeps_active_and_ids_are_not_reused() {
        let mut tabs = TabSet::new();
        let first = tabs.add("first");
        let second = tabs.add("second");

        assert_eq!(tabs.close(first), Some("first"));
        assert_eq!(tabs.active().unwrap().id, second);
        let third = tabs.add("third");
        assert!(third > second);
        assert!(tabs.get_mut(first).is_none());
        assert!(!tabs.select(first));
        assert_eq!(tabs.close(first), None);
    }

    #[test]
    fn closing_last_tab_leaves_no_active_tab() {
        let mut tabs = TabSet::new();
        let id = tabs.add("only");

        assert_eq!(tabs.close(id), Some("only"));
        assert_eq!(tabs.len(), 0);
        assert!(tabs.active().is_none());
        assert!(tabs.active_mut().is_none());
    }
}
