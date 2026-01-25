//! Nudge CRUD view layer
//!
//! Provides rendering for:
//! - NudgeListView: Browse and manage nudges
//! - NudgeCreateForm: Create new nudges
//! - NudgeEditForm: Edit existing nudges
//! - NudgeDetailView: Read-only nudge view
//! - NudgeDeleteConfirm: Deletion confirmation

mod list_view;
mod create_form;
mod edit_form;
mod detail_view;
mod delete_confirm;

pub use list_view::render_nudge_list;
pub use create_form::render_nudge_create;
pub use edit_form::render_nudge_edit;
pub use detail_view::render_nudge_detail;
pub use delete_confirm::render_nudge_delete_confirm;
