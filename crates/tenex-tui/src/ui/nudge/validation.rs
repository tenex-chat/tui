//! Nudge validation logic
//!
//! Validates nudge content, title, and tool permissions
//!
//! Supports two mutually exclusive permission modes:
//! - Additive: allow-tool + deny-tool (modifies agent's defaults)
//! - Exclusive: only-tool (complete override)

use super::tool_permissions::ToolMode;
use super::ToolPermissions;

/// Validation errors for nudge data
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NudgeValidationError {
    /// Title is empty or whitespace only
    EmptyTitle,
    /// Title exceeds maximum length
    TitleTooLong { max: usize, actual: usize },
    /// Content is empty
    EmptyContent,
    /// Content exceeds maximum length
    ContentTooLong { max: usize, actual: usize },
    /// Hashtag is invalid (empty, starts with #, etc.)
    InvalidHashtag(String),
    /// Tool permission conflict detected (Additive mode only)
    ToolConflict { tool: String },
    /// Tool name not found in registry
    UnknownTool { tool: String },
    /// Mixed mode: both only-tool and allow/deny-tool present
    MixedModes,
    /// Exclusive mode with no tools (warning, not error)
    EmptyExclusiveTools,
}

impl std::fmt::Display for NudgeValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NudgeValidationError::EmptyTitle => write!(f, "Title cannot be empty"),
            NudgeValidationError::TitleTooLong { max, actual } => {
                write!(f, "Title too long ({} chars, max {})", actual, max)
            }
            NudgeValidationError::EmptyContent => write!(f, "Content cannot be empty"),
            NudgeValidationError::ContentTooLong { max, actual } => {
                write!(f, "Content too long ({} chars, max {})", actual, max)
            }
            NudgeValidationError::InvalidHashtag(tag) => {
                write!(f, "Invalid hashtag: '{}'", tag)
            }
            NudgeValidationError::ToolConflict { tool } => {
                write!(f, "Tool '{}' is both allowed and denied", tool)
            }
            NudgeValidationError::UnknownTool { tool } => {
                write!(f, "Unknown tool: '{}'", tool)
            }
            NudgeValidationError::MixedModes => {
                write!(
                    f,
                    "Cannot mix 'only-tool' with 'allow-tool'/'deny-tool' - choose one mode"
                )
            }
            NudgeValidationError::EmptyExclusiveTools => {
                write!(
                    f,
                    "Exclusive mode with no tools - agent will have no tools!"
                )
            }
        }
    }
}

/// Nudge validation configuration and results
pub struct NudgeValidation {
    /// Maximum title length (0 = no limit)
    pub max_title_length: usize,
    /// Maximum content length (0 = no limit)
    pub max_content_length: usize,
    /// Whether to validate tool names against registry
    pub validate_tool_names: bool,
}

impl Default for NudgeValidation {
    fn default() -> Self {
        Self {
            max_title_length: 100,
            max_content_length: 0,      // No limit by default
            validate_tool_names: false, // Don't require tools to be in registry
        }
    }
}

impl NudgeValidation {
    pub fn new() -> Self {
        Self::default()
    }

    /// Validate a title
    pub fn validate_title(&self, title: &str) -> Result<(), NudgeValidationError> {
        let trimmed = title.trim();

        if trimmed.is_empty() {
            return Err(NudgeValidationError::EmptyTitle);
        }

        if self.max_title_length > 0 && trimmed.len() > self.max_title_length {
            return Err(NudgeValidationError::TitleTooLong {
                max: self.max_title_length,
                actual: trimmed.len(),
            });
        }

        Ok(())
    }

    /// Validate content
    pub fn validate_content(&self, content: &str) -> Result<(), NudgeValidationError> {
        let trimmed = content.trim();

        if trimmed.is_empty() {
            return Err(NudgeValidationError::EmptyContent);
        }

        if self.max_content_length > 0 && trimmed.len() > self.max_content_length {
            return Err(NudgeValidationError::ContentTooLong {
                max: self.max_content_length,
                actual: trimmed.len(),
            });
        }

        Ok(())
    }

    /// Validate a hashtag
    pub fn validate_hashtag(&self, tag: &str) -> Result<(), NudgeValidationError> {
        let trimmed = tag.trim();

        if trimmed.is_empty() {
            return Err(NudgeValidationError::InvalidHashtag("empty".to_string()));
        }

        // Don't allow # prefix (it's added automatically)
        if trimmed.starts_with('#') {
            return Err(NudgeValidationError::InvalidHashtag(
                "should not start with #".to_string(),
            ));
        }

        // Don't allow spaces
        if trimmed.contains(' ') {
            return Err(NudgeValidationError::InvalidHashtag(
                "cannot contain spaces".to_string(),
            ));
        }

        Ok(())
    }

    /// Validate tool permissions
    pub fn validate_permissions(
        &self,
        permissions: &ToolPermissions,
        available_tools: Option<&[String]>,
    ) -> Vec<NudgeValidationError> {
        let mut errors = Vec::new();

        // Check for mixed modes (should be prevented by UI, but validate anyway)
        let has_additive =
            !permissions.allow_tools.is_empty() || !permissions.deny_tools.is_empty();
        let has_exclusive = !permissions.only_tools.is_empty();

        if has_additive && has_exclusive {
            errors.push(NudgeValidationError::MixedModes);
            return errors; // Don't continue validation with invalid state
        }

        // Mode-specific validation
        match permissions.mode {
            ToolMode::Additive => {
                // Check for conflicts (tool in both allow and deny)
                for conflict in permissions.detect_conflicts() {
                    errors.push(NudgeValidationError::ToolConflict {
                        tool: conflict.tool_name,
                    });
                }

                // Optionally validate tool names against registry
                if self.validate_tool_names {
                    if let Some(tools) = available_tools {
                        for tool in &permissions.allow_tools {
                            if !tools.contains(tool) {
                                errors
                                    .push(NudgeValidationError::UnknownTool { tool: tool.clone() });
                            }
                        }
                        for tool in &permissions.deny_tools {
                            if !tools.contains(tool) {
                                errors
                                    .push(NudgeValidationError::UnknownTool { tool: tool.clone() });
                            }
                        }
                    }
                }
            }
            ToolMode::Exclusive => {
                // Error if no tools specified in exclusive mode (agent will have NO tools)
                if permissions.only_tools.is_empty() {
                    errors.push(NudgeValidationError::EmptyExclusiveTools);
                }

                // Optionally validate tool names against registry
                if self.validate_tool_names {
                    if let Some(tools) = available_tools {
                        for tool in &permissions.only_tools {
                            if !tools.contains(tool) {
                                errors
                                    .push(NudgeValidationError::UnknownTool { tool: tool.clone() });
                            }
                        }
                    }
                }
            }
        }

        errors
    }

    /// Validate all nudge data
    pub fn validate_all(
        &self,
        title: &str,
        content: &str,
        hashtags: &[String],
        permissions: &ToolPermissions,
        available_tools: Option<&[String]>,
    ) -> Vec<NudgeValidationError> {
        let mut errors = Vec::new();

        if let Err(e) = self.validate_title(title) {
            errors.push(e);
        }

        if let Err(e) = self.validate_content(content) {
            errors.push(e);
        }

        for tag in hashtags {
            if let Err(e) = self.validate_hashtag(tag) {
                errors.push(e);
            }
        }

        errors.extend(self.validate_permissions(permissions, available_tools));

        errors
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_title() {
        let v = NudgeValidation::new();

        assert!(v.validate_title("My Nudge").is_ok());
        assert!(v.validate_title("").is_err());
        assert!(v.validate_title("   ").is_err());

        let long_title = "a".repeat(150);
        assert!(v.validate_title(&long_title).is_err());
    }

    #[test]
    fn test_validate_hashtag() {
        let v = NudgeValidation::new();

        assert!(v.validate_hashtag("coding").is_ok());
        assert!(v.validate_hashtag("#coding").is_err());
        assert!(v.validate_hashtag("my tag").is_err());
        assert!(v.validate_hashtag("").is_err());
    }

    #[test]
    fn test_validate_permissions_conflict() {
        let v = NudgeValidation::new();
        let mut perms = ToolPermissions::new();
        perms.add_allow_tool("Read".to_string());
        perms.add_deny_tool("Read".to_string());

        let errors = v.validate_permissions(&perms, None);
        assert_eq!(errors.len(), 1);
        assert!(matches!(
            errors[0],
            NudgeValidationError::ToolConflict { .. }
        ));
    }

    #[test]
    fn test_validate_exclusive_mode() {
        let v = NudgeValidation::new();
        let mut perms = ToolPermissions::new();
        perms.set_mode(ToolMode::Exclusive);
        perms.add_only_tool("grep".to_string());
        perms.add_only_tool("fs_read".to_string());

        let errors = v.validate_permissions(&perms, None);
        assert!(errors.is_empty()); // Valid exclusive mode
    }

    #[test]
    fn test_validate_mixed_modes_error() {
        let v = NudgeValidation::new();
        let mut perms = ToolPermissions::new();
        // Manually set invalid state (UI should prevent this)
        perms.allow_tools.push("Read".to_string());
        perms.only_tools.push("grep".to_string());

        let errors = v.validate_permissions(&perms, None);
        assert_eq!(errors.len(), 1);
        assert!(matches!(errors[0], NudgeValidationError::MixedModes));
    }

    #[test]
    fn test_validate_empty_exclusive_tools_error() {
        let v = NudgeValidation::new();
        let mut perms = ToolPermissions::new();
        perms.set_mode(ToolMode::Exclusive);
        // No tools added - should error

        let errors = v.validate_permissions(&perms, None);
        assert_eq!(errors.len(), 1);
        assert!(matches!(
            errors[0],
            NudgeValidationError::EmptyExclusiveTools
        ));
    }

    #[test]
    fn test_validate_additive_mode_no_error_when_empty() {
        let v = NudgeValidation::new();
        let perms = ToolPermissions::new(); // Additive mode by default, no tools

        let errors = v.validate_permissions(&perms, None);
        assert!(errors.is_empty()); // Additive mode allows no tools
    }

    #[test]
    fn test_validation_error_display() {
        assert_eq!(
            NudgeValidationError::EmptyTitle.to_string(),
            "Title cannot be empty"
        );
        assert_eq!(
            NudgeValidationError::EmptyExclusiveTools.to_string(),
            "Exclusive mode with no tools - agent will have no tools!"
        );
        assert_eq!(
            NudgeValidationError::MixedModes.to_string(),
            "Cannot mix 'only-tool' with 'allow-tool'/'deny-tool' - choose one mode"
        );
    }
}
