use std::collections::{HashMap, HashSet};

/// Unix timestamp for January 24, 2025 00:00:00 UTC.
/// Runtime data from before this date should be excluded from stats calculations.
///
/// **Why this cutoff exists:**
/// The way LLM runtime was tracked changed on January 24, 2025.
/// Data from before this date uses a different tracking methodology and is not
/// comparable to current data. Including it would skew statistics with
/// inconsistent/unreliable values. This affects:
/// - Per-day LLM runtime bar chart
/// - Total runtime calculations
/// - Average runtime calculations
/// - Top 10 Longest Conversations ranking
///
/// Cost data is NOT affected by this cutoff - only runtime data.
pub const RUNTIME_CUTOFF_TIMESTAMP: u64 = 1737676800;

/// Centralized store for hierarchical conversation runtime tracking.
/// Tracks individual conversation runtimes and parent-child relationships
/// to enable efficient recursive runtime aggregation.
/// Also tracks last_activity timestamps for hierarchical sorting in the Conversations tab.
#[derive(Debug, Default)]
pub struct RuntimeHierarchy {
    /// Each conversation's individual (net) runtime in milliseconds
    /// This is the sum of llm-runtime tags from messages in that conversation only
    individual_runtimes: HashMap<String, u64>,

    /// Each conversation's creation timestamp (Unix seconds)
    /// Used for today-only filtering in get_today_unique_runtime()
    conversation_created_at: HashMap<String, u64>,

    /// Each conversation's individual last_activity timestamp (Unix seconds)
    /// This is the conversation's own last_activity, not including descendants.
    /// Used as input for effective_last_activity calculation.
    individual_last_activity: HashMap<String, u64>,

    /// Parent-child relationships: parent_id -> set of child_ids
    /// Built via set_parent() from both q-tags (parent has q-tag pointing to child)
    /// and delegation tags (child's delegation tag points to parent)
    children: HashMap<String, HashSet<String>>,

    /// Reverse lookup: child_id -> parent_id
    /// A conversation can have at most one parent.
    /// Updated via set_parent() which is the single source of truth for parent-child relationships.
    parents: HashMap<String, String>,

    /// Cached sum of all individual runtimes (flat aggregation).
    /// Updated incrementally when individual runtimes change.
    /// Avoids O(n) calculation on every render.
    cached_total_unique_runtime: u64,

    /// Cached sum of today's runtimes (flat aggregation).
    /// Calculated on demand and cached for the current day.
    /// Stores (day_start_timestamp, cached_runtime) to detect day changes.
    cached_today_runtime: Option<(u64, u64)>,
}

impl RuntimeHierarchy {
    pub fn new() -> Self {
        Self::default()
    }

    /// Update the individual runtime for a conversation.
    /// Also updates the cached total to avoid O(n) recalculation on read.
    pub fn set_individual_runtime(&mut self, conversation_id: &str, runtime_ms: u64) {
        // Get the old value (0 if not present)
        let old_runtime = self
            .individual_runtimes
            .get(conversation_id)
            .copied()
            .unwrap_or(0);

        // Update the individual runtime
        self.individual_runtimes
            .insert(conversation_id.to_string(), runtime_ms);

        // Incrementally update the cached total: add new, subtract old
        self.cached_total_unique_runtime = self
            .cached_total_unique_runtime
            .saturating_sub(old_runtime)
            .saturating_add(runtime_ms);

        // Invalidate today's cache when runtimes change (will be recalculated on next read)
        self.cached_today_runtime = None;
    }

    /// Set the creation timestamp for a conversation.
    /// This is used for today-only filtering in get_today_unique_runtime().
    pub fn set_conversation_created_at(&mut self, conversation_id: &str, created_at: u64) {
        let old_created_at = self.conversation_created_at.get(conversation_id).copied();

        self.conversation_created_at
            .insert(conversation_id.to_string(), created_at);

        // Only invalidate today's cache if the creation date changed
        // (which could affect whether it's included in today's total)
        if old_created_at != Some(created_at) {
            self.cached_today_runtime = None;
        }
    }

    /// Get the creation timestamp for a conversation (if known)
    pub fn get_conversation_created_at(&self, conversation_id: &str) -> Option<u64> {
        self.conversation_created_at.get(conversation_id).copied()
    }

    /// Get the individual (net) runtime for a conversation
    pub fn get_individual_runtime(&self, conversation_id: &str) -> u64 {
        self.individual_runtimes
            .get(conversation_id)
            .copied()
            .unwrap_or(0)
    }

    // ===== Last Activity Methods (for hierarchical sorting) =====

    /// Set the individual last_activity timestamp for a conversation.
    /// This is the conversation's own last_activity (not including descendants).
    pub fn set_individual_last_activity(&mut self, conversation_id: &str, timestamp: u64) {
        self.individual_last_activity
            .insert(conversation_id.to_string(), timestamp);
    }

    /// Get the individual last_activity timestamp for a conversation (if set)
    pub fn get_individual_last_activity(&self, conversation_id: &str) -> Option<u64> {
        self.individual_last_activity.get(conversation_id).copied()
    }

    /// Calculate effective last_activity for a conversation including all descendants.
    /// This is the maximum of the conversation's own last_activity and all its descendants' last_activity.
    /// Used for hierarchical sorting in the Conversations tab.
    pub fn get_effective_last_activity(&self, conversation_id: &str) -> u64 {
        self.get_effective_last_activity_with_visited(conversation_id, &mut HashSet::new())
    }

    /// Internal recursive implementation with cycle detection
    fn get_effective_last_activity_with_visited(
        &self,
        conversation_id: &str,
        visited: &mut HashSet<String>,
    ) -> u64 {
        // Cycle detection
        if visited.contains(conversation_id) {
            return 0;
        }
        visited.insert(conversation_id.to_string());

        // Start with this conversation's individual last_activity
        let mut max_activity = self.individual_last_activity
            .get(conversation_id)
            .copied()
            .unwrap_or(0);

        // Check all children recursively for their effective last_activity
        if let Some(children) = self.children.get(conversation_id) {
            for child_id in children {
                let child_activity = self.get_effective_last_activity_with_visited(child_id, visited);
                if child_activity > max_activity {
                    max_activity = child_activity;
                }
            }
        }

        max_activity
    }

    /// Add a parent-child relationship from the parent's perspective.
    /// This is typically discovered from q-tags (parent has q-tag pointing to child).
    /// Delegates to set_parent() to ensure graph consistency.
    /// Returns true if this was a new relationship (not already known).
    pub fn add_child(&mut self, parent_id: &str, child_id: &str) -> bool {
        // Delegate to set_parent which is the single source of truth
        self.set_parent(child_id, parent_id)
    }

    /// Set parent for a child conversation.
    /// This is the single source of truth for parent-child relationships.
    /// Handles re-parenting by cleaning up old edges before adding new ones.
    /// Returns true if this was a new or changed relationship.
    ///
    /// When setting a new parent:
    /// 1. Remove child from old parent's children set (if any)
    /// 2. Clear the child's old parent entry
    /// 3. Add the new relationship
    pub fn set_parent(&mut self, child_id: &str, parent_id: &str) -> bool {
        // Don't add self-referential relationships
        if parent_id == child_id {
            return false;
        }

        // Check if relationship already exists with the same parent
        if let Some(existing_parent) = self.parents.get(child_id) {
            if existing_parent == parent_id {
                return false; // No change needed
            }
        }

        // Clean up old relationship if child already has a different parent
        if let Some(old_parent_id) = self.parents.get(child_id).cloned() {
            // Only clean up if we're actually changing parents
            if old_parent_id != parent_id {
                // Remove child from old parent's children set
                if let Some(old_children) = self.children.get_mut(&old_parent_id) {
                    old_children.remove(child_id);
                    // If old parent has no more children, remove the empty set
                    if old_children.is_empty() {
                        self.children.remove(&old_parent_id);
                    }
                }
            }
        }

        // Add new relationship
        self.parents
            .insert(child_id.to_string(), parent_id.to_string());
        self.children
            .entry(parent_id.to_string())
            .or_default()
            .insert(child_id.to_string());

        true // Relationship was added or changed
    }

    /// Get the parent of a conversation (if any)
    pub fn get_parent(&self, child_id: &str) -> Option<&String> {
        self.parents.get(child_id)
    }

    /// Get direct children of a conversation
    pub fn get_children(&self, parent_id: &str) -> Option<&HashSet<String>> {
        self.children.get(parent_id)
    }

    /// Check if a conversation has any children
    pub fn has_children(&self, conversation_id: &str) -> bool {
        self.children
            .get(conversation_id)
            .map(|c| !c.is_empty())
            .unwrap_or(false)
    }

    /// Calculate total runtime for a conversation including all descendants (recursive)
    /// This implements the hierarchical rollup:
    /// - conv1 = conv1_runtime + conv2_total + conv3_total + ...
    pub fn get_total_runtime(&self, conversation_id: &str) -> u64 {
        self.get_total_runtime_with_visited(conversation_id, &mut HashSet::new())
    }

    /// Internal recursive implementation with cycle detection
    fn get_total_runtime_with_visited(
        &self,
        conversation_id: &str,
        visited: &mut HashSet<String>,
    ) -> u64 {
        // Cycle detection
        if visited.contains(conversation_id) {
            return 0;
        }
        visited.insert(conversation_id.to_string());

        // Start with this conversation's individual runtime
        let mut total = self.get_individual_runtime(conversation_id);

        // Add runtime from all children recursively
        if let Some(children) = self.children.get(conversation_id) {
            for child_id in children {
                total += self.get_total_runtime_with_visited(child_id, visited);
            }
        }

        total
    }

    /// Get all ancestor conversation IDs (parents, grandparents, etc.)
    /// Used to know which conversations need to update their displayed runtime
    /// when this conversation's runtime changes
    pub fn get_ancestors(&self, conversation_id: &str) -> Vec<String> {
        let mut ancestors = Vec::new();
        let mut current = conversation_id;

        loop {
            match self.parents.get(current) {
                Some(parent_id) => {
                    // Cycle detection
                    if ancestors.contains(parent_id) {
                        break;
                    }
                    ancestors.push(parent_id.clone());
                    current = parent_id;
                }
                None => break,
            }
        }

        ancestors
    }

    /// Get all descendant conversation IDs (children, grandchildren, etc.)
    pub fn get_descendants(&self, conversation_id: &str) -> Vec<String> {
        let mut descendants = Vec::new();
        self.collect_descendants(conversation_id, &mut descendants, &mut HashSet::new());
        descendants
    }

    fn collect_descendants(
        &self,
        conversation_id: &str,
        result: &mut Vec<String>,
        visited: &mut HashSet<String>,
    ) {
        if visited.contains(conversation_id) {
            return;
        }
        visited.insert(conversation_id.to_string());

        if let Some(children) = self.children.get(conversation_id) {
            for child_id in children {
                result.push(child_id.clone());
                self.collect_descendants(child_id, result, visited);
            }
        }
    }

    /// Clear all data (useful for rebuilds)
    pub fn clear(&mut self) {
        self.individual_runtimes.clear();
        self.conversation_created_at.clear();
        self.individual_last_activity.clear();
        self.children.clear();
        self.parents.clear();
        self.cached_total_unique_runtime = 0;
        self.cached_today_runtime = None;
    }

    /// Get the number of conversations tracked
    pub fn conversation_count(&self) -> usize {
        self.individual_runtimes.len()
    }

    /// Get the number of relationships tracked
    pub fn relationship_count(&self) -> usize {
        self.parents.len()
    }

    /// Get the sum of all individual runtimes (flat aggregation).
    /// Unlike get_total_runtime which follows hierarchies and counts children
    /// under their parents, this simply sums each conversation's runtime exactly once.
    /// Used for the global status bar runtime display.
    ///
    /// **Note:** Excludes runtime data from before RUNTIME_CUTOFF_TIMESTAMP
    /// (January 24, 2025) due to tracking methodology changes.
    ///
    /// This is O(n) as it filters by creation date. For unfiltered totals,
    /// the cached_total_unique_runtime is still maintained for internal use.
    pub fn get_total_unique_runtime(&self) -> u64 {
        self.individual_runtimes
            .iter()
            .filter(|(conv_id, _)| {
                self.conversation_created_at
                    .get(*conv_id)
                    .map(|created_at| *created_at >= RUNTIME_CUTOFF_TIMESTAMP)
                    .unwrap_or(false)
            })
            .map(|(_, runtime)| runtime)
            .sum()
    }

    /// Get the sum of individual runtimes for conversations created TODAY only (UTC).
    /// Filters conversations by creation date (today's UTC day boundaries), then sums their runtimes.
    /// Used for the global status bar to show today's cumulative runtime.
    ///
    /// Uses caching: O(n) on first call or after cache invalidation, O(1) on subsequent calls
    /// within the same day.
    ///
    /// **Note:** Excludes runtime data from before RUNTIME_CUTOFF_TIMESTAMP
    /// (January 24, 2025) due to tracking methodology changes.
    pub fn get_today_unique_runtime(&mut self) -> u64 {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        // Calculate day start (midnight UTC, simplified - for local time would need chrono)
        // Using a simple approach: day = timestamp / 86400, day_start = day * 86400
        let seconds_per_day: u64 = 86400;
        let current_day_start = (now / seconds_per_day) * seconds_per_day;

        // Check if cache is valid (same day)
        if let Some((cached_day_start, cached_runtime)) = self.cached_today_runtime {
            if cached_day_start == current_day_start {
                return cached_runtime;
            }
        }

        // Cache miss or day changed - recalculate
        let today_runtime: u64 = self
            .individual_runtimes
            .iter()
            .filter(|(conv_id, _)| {
                // Check if conversation was created today AND after the runtime cutoff
                self.conversation_created_at
                    .get(*conv_id)
                    .map(|created_at| {
                        // Filter out data from before the runtime tracking cutoff date
                        if *created_at < RUNTIME_CUTOFF_TIMESTAMP {
                            return false;
                        }
                        let conv_day_start = (*created_at / seconds_per_day) * seconds_per_day;
                        conv_day_start == current_day_start
                    })
                    .unwrap_or(false)
            })
            .map(|(_, runtime)| runtime)
            .sum();

        // Update cache
        self.cached_today_runtime = Some((current_day_start, today_runtime));

        today_runtime
    }

    /// Get runtime aggregated by calendar day (UTC).
    /// Returns a vector of (day_start_timestamp, total_runtime_ms) tuples,
    /// sorted by day in ascending order (oldest first).
    /// Only includes days with non-zero runtime.
    /// Used for displaying runtime bar charts in the Stats tab.
    ///
    /// **Note:** Excludes runtime data from before RUNTIME_CUTOFF_TIMESTAMP
    /// (January 24, 2025) due to tracking methodology changes.
    pub fn get_runtime_by_day(&self, num_days: usize) -> Vec<(u64, u64)> {
        // Guard against zero days
        if num_days == 0 {
            return Vec::new();
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let seconds_per_day: u64 = 86400;
        let today_start = (now / seconds_per_day) * seconds_per_day;
        // Use saturating_sub to prevent underflow
        let time_window_cutoff = today_start.saturating_sub((num_days as u64).saturating_sub(1) * seconds_per_day);

        // Group runtimes by day
        let mut by_day: std::collections::HashMap<u64, u64> = std::collections::HashMap::new();

        for (conv_id, runtime) in &self.individual_runtimes {
            if *runtime == 0 {
                continue;
            }
            if let Some(created_at) = self.conversation_created_at.get(conv_id) {
                // Filter out data from before the runtime tracking cutoff date
                if *created_at < RUNTIME_CUTOFF_TIMESTAMP {
                    continue;
                }
                let day_start = (*created_at / seconds_per_day) * seconds_per_day;
                if day_start >= time_window_cutoff {
                    *by_day.entry(day_start).or_insert(0) += runtime;
                }
            }
        }

        // Convert to sorted vector (oldest first)
        let mut result: Vec<(u64, u64)> = by_day.into_iter().collect();
        result.sort_by_key(|(day, _)| *day);
        result
    }

    /// Get top N root conversations by total runtime (including all descendants).
    /// Returns (conversation_id, total_runtime_ms) tuples sorted by runtime descending.
    /// Only includes ROOT conversations (those without parents).
    /// Used for the Stats tab's "Top 10 Longest Conversations" display.
    ///
    /// **Note:** Excludes runtime data from before RUNTIME_CUTOFF_TIMESTAMP
    /// (January 24, 2025) due to tracking methodology changes.
    pub fn get_top_conversations_by_runtime(&self, limit: usize) -> Vec<(String, u64)> {
        // Build unified set of all conversation IDs from:
        // - individual_runtimes keys
        // - parents keys (children that may not have runtime)
        // - children keys (parents that may not have direct runtime)
        let mut all_conv_ids: std::collections::HashSet<&String> = std::collections::HashSet::new();

        for id in self.individual_runtimes.keys() {
            all_conv_ids.insert(id);
        }
        for id in self.parents.keys() {
            all_conv_ids.insert(id);
        }
        for id in self.children.keys() {
            all_conv_ids.insert(id);
        }

        // Find all root conversations (those without parents)
        let root_conversations: Vec<&String> = all_conv_ids
            .into_iter()
            .filter(|id| !self.parents.contains_key(*id))
            .collect();

        // Calculate total runtime for each root (including all descendants)
        // using the filtered version that excludes pre-cutoff data
        let mut runtimes: Vec<(String, u64)> = root_conversations
            .into_iter()
            .map(|id| (id.clone(), self.get_total_runtime_filtered(id)))
            .filter(|(_, runtime)| *runtime > 0)
            .collect();

        // Sort by runtime descending
        runtimes.sort_by(|a, b| b.1.cmp(&a.1));
        runtimes.truncate(limit);
        runtimes
    }

    /// Calculate total runtime for a conversation including all descendants,
    /// but ONLY counting runtime from conversations created after RUNTIME_CUTOFF_TIMESTAMP.
    /// This is used for stats display to exclude pre-cutoff data.
    fn get_total_runtime_filtered(&self, conversation_id: &str) -> u64 {
        self.get_total_runtime_filtered_with_visited(conversation_id, &mut HashSet::new())
    }

    /// Internal recursive implementation with cycle detection and timestamp filtering
    fn get_total_runtime_filtered_with_visited(
        &self,
        conversation_id: &str,
        visited: &mut HashSet<String>,
    ) -> u64 {
        // Cycle detection
        if visited.contains(conversation_id) {
            return 0;
        }
        visited.insert(conversation_id.to_string());

        // Check if this conversation was created after the cutoff
        let is_after_cutoff = self
            .conversation_created_at
            .get(conversation_id)
            .map(|created_at| *created_at >= RUNTIME_CUTOFF_TIMESTAMP)
            .unwrap_or(false);

        // Start with this conversation's individual runtime (only if after cutoff)
        let mut total = if is_after_cutoff {
            self.get_individual_runtime(conversation_id)
        } else {
            0
        };

        // Add runtime from all children recursively (each child also filtered)
        if let Some(children) = self.children.get(conversation_id) {
            for child_id in children {
                total += self.get_total_runtime_filtered_with_visited(child_id, visited);
            }
        }

        total
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_individual_runtime() {
        let mut hierarchy = RuntimeHierarchy::new();
        hierarchy.set_individual_runtime("conv1", 10_000);

        assert_eq!(hierarchy.get_individual_runtime("conv1"), 10_000);
        assert_eq!(hierarchy.get_individual_runtime("unknown"), 0);
    }

    #[test]
    fn test_parent_child_relationship() {
        let mut hierarchy = RuntimeHierarchy::new();
        hierarchy.add_child("parent", "child");

        assert_eq!(hierarchy.get_parent("child"), Some(&"parent".to_string()));
        assert!(hierarchy.has_children("parent"));
        assert!(!hierarchy.has_children("child"));
    }

    #[test]
    fn test_recursive_runtime_simple() {
        // conv1 -> conv2 -> conv3
        let mut hierarchy = RuntimeHierarchy::new();
        hierarchy.set_individual_runtime("conv1", 10_000); // 10s
        hierarchy.set_individual_runtime("conv2", 30_000); // 30s
        hierarchy.set_individual_runtime("conv3", 300_000); // 5min

        hierarchy.add_child("conv1", "conv2");
        hierarchy.add_child("conv2", "conv3");

        // conv3: 5min
        assert_eq!(hierarchy.get_total_runtime("conv3"), 300_000);
        // conv2: 30s + 5min = 5min30s
        assert_eq!(hierarchy.get_total_runtime("conv2"), 330_000);
        // conv1: 10s + 30s + 5min = 5min40s
        assert_eq!(hierarchy.get_total_runtime("conv1"), 340_000);
    }

    #[test]
    fn test_recursive_runtime_multiple_children() {
        // conv1 -> conv2, conv3
        let mut hierarchy = RuntimeHierarchy::new();
        hierarchy.set_individual_runtime("conv1", 10_000);
        hierarchy.set_individual_runtime("conv2", 20_000);
        hierarchy.set_individual_runtime("conv3", 30_000);

        hierarchy.add_child("conv1", "conv2");
        hierarchy.add_child("conv1", "conv3");

        // conv1: 10s + 20s + 30s = 60s
        assert_eq!(hierarchy.get_total_runtime("conv1"), 60_000);
    }

    #[test]
    fn test_cycle_detection() {
        let mut hierarchy = RuntimeHierarchy::new();
        hierarchy.set_individual_runtime("conv1", 10_000);
        hierarchy.set_individual_runtime("conv2", 20_000);

        // Create a cycle: conv1 -> conv2 -> conv1
        hierarchy.add_child("conv1", "conv2");
        hierarchy.add_child("conv2", "conv1");

        // Should not infinite loop, should only count each once
        let total = hierarchy.get_total_runtime("conv1");
        assert_eq!(total, 30_000); // 10s + 20s, no infinite loop
    }

    #[test]
    fn test_self_reference_ignored() {
        let mut hierarchy = RuntimeHierarchy::new();
        hierarchy.set_individual_runtime("conv1", 10_000);

        // Try to add self as child
        hierarchy.add_child("conv1", "conv1");

        assert!(!hierarchy.has_children("conv1"));
        assert_eq!(hierarchy.get_total_runtime("conv1"), 10_000);
    }

    #[test]
    fn test_get_ancestors() {
        let mut hierarchy = RuntimeHierarchy::new();
        hierarchy.add_child("root", "child1");
        hierarchy.add_child("child1", "child2");
        hierarchy.add_child("child2", "child3");

        let ancestors = hierarchy.get_ancestors("child3");
        assert_eq!(ancestors, vec!["child2", "child1", "root"]);

        let ancestors = hierarchy.get_ancestors("root");
        assert!(ancestors.is_empty());
    }

    #[test]
    fn test_get_descendants() {
        let mut hierarchy = RuntimeHierarchy::new();
        hierarchy.add_child("root", "child1");
        hierarchy.add_child("root", "child2");
        hierarchy.add_child("child1", "grandchild1");

        let descendants = hierarchy.get_descendants("root");
        assert_eq!(descendants.len(), 3);
        assert!(descendants.contains(&"child1".to_string()));
        assert!(descendants.contains(&"child2".to_string()));
        assert!(descendants.contains(&"grandchild1".to_string()));
    }

    // ===== Re-parenting and Edge Case Tests =====

    #[test]
    fn test_reparenting_removes_from_old_parent() {
        // Test that when a child is re-parented, the old parent no longer includes
        // the child in its runtime calculations
        let mut hierarchy = RuntimeHierarchy::new();

        // Initial setup: parent1 -> child
        hierarchy.set_individual_runtime("parent1", 10_000);
        hierarchy.set_individual_runtime("parent2", 20_000);
        hierarchy.set_individual_runtime("child", 30_000);

        hierarchy.set_parent("child", "parent1");

        // Verify initial state
        assert_eq!(hierarchy.get_total_runtime("parent1"), 40_000); // 10k + 30k
        assert_eq!(hierarchy.get_total_runtime("parent2"), 20_000); // just itself
        assert!(hierarchy.has_children("parent1"));
        assert!(!hierarchy.has_children("parent2"));

        // Re-parent: child now belongs to parent2
        hierarchy.set_parent("child", "parent2");

        // Verify re-parenting worked
        assert_eq!(hierarchy.get_parent("child"), Some(&"parent2".to_string()));

        // Old parent should no longer include child's runtime
        assert_eq!(hierarchy.get_total_runtime("parent1"), 10_000); // just itself now
        assert!(!hierarchy.has_children("parent1")); // no more children

        // New parent should include child's runtime
        assert_eq!(hierarchy.get_total_runtime("parent2"), 50_000); // 20k + 30k
        assert!(hierarchy.has_children("parent2"));
    }

    #[test]
    fn test_reparenting_retains_siblings() {
        // Test that when one child is re-parented, siblings remain with the old parent
        // Scenario: parent1 has [child1, child2], reparent child1 to parent2
        // Expected: parent1 keeps child2, parent2 gains child1
        let mut hierarchy = RuntimeHierarchy::new();

        hierarchy.set_individual_runtime("parent1", 10_000);
        hierarchy.set_individual_runtime("parent2", 20_000);
        hierarchy.set_individual_runtime("child1", 30_000);
        hierarchy.set_individual_runtime("child2", 40_000);

        // Initial: parent1 -> [child1, child2]
        hierarchy.set_parent("child1", "parent1");
        hierarchy.set_parent("child2", "parent1");

        // Verify initial state
        assert_eq!(hierarchy.get_total_runtime("parent1"), 80_000); // 10k + 30k + 40k
        assert_eq!(hierarchy.get_total_runtime("parent2"), 20_000); // just itself
        assert_eq!(hierarchy.get_children("parent1").unwrap().len(), 2);
        assert!(hierarchy.get_children("parent1").unwrap().contains("child1"));
        assert!(hierarchy.get_children("parent1").unwrap().contains("child2"));

        // Re-parent child1 to parent2
        hierarchy.set_parent("child1", "parent2");

        // Verify: parent1 retains child2
        assert_eq!(hierarchy.get_total_runtime("parent1"), 50_000); // 10k + 40k (child2 only)
        assert!(hierarchy.has_children("parent1")); // still has children
        assert_eq!(hierarchy.get_children("parent1").unwrap().len(), 1);
        assert!(hierarchy.get_children("parent1").unwrap().contains("child2"));
        assert!(!hierarchy.get_children("parent1").unwrap().contains("child1")); // child1 gone

        // Verify: parent2 gained child1
        assert_eq!(hierarchy.get_total_runtime("parent2"), 50_000); // 20k + 30k
        assert!(hierarchy.has_children("parent2"));
        assert_eq!(hierarchy.get_children("parent2").unwrap().len(), 1);
        assert!(hierarchy.get_children("parent2").unwrap().contains("child1"));

        // Verify child1 now points to parent2
        assert_eq!(hierarchy.get_parent("child1"), Some(&"parent2".to_string()));
        // Verify child2 still points to parent1
        assert_eq!(hierarchy.get_parent("child2"), Some(&"parent1".to_string()));
    }

    #[test]
    fn test_reparenting_chain() {
        // Test re-parenting with a chain: grandparent -> parent -> child
        // Then move child to grandparent directly
        let mut hierarchy = RuntimeHierarchy::new();

        hierarchy.set_individual_runtime("grandparent", 10_000);
        hierarchy.set_individual_runtime("parent", 20_000);
        hierarchy.set_individual_runtime("child", 30_000);

        // Initial chain: grandparent -> parent -> child
        hierarchy.set_parent("parent", "grandparent");
        hierarchy.set_parent("child", "parent");

        // grandparent total = 10k + (20k + 30k) = 60k
        assert_eq!(hierarchy.get_total_runtime("grandparent"), 60_000);
        // parent total = 20k + 30k = 50k
        assert_eq!(hierarchy.get_total_runtime("parent"), 50_000);

        // Re-parent child directly to grandparent
        hierarchy.set_parent("child", "grandparent");

        // Now: grandparent -> parent, grandparent -> child (siblings)
        // parent total = 20k (no children)
        assert_eq!(hierarchy.get_total_runtime("parent"), 20_000);
        assert!(!hierarchy.has_children("parent"));

        // grandparent total = 10k + 20k + 30k = 60k (same total, different structure)
        assert_eq!(hierarchy.get_total_runtime("grandparent"), 60_000);
        assert!(hierarchy.has_children("grandparent"));
        assert_eq!(hierarchy.get_children("grandparent").unwrap().len(), 2);
    }

    #[test]
    fn test_set_parent_same_parent_is_noop() {
        // Setting the same parent twice should not cause issues
        let mut hierarchy = RuntimeHierarchy::new();

        hierarchy.set_individual_runtime("parent", 10_000);
        hierarchy.set_individual_runtime("child", 20_000);

        hierarchy.set_parent("child", "parent");

        // Call set_parent again with same parent
        hierarchy.set_parent("child", "parent");

        // Should still work correctly
        assert_eq!(hierarchy.get_parent("child"), Some(&"parent".to_string()));
        assert_eq!(hierarchy.get_total_runtime("parent"), 30_000);
        assert!(hierarchy.has_children("parent"));
        // Children set should have exactly 1 child (no duplicates)
        assert_eq!(hierarchy.get_children("parent").unwrap().len(), 1);
    }

    #[test]
    fn test_add_child_uses_set_parent() {
        // Test that add_child properly delegates to set_parent
        // This verifies the refactoring is correct
        let mut hierarchy = RuntimeHierarchy::new();

        hierarchy.set_individual_runtime("parent1", 10_000);
        hierarchy.set_individual_runtime("parent2", 20_000);
        hierarchy.set_individual_runtime("child", 30_000);

        // Use add_child to establish relationship
        hierarchy.add_child("parent1", "child");

        assert_eq!(hierarchy.get_parent("child"), Some(&"parent1".to_string()));
        assert_eq!(hierarchy.get_total_runtime("parent1"), 40_000);

        // Use add_child again to re-parent (should work because it uses set_parent)
        hierarchy.add_child("parent2", "child");

        assert_eq!(hierarchy.get_parent("child"), Some(&"parent2".to_string()));
        assert_eq!(hierarchy.get_total_runtime("parent1"), 10_000); // old parent lost child
        assert_eq!(hierarchy.get_total_runtime("parent2"), 50_000); // new parent gained child
    }

    #[test]
    fn test_conflicting_tags_last_writer_wins() {
        // When both q-tag and delegation tag set parent for same child,
        // the last call wins (this is the expected behavior per set_parent semantics)
        let mut hierarchy = RuntimeHierarchy::new();

        hierarchy.set_individual_runtime("parent_a", 10_000);
        hierarchy.set_individual_runtime("parent_b", 20_000);
        hierarchy.set_individual_runtime("child", 30_000);

        // First: q-tag from parent_a points to child
        hierarchy.add_child("parent_a", "child");
        assert_eq!(hierarchy.get_parent("child"), Some(&"parent_a".to_string()));

        // Then: delegation tag on child points to parent_b
        hierarchy.set_parent("child", "parent_b");
        assert_eq!(hierarchy.get_parent("child"), Some(&"parent_b".to_string()));

        // parent_a should no longer include child
        assert_eq!(hierarchy.get_total_runtime("parent_a"), 10_000);
        // parent_b should now include child
        assert_eq!(hierarchy.get_total_runtime("parent_b"), 50_000);
    }

    #[test]
    fn test_incremental_update_preserves_existing_relationships() {
        // Test that adding a new child doesn't affect existing children
        let mut hierarchy = RuntimeHierarchy::new();

        hierarchy.set_individual_runtime("parent", 10_000);
        hierarchy.set_individual_runtime("child1", 20_000);
        hierarchy.set_individual_runtime("child2", 30_000);
        hierarchy.set_individual_runtime("child3", 40_000);

        // Add children incrementally
        hierarchy.set_parent("child1", "parent");
        assert_eq!(hierarchy.get_total_runtime("parent"), 30_000);

        hierarchy.set_parent("child2", "parent");
        assert_eq!(hierarchy.get_total_runtime("parent"), 60_000);

        hierarchy.set_parent("child3", "parent");
        assert_eq!(hierarchy.get_total_runtime("parent"), 100_000);

        // Verify all children are tracked
        let children = hierarchy.get_children("parent").unwrap();
        assert_eq!(children.len(), 3);
        assert!(children.contains("child1"));
        assert!(children.contains("child2"));
        assert!(children.contains("child3"));
    }

    #[test]
    fn test_relationship_count_after_reparenting() {
        // Verify relationship_count is accurate after re-parenting
        let mut hierarchy = RuntimeHierarchy::new();

        hierarchy.set_parent("child1", "parent1");
        hierarchy.set_parent("child2", "parent2");
        assert_eq!(hierarchy.relationship_count(), 2);

        // Re-parent child1 to parent2
        hierarchy.set_parent("child1", "parent2");
        // Still 2 relationships (child1->parent2, child2->parent2)
        assert_eq!(hierarchy.relationship_count(), 2);
    }

    #[test]
    fn test_total_unique_runtime() {
        // Test that get_total_unique_runtime sums all individual runtimes exactly once
        // regardless of hierarchy relationships
        let mut hierarchy = RuntimeHierarchy::new();

        // Use a timestamp after the RUNTIME_CUTOFF_TIMESTAMP
        let valid_timestamp = RUNTIME_CUTOFF_TIMESTAMP + 86400; // One day after cutoff

        // Setup: parent -> [child1, child2], child1 -> grandchild
        hierarchy.set_individual_runtime("parent", 10_000);
        hierarchy.set_conversation_created_at("parent", valid_timestamp);
        hierarchy.set_individual_runtime("child1", 20_000);
        hierarchy.set_conversation_created_at("child1", valid_timestamp);
        hierarchy.set_individual_runtime("child2", 30_000);
        hierarchy.set_conversation_created_at("child2", valid_timestamp);
        hierarchy.set_individual_runtime("grandchild", 40_000);
        hierarchy.set_conversation_created_at("grandchild", valid_timestamp);

        hierarchy.set_parent("child1", "parent");
        hierarchy.set_parent("child2", "parent");
        hierarchy.set_parent("grandchild", "child1");

        // Hierarchical runtime for parent = 10k + 20k + 30k + 40k = 100k (correct for that conversation)
        assert_eq!(hierarchy.get_total_runtime("parent"), 100_000);

        // But total unique runtime = sum of all individual runtimes = 100k
        // (same value, but conceptually each conversation counted once)
        assert_eq!(hierarchy.get_total_unique_runtime(), 100_000);

        // Now add an unrelated conversation
        hierarchy.set_individual_runtime("unrelated", 50_000);
        hierarchy.set_conversation_created_at("unrelated", valid_timestamp);

        // Parent's hierarchical runtime unchanged
        assert_eq!(hierarchy.get_total_runtime("parent"), 100_000);

        // But total unique runtime now includes unrelated = 150k
        assert_eq!(hierarchy.get_total_unique_runtime(), 150_000);
    }

    // ===== Cache Correctness Tests =====

    #[test]
    fn test_update_existing_runtime_updates_cache_correctly() {
        // Test that updating an existing conversation's runtime correctly updates the cache
        let mut hierarchy = RuntimeHierarchy::new();

        // Use a timestamp after the RUNTIME_CUTOFF_TIMESTAMP
        let valid_timestamp = RUNTIME_CUTOFF_TIMESTAMP + 86400; // One day after cutoff

        // Initial setup
        hierarchy.set_individual_runtime("conv1", 10_000);
        hierarchy.set_conversation_created_at("conv1", valid_timestamp);
        hierarchy.set_individual_runtime("conv2", 20_000);
        hierarchy.set_conversation_created_at("conv2", valid_timestamp);

        // Verify initial total
        assert_eq!(hierarchy.get_total_unique_runtime(), 30_000);

        // Update conv1's runtime (simulating more messages with llm-runtime)
        hierarchy.set_individual_runtime("conv1", 15_000);

        // Cache should be updated: 15k + 20k = 35k
        assert_eq!(hierarchy.get_total_unique_runtime(), 35_000);

        // Update again with larger value
        hierarchy.set_individual_runtime("conv1", 50_000);
        assert_eq!(hierarchy.get_total_unique_runtime(), 70_000);

        // Update conv2 as well
        hierarchy.set_individual_runtime("conv2", 30_000);
        assert_eq!(hierarchy.get_total_unique_runtime(), 80_000);
    }

    #[test]
    fn test_update_runtime_to_zero() {
        // Test that setting runtime to 0 correctly updates the cache
        let mut hierarchy = RuntimeHierarchy::new();

        // Use a timestamp after the RUNTIME_CUTOFF_TIMESTAMP
        let valid_timestamp = RUNTIME_CUTOFF_TIMESTAMP + 86400; // One day after cutoff

        hierarchy.set_individual_runtime("conv1", 10_000);
        hierarchy.set_conversation_created_at("conv1", valid_timestamp);
        hierarchy.set_individual_runtime("conv2", 20_000);
        hierarchy.set_conversation_created_at("conv2", valid_timestamp);
        assert_eq!(hierarchy.get_total_unique_runtime(), 30_000);

        // Set conv1 to 0
        hierarchy.set_individual_runtime("conv1", 0);
        assert_eq!(hierarchy.get_total_unique_runtime(), 20_000);

        // Set conv2 to 0 as well
        hierarchy.set_individual_runtime("conv2", 0);
        assert_eq!(hierarchy.get_total_unique_runtime(), 0);
    }

    #[test]
    fn test_clear_resets_all_caches() {
        // Test that clear() properly resets all data including caches
        let mut hierarchy = RuntimeHierarchy::new();

        // Use a timestamp after the RUNTIME_CUTOFF_TIMESTAMP
        let valid_timestamp = RUNTIME_CUTOFF_TIMESTAMP + 86400; // One day after cutoff

        // Setup some data
        hierarchy.set_individual_runtime("conv1", 10_000);
        hierarchy.set_individual_runtime("conv2", 20_000);
        hierarchy.set_conversation_created_at("conv1", valid_timestamp);
        hierarchy.set_conversation_created_at("conv2", valid_timestamp);
        hierarchy.set_parent("conv2", "conv1");

        // Verify data exists
        assert_eq!(hierarchy.get_total_unique_runtime(), 30_000);
        assert_eq!(hierarchy.conversation_count(), 2);
        assert_eq!(hierarchy.relationship_count(), 1);
        assert!(hierarchy.get_conversation_created_at("conv1").is_some());

        // Clear everything
        hierarchy.clear();

        // Verify all data is cleared
        assert_eq!(hierarchy.get_total_unique_runtime(), 0);
        assert_eq!(hierarchy.conversation_count(), 0);
        assert_eq!(hierarchy.relationship_count(), 0);
        assert!(hierarchy.get_conversation_created_at("conv1").is_none());
        assert!(hierarchy.get_individual_runtime("conv1") == 0);
        assert!(hierarchy.get_parent("conv2").is_none());
    }

    #[test]
    fn test_cache_consistency_under_multiple_updates() {
        // Stress test: many updates should keep cache consistent
        let mut hierarchy = RuntimeHierarchy::new();

        // Use a timestamp after the RUNTIME_CUTOFF_TIMESTAMP
        let valid_timestamp = RUNTIME_CUTOFF_TIMESTAMP + 86400; // One day after cutoff

        // Add many conversations
        for i in 0..100 {
            hierarchy.set_individual_runtime(&format!("conv{}", i), 1000);
            hierarchy.set_conversation_created_at(&format!("conv{}", i), valid_timestamp);
        }
        assert_eq!(hierarchy.get_total_unique_runtime(), 100_000);

        // Update half of them
        for i in 0..50 {
            hierarchy.set_individual_runtime(&format!("conv{}", i), 2000);
        }
        // 50 * 2000 + 50 * 1000 = 150_000
        assert_eq!(hierarchy.get_total_unique_runtime(), 150_000);

        // Set some to zero
        for i in 0..25 {
            hierarchy.set_individual_runtime(&format!("conv{}", i), 0);
        }
        // 25 * 0 + 25 * 2000 + 50 * 1000 = 100_000
        assert_eq!(hierarchy.get_total_unique_runtime(), 100_000);
    }

    // ===== Today-Only Filtering Tests =====

    #[test]
    fn test_today_unique_runtime_filters_by_creation_date() {
        let mut hierarchy = RuntimeHierarchy::new();

        // Get current time
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let seconds_per_day: u64 = 86400;
        let today_start = (now / seconds_per_day) * seconds_per_day;
        let yesterday = today_start - seconds_per_day;

        // Create conversations with different creation dates
        hierarchy.set_individual_runtime("today_conv1", 10_000);
        hierarchy.set_conversation_created_at("today_conv1", now);

        hierarchy.set_individual_runtime("today_conv2", 20_000);
        hierarchy.set_conversation_created_at("today_conv2", today_start + 100);

        hierarchy.set_individual_runtime("yesterday_conv", 50_000);
        hierarchy.set_conversation_created_at("yesterday_conv", yesterday);

        hierarchy.set_individual_runtime("old_conv", 100_000);
        hierarchy.set_conversation_created_at("old_conv", yesterday - seconds_per_day * 30);

        // Total unique runtime should include all
        assert_eq!(hierarchy.get_total_unique_runtime(), 180_000);

        // Today's unique runtime should only include today's conversations
        assert_eq!(hierarchy.get_today_unique_runtime(), 30_000);
    }

    #[test]
    fn test_today_runtime_excludes_conversations_without_creation_date() {
        let mut hierarchy = RuntimeHierarchy::new();

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        // Conversation with creation date (today, after cutoff)
        hierarchy.set_individual_runtime("with_date", 10_000);
        hierarchy.set_conversation_created_at("with_date", now);

        // Conversation without creation date
        hierarchy.set_individual_runtime("without_date", 20_000);
        // No set_conversation_created_at call

        // Total only includes conversations with valid creation dates (after cutoff)
        // Conversations without creation dates are excluded (no way to verify they're after cutoff)
        assert_eq!(hierarchy.get_total_unique_runtime(), 10_000);

        // Today only includes the one with a date
        assert_eq!(hierarchy.get_today_unique_runtime(), 10_000);
    }

    #[test]
    fn test_today_runtime_cache_invalidation() {
        let mut hierarchy = RuntimeHierarchy::new();

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        // Initial conversation
        hierarchy.set_individual_runtime("conv1", 10_000);
        hierarchy.set_conversation_created_at("conv1", now);

        assert_eq!(hierarchy.get_today_unique_runtime(), 10_000);

        // Add another conversation - cache should be invalidated
        hierarchy.set_individual_runtime("conv2", 20_000);
        hierarchy.set_conversation_created_at("conv2", now);

        assert_eq!(hierarchy.get_today_unique_runtime(), 30_000);

        // Update existing runtime - cache should be invalidated
        hierarchy.set_individual_runtime("conv1", 15_000);
        assert_eq!(hierarchy.get_today_unique_runtime(), 35_000);
    }

    #[test]
    fn test_today_runtime_returns_zero_when_no_today_conversations() {
        let mut hierarchy = RuntimeHierarchy::new();

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let seconds_per_day: u64 = 86400;
        let yesterday = now - seconds_per_day;

        // Only old conversations
        hierarchy.set_individual_runtime("old1", 10_000);
        hierarchy.set_conversation_created_at("old1", yesterday);

        hierarchy.set_individual_runtime("old2", 20_000);
        hierarchy.set_conversation_created_at("old2", yesterday - seconds_per_day);

        // Total includes old conversations
        assert_eq!(hierarchy.get_total_unique_runtime(), 30_000);

        // Today should be zero
        assert_eq!(hierarchy.get_today_unique_runtime(), 0);
    }

    #[test]
    fn test_conversation_created_at_update_invalidates_cache() {
        let mut hierarchy = RuntimeHierarchy::new();

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let seconds_per_day: u64 = 86400;
        let yesterday = now - seconds_per_day;

        // Create conversation dated yesterday
        hierarchy.set_individual_runtime("conv1", 10_000);
        hierarchy.set_conversation_created_at("conv1", yesterday);

        assert_eq!(hierarchy.get_today_unique_runtime(), 0);

        // "Correct" the creation date to today - cache should invalidate
        hierarchy.set_conversation_created_at("conv1", now);

        assert_eq!(hierarchy.get_today_unique_runtime(), 10_000);
    }

    // ===== Effective Last Activity Tests =====

    #[test]
    fn test_effective_last_activity_single_conversation() {
        let mut hierarchy = RuntimeHierarchy::new();

        hierarchy.set_individual_last_activity("conv1", 100);

        assert_eq!(hierarchy.get_effective_last_activity("conv1"), 100);
        assert_eq!(hierarchy.get_effective_last_activity("unknown"), 0);
    }

    #[test]
    fn test_effective_last_activity_parent_child() {
        // Scenario: parent (last_activity=50) has child (last_activity=100)
        // Parent's effective_last_activity should be 100
        let mut hierarchy = RuntimeHierarchy::new();

        hierarchy.set_individual_last_activity("parent", 50);
        hierarchy.set_individual_last_activity("child", 100);
        hierarchy.set_parent("child", "parent");

        // Parent's effective = max(50, 100) = 100
        assert_eq!(hierarchy.get_effective_last_activity("parent"), 100);
        // Child's effective = 100 (no children)
        assert_eq!(hierarchy.get_effective_last_activity("child"), 100);
    }

    #[test]
    fn test_effective_last_activity_deep_hierarchy() {
        // Scenario: conv1 -> conv2 -> conv3
        // conv1.last_activity = 50
        // conv2.last_activity = 75
        // conv3.last_activity = 100
        // conv1.effective = 100 (from conv3)
        let mut hierarchy = RuntimeHierarchy::new();

        hierarchy.set_individual_last_activity("conv1", 50);
        hierarchy.set_individual_last_activity("conv2", 75);
        hierarchy.set_individual_last_activity("conv3", 100);

        hierarchy.set_parent("conv2", "conv1");
        hierarchy.set_parent("conv3", "conv2");

        assert_eq!(hierarchy.get_effective_last_activity("conv3"), 100);
        assert_eq!(hierarchy.get_effective_last_activity("conv2"), 100);
        assert_eq!(hierarchy.get_effective_last_activity("conv1"), 100);
    }

    #[test]
    fn test_effective_last_activity_multiple_children() {
        // Scenario: parent has child1 (80) and child2 (120)
        // Parent's effective should be 120 (max of children)
        let mut hierarchy = RuntimeHierarchy::new();

        hierarchy.set_individual_last_activity("parent", 50);
        hierarchy.set_individual_last_activity("child1", 80);
        hierarchy.set_individual_last_activity("child2", 120);

        hierarchy.set_parent("child1", "parent");
        hierarchy.set_parent("child2", "parent");

        // Parent's effective = max(50, 80, 120) = 120
        assert_eq!(hierarchy.get_effective_last_activity("parent"), 120);
    }

    #[test]
    fn test_effective_last_activity_parent_is_newest() {
        // Scenario: parent (200) has child (100)
        // Parent's effective should still be 200 (parent is newest)
        let mut hierarchy = RuntimeHierarchy::new();

        hierarchy.set_individual_last_activity("parent", 200);
        hierarchy.set_individual_last_activity("child", 100);
        hierarchy.set_parent("child", "parent");

        // Parent's effective = max(200, 100) = 200
        assert_eq!(hierarchy.get_effective_last_activity("parent"), 200);
    }

    #[test]
    fn test_effective_last_activity_cycle_detection() {
        // Create a cycle: conv1 -> conv2 -> conv1
        let mut hierarchy = RuntimeHierarchy::new();

        hierarchy.set_individual_last_activity("conv1", 100);
        hierarchy.set_individual_last_activity("conv2", 200);

        hierarchy.set_parent("conv2", "conv1");
        hierarchy.set_parent("conv1", "conv2");

        // Should not infinite loop and should return the max of all values in cycle
        // conv1's effective = max(100, 200) = 200
        let effective = hierarchy.get_effective_last_activity("conv1");
        assert_eq!(effective, 200, "cycle detection should still return max value");

        // conv2's effective = max(200, 100) = 200
        let effective2 = hierarchy.get_effective_last_activity("conv2");
        assert_eq!(effective2, 200, "cycle detection should still return max value from conv2");
    }

    #[test]
    fn test_effective_last_activity_update() {
        // Test that updating a child's last_activity affects parent's effective
        let mut hierarchy = RuntimeHierarchy::new();

        hierarchy.set_individual_last_activity("parent", 50);
        hierarchy.set_individual_last_activity("child", 100);
        hierarchy.set_parent("child", "parent");

        assert_eq!(hierarchy.get_effective_last_activity("parent"), 100);

        // Update child to newer activity
        hierarchy.set_individual_last_activity("child", 200);

        assert_eq!(hierarchy.get_effective_last_activity("parent"), 200);
    }

    #[test]
    fn test_effective_last_activity_clear() {
        let mut hierarchy = RuntimeHierarchy::new();

        hierarchy.set_individual_last_activity("conv1", 100);
        hierarchy.set_parent("child", "conv1");

        hierarchy.clear();

        assert_eq!(hierarchy.get_effective_last_activity("conv1"), 0);
        assert!(hierarchy.get_individual_last_activity("conv1").is_none());
    }

    // ===== Runtime Cutoff Filtering Tests =====

    #[test]
    fn test_get_total_runtime_filtered_excludes_pre_cutoff() {
        // Test that get_total_runtime_filtered excludes conversations created before cutoff
        let mut hierarchy = RuntimeHierarchy::new();

        let pre_cutoff = RUNTIME_CUTOFF_TIMESTAMP - 86400;  // 1 day before
        let post_cutoff = RUNTIME_CUTOFF_TIMESTAMP + 86400; // 1 day after

        // Parent created before cutoff (should be excluded)
        hierarchy.set_individual_runtime("parent", 100_000);
        hierarchy.set_conversation_created_at("parent", pre_cutoff);

        // Child created after cutoff (should be included)
        hierarchy.set_individual_runtime("child", 50_000);
        hierarchy.set_conversation_created_at("child", post_cutoff);

        hierarchy.set_parent("child", "parent");

        // Unfiltered total should include both
        assert_eq!(hierarchy.get_total_runtime("parent"), 150_000);

        // Filtered total should only include child
        assert_eq!(hierarchy.get_total_runtime_filtered("parent"), 50_000);
    }

    #[test]
    fn test_get_total_runtime_filtered_at_cutoff_boundary() {
        // Test that conversations exactly at cutoff are included
        let mut hierarchy = RuntimeHierarchy::new();

        let before_cutoff = RUNTIME_CUTOFF_TIMESTAMP - 1;
        let at_cutoff = RUNTIME_CUTOFF_TIMESTAMP;

        hierarchy.set_individual_runtime("before", 100_000);
        hierarchy.set_conversation_created_at("before", before_cutoff);

        hierarchy.set_individual_runtime("at", 50_000);
        hierarchy.set_conversation_created_at("at", at_cutoff);

        hierarchy.set_parent("at", "before");

        // Filtered should only include "at" (created exactly at cutoff)
        assert_eq!(hierarchy.get_total_runtime_filtered("before"), 50_000);
    }

    #[test]
    fn test_get_total_runtime_filtered_deep_hierarchy() {
        // Test filtering with a deep parent-child-grandchild hierarchy
        let mut hierarchy = RuntimeHierarchy::new();

        let pre_cutoff = RUNTIME_CUTOFF_TIMESTAMP - 1;
        let post_cutoff = RUNTIME_CUTOFF_TIMESTAMP + 1;

        // grandparent -> parent -> child
        hierarchy.set_individual_runtime("grandparent", 10_000);
        hierarchy.set_conversation_created_at("grandparent", pre_cutoff); // Excluded

        hierarchy.set_individual_runtime("parent", 20_000);
        hierarchy.set_conversation_created_at("parent", post_cutoff); // Included

        hierarchy.set_individual_runtime("child", 30_000);
        hierarchy.set_conversation_created_at("child", post_cutoff); // Included

        hierarchy.set_parent("parent", "grandparent");
        hierarchy.set_parent("child", "parent");

        // Unfiltered: 10k + 20k + 30k = 60k
        assert_eq!(hierarchy.get_total_runtime("grandparent"), 60_000);

        // Filtered: only parent (20k) + child (30k) = 50k
        assert_eq!(hierarchy.get_total_runtime_filtered("grandparent"), 50_000);
    }

    #[test]
    fn test_get_total_runtime_filtered_multiple_children() {
        // Test filtering with multiple children, some pre/post cutoff
        let mut hierarchy = RuntimeHierarchy::new();

        let pre_cutoff = RUNTIME_CUTOFF_TIMESTAMP - 1;
        let post_cutoff = RUNTIME_CUTOFF_TIMESTAMP + 1;

        hierarchy.set_individual_runtime("parent", 10_000);
        hierarchy.set_conversation_created_at("parent", post_cutoff);

        hierarchy.set_individual_runtime("child1", 20_000);
        hierarchy.set_conversation_created_at("child1", pre_cutoff); // Excluded

        hierarchy.set_individual_runtime("child2", 30_000);
        hierarchy.set_conversation_created_at("child2", post_cutoff); // Included

        hierarchy.set_individual_runtime("child3", 40_000);
        hierarchy.set_conversation_created_at("child3", post_cutoff); // Included

        hierarchy.set_parent("child1", "parent");
        hierarchy.set_parent("child2", "parent");
        hierarchy.set_parent("child3", "parent");

        // Unfiltered: 10k + 20k + 30k + 40k = 100k
        assert_eq!(hierarchy.get_total_runtime("parent"), 100_000);

        // Filtered: parent (10k) + child2 (30k) + child3 (40k) = 80k
        assert_eq!(hierarchy.get_total_runtime_filtered("parent"), 80_000);
    }

    #[test]
    fn test_get_total_runtime_filtered_without_created_at() {
        // Test that conversations without created_at timestamps are excluded
        let mut hierarchy = RuntimeHierarchy::new();

        let post_cutoff = RUNTIME_CUTOFF_TIMESTAMP + 1;

        // Conversation with created_at
        hierarchy.set_individual_runtime("with_date", 50_000);
        hierarchy.set_conversation_created_at("with_date", post_cutoff);

        // Conversation without created_at (not set)
        hierarchy.set_individual_runtime("without_date", 100_000);

        hierarchy.set_parent("without_date", "with_date");

        // Unfiltered: 50k + 100k = 150k
        assert_eq!(hierarchy.get_total_runtime("with_date"), 150_000);

        // Filtered: only with_date (50k) because without_date has no created_at
        assert_eq!(hierarchy.get_total_runtime_filtered("with_date"), 50_000);
    }

    #[test]
    fn test_get_top_conversations_by_runtime_filters_cutoff() {
        // Test that get_top_conversations_by_runtime correctly filters by cutoff
        let mut hierarchy = RuntimeHierarchy::new();

        let pre_cutoff = RUNTIME_CUTOFF_TIMESTAMP - 1;
        let post_cutoff = RUNTIME_CUTOFF_TIMESTAMP + 1;

        // Root 1: created before cutoff (should be excluded from ranking)
        hierarchy.set_individual_runtime("root1", 200_000);
        hierarchy.set_conversation_created_at("root1", pre_cutoff);

        // Root 2: created after cutoff
        hierarchy.set_individual_runtime("root2", 100_000);
        hierarchy.set_conversation_created_at("root2", post_cutoff);

        // Root 3: created after cutoff with child
        hierarchy.set_individual_runtime("root3", 50_000);
        hierarchy.set_conversation_created_at("root3", post_cutoff);
        hierarchy.set_individual_runtime("child3", 75_000);
        hierarchy.set_conversation_created_at("child3", post_cutoff);
        hierarchy.set_parent("child3", "root3");

        let top = hierarchy.get_top_conversations_by_runtime(10);

        // Should return 2 conversations (root1 excluded because it's before cutoff)
        assert_eq!(top.len(), 2, "Should exclude pre-cutoff conversations");

        // root3 should be first (50k + 75k = 125k)
        assert_eq!(top[0].0, "root3");
        assert_eq!(top[0].1, 125_000);

        // root2 should be second (100k)
        assert_eq!(top[1].0, "root2");
        assert_eq!(top[1].1, 100_000);
    }

    #[test]
    fn test_get_top_conversations_by_runtime_mixed_children() {
        // Test top conversations with children that span the cutoff
        let mut hierarchy = RuntimeHierarchy::new();

        let pre_cutoff = RUNTIME_CUTOFF_TIMESTAMP - 1;
        let post_cutoff = RUNTIME_CUTOFF_TIMESTAMP + 1;

        // Root created after cutoff
        hierarchy.set_individual_runtime("root", 10_000);
        hierarchy.set_conversation_created_at("root", post_cutoff);

        // Child 1: before cutoff (should be excluded)
        hierarchy.set_individual_runtime("child1", 50_000);
        hierarchy.set_conversation_created_at("child1", pre_cutoff);
        hierarchy.set_parent("child1", "root");

        // Child 2: after cutoff (should be included)
        hierarchy.set_individual_runtime("child2", 30_000);
        hierarchy.set_conversation_created_at("child2", post_cutoff);
        hierarchy.set_parent("child2", "root");

        let top = hierarchy.get_top_conversations_by_runtime(10);

        assert_eq!(top.len(), 1);
        assert_eq!(top[0].0, "root");
        // Should only count root (10k) + child2 (30k) = 40k, excluding child1
        assert_eq!(top[0].1, 40_000);
    }

    #[test]
    fn test_get_top_conversations_by_runtime_limit() {
        // Test that limit parameter works correctly
        let mut hierarchy = RuntimeHierarchy::new();

        let post_cutoff = RUNTIME_CUTOFF_TIMESTAMP + 1;

        for i in 1..=20 {
            let conv_id = format!("conv{}", i);
            hierarchy.set_individual_runtime(&conv_id, i * 1000);
            hierarchy.set_conversation_created_at(&conv_id, post_cutoff);
        }

        let top_5 = hierarchy.get_top_conversations_by_runtime(5);
        assert_eq!(top_5.len(), 5, "Should respect limit parameter");

        // Should be sorted by runtime descending
        assert_eq!(top_5[0].0, "conv20");
        assert_eq!(top_5[0].1, 20_000);
        assert_eq!(top_5[4].0, "conv16");
        assert_eq!(top_5[4].1, 16_000);
    }

    #[test]
    fn test_get_runtime_by_day_filters_cutoff() {
        // Test that get_runtime_by_day excludes data before cutoff
        let mut hierarchy = RuntimeHierarchy::new();

        // Use current time to ensure data is within the time window
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let seconds_per_day: u64 = 86400;
        let today_start = (now / seconds_per_day) * seconds_per_day;

        // Create timestamps relative to today but respecting the cutoff
        let yesterday = today_start - seconds_per_day;
        let two_days_ago = today_start - (2 * seconds_per_day);

        // Use the cutoff timestamp if it's more recent than our test data
        let pre_cutoff = RUNTIME_CUTOFF_TIMESTAMP - 86400;

        // Conversation before cutoff (should be excluded)
        hierarchy.set_individual_runtime("conv1", 100_000);
        hierarchy.set_conversation_created_at("conv1", pre_cutoff);

        // Conversation from 2 days ago (after cutoff, within window)
        hierarchy.set_individual_runtime("conv2", 50_000);
        hierarchy.set_conversation_created_at("conv2", two_days_ago.max(RUNTIME_CUTOFF_TIMESTAMP));

        // Conversation from yesterday (after cutoff, within window)
        hierarchy.set_individual_runtime("conv3", 75_000);
        hierarchy.set_conversation_created_at("conv3", yesterday.max(RUNTIME_CUTOFF_TIMESTAMP + 86400));

        let by_day = hierarchy.get_runtime_by_day(365);

        // Should have 2 days (two_days_ago and yesterday), excluding pre-cutoff
        assert_eq!(by_day.len(), 2, "Should have 2 days of runtime data");

        // Verify total runtime across both days
        let total_runtime: u64 = by_day.iter().map(|(_, runtime)| runtime).sum();
        assert_eq!(total_runtime, 125_000, "Total should be conv2 + conv3");
    }
}
