//! Tool permission management for nudge events
//!
//! Handles two mutually exclusive permission modes:
//!
//! **Mode 1: Additive/Subtractive (allow-tool + deny-tool)**
//! Modifies the agent's default tools:
//! - `allow-tool` adds tools the agent doesn't normally have
//! - `deny-tool` removes tools the agent normally has
//!
//! **Mode 2: Exclusive (only-tool)**
//! Completely overrides everything:
//! - Agent gets EXACTLY the specified tools, nothing else
//! - Ignores agent defaults, allow-tool, and deny-tool
//! - Highest priority - if ANY only-tool tag exists, agent gets ONLY those tools
//!
//! The UI enforces XOR: users must choose one mode, cannot mix them.

use std::collections::HashSet;

/// Tool permission mode for nudges
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToolMode {
    /// Additive/Subtractive mode: allow-tool adds, deny-tool removes from defaults
    #[default]
    Additive,
    /// Exclusive mode: only-tool specifies exact tool set, ignores everything else
    Exclusive,
}

impl ToolMode {
    pub fn label(&self) -> &'static str {
        match self {
            ToolMode::Additive => "Modify Defaults",
            ToolMode::Exclusive => "Exact Tools Only",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            ToolMode::Additive => "Add or remove tools from agent's default set",
            ToolMode::Exclusive => "Agent gets EXACTLY these tools, nothing else",
        }
    }
}

/// Tool permission configuration for a nudge
#[derive(Debug, Clone, Default)]
pub struct ToolPermissions {
    /// Current permission mode (XOR - only one mode can be active)
    pub mode: ToolMode,
    /// Tools to add to agent's available tools (Additive mode only)
    pub allow_tools: Vec<String>,
    /// Tools to remove from agent's available tools (Additive mode only)
    pub deny_tools: Vec<String>,
    /// Exclusive tool list - agent gets EXACTLY these tools (Exclusive mode only)
    pub only_tools: Vec<String>,
}

impl ToolPermissions {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the permission mode (clears tools from the inactive mode)
    pub fn set_mode(&mut self, mode: ToolMode) {
        if self.mode != mode {
            self.mode = mode;
            // Clear tools from the mode we're leaving to enforce XOR
            match mode {
                ToolMode::Additive => {
                    self.only_tools.clear();
                }
                ToolMode::Exclusive => {
                    self.allow_tools.clear();
                    self.deny_tools.clear();
                }
            }
        }
    }

    /// Add a tool to the allow list (Additive mode)
    pub fn add_allow_tool(&mut self, tool: String) {
        if !self.allow_tools.contains(&tool) {
            self.allow_tools.push(tool);
        }
    }

    /// Remove a tool from the allow list
    pub fn remove_allow_tool(&mut self, tool: &str) {
        self.allow_tools.retain(|t| t != tool);
    }

    /// Add a tool to the deny list (Additive mode)
    pub fn add_deny_tool(&mut self, tool: String) {
        if !self.deny_tools.contains(&tool) {
            self.deny_tools.push(tool);
        }
    }

    /// Remove a tool from the deny list
    pub fn remove_deny_tool(&mut self, tool: &str) {
        self.deny_tools.retain(|t| t != tool);
    }

    /// Add a tool to the only list (Exclusive mode)
    pub fn add_only_tool(&mut self, tool: String) {
        if !self.only_tools.contains(&tool) {
            self.only_tools.push(tool);
        }
    }

    /// Remove a tool from the only list
    pub fn remove_only_tool(&mut self, tool: &str) {
        self.only_tools.retain(|t| t != tool);
    }

    /// Toggle a tool in the allow list
    pub fn toggle_allow_tool(&mut self, tool: &str) {
        if self.is_allowed(tool) {
            self.remove_allow_tool(tool);
        } else {
            self.add_allow_tool(tool.to_string());
        }
    }

    /// Toggle a tool in the deny list
    pub fn toggle_deny_tool(&mut self, tool: &str) {
        if self.is_denied(tool) {
            self.remove_deny_tool(tool);
        } else {
            self.add_deny_tool(tool.to_string());
        }
    }

    /// Toggle a tool in the only list (Exclusive mode)
    pub fn toggle_only_tool(&mut self, tool: &str) {
        if self.is_only(tool) {
            self.remove_only_tool(tool);
        } else {
            self.add_only_tool(tool.to_string());
        }
    }

    /// Check if a tool is in the allow list
    pub fn is_allowed(&self, tool: &str) -> bool {
        self.allow_tools.iter().any(|t| t == tool)
    }

    /// Check if a tool is in the deny list
    pub fn is_denied(&self, tool: &str) -> bool {
        self.deny_tools.iter().any(|t| t == tool)
    }

    /// Check if a tool is in the only list
    pub fn is_only(&self, tool: &str) -> bool {
        self.only_tools.iter().any(|t| t == tool)
    }

    /// Detect conflicts - tools that appear in both allow and deny lists
    /// Only relevant in Additive mode
    pub fn detect_conflicts(&self) -> Vec<ToolPermissionConflict> {
        if self.mode == ToolMode::Exclusive {
            return Vec::new(); // No conflicts possible in Exclusive mode
        }

        let allow_set: HashSet<_> = self.allow_tools.iter().collect();
        let deny_set: HashSet<_> = self.deny_tools.iter().collect();

        allow_set
            .intersection(&deny_set)
            .map(|tool| ToolPermissionConflict {
                tool_name: (*tool).clone(),
                resolution: ConflictResolution::DenyWins,
            })
            .collect()
    }

    /// Check if there are any conflicts
    pub fn has_conflicts(&self) -> bool {
        !self.detect_conflicts().is_empty()
    }

    /// Get count of permissions set (based on current mode)
    pub fn permission_count(&self) -> usize {
        match self.mode {
            ToolMode::Additive => self.allow_tools.len() + self.deny_tools.len(),
            ToolMode::Exclusive => self.only_tools.len(),
        }
    }

    /// Clear all permissions (current mode only)
    pub fn clear(&mut self) {
        match self.mode {
            ToolMode::Additive => {
                self.allow_tools.clear();
                self.deny_tools.clear();
            }
            ToolMode::Exclusive => {
                self.only_tools.clear();
            }
        }
    }

    /// Clear all permissions in all modes
    pub fn clear_all(&mut self) {
        self.allow_tools.clear();
        self.deny_tools.clear();
        self.only_tools.clear();
    }

    /// Build Nostr tags from permissions (based on current mode)
    /// Returns Vec of (tag_name, tool_name) tuples
    pub fn to_tags(&self) -> Vec<(String, String)> {
        let mut tags = Vec::new();

        match self.mode {
            ToolMode::Additive => {
                for tool in &self.allow_tools {
                    tags.push(("allow-tool".to_string(), tool.clone()));
                }
                for tool in &self.deny_tools {
                    tags.push(("deny-tool".to_string(), tool.clone()));
                }
            }
            ToolMode::Exclusive => {
                for tool in &self.only_tools {
                    tags.push(("only-tool".to_string(), tool.clone()));
                }
            }
        }

        tags
    }

    /// Create from Nostr tags (auto-detects mode)
    pub fn from_tags(tags: &[(String, String)]) -> Self {
        let mut perms = Self::new();

        // First pass: detect if we have only-tool tags (Exclusive mode takes priority)
        let has_only_tools = tags.iter().any(|(name, _)| name == "only-tool");

        if has_only_tools {
            perms.mode = ToolMode::Exclusive;
            for (tag_name, tool_name) in tags {
                if tag_name == "only-tool" {
                    perms.add_only_tool(tool_name.clone());
                }
            }
        } else {
            perms.mode = ToolMode::Additive;
            for (tag_name, tool_name) in tags {
                match tag_name.as_str() {
                    "allow-tool" => perms.add_allow_tool(tool_name.clone()),
                    "deny-tool" => perms.add_deny_tool(tool_name.clone()),
                    _ => {}
                }
            }
        }

        perms
    }

    /// Check if in Exclusive mode
    pub fn is_exclusive_mode(&self) -> bool {
        self.mode == ToolMode::Exclusive
    }

    /// Check if in Additive mode
    pub fn is_additive_mode(&self) -> bool {
        self.mode == ToolMode::Additive
    }
}

/// A conflict between allow and deny permissions
#[derive(Debug, Clone)]
pub struct ToolPermissionConflict {
    pub tool_name: String,
    pub resolution: ConflictResolution,
}

/// How conflicts are resolved (backend behavior)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictResolution {
    /// Deny wins if a tool appears in both allow and deny
    DenyWins,
}

impl std::fmt::Display for ConflictResolution {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConflictResolution::DenyWins => write!(f, "deny wins"),
        }
    }
}

/// Tool registry that discovers available tools from kind:24010 project status events
/// This provides dynamic tool autocomplete without hardcoded tool lists
#[derive(Debug, Clone, Default)]
pub struct ToolRegistry {
    /// All unique tools discovered from project status events
    tools: Vec<String>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Update registry from project status events
    /// Called when project_statuses are updated in AppDataStore
    pub fn update_from_statuses(&mut self, statuses: &std::collections::HashMap<String, tenex_core::models::ProjectStatus>) {
        let mut all_tools: HashSet<String> = HashSet::new();

        for status in statuses.values() {
            for tool in status.all_tools() {
                all_tools.insert(tool.to_string());
            }
        }

        self.tools = all_tools.into_iter().collect();
        self.tools.sort();
    }

    /// Get all available tools
    pub fn all_tools(&self) -> &[String] {
        &self.tools
    }

    /// Filter tools by prefix (for autocomplete)
    pub fn filter_tools(&self, prefix: &str) -> Vec<&str> {
        let prefix_lower = prefix.to_lowercase();
        self.tools
            .iter()
            .filter(|t| t.to_lowercase().contains(&prefix_lower))
            .map(|s| s.as_str())
            .collect()
    }

    /// Check if a tool exists in the registry
    pub fn contains(&self, tool: &str) -> bool {
        self.tools.iter().any(|t| t == tool)
    }

    /// Get tool count
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    /// Check if registry is empty
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }
}

/// Helper function to extract sorted tools from project statuses
/// This centralizes the tool discovery logic used by nudge forms
pub fn get_available_tools_from_statuses(
    statuses: &std::collections::HashMap<String, tenex_core::models::ProjectStatus>,
) -> Vec<String> {
    let mut all_tools: HashSet<String> = HashSet::new();
    for status in statuses.values() {
        for tool in status.all_tools() {
            all_tools.insert(tool.to_string());
        }
    }
    let mut tools: Vec<String> = all_tools.into_iter().collect();
    tools.sort();
    tools
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_remove_tools() {
        let mut perms = ToolPermissions::new();

        perms.add_allow_tool("Read".to_string());
        perms.add_allow_tool("Write".to_string());
        assert!(perms.is_allowed("Read"));
        assert!(perms.is_allowed("Write"));
        assert!(!perms.is_denied("Read"));

        perms.add_deny_tool("Bash".to_string());
        assert!(perms.is_denied("Bash"));

        perms.remove_allow_tool("Read");
        assert!(!perms.is_allowed("Read"));
    }

    #[test]
    fn test_conflict_detection() {
        let mut perms = ToolPermissions::new();

        perms.add_allow_tool("Read".to_string());
        perms.add_deny_tool("Read".to_string());

        let conflicts = perms.detect_conflicts();
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].tool_name, "Read");
    }

    #[test]
    fn test_to_from_tags_additive() {
        let mut perms = ToolPermissions::new();
        perms.add_allow_tool("Read".to_string());
        perms.add_deny_tool("Bash".to_string());

        let tags = perms.to_tags();
        let restored = ToolPermissions::from_tags(&tags);

        assert!(restored.is_additive_mode());
        assert!(restored.is_allowed("Read"));
        assert!(restored.is_denied("Bash"));
    }

    #[test]
    fn test_exclusive_mode() {
        let mut perms = ToolPermissions::new();
        perms.set_mode(ToolMode::Exclusive);

        perms.add_only_tool("grep".to_string());
        perms.add_only_tool("fs_read".to_string());

        assert!(perms.is_exclusive_mode());
        assert!(perms.is_only("grep"));
        assert!(perms.is_only("fs_read"));
        assert!(!perms.is_only("shell"));
        assert_eq!(perms.permission_count(), 2);
    }

    #[test]
    fn test_mode_switching_clears_other_mode() {
        let mut perms = ToolPermissions::new();

        // Start in Additive mode with some tools
        perms.add_allow_tool("Read".to_string());
        perms.add_deny_tool("Bash".to_string());
        assert!(perms.is_additive_mode());
        assert_eq!(perms.allow_tools.len(), 1);
        assert_eq!(perms.deny_tools.len(), 1);

        // Switch to Exclusive mode - should clear additive tools
        perms.set_mode(ToolMode::Exclusive);
        assert!(perms.is_exclusive_mode());
        assert!(perms.allow_tools.is_empty());
        assert!(perms.deny_tools.is_empty());

        // Add some only tools
        perms.add_only_tool("grep".to_string());
        assert_eq!(perms.only_tools.len(), 1);

        // Switch back to Additive - should clear only tools
        perms.set_mode(ToolMode::Additive);
        assert!(perms.is_additive_mode());
        assert!(perms.only_tools.is_empty());
    }

    #[test]
    fn test_to_from_tags_exclusive() {
        let mut perms = ToolPermissions::new();
        perms.set_mode(ToolMode::Exclusive);
        perms.add_only_tool("grep".to_string());
        perms.add_only_tool("fs_read".to_string());

        let tags = perms.to_tags();
        assert_eq!(tags.len(), 2);
        assert!(tags.iter().all(|(name, _)| name == "only-tool"));

        let restored = ToolPermissions::from_tags(&tags);
        assert!(restored.is_exclusive_mode());
        assert!(restored.is_only("grep"));
        assert!(restored.is_only("fs_read"));
    }

    #[test]
    fn test_exclusive_mode_no_conflicts() {
        let mut perms = ToolPermissions::new();
        perms.set_mode(ToolMode::Exclusive);
        perms.add_only_tool("Read".to_string());

        // No conflicts in Exclusive mode (allow/deny not used)
        let conflicts = perms.detect_conflicts();
        assert!(conflicts.is_empty());
    }

    #[test]
    fn test_from_tags_exclusive_priority() {
        // If both only-tool and allow-tool tags exist, only-tool takes priority
        let tags = vec![
            ("allow-tool".to_string(), "Read".to_string()),
            ("only-tool".to_string(), "grep".to_string()),
            ("deny-tool".to_string(), "Bash".to_string()),
        ];

        let perms = ToolPermissions::from_tags(&tags);

        // Should be in Exclusive mode, only parsing only-tool tags
        assert!(perms.is_exclusive_mode());
        assert!(perms.is_only("grep"));
        assert!(perms.allow_tools.is_empty()); // Ignored
        assert!(perms.deny_tools.is_empty()); // Ignored
    }

    // =========================================================================
    // Tag emission tests (to_tags)
    // =========================================================================

    #[test]
    fn test_to_tags_exclusive_mode() {
        let mut perms = ToolPermissions::new();
        perms.set_mode(ToolMode::Exclusive);
        perms.add_only_tool("grep".to_string());
        perms.add_only_tool("fs_read".to_string());
        perms.add_only_tool("shell".to_string());

        let tags = perms.to_tags();

        // Should only emit only-tool tags
        assert_eq!(tags.len(), 3);
        for (tag_name, _) in &tags {
            assert_eq!(tag_name, "only-tool");
        }
        assert!(tags.iter().any(|(_, v)| v == "grep"));
        assert!(tags.iter().any(|(_, v)| v == "fs_read"));
        assert!(tags.iter().any(|(_, v)| v == "shell"));
    }

    #[test]
    fn test_to_tags_additive_mode() {
        let mut perms = ToolPermissions::new();
        perms.set_mode(ToolMode::Additive);
        perms.add_allow_tool("Read".to_string());
        perms.add_allow_tool("Write".to_string());
        perms.add_deny_tool("Bash".to_string());

        let tags = perms.to_tags();

        // Should emit allow-tool and deny-tool tags
        assert_eq!(tags.len(), 3);

        let allow_tags: Vec<_> = tags.iter().filter(|(n, _)| n == "allow-tool").collect();
        let deny_tags: Vec<_> = tags.iter().filter(|(n, _)| n == "deny-tool").collect();

        assert_eq!(allow_tags.len(), 2);
        assert_eq!(deny_tags.len(), 1);
        assert!(allow_tags.iter().any(|(_, v)| v == "Read"));
        assert!(allow_tags.iter().any(|(_, v)| v == "Write"));
        assert!(deny_tags.iter().any(|(_, v)| v == "Bash"));
    }

    #[test]
    fn test_to_tags_exclusive_mode_empty() {
        let mut perms = ToolPermissions::new();
        perms.set_mode(ToolMode::Exclusive);
        // No tools added

        let tags = perms.to_tags();

        // Should emit no tags
        assert!(tags.is_empty());
    }

    #[test]
    fn test_to_tags_additive_mode_empty() {
        let perms = ToolPermissions::new();
        // Additive mode by default, no tools

        let tags = perms.to_tags();

        // Should emit no tags
        assert!(tags.is_empty());
    }

    #[test]
    fn test_tag_roundtrip_exclusive() {
        let mut original = ToolPermissions::new();
        original.set_mode(ToolMode::Exclusive);
        original.add_only_tool("Tool1".to_string());
        original.add_only_tool("Tool2".to_string());

        let tags = original.to_tags();
        let restored = ToolPermissions::from_tags(&tags);

        assert!(restored.is_exclusive_mode());
        assert_eq!(restored.only_tools.len(), 2);
        assert!(restored.is_only("Tool1"));
        assert!(restored.is_only("Tool2"));
    }

    #[test]
    fn test_tag_roundtrip_additive() {
        let mut original = ToolPermissions::new();
        original.add_allow_tool("AllowMe".to_string());
        original.add_deny_tool("DenyMe".to_string());

        let tags = original.to_tags();
        let restored = ToolPermissions::from_tags(&tags);

        assert!(restored.is_additive_mode());
        assert!(restored.is_allowed("AllowMe"));
        assert!(restored.is_denied("DenyMe"));
    }

    // =========================================================================
    // is_*/toggle method tests (no allocations)
    // =========================================================================

    #[test]
    fn test_is_methods_no_false_positives() {
        let mut perms = ToolPermissions::new();
        perms.add_allow_tool("Read".to_string());
        perms.add_deny_tool("Write".to_string());

        assert!(perms.is_allowed("Read"));
        assert!(!perms.is_allowed("NotAdded"));
        assert!(!perms.is_allowed("Write")); // In deny, not allow

        assert!(perms.is_denied("Write"));
        assert!(!perms.is_denied("Read")); // In allow, not deny
        assert!(!perms.is_denied("NotAdded"));
    }

    #[test]
    fn test_toggle_methods() {
        let mut perms = ToolPermissions::new();

        // Toggle on
        perms.toggle_allow_tool("Tool1");
        assert!(perms.is_allowed("Tool1"));

        // Toggle off
        perms.toggle_allow_tool("Tool1");
        assert!(!perms.is_allowed("Tool1"));

        // Same for deny
        perms.toggle_deny_tool("Tool2");
        assert!(perms.is_denied("Tool2"));
        perms.toggle_deny_tool("Tool2");
        assert!(!perms.is_denied("Tool2"));

        // Same for only
        perms.set_mode(ToolMode::Exclusive);
        perms.toggle_only_tool("Tool3");
        assert!(perms.is_only("Tool3"));
        perms.toggle_only_tool("Tool3");
        assert!(!perms.is_only("Tool3"));
    }
}
