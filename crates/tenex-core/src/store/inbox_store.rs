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
