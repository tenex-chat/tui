use crate::models::{Report, Thread};
use nostrdb::{Filter, Ndb, Note, Transaction};
use std::collections::HashMap;
use std::sync::Arc;

/// Sub-store for reports (kind:30023) and document discussion threads.
pub struct ReportsStore {
    /// Latest version of each report, keyed by slug
    pub reports: HashMap<String, Report>,
    /// All versions by slug (for version history)
    pub reports_all_versions: HashMap<String, Vec<Report>>,
    /// Threads by document a-tag (kind:1 events that a-tag a document)
    pub document_threads: HashMap<String, Vec<Thread>>,
}

impl Default for ReportsStore {
    fn default() -> Self {
        Self::new()
    }
}

impl ReportsStore {
    pub fn new() -> Self {
        Self {
            reports: HashMap::new(),
            reports_all_versions: HashMap::new(),
            document_threads: HashMap::new(),
        }
    }

    pub fn clear(&mut self) {
        self.reports.clear();
        self.reports_all_versions.clear();
        self.document_threads.clear();
    }

    // ===== Getters =====

    pub fn get_reports(&self) -> Vec<&Report> {
        let mut reports: Vec<_> = self.reports.values().collect();
        reports.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        reports
    }

    pub fn get_reports_by_project(&self, project_a_tag: &str) -> Vec<&Report> {
        let mut reports: Vec<_> = self
            .reports
            .values()
            .filter(|r| r.project_a_tag == project_a_tag)
            .collect();
        reports.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        reports
    }

    pub fn get_report(&self, slug: &str) -> Option<&Report> {
        self.reports.get(slug)
    }

    /// Get report by a_tag (30023:pubkey:slug) - globally unique identifier
    /// This is preferred over get_report() when you have the a_tag available,
    /// as it handles slug collisions between different authors.
    pub fn get_report_by_a_tag(&self, a_tag: &str) -> Option<&Report> {
        // a_tag format is "30023:pubkey:slug"
        // We need to search through all reports to find a match
        self.reports.values().find(|r| r.a_tag() == a_tag)
    }

    pub fn get_report_versions(&self, slug: &str) -> Vec<&Report> {
        self.reports_all_versions
            .get(slug)
            .map(|v| v.iter().collect())
            .unwrap_or_default()
    }

    pub fn get_previous_report_version(&self, slug: &str, current_id: &str) -> Option<&Report> {
        let versions = self.reports_all_versions.get(slug)?;
        let current_idx = versions.iter().position(|r| r.id == current_id)?;
        versions.get(current_idx + 1)
    }

    pub fn get_document_threads(&self, document_a_tag: &str) -> &[Thread] {
        self.document_threads
            .get(document_a_tag)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    // ===== Mutations =====

    pub fn add_report(&mut self, report: Report) {
        let slug = report.slug.clone();

        let versions = self.reports_all_versions.entry(slug.clone()).or_default();

        if versions.iter().any(|r| r.id == report.id) {
            return;
        }

        versions.push(report.clone());
        versions.sort_by(|a, b| b.created_at.cmp(&a.created_at));

        if let Some(latest) = versions.first() {
            self.reports.insert(slug, latest.clone());
        }
    }

    pub fn add_document_thread(&mut self, document_a_tag: &str, thread: Thread) {
        let threads = self
            .document_threads
            .entry(document_a_tag.to_string())
            .or_default();
        if !threads.iter().any(|t| t.id == thread.id) {
            threads.push(thread);
            threads.sort_by(|a, b| b.last_activity.cmp(&a.last_activity));
        }
    }

    pub fn handle_report_event(
        &mut self,
        note: &Note,
        known_project_a_tags: &[String],
    ) -> Option<Report> {
        if let Some(report) = Report::from_note(note) {
            if known_project_a_tags.contains(&report.project_a_tag) {
                self.add_report(report.clone());
                return Some(report);
            }
        }
        None
    }

    // ===== Loader =====

    pub fn load_reports(&mut self, ndb: &Arc<Ndb>, project_a_tags: &[String]) {
        if project_a_tags.is_empty() {
            return;
        }

        let Ok(txn) = Transaction::new(ndb) else {
            return;
        };

        let a_tag_refs: Vec<&str> = project_a_tags.iter().map(|s| s.as_str()).collect();
        let filter = Filter::new().kinds([30023]).tags(a_tag_refs, 'a').build();
        let Ok(results) = ndb.query(&txn, &[filter], 1000) else {
            return;
        };

        for result in results {
            if let Ok(note) = ndb.get_note_by_key(&txn, result.note_key) {
                if let Some(report) = Report::from_note(&note) {
                    self.add_report(report);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_report(slug: &str, project_a_tag: &str, created_at: u64) -> Report {
        Report {
            id: format!("report-{}-{}", slug, created_at),
            slug: slug.to_string(),
            project_a_tag: project_a_tag.to_string(),
            author: "author1".to_string(),
            title: format!("Report {}", slug),
            summary: String::new(),
            content: "Report content".to_string(),
            hashtags: vec![],
            created_at,
            reading_time_mins: 1,
        }
    }

    fn make_test_thread(id: &str, pubkey: &str, last_activity: u64) -> Thread {
        Thread {
            id: id.to_string(),
            title: format!("Thread {}", id),
            content: String::new(),
            pubkey: pubkey.to_string(),
            last_activity,
            effective_last_activity: last_activity,
            status_label: None,
            status_current_activity: None,
            summary: None,
            hashtags: vec![],
            parent_conversation_id: None,
            p_tags: vec![],
            ask_event: None,
            is_scheduled: false,
        }
    }

    #[test]
    fn test_empty_store() {
        let store = ReportsStore::new();
        assert!(store.get_reports().is_empty());
        assert!(store.get_report("slug").is_none());
        assert!(store.get_reports_by_project("proj").is_empty());
        assert!(store.get_document_threads("doc").is_empty());
    }

    #[test]
    fn test_sorted_by_created_at_descending() {
        let mut store = ReportsStore::new();
        store.add_report(make_test_report("slug-a", "proj1", 100));
        store.add_report(make_test_report("slug-b", "proj1", 300));
        store.add_report(make_test_report("slug-c", "proj1", 200));

        let reports = store.get_reports();
        assert_eq!(reports.len(), 3);
        assert_eq!(reports[0].slug, "slug-b");
        assert_eq!(reports[1].slug, "slug-c");
        assert_eq!(reports[2].slug, "slug-a");
    }

    #[test]
    fn test_filtered_by_project() {
        let mut store = ReportsStore::new();
        store.add_report(make_test_report("slug-a", "proj1", 100));
        store.add_report(make_test_report("slug-b", "proj2", 200));
        store.add_report(make_test_report("slug-c", "proj1", 300));

        let proj1_reports = store.get_reports_by_project("proj1");
        assert_eq!(proj1_reports.len(), 2);
        assert_eq!(proj1_reports[0].slug, "slug-c");
        assert_eq!(proj1_reports[1].slug, "slug-a");

        let proj2_reports = store.get_reports_by_project("proj2");
        assert_eq!(proj2_reports.len(), 1);
    }

    #[test]
    fn test_version_history() {
        let mut store = ReportsStore::new();

        let v1 = Report {
            id: "v1-id".to_string(),
            slug: "my-report".to_string(),
            project_a_tag: "proj1".to_string(),
            author: "author".to_string(),
            title: "Version 1".to_string(),
            summary: String::new(),
            content: "v1 content".to_string(),
            hashtags: vec![],
            created_at: 100,
            reading_time_mins: 1,
        };
        let v2 = Report {
            id: "v2-id".to_string(),
            slug: "my-report".to_string(),
            project_a_tag: "proj1".to_string(),
            author: "author".to_string(),
            title: "Version 2".to_string(),
            summary: String::new(),
            content: "v2 content".to_string(),
            hashtags: vec![],
            created_at: 200,
            reading_time_mins: 1,
        };

        store.add_report(v1);
        store.add_report(v2);

        let latest = store.get_report("my-report").unwrap();
        assert_eq!(latest.title, "Version 2");

        let versions = store.get_report_versions("my-report");
        assert_eq!(versions.len(), 2);
        assert_eq!(versions[0].id, "v2-id");
        assert_eq!(versions[1].id, "v1-id");

        let prev = store.get_previous_report_version("my-report", "v2-id");
        assert!(prev.is_some());
        assert_eq!(prev.unwrap().id, "v1-id");

        assert!(store
            .get_previous_report_version("my-report", "v1-id")
            .is_none());
    }

    #[test]
    fn test_document_threads() {
        let mut store = ReportsStore::new();
        let thread = make_test_thread("t1", "pk1", 100);
        store
            .document_threads
            .entry("doc-atag".to_string())
            .or_default()
            .push(thread);

        let threads = store.get_document_threads("doc-atag");
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0].id, "t1");

        assert!(store.get_document_threads("missing").is_empty());
    }

    #[test]
    fn test_cleared_on_clear() {
        let mut store = ReportsStore::new();
        store.add_report(make_test_report("slug", "proj1", 100));
        store
            .document_threads
            .entry("doc".to_string())
            .or_default()
            .push(make_test_thread("t1", "pk1", 100));

        store.clear();

        assert!(store.get_reports().is_empty());
        assert!(store.get_document_threads("doc").is_empty());
    }
}
