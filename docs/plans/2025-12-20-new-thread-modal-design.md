# New Thread Modal from Home View

## Overview

Add ability to create a new thread by pressing 'n' from the Home view. Shows a modal with project selection (fuzzy finder), agent selection (required), and content input (full TextEditor capabilities).

## Modal Structure

```
┌─────────────────────────────────────────┐
│ New Thread                              │
├─────────────────────────────────────────┤
│ Project: [fuzzy filter input] ▼         │
│   (dropdown with filtered projects)     │
│                                         │
│ Agent: [fuzzy filter input] ▼           │
│   (dropdown with filtered agents)       │
│                                         │
│ ┌─────────────────────────────────────┐ │
│ │ Content textarea (autofocus)        │ │
│ │ Supports attachments, images        │ │
│ └─────────────────────────────────────┘ │
├─────────────────────────────────────────┤
│ Tab: next · Enter: send · Esc: close    │
└─────────────────────────────────────────┘
```

## State

New fields in App:
- `showing_new_thread_modal: bool`
- `new_thread_modal_focus: NewThreadField` (Content, Project, Agent)
- `new_thread_project_filter: String`
- `new_thread_agent_filter: String`
- `new_thread_selected_project: Option<Project>`
- `new_thread_selected_agent: Option<ProjectAgent>`
- `new_thread_editor: TextEditor`
- `project_draft_storage: RefCell<ProjectDraftStorage>`

## Interaction

- **'n' on Home**: Open modal, load last used project, load project draft if exists, focus Content
- **Tab**: Cycle Content → Project → Agent → Content
- **Typing in Project/Agent**: Fuzzy filter the dropdown
- **Up/Down in Project/Agent**: Navigate dropdown
- **Enter in dropdown**: Select item, move to next field
- **Enter in Content (with valid selections)**: Create thread, close modal, stay on Home
- **Esc**: Save project draft, close modal

## Project Draft Storage

```rust
pub struct ProjectDraft {
    pub project_a_tag: String,
    pub text: String,
    pub selected_agent_pubkey: Option<String>,
    pub last_modified: u64,
}
```

Stored in `tenex_data/project_drafts.json`

## Last Used Project

Store `last_project_a_tag` in `tenex_data/preferences.json`

## Files to Modify

1. `src/models/mod.rs` - export new module
2. `src/models/project_draft.rs` - new: ProjectDraft, ProjectDraftStorage
3. `src/ui/app.rs` - add state fields, draft storage, preference loading
4. `src/ui/views/home.rs` - render_new_thread_modal()
5. `src/main.rs` - key handling for modal
