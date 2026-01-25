//! Tool permission management for nudge events
//!
//! Handles allow-tool and deny-tool tags with:
//! - Conflict detection (tool in both allow and deny)
//! - Dynamic tool discovery from kind:24010 events (no hardcoded lists)
//! - Permission validation

use std::collections::HashSet;

/// Tool permission configuration for a nudge
#[derive(Debug, Clone, Default)]
pub struct ToolPermissions {
    /// Tools to add to agent's available tools
    pub allow_tools: Vec<String>,
    /// Tools to remove from agent's available tools
    pub deny_tools: Vec<String>,
}

impl ToolPermissions {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a tool to the allow list
    pub fn add_allow_tool(&mut self, tool: String) {
        if !self.allow_tools.contains(&tool) {
            self.allow_tools.push(tool);
        }
    }

    /// Remove a tool from the allow list
    pub fn remove_allow_tool(&mut self, tool: &str) {
        self.allow_tools.retain(|t| t != tool);
    }

    /// Add a tool to the deny list
    pub fn add_deny_tool(&mut self, tool: String) {
        if !self.deny_tools.contains(&tool) {
            self.deny_tools.push(tool);
        }
    }

    /// Remove a tool from the deny list
    pub fn remove_deny_tool(&mut self, tool: &str) {
        self.deny_tools.retain(|t| t != tool);
    }

    /// Toggle a tool in the allow list
    pub fn toggle_allow_tool(&mut self, tool: &str) {
        if self.allow_tools.contains(&tool.to_string()) {
            self.remove_allow_tool(tool);
        } else {
            self.add_allow_tool(tool.to_string());
        }
    }

    /// Toggle a tool in the deny list
    pub fn toggle_deny_tool(&mut self, tool: &str) {
        if self.deny_tools.contains(&tool.to_string()) {
            self.remove_deny_tool(tool);
        } else {
            self.add_deny_tool(tool.to_string());
        }
    }

    /// Check if a tool is in the allow list
    pub fn is_allowed(&self, tool: &str) -> bool {
        self.allow_tools.contains(&tool.to_string())
    }

    /// Check if a tool is in the deny list
    pub fn is_denied(&self, tool: &str) -> bool {
        self.deny_tools.contains(&tool.to_string())
    }

    /// Detect conflicts - tools that appear in both allow and deny lists
    pub fn detect_conflicts(&self) -> Vec<ToolPermissionConflict> {
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

    /// Get count of permissions set
    pub fn permission_count(&self) -> usize {
        self.allow_tools.len() + self.deny_tools.len()
    }

    /// Clear all permissions
    pub fn clear(&mut self) {
        self.allow_tools.clear();
        self.deny_tools.clear();
    }

    /// Build Nostr tags from permissions
    /// Returns Vec of (tag_name, tool_name) tuples
    pub fn to_tags(&self) -> Vec<(String, String)> {
        let mut tags = Vec::new();

        for tool in &self.allow_tools {
            tags.push(("allow-tool".to_string(), tool.clone()));
        }

        for tool in &self.deny_tools {
            tags.push(("deny-tool".to_string(), tool.clone()));
        }

        tags
    }

    /// Create from Nostr tags
    pub fn from_tags(tags: &[(String, String)]) -> Self {
        let mut perms = Self::new();

        for (tag_name, tool_name) in tags {
            match tag_name.as_str() {
                "allow-tool" => perms.add_allow_tool(tool_name.clone()),
                "deny-tool" => perms.add_deny_tool(tool_name.clone()),
                _ => {}
            }
        }

        perms
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
            for tool in &status.all_tools {
                all_tools.insert(tool.clone());
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
    fn test_to_from_tags() {
        let mut perms = ToolPermissions::new();
        perms.add_allow_tool("Read".to_string());
        perms.add_deny_tool("Bash".to_string());

        let tags = perms.to_tags();
        let restored = ToolPermissions::from_tags(&tags);

        assert!(restored.is_allowed("Read"));
        assert!(restored.is_denied("Bash"));
    }
}
