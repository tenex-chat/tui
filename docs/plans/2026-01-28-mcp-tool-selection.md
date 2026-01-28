# MCP Tool Selection in TUI Create Project Wizard

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add MCP tool selection as a third step in the TUI's create project wizard, matching the Svelte app's behavior of auto-including agent dependencies plus manual tool selection.

**Architecture:** Introduce MCPTool model to parse kind:4200 events, subscribe to these events in the nostr worker, store in AppDataStore, and add a third wizard step (Details → Agents → Tools) that merges manually selected tools with auto-dependencies from selected agents.

**Tech Stack:** Rust, nostr-sdk, nostrdb, ratatui

---

## Task 1: Create MCPTool Model

**Files:**
- Create: `crates/tenex-core/src/models/mcp_tool.rs`
- Modify: `crates/tenex-core/src/models/mod.rs`

**Step 1: Write MCPTool struct and from_note parser**

Create `crates/tenex-core/src/models/mcp_tool.rs`:

```rust
use nostrdb::Note;

/// MCP Tool Definition - kind:4200 events describing MCP servers/tools
#[derive(Debug, Clone)]
pub struct MCPTool {
    pub id: String,
    pub pubkey: String,
    pub d_tag: String,
    pub name: String,
    pub description: String,
    pub server_url: Option<String>,
    pub version: Option<String>,
    pub created_at: u64,
}

impl MCPTool {
    /// Parse an MCPTool from a kind:4200 note
    pub fn from_note(note: &Note) -> Option<Self> {
        if note.kind() != 4200 {
            return None;
        }

        let id = hex::encode(note.id());
        let pubkey = hex::encode(note.pubkey());
        let description = note.content().to_string();
        let created_at = note.created_at();

        let mut d_tag = None;
        let mut name = None;
        let mut server_url = None;
        let mut version = None;

        for tag in note.tags() {
            if tag.count() >= 2 {
                if let (Some(tag_name), Some(value)) = (
                    tag.get(0).and_then(|t| t.variant().str()),
                    tag.get(1).and_then(|t| t.variant().str())
                ) {
                    match tag_name {
                        "d" => d_tag = Some(value.to_string()),
                        "title" | "name" => name = Some(value.to_string()),
                        "server" | "url" => server_url = Some(value.to_string()),
                        "version" => version = Some(value.to_string()),
                        _ => {}
                    }
                }
            }
        }

        Some(MCPTool {
            id,
            pubkey,
            d_tag: d_tag.unwrap_or_default(),
            name: name.unwrap_or_else(|| "Unnamed Tool".to_string()),
            description,
            server_url,
            version,
            created_at,
        })
    }

    pub fn description_preview(&self, max_chars: usize) -> String {
        if self.description.len() <= max_chars {
            self.description.clone()
        } else {
            format!("{}...", &self.description[..max_chars.saturating_sub(3)])
        }
    }
}
```

**Step 2: Export MCPTool from models module**

Modify `crates/tenex-core/src/models/mod.rs`:

Find the module declarations and add:
```rust
mod mcp_tool;
```

Find the pub use statements and add:
```rust
pub use mcp_tool::MCPTool;
```

**Step 3: Commit**

```bash
git add crates/tenex-core/src/models/mcp_tool.rs crates/tenex-core/src/models/mod.rs
git commit -m "feat(models): add MCPTool model for kind:4200 events"
```

---

## Task 2: Add MCPTool Storage to AppDataStore

**Files:**
- Modify: `crates/tenex-core/src/store/app_data_store.rs`

**Step 1: Add mcp_tools field to AppDataStore**

In `crates/tenex-core/src/store/app_data_store.rs`, find the struct definition and add:

```rust
use crate::models::MCPTool;  // Add to imports at top

pub struct AppDataStore {
    // ... existing fields ...

    // MCP Tools - kind:4200 events
    pub mcp_tools: HashMap<String, MCPTool>,  // keyed by id
}
```

**Step 2: Initialize mcp_tools in new() method**

Find the `impl AppDataStore` block and the `new()` method, add to the struct initialization:

```rust
pub fn new(ndb: Arc<Ndb>) -> Self {
    Self {
        // ... existing fields ...
        mcp_tools: HashMap::new(),
    }
}
```

**Step 3: Add insert_mcp_tool method**

Add method to `impl AppDataStore`:

```rust
pub fn insert_mcp_tool(&mut self, note: &Note) {
    if let Some(tool) = MCPTool::from_note(note) {
        self.mcp_tools.insert(tool.id.clone(), tool);
    }
}
```

**Step 4: Add get_mcp_tools method**

Add method to `impl AppDataStore`:

```rust
pub fn get_mcp_tools(&self) -> Vec<&MCPTool> {
    let mut tools: Vec<_> = self.mcp_tools.values().collect();
    tools.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    tools
}
```

**Step 5: Add get_mcp_tool method**

Add method to `impl AppDataStore`:

```rust
pub fn get_mcp_tool(&self, id: &str) -> Option<&MCPTool> {
    self.mcp_tools.get(id)
}
```

**Step 6: Commit**

```bash
git add crates/tenex-core/src/store/app_data_store.rs
git commit -m "feat(store): add MCP tool storage and query methods"
```

---

## Task 3: Add DataChange Variant for MCPTools

**Files:**
- Modify: `crates/tenex-core/src/nostr/mod.rs`

**Step 1: Add MCPToolsChanged variant**

In `crates/tenex-core/src/nostr/mod.rs`, find the `DataChange` enum and add:

```rust
pub enum DataChange {
    // ... existing variants ...
    MCPToolsChanged,
}
```

**Step 2: Commit**

```bash
git add crates/tenex-core/src/nostr/mod.rs
git commit -m "feat(nostr): add MCPToolsChanged data change variant"
```

---

## Task 4: Subscribe to kind:4200 Events in Nostr Worker

**Files:**
- Modify: `crates/tenex-core/src/nostr/worker.rs`

**Step 1: Add kind:4200 subscription in handle_connect**

In `crates/tenex-core/src/nostr/worker.rs`, find the `handle_connect` method around line 558 (after agent lessons subscription), and add:

```rust
// 6. MCP Tools (kind:4200)
let mcp_tool_filter = Filter::new().kind(Kind::Custom(4200));
let output = client.subscribe(vec![mcp_tool_filter], None).await?;
self.subscription_ids.insert(
    output.val.to_string(),
    SubscriptionInfo::new("MCP tools".to_string(), vec![4200], None),
);
tlog!("CONN", "Subscribed to MCP tools (kind:4200)");
```

**Step 2: Add kind:4200 event processing**

Find the `process_event` function and add a new match arm for kind 4200:

```rust
4200 => {
    // MCP tool definition
    store.insert_mcp_tool(&note);
    changes.push(DataChange::MCPToolsChanged);
}
```

**Step 3: Add kind:4200 to negentropy sync**

Find the `run_negentropy_sync` function around line 1630 (after agent lessons sync), and add:

```rust
// MCP tools (kind 4200)
let mcp_tool_filter = Filter::new().kind(Kind::Custom(4200));
total_new += sync_filter(client, mcp_tool_filter, "4200", stats).await;
```

**Step 4: Commit**

```bash
git add crates/tenex-core/src/nostr/worker.rs
git commit -m "feat(nostr): subscribe to kind:4200 MCP tool events"
```

---

## Task 5: Add MCP Tool Query Methods to App

**Files:**
- Modify: `crates/tenex-tui/src/ui/app.rs`

**Step 1: Add get_mcp_tools method**

In `crates/tenex-tui/src/ui/app.rs`, find `impl App` and add:

```rust
pub fn get_mcp_tools(&self) -> Vec<&MCPTool> {
    self.data.get_mcp_tools()
}
```

**Step 2: Add get_mcp_tool method**

Add to `impl App`:

```rust
pub fn get_mcp_tool(&self, id: &str) -> Option<&MCPTool> {
    self.data.get_mcp_tool(id)
}
```

**Step 3: Add mcp_tools_filtered_by method**

Add to `impl App`:

```rust
pub fn mcp_tools_filtered_by(&self, filter: &str) -> Vec<&MCPTool> {
    if filter.is_empty() {
        return self.get_mcp_tools();
    }

    let lower_filter = filter.to_lowercase();
    self.get_mcp_tools()
        .into_iter()
        .filter(|tool| {
            tool.name.to_lowercase().contains(&lower_filter)
                || tool.description.to_lowercase().contains(&lower_filter)
        })
        .collect()
}
```

**Step 4: Add import for MCPTool**

At the top of `app.rs`, find the imports and add:

```rust
use tenex_core::models::MCPTool;
```

**Step 5: Commit**

```bash
git add crates/tenex-tui/src/ui/app.rs
git commit -m "feat(app): add MCP tool query methods"
```

---

## Task 6: Update CreateProjectState for MCP Tool Selection

**Files:**
- Modify: `crates/tenex-tui/src/ui/modal.rs`

**Step 1: Add SelectTools to CreateProjectStep enum**

In `crates/tenex-tui/src/ui/modal.rs`, find the `CreateProjectStep` enum around line 131 and modify:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreateProjectStep {
    Details,
    SelectAgents,
    SelectTools,
}
```

**Step 2: Add MCP tool fields to CreateProjectState**

Find the `CreateProjectState` struct around line 145 and add new fields:

```rust
#[derive(Debug, Clone)]
pub struct CreateProjectState {
    pub step: CreateProjectStep,
    pub focus: CreateProjectFocus,
    pub name: String,
    pub description: String,
    pub agent_ids: Vec<String>,
    pub agent_selector: SelectorState,
    pub mcp_tool_ids: Vec<String>,
    pub tool_selector: SelectorState,
}
```

**Step 3: Update CreateProjectState::new() to initialize new fields**

Find the `impl CreateProjectState` block and update the `new()` method:

```rust
pub fn new() -> Self {
    Self {
        step: CreateProjectStep::Details,
        focus: CreateProjectFocus::Name,
        name: String::new(),
        description: String::new(),
        agent_ids: Vec::new(),
        agent_selector: SelectorState::default(),
        mcp_tool_ids: Vec::new(),
        tool_selector: SelectorState::default(),
    }
}
```

**Step 4: Update can_proceed to handle SelectTools**

In the same impl block, update `can_proceed`:

```rust
pub fn can_proceed(&self) -> bool {
    match self.step {
        CreateProjectStep::Details => !self.name.trim().is_empty(),
        CreateProjectStep::SelectAgents => true,
        CreateProjectStep::SelectTools => true,
    }
}
```

**Step 5: Add toggle_mcp_tool method**

Add new method to `impl CreateProjectState`:

```rust
pub fn toggle_mcp_tool(&mut self, tool_id: String) {
    if let Some(pos) = self.mcp_tool_ids.iter().position(|id| id == &tool_id) {
        self.mcp_tool_ids.remove(pos);
    } else {
        self.mcp_tool_ids.push(tool_id);
    }
}
```

**Step 6: Add all_mcp_tool_ids method**

Add new method to `impl CreateProjectState`:

```rust
use std::collections::HashSet;  // Add to imports at top if not present

pub fn all_mcp_tool_ids(&self, app: &App) -> Vec<String> {
    let mut tool_ids = HashSet::new();

    // Add manually selected
    for id in &self.mcp_tool_ids {
        tool_ids.insert(id.clone());
    }

    // Add from selected agents
    for agent_id in &self.agent_ids {
        if let Some(agent) = app.get_agent_definition(agent_id) {
            for mcp_id in &agent.mcp_servers {
                tool_ids.insert(mcp_id.clone());
            }
        }
    }

    tool_ids.into_iter().collect()
}
```

**Step 7: Commit**

```bash
git add crates/tenex-tui/src/ui/modal.rs
git commit -m "feat(modal): add MCP tool selection state to CreateProjectState"
```

---

## Task 7: Add Render Function for Tools Step

**Files:**
- Modify: `crates/tenex-tui/src/ui/views/create_project.rs`

**Step 1: Update step indicator in render_create_project**

In `crates/tenex-tui/src/ui/views/create_project.rs`, find the `render_create_project` function and update the step indicator around line 15:

```rust
let step_indicator = match state.step {
    CreateProjectStep::Details => "Step 1/3: Details",
    CreateProjectStep::SelectAgents => "Step 2/3: Select Agents",
    CreateProjectStep::SelectTools => "Step 3/3: Select MCP Tools",
};
```

**Step 2: Add SelectTools rendering in match statement**

Find the match statement around line 35 and add the new case:

```rust
match state.step {
    CreateProjectStep::Details => {
        render_details_step(f, inner_area, state);
    }
    CreateProjectStep::SelectAgents => {
        render_agents_step(f, app, inner_area, state);
    }
    CreateProjectStep::SelectTools => {
        render_tools_step(f, app, inner_area, state);
    }
}
```

**Step 3: Add hints for SelectTools step**

Find the hint_spans match statement around line 52 and add:

```rust
CreateProjectStep::SelectTools => vec![
    Span::styled("↑↓", Style::default().fg(theme::ACCENT_WARNING)),
    Span::styled(" navigate", Style::default().fg(theme::TEXT_MUTED)),
    Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
    Span::styled("Space", Style::default().fg(theme::ACCENT_WARNING)),
    Span::styled(" toggle", Style::default().fg(theme::TEXT_MUTED)),
    Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
    Span::styled("Enter", Style::default().fg(theme::ACCENT_SUCCESS)),
    Span::styled(" create", Style::default().fg(theme::TEXT_MUTED)),
    Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
    Span::styled("Backspace", Style::default().fg(theme::ACCENT_WARNING)),
    Span::styled(" back", Style::default().fg(theme::TEXT_MUTED)),
],
```

**Step 4: Add render_tools_step function**

At the end of the file, add the new rendering function:

```rust
fn render_tools_step(f: &mut Frame, app: &App, area: Rect, state: &CreateProjectState) {
    // Search bar
    let remaining = render_modal_search(f, area, &state.tool_selector.filter, "Search MCP tools...");

    // Get filtered tools
    let filtered_tools = app.mcp_tools_filtered_by(&state.tool_selector.filter);

    // List area
    let list_area = Rect::new(
        remaining.x,
        remaining.y + 1,
        remaining.width,
        remaining.height.saturating_sub(3),
    );

    if filtered_tools.is_empty() {
        let msg = if state.tool_selector.filter.is_empty() {
            "No MCP tools available."
        } else {
            "No tools match your search."
        };
        let empty_msg = Paragraph::new(msg).style(Style::default().fg(theme::TEXT_MUTED));
        f.render_widget(empty_msg, list_area);
    } else {
        let visible_height = list_area.height as usize;
        let selected_index = state.tool_selector.index.min(filtered_tools.len().saturating_sub(1));

        let scroll_offset = if selected_index >= visible_height {
            selected_index - visible_height + 1
        } else {
            0
        };

        let items: Vec<ListItem> = filtered_tools
            .iter()
            .enumerate()
            .skip(scroll_offset)
            .take(visible_height)
            .map(|(i, tool)| {
                let is_cursor = i == selected_index;
                let is_selected = state.mcp_tool_ids.contains(&tool.id);

                let mut spans = vec![];

                // Checkbox
                let checkbox = if is_selected { "[✓] " } else { "[ ] " };
                let checkbox_style = if is_selected {
                    Style::default().fg(theme::ACCENT_SUCCESS)
                } else {
                    Style::default().fg(theme::TEXT_MUTED)
                };
                spans.push(Span::styled(checkbox, checkbox_style));

                // Cursor indicator
                if is_cursor {
                    spans.push(Span::styled("▌", Style::default().fg(theme::ACCENT_PRIMARY)));
                } else {
                    spans.push(Span::styled(" ", Style::default()));
                }

                // Tool name
                let name_style = if is_cursor {
                    Style::default().fg(theme::ACCENT_PRIMARY).add_modifier(Modifier::BOLD)
                } else if is_selected {
                    Style::default().fg(theme::ACCENT_SUCCESS)
                } else {
                    Style::default().fg(theme::TEXT_PRIMARY)
                };
                spans.push(Span::styled(tool.name.clone(), name_style));

                // Description preview
                if !tool.description.is_empty() {
                    let desc_preview = if tool.description.len() > 40 {
                        format!(" - {}...", &tool.description[..37])
                    } else {
                        format!(" - {}", tool.description)
                    };
                    spans.push(Span::styled(desc_preview, Style::default().fg(theme::TEXT_MUTED)));
                }

                ListItem::new(Line::from(spans))
            })
            .collect();

        let list = List::new(items);
        f.render_widget(list, list_area);
    }

    // Show selected count
    let count_area = Rect::new(
        remaining.x,
        list_area.y + list_area.height,
        remaining.width,
        1,
    );

    // Calculate total (manual + auto from agents)
    let total_tool_count = state.all_mcp_tool_ids(app).len();
    let manual_count = state.mcp_tool_ids.len();

    let count_text = if total_tool_count > manual_count {
        format!("{} tool(s) selected ({} from agents)", total_tool_count, total_tool_count - manual_count)
    } else {
        format!("{} tool(s) selected", total_tool_count)
    };

    let count = Paragraph::new(count_text).style(Style::default().fg(theme::TEXT_DIM));
    f.render_widget(count, count_area);
}
```

**Step 5: Commit**

```bash
git add crates/tenex-tui/src/ui/views/create_project.rs
git commit -m "feat(views): add MCP tool selection step rendering"
```

---

## Task 8: Update Input Handling for Tool Selection Step

**Files:**
- Modify: `crates/tenex-tui/src/input/view_handlers.rs`

**Step 1: Update SelectAgents step to transition to SelectTools**

In `crates/tenex-tui/src/input/view_handlers.rs`, find the `handle_create_project_key` function around line 892. Find the `CreateProjectStep::SelectAgents` match arm and update the `KeyCode::Enter` case:

```rust
KeyCode::Enter => {
    // Move to tool selection step
    state.step = CreateProjectStep::SelectTools;
    state.tool_selector.filter.clear();
    state.tool_selector.index = 0;
}
```

**Step 2: Add input handling for SelectTools step**

After the `SelectAgents` match arm, add the new `SelectTools` case:

```rust
CreateProjectStep::SelectTools => {
    let filtered_tools = app.mcp_tools_filtered_by(&state.tool_selector.filter);
    let item_count = filtered_tools.len();

    match code {
        KeyCode::Esc => {
            app.modal_state = ModalState::None;
            return;
        }
        KeyCode::Backspace if state.tool_selector.filter.is_empty() => {
            state.step = CreateProjectStep::SelectAgents;
        }
        KeyCode::Backspace => {
            state.tool_selector.filter.pop();
            state.tool_selector.index = 0;
        }
        KeyCode::Up => {
            if state.tool_selector.index > 0 {
                state.tool_selector.index -= 1;
            }
        }
        KeyCode::Down => {
            if item_count > 0 && state.tool_selector.index + 1 < item_count {
                state.tool_selector.index += 1;
            }
        }
        KeyCode::Char(' ') => {
            if let Some(tool) = filtered_tools.get(state.tool_selector.index) {
                state.toggle_mcp_tool(tool.id.clone());
            }
        }
        KeyCode::Enter => {
            // Create the project with all tool IDs
            if let Some(ref core_handle) = app.core_handle {
                let all_tool_ids = state.all_mcp_tool_ids(app);

                if let Err(e) = core_handle.send(NostrCommand::CreateProject {
                    name: state.name.clone(),
                    description: state.description.clone(),
                    agent_ids: state.agent_ids.clone(),
                    mcp_tool_ids: all_tool_ids,
                }) {
                    app.set_warning_status(&format!("Failed to create project: {}", e));
                } else {
                    app.set_warning_status("Project created");
                }
            }
            app.modal_state = ModalState::None;
            return;
        }
        KeyCode::Char(c) => {
            state.tool_selector.filter.push(c);
            state.tool_selector.index = 0;
        }
        _ => {}
    }
}
```

**Step 3: Commit**

```bash
git add crates/tenex-tui/src/input/view_handlers.rs
git commit -m "feat(input): add tool selection step keyboard handling"
```

---

## Task 9: Update NostrCommand to Include MCP Tool IDs

**Files:**
- Modify: `crates/tenex-core/src/nostr/worker.rs`

**Step 1: Add mcp_tool_ids to CreateProject command**

In `crates/tenex-core/src/nostr/worker.rs`, find the `NostrCommand` enum around line 106 and update:

```rust
CreateProject {
    name: String,
    description: String,
    agent_ids: Vec<String>,
    mcp_tool_ids: Vec<String>,
},
```

**Step 2: Update handle_create_project signature**

Find the `handle_create_project` method around line 1025 and update the signature:

```rust
async fn handle_create_project(
    &self,
    name: String,
    description: String,
    agent_ids: Vec<String>,
    mcp_tool_ids: Vec<String>,
) -> Result<()> {
```

**Step 3: Add MCP tool tags to event builder**

In the same `handle_create_project` method, after the agent tags loop around line 1062, add:

```rust
// Add MCP tool tags
for tool_id in &mcp_tool_ids {
    event = event.tag(Tag::custom(
        TagKind::Custom(std::borrow::Cow::Borrowed("mcp")),
        vec![tool_id.clone()],
    ));
}
```

**Step 4: Update command handler to pass mcp_tool_ids**

Find the command handler around line 321 and update:

```rust
NostrCommand::CreateProject { name, description, agent_ids, mcp_tool_ids } => {
    debug_log(&format!("Worker: Creating project {}", name));
    if let Err(e) = rt.block_on(self.handle_create_project(name, description, agent_ids, mcp_tool_ids)) {
        tlog!("ERROR", "Failed to create project: {}", e);
    }
}
```

**Step 5: Commit**

```bash
git add crates/tenex-core/src/nostr/worker.rs
git commit -m "feat(nostr): add MCP tool IDs to project creation"
```

---

## Task 10: Test End-to-End Functionality

**Files:**
- N/A (manual testing)

**Step 1: Build the project**

```bash
cargo build --release
```

Expected: Clean build with no errors

**Step 2: Run the TUI**

```bash
cargo run --release
```

**Step 3: Open create project wizard**

- Navigate to home view
- Press the hotkey to open create project modal (check hotkeys.rs for the binding)

Expected: Modal opens showing "Step 1/3: Details"

**Step 4: Fill in project details**

- Enter project name: "Test MCP Project"
- Tab to description
- Enter description: "Testing MCP tool selection"
- Press Enter

Expected: Advances to "Step 2/3: Select Agents"

**Step 5: Select an agent with MCP dependencies**

- Use arrow keys to navigate
- Press Space to select an agent that has mcp_servers
- Press Enter

Expected: Advances to "Step 3/3: Select MCP Tools"

**Step 6: Verify auto-included tools**

Expected: Status line shows tool count including "(X from agents)"

**Step 7: Manually select additional tools**

- Navigate with arrow keys
- Press Space to toggle selection
- Verify checkbox updates

Expected: Tool count updates correctly

**Step 8: Create the project**

- Press Enter

Expected:
- Modal closes
- Status shows "Project created"
- New kind:31933 event published with both "agent" and "mcp" tags

**Step 9: Verify event structure**

Check nostrdb or relay for the created event. It should have:
- kind: 31933
- tags: ["d", ...], ["title", ...], ["agent", ...], ["mcp", ...]

**Step 10: Final commit**

```bash
git add -A
git commit -m "test: verify MCP tool selection workflow"
```

---

## Completion Checklist

- [ ] MCPTool model created and parsing kind:4200 events
- [ ] AppDataStore storing and querying MCP tools
- [ ] Worker subscribed to kind:4200 events and syncing via negentropy
- [ ] CreateProjectState has 3-step wizard with tool selection
- [ ] Render function displays tool selection UI with search
- [ ] Input handling navigates through all 3 steps correctly
- [ ] Auto-includes agent MCP dependencies + manual selections
- [ ] Project creation sends "mcp" tags in kind:31933 event
- [ ] End-to-end manual testing confirms functionality
- [ ] All commits follow conventional commit format

---
