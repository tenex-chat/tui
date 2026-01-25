//! Nudge edit form - reuses create form with edit state
//!
//! The edit form is identical to create form, just with pre-populated data
//! and different title. We re-export the create form renderer.

pub use super::create_form::render_nudge_create as render_nudge_edit;
