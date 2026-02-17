//! Nudge CRUD module - comprehensive interface for kind:4201 events
//!
//! This module provides:
//! - NudgeFormState: State for nudge creation forms
//! - Tool permission management (allow-tool, deny-tool tags)
//! - Dynamic tool registry from kind:24010 events
//! - Validation and conflict detection

pub mod form_state;
pub mod tool_permissions;
pub mod validation;
pub mod views;

pub use form_state::{NudgeFormFocus, NudgeFormState, NudgeFormStep, PermissionMode};
pub use tool_permissions::{get_available_tools_from_statuses, ToolMode, ToolPermissions};
