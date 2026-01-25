//! Nudge validation logic
//!
//! Validates nudge content, title, and tool permissions

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
    /// Tool permission conflict detected
    ToolConflict { tool: String },
    /// Tool name not found in registry
    UnknownTool { tool: String },
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
            max_content_length: 0, // No limit by default
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

        // Check for conflicts
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
                        errors.push(NudgeValidationError::UnknownTool { tool: tool.clone() });
                    }
                }
                for tool in &permissions.deny_tools {
                    if !tools.contains(tool) {
                        errors.push(NudgeValidationError::UnknownTool { tool: tool.clone() });
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
        assert!(matches!(errors[0], NudgeValidationError::ToolConflict { .. }));
    }
}
