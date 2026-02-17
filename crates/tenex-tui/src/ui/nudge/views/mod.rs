//! Nudge CRUD view layer
//!
//! Provides rendering for:
//! - NudgeListView: Browse and manage nudges
//! - NudgeCreateForm: Create new nudges (also used for copy workflow)
//! - NudgeDetailView: Read-only nudge view
//! - NudgeDeleteConfirm: Deletion confirmation

mod create_form;
mod delete_confirm;
mod detail_view;
mod list_view;

pub use create_form::render_nudge_create;
pub use delete_confirm::render_nudge_delete_confirm;
pub use detail_view::render_nudge_detail;
pub use list_view::render_nudge_list;
