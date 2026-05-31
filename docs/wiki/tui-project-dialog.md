---
title: TUI Project Dialog
slug: tui-project-dialog
summary: The TUI uses a single, unified ProjectDialog for both creating a new project and editing an existing project's settings.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-04
updated: 2026-05-04
verified: 2026-05-04
compiled-from: conversation
sources:
  - session:5d02ee05-a5a2-4860-9ccd-a235c89e7a67
---

# TUI Project Dialog

## Overview

The TUI uses a single, unified ProjectDialog for both creating a new project and editing an existing project's settings. [^5d02e-1]


## Modes

The ProjectDialog operates in a Creating mode or an Editing mode, which determines whether hitting Save creates a new project or publishes an update event. [^5d02e-2]

## Tabs

The ProjectDialog contains three tabs: Details, Agents, and MCP Servers. [^5d02e-3]

## Details Tab Fields

The Details tab includes fields for name, description, repo URL, and a private toggle. [^5d02e-4]

## Keybindings

Global keybindings across all three ProjectDialog tabs use left/right arrows to switch tabs, Esc to close, and Enter to save. [^5d02e-5]

## Save Events

The Create path sends a NostrCommand::SaveProject event (including the repo_url field), and the Edit path sends a NostrCommand::UpdateProject event with all fields including the repo URL. [^5d02e-6]

## Backend Online Agent Assignment

When a backend comes online and needs agent assignment, open_selected_project_agent_picker opens ProjectDialog in Editing mode, landing on the Agents tab. [^5d02e-7]

## Migration from ProjectSettings

All uses of the old ProjectSettings modal are replaced by the ProjectDialog, including the AgentPickerOnly sub-flow triggered when a backend comes online. ModalState::ProjectSettings, ProjectSettingsState, and its related enums (ProjectSettingsAddMode, ProjectSettingsPresentation, ProjectSettingsFocus) are deleted from the codebase. The old handle_project_settings_key and save_project_settings_changes functions are deleted. The project_settings.rs file is reduced to only rendering the agent deletion confirmation dialog (kind:24030). [^5d02e-8]
## See Also

