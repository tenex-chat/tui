---
title: Private Projects
slug: private-projects
summary: Private projects are not visible to anyone other than their members.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-04
updated: 2026-05-04
verified: 2026-05-04
compiled-from: conversation
sources:
  - session:583f61d8-de84-4927-a5fe-f2fa6dbbe741
---

# Private Projects

## Visibility

Private projects are not visible to anyone other than their members. [^583f6-1]


## Tags and Data Model

Private projects are tagged with the tag ["scope", "private"] on project CRUD operations. The Project struct includes an `is_private: bool` field parsed from the ["scope", "private"] tag. [^583f6-2]

## Emission

The `build_project_event_builder` emits the ["scope", "private"] tag when `is_private` is true. [^583f6-3]

## Command Layer

`NostrCommand::SaveProject`, `UpdateProject`, and `UpdateProjectAgents` all carry an `is_private` field. [^583f6-4]

## FFI

The FFI `create_project` and `update_project` functions expose `is_private: bool`. [^583f6-5]

## TUI — Create Project Wizard

In the TUI create project wizard, Tab cycles through Name, Description, Private, and back to Name. Space toggles the Private checkbox. [^583f6-6]

## TUI — Project Settings Modal

The TUI project settings modal title shows a [private] badge when the project is private. Shift+P toggles the privacy setting. Privacy changes are tracked and saved. [^583f6-7]

## Swift — Data Model

The Swift `Project` struct includes `isPrivate: Bool` with `FfiConverter` reading/writing in the correct field order. [^583f6-8]

## Swift — Core API

`TenexCore`, `SafeTenexCoreProtocol`, and `SafeTenexCore` all expose `isPrivate: Bool` on create and update operations. [^583f6-9]

## iOS — Create Project View

The iOS `CreateProjectView` includes a privacy toggle in the Project Info step and shows the privacy status in the review screen. [^583f6-10]

## iOS — Project Settings View

The iOS `ProjectSettingsView` includes a privacy toggle in the General section that persists on both general and agent saves and is initialized from `project.isPrivate`. [^583f6-11]
## See Also

