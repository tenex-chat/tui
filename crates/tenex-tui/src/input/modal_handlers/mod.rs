//! Modal-specific keyboard event handlers.
//!
//! Each modal type has its own handler function, keeping the logic focused and testable.

mod actions;
mod agent;
mod ask;
mod attachments;
mod backend;
mod command_palette;
mod drafts;
mod helpers;
mod nudge;
mod search;
mod selectors;
mod view;
mod workspace;

pub(crate) use helpers::export_thread_as_jsonl;

use anyhow::Result;
use crossterm::event::KeyEvent;

use crate::ui::{App, ModalState};

/// Routes input to the appropriate modal handler.
/// Returns `true` if the input was handled by a modal, `false` otherwise.
pub(super) fn handle_modal_input(app: &mut App, key: KeyEvent) -> Result<bool> {
    // Handle command palette when open - MUST come before sidebar search
    // so that Ctrl+T + / can toggle search off when palette is opened from search
    if matches!(app.modal_state, ModalState::CommandPalette(_)) {
        command_palette::handle_command_palette_key(app, key);
        return Ok(true);
    }

    // Handle sidebar search when visible
    if app.sidebar_search.visible {
        search::handle_sidebar_search_key(app, key);
        return Ok(true);
    }

    // Handle attachment modal when open
    if app.is_attachment_modal_open() {
        attachments::handle_attachment_modal_key(app, key);
        return Ok(true);
    }

    // Handle ask modal when open
    if matches!(app.modal_state, ModalState::AskModal(_)) {
        ask::handle_ask_modal_key(app, key);
        return Ok(true);
    }

    // Handle tab modal when open
    if app.showing_tab_modal() {
        command_palette::handle_tab_modal_key(app, key);
        return Ok(true);
    }

    // Handle search modal when open
    if app.showing_search_modal {
        search::handle_search_modal_key(app, key);
        return Ok(true);
    }

    // Handle in-conversation search when active (per-tab isolated)
    if app.is_chat_search_active() {
        search::handle_chat_search_key(app, key);
        return Ok(true);
    }

    // Handle unified agent configuration modal when open
    if matches!(app.modal_state, ModalState::AgentConfig(_)) {
        agent::handle_agent_config_modal_key(app, key);
        return Ok(true);
    }

    // Handle projects modal when open (for new thread or switching projects)
    if matches!(app.modal_state, ModalState::ProjectsModal { .. }) {
        selectors::handle_projects_modal_key(app, key)?;
        return Ok(true);
    }

    // Handle view raw event modal when open
    if matches!(app.modal_state, ModalState::ViewRawEvent { .. }) {
        view::handle_view_raw_event_modal_key(app, key);
        return Ok(true);
    }

    // Handle hotkey help modal when open
    if matches!(app.modal_state, ModalState::HotkeyHelp) {
        view::handle_hotkey_help_modal_key(app, key);
        return Ok(true);
    }

    // Handle unified nudge/skill selector modal when open
    if matches!(app.modal_state, ModalState::NudgeSkillSelector(_)) {
        selectors::handle_nudge_skill_selector_key(app, key);
        return Ok(true);
    }

    // Handle composer project selector modal when open (for changing project in new conversations)
    if matches!(app.modal_state, ModalState::ComposerProjectSelector { .. }) {
        selectors::handle_composer_project_selector_key(app, key)?;
        return Ok(true);
    }

    // Handle create agent modal when open (global, works in any view)
    if matches!(app.modal_state, ModalState::CreateAgent(_)) {
        agent::handle_create_agent_key(app, key);
        return Ok(true);
    }

    // Handle project actions modal when open
    if matches!(app.modal_state, ModalState::ProjectActions(_)) {
        actions::handle_project_actions_modal_key(app, key);
        return Ok(true);
    }

    // Handle report viewer modal when open
    if matches!(app.modal_state, ModalState::ReportViewer(_)) {
        view::handle_report_viewer_modal_key(app, key);
        return Ok(true);
    }

    // Handle conversation actions modal when open
    if matches!(app.modal_state, ModalState::ConversationActions(_)) {
        actions::handle_conversation_actions_modal_key(app, key);
        return Ok(true);
    }

    // Handle chat actions modal when open (via Ctrl+T command palette)
    if matches!(app.modal_state, ModalState::ChatActions(_)) {
        actions::handle_chat_actions_modal_key(app, key);
        return Ok(true);
    }

    // Handle expanded editor modal when open
    if matches!(app.modal_state, ModalState::ExpandedEditor { .. }) {
        attachments::handle_expanded_editor_key(app, key);
        return Ok(true);
    }

    // Handle draft navigator modal when open
    if matches!(app.modal_state, ModalState::DraftNavigator(_)) {
        drafts::handle_draft_navigator_key(app, key);
        return Ok(true);
    }

    // Handle backend approval modal when open
    if matches!(app.modal_state, ModalState::BackendApproval(_)) {
        backend::handle_backend_approval_modal_key(app, key);
        return Ok(true);
    }

    // Handle debug stats modal when open
    if matches!(app.modal_state, ModalState::DebugStats(_)) {
        view::handle_debug_stats_modal_key(app, key);
        return Ok(true);
    }

    // Handle history search modal when open
    if matches!(app.modal_state, ModalState::HistorySearch(_)) {
        search::handle_history_search_key(app, key);
        return Ok(true);
    }

    // Handle nudge list modal when open
    if matches!(app.modal_state, ModalState::NudgeList(_)) {
        nudge::handle_nudge_list_key(app, key);
        return Ok(true);
    }

    // Handle nudge create form when open (also handles copy/pre-populated form)
    if matches!(app.modal_state, ModalState::NudgeCreate(_)) {
        nudge::handle_nudge_form_key(app, key);
        return Ok(true);
    }

    // Handle nudge detail view when open
    if matches!(app.modal_state, ModalState::NudgeDetail(_)) {
        nudge::handle_nudge_detail_key(app, key);
        return Ok(true);
    }

    // Handle nudge delete confirmation when open
    if matches!(app.modal_state, ModalState::NudgeDeleteConfirm(_)) {
        nudge::handle_nudge_delete_confirm_key(app, key);
        return Ok(true);
    }

    // Handle workspace manager modal when open
    if matches!(app.modal_state, ModalState::WorkspaceManager(_)) {
        workspace::handle_workspace_manager_key(app, key);
        return Ok(true);
    }

    // Handle app settings modal when open
    if matches!(app.modal_state, ModalState::AppSettings(_)) {
        workspace::handle_app_settings_key(app, key);
        return Ok(true);
    }

    Ok(false)
}
