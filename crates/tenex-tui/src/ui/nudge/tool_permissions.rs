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
            })
            .collect()
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
    fn test_exclusive_mode() {
        let mut perms = ToolPermissions::new();
        perms.set_mode(ToolMode::Exclusive);

        perms.add_only_tool("grep".to_string());
        perms.add_only_tool("fs_read".to_string());

        assert!(perms.is_exclusive_mode());
        assert!(perms.is_only("grep"));
        assert!(perms.is_only("fs_read"));
        assert!(!perms.is_only("shell"));
        assert_eq!(perms.only_tools.len(), 2);
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
    fn test_exclusive_mode_no_conflicts() {
        let mut perms = ToolPermissions::new();
        perms.set_mode(ToolMode::Exclusive);
        perms.add_only_tool("Read".to_string());

        // No conflicts in Exclusive mode (allow/deny not used)
        let conflicts = perms.detect_conflicts();
        assert!(conflicts.is_empty());
    }

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
}
