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
        let mut reports: Vec<_> = self.reports
            .values()
            .filter(|r| r.project_a_tag == project_a_tag)
            .collect();
        reports.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        reports
    }

    pub fn get_report(&self, slug: &str) -> Option<&Report> {
        self.reports.get(slug)
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
        self.document_threads.get(document_a_tag)
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
        let threads = self.document_threads.entry(document_a_tag.to_string()).or_default();
        if !threads.iter().any(|t| t.id == thread.id) {
            threads.push(thread);
            threads.sort_by(|a, b| b.last_activity.cmp(&a.last_activity));
        }
    }

    pub fn handle_report_event(&mut self, note: &Note, known_project_a_tags: &[String]) {
        if let Some(report) = Report::from_note(note) {
            if known_project_a_tags.iter().any(|tag| *tag == report.project_a_tag) {
                self.add_report(report);
            }
        }
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
        let filter = Filter::new()
            .kinds([30023])
            .tags(a_tag_refs, 'a')
            .build();
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
