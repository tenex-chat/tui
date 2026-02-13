use crate::models::InboxItem;
use std::collections::HashSet;

/// Sub-store for inbox items (events that p-tag the current user).
pub struct InboxStore {
    pub items: Vec<InboxItem>,
    read_ids: HashSet<String>,
}

impl InboxStore {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            read_ids: HashSet::new(),
        }
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.read_ids.clear();
    }

    // ===== Getters =====

    pub fn get_items(&self) -> &[InboxItem] {
        &self.items
    }

    pub fn is_read(&self, id: &str) -> bool {
        self.read_ids.contains(id)
    }

    pub fn contains(&self, id: &str) -> bool {
        self.items.iter().any(|i| i.id == id)
    }

    // ===== Mutations =====

    pub fn add_item(&mut self, item: InboxItem) {
        // Check if already read (persisted)
        let is_read = self.read_ids.contains(&item.id);
        let mut item = item;
        item.is_read = is_read;

        // Deduplicate by id
        if !self.items.iter().any(|i| i.id == item.id) {
            // Insert sorted by created_at (most recent first)
            let pos = self.items.partition_point(|i| i.created_at > item.created_at);
            self.items.insert(pos, item);
        }
    }

    pub fn mark_read(&mut self, id: &str) {
        if let Some(item) = self.items.iter_mut().find(|i| i.id == id) {
            item.is_read = true;
        }
        self.read_ids.insert(id.to_string());
    }

    /// Push an item directly (used by populate_inbox_from_existing which handles its own ordering)
    pub fn push_raw(&mut self, item: InboxItem) {
        self.items.push(item);
    }

    /// Sort items by created_at descending
    pub fn sort(&mut self) {
        self.items.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::InboxEventType;

    fn make_test_inbox_item(id: &str, event_type: InboxEventType, created_at: u64) -> InboxItem {
        InboxItem {
            id: id.to_string(),
            event_type,
            title: format!("Inbox {}", id),
            content: "content".to_string(),
            project_a_tag: "31933:pk:proj1".to_string(),
            author_pubkey: "author1".to_string(),
            created_at,
            is_read: false,
            thread_id: None,
            ask_event: None,
        }
    }

    #[test]
    fn test_add_and_sort_order() {
        let mut store = InboxStore::new();
        store.add_item(make_test_inbox_item("i1", InboxEventType::Ask, 100));
        store.add_item(make_test_inbox_item("i3", InboxEventType::Mention, 300));
        store.add_item(make_test_inbox_item("i2", InboxEventType::Ask, 200));

        let items = store.get_items();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].id, "i3");
        assert_eq!(items[1].id, "i2");
        assert_eq!(items[2].id, "i1");
    }

    #[test]
    fn test_deduplication() {
        let mut store = InboxStore::new();
        store.add_item(make_test_inbox_item("i1", InboxEventType::Ask, 100));
        store.add_item(make_test_inbox_item("i1", InboxEventType::Ask, 200));

        assert_eq!(store.get_items().len(), 1);
    }

    #[test]
    fn test_mark_read() {
        let mut store = InboxStore::new();
        store.add_item(make_test_inbox_item("i1", InboxEventType::Ask, 100));
        assert!(!store.get_items()[0].is_read);

        store.mark_read("i1");
        assert!(store.get_items()[0].is_read);
    }

    #[test]
    fn test_read_state_persists_for_new_items() {
        let mut store = InboxStore::new();
        store.mark_read("i1");

        store.add_item(make_test_inbox_item("i1", InboxEventType::Ask, 100));
        assert!(store.get_items()[0].is_read);
    }

    #[test]
    fn test_cleared_on_clear() {
        let mut store = InboxStore::new();
        store.add_item(make_test_inbox_item("i1", InboxEventType::Ask, 100));
        store.clear();

        assert!(store.get_items().is_empty());
    }
}
