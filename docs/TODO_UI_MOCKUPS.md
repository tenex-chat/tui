# Todo List UI Mockups for TUI

Based on the web client's todo list feature, here are multiple design options for displaying todos in the terminal interface.

---

## Option 1: Inline in Chat (After Agent Messages)

**Location:** Displayed directly in the chat message area after agent messages that create todos

**Visual Design:**
```
┌─────────────────────────────────────────────────────────────────────────┐
│ @agent-name  2m ago                                                     │
│ I'll work on this task in the following steps...                       │
│                                                                         │
│ ┌─ ✓ Todo List (3/3 done) ───────────────────────────────────────┐   │
│ │                                                                  │   │
│ │ ✓ Initial exploration: capture ideas & landscape                │   │
│ │   Write initial report capturing the "magical model" concept... │   │
│ │                                                                  │   │
│ │ ✓ Deep-dive: SDK & pattern research                             │   │
│ │   Collect and summarize relevant AI SDK capabilities...         │   │
│ │                                                                  │   │
│ │ ✓ Double-check & synthesize report                              │   │
│ │   Validate the combined findings, incorporate good ideas...     │   │
│ │                                                                  │   │
│ └──────────────────────────────────────────────────────────────────┘   │
├─────────────────────────────────────────────────────────────────────────┤
│ ⟩ Type message...                                                       │
└─────────────────────────────────────────────────────────────────────────┘
```

**Symbols:**
- `✓` = Completed task (green)
- `○` = Pending task (white)
- `●` = In progress task (yellow/cyan)
- `✗` = Skipped task (red)

**Pros:**
- Contextual - shows todos right where agent created them
- Easy to see task progress in conversation flow
- No separate view needed

**Cons:**
- Takes up chat space
- Not filterable/sortable
- Disappears as you scroll

---

## Option 2: Sticky Header in Chat View

**Location:** Fixed panel at top of chat view (like web client's sticky todo header)

**Visual Design:**
```
┌─ TENEX ─ Sample Questions ──────────────────────────────────────────────┐
├─────────────────────────────────────────────────────────────────────────┤
│ ✓ Todo (3/3 done) ▼                                  [t] toggle details │
│ ✓ Initial exploration  ✓ Deep-dive research  ✓ Double-check report     │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                         │
│ @agent-name  5m ago                                                    │
│ I'll complete these tasks...                                           │
│                                                                         │
│ @user  3m ago                                                          │
│ Sounds good!                                                           │
│                                                                         │
├─────────────────────────────────────────────────────────────────────────┤
│ ⟩ Type message...                                                       │
└─────────────────────────────────────────────────────────────────────────┘
```

**With Expanded Details (press 't' to toggle):**
```
┌─ TENEX ─ Sample Questions ──────────────────────────────────────────────┐
├─────────────────────────────────────────────────────────────────────────┤
│ ✓ Todo (3/3 done) ▲                                  [t] toggle details │
│                                                                         │
│ ✓ Initial exploration: capture ideas & landscape                       │
│   Write initial report capturing the "magical model" concept, list...  │
│                                                                         │
│ ✓ Deep-dive: SDK & pattern research                                    │
│   Collect and summarize relevant AI SDK capabilities (OpenAI, Anthro..│
│                                                                         │
│ ✓ Double-check & synthesize report                                     │
│   Validate the combined findings, incorporate good ideas into the...   │
│                                                                         │
├─────────────────────────────────────────────────────────────────────────┤
│ [scrollable chat messages below...]                                    │
```

**Pros:**
- Always visible while in conversation
- Collapsible to save space
- Shows progress at a glance
- Contextual to current thread

**Cons:**
- Takes vertical space
- Only shows todos for current thread

---

## Option 3: Dedicated Home Tab

**Location:** Add 4th tab to Home view: Recent | Inbox | Todos | Projects

**Visual Design:**
```
┌─ TENEX ────────────────────────────────────────────────────────────────┐
│ Recent    Inbox (17)    Todos (5)    Projects                          │
│                         ─────                                           │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                         │
│ ┌─ Active Todos (5) ──────────────────────────────────────────────┐   │
│ │                                                                  │   │
│ │ ● Initial exploration: capture ideas & landscape                │   │
│ │   ● DDD @Agent1                                                  │   │
│ │   Write initial report capturing the "magical model" concept... │   │
│ │   [i] View conversation                                          │   │
│ │                                                                  │   │
│ │ ○ Review implementation plan                                     │   │
│ │   ● Backend @PM-WIP                                              │   │
│ │   Check the implementation plan for completeness...              │   │
│ │   [i] View conversation                                          │   │
│ │                                                                  │   │
│ └──────────────────────────────────────────────────────────────────┘   │
│                                                                         │
│ ┌─ Completed Todos (3) ───────────────────────────────────────────┐   │
│ │                                                                  │   │
│ │ ✓ Deep-dive: SDK research                                        │   │
│ │   ● DDD @Agent1  [completed 1h ago]                              │   │
│ │                                                                  │   │
│ │ ✓ Synthesize findings                                            │   │
│ │   ● DDD @Agent1  [completed 30m ago]                             │   │
│ │                                                                  │   │
│ └──────────────────────────────────────────────────────────────────┘   │
├─────────────────────────────────────────────────────────────────────────┤
│ ↑↓ navigate · Enter view thread · f filter · s sort · q quit           │
└─────────────────────────────────────────────────────────────────────────┘
```

**Features:**
- Aggregates todos from ALL conversations
- Shows project and agent for each todo
- Separates active from completed
- Can filter by project/status
- Jump to source conversation with Enter

**Pros:**
- Global view of all work across projects
- Filterable and sortable
- Clear overview of what needs attention
- Terminal-optimized layout

**Cons:**
- Requires new tab
- Loses thread context

---

## Option 4: Collapsible Sidebar in Chat

**Location:** Optional right panel in chat view (toggle with 't' key)

**Visual Design (Collapsed - default):**
```
┌─ TENEX ─ Sample Questions ──────────────────────────────────────────────┐
│ 1. ● DDD | Sample Qu... │ 2. ● Backend | Another... │      [t] show todos│
├─────────────────────────────────────────────────────────────────────────┤
│ @agent-name  5m ago                                                    │
│ Let me work on these tasks...                                          │
│                                                                         │
│ [Normal chat messages continue...]                                     │
```

**Visual Design (Expanded - press 't'):**
```
┌─ TENEX ─ Sample Questions ─────────────┬─ Todos (3) ──────────────────┐
│ 1. ● DDD | Sample Questions │          │ ✓ Task 1                     │
├────────────────────────────────────────┤ ○ Task 2 (in_progress)       │
│ @agent-name  5m ago                    │ ○ Task 3                     │
│ Let me work on these tasks...          │                              │
│                                        │ [t] hide  [Enter] view all   │
│ ⟩ Type message...                      │                              │
└────────────────────────────────────────┴──────────────────────────────┘
```

**Pros:**
- Available in context without leaving chat
- Toggle on/off to manage screen space
- Shows current thread's todos

**Cons:**
- Reduces chat area width
- Complex layout management

---

## Option 5: Compact Inline Badge (Minimalist)

**Location:** Small indicator in message header with quick expand

**Visual Design (Collapsed):**
```
│ @agent-name  2m ago                               [3 todos: 2✓ 1○]
│ I've created a task list for this work...
│ [Press 't' to view todos]
```

**Visual Design (Expanded - press 't'):**
```
│ @agent-name  2m ago                               [3 todos: 2✓ 1○]
│ I've created a task list for this work...
│
│ ┌─ Todos ────────────────────────────────────────────────────────┐
│ │ ✓ Initial exploration: capture ideas                           │
│ │ ✓ Deep-dive: SDK research                                      │
│ │ ○ Synthesize findings                                          │
│ └────────────────────────────────────────────────────────────────┘
│
│ [Press 't' to hide]
```

**Pros:**
- Minimal visual intrusion
- Expandable on demand
- Clear status at a glance (2✓ 1○)

**Cons:**
- No descriptions shown when collapsed
- Manual toggling required

---

## Recommended Hybrid Approach

**Combine Options 2 + 5:**

1. **Sticky header** in chat view (Option 2) - Always visible, collapsible
2. **Compact badge** in messages (Option 5) - Historical reference
3. **Optional dedicated tab** (Option 3) - For power users who want global view

### Implementation Priority:

**Phase 1: Sticky Header (High Value)**
- Always visible in active conversation
- Shows real-time progress
- Keyboard shortcut to toggle expanded/collapsed
- Minimal code (~200 LOC)

**Phase 2: Message Badge (Nice to Have)**
- Shows todo count in message header
- Expandable with 't' key
- Historical reference
- Minimal code (~50 LOC)

**Phase 3: Global Todos Tab (Optional)**
- Aggregates all todos across projects
- For users managing multiple concurrent tasks
- More complex (~300 LOC)

---

## Visual Symbol Legend

```
Status Symbols:
  ○  Pending task
  ●  In progress task (pulsing/highlighted)
  ✓  Completed task
  ✗  Skipped task

Colors (if terminal supports):
  Pending: White/Gray
  In Progress: Cyan/Yellow
  Completed: Green
  Skipped: Red

Expansion:
  ▼  Expanded (details showing)
  ▶  Collapsed (compact view)
```

---

## Keyboard Shortcuts

```
't' - Toggle todo panel/details
'Enter' on todo - Jump to source message
'Space' on todo - Mark as done (if editable)
'x' on todo - Mark as skipped
'f' in todos tab - Filter by status
's' in todos tab - Sort by project/status/time
```

---

## Data Flow

**Backend provides:**
- `todo_write` tool creates/updates todos (replaces entire list)
- Todos stored in conversation context
- Each todo has: content, title, description, status, activeForm, skip_reason

**TUI should:**
- Parse `todo_write` events from conversation
- Display aggregated view (latest todo_write replaces previous)
- Allow navigation to source conversation
- Real-time updates as agent works

---

## Next Steps

1. Review these mockups with you
2. Choose preferred approach(es)
3. Implement Phase 1 (sticky header) first
4. Test with real todo events from backend
5. Iterate based on usage

Which layout resonates best with your vision for the TUI experience?
