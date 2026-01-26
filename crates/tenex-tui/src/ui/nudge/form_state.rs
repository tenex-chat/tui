//! Nudge form state for create operations
//!
//! Multi-step wizard state following existing patterns (CreateProject, CreateAgent)

use super::ToolPermissions;
use tenex_core::models::Nudge;

/// Step in the nudge creation wizard
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NudgeFormStep {
    /// Basic info: title and description
    Basics,
    /// Content: the behavioral instruction text
    Content,
    /// Tool permissions: allow-tool and deny-tool configuration
    Permissions,
    /// Review: preview before publish
    Review,
}

impl NudgeFormStep {
    pub fn label(&self) -> &'static str {
        match self {
            NudgeFormStep::Basics => "Basics",
            NudgeFormStep::Content => "Content",
            NudgeFormStep::Permissions => "Tools",
            NudgeFormStep::Review => "Review",
        }
    }

    pub fn index(&self) -> usize {
        match self {
            NudgeFormStep::Basics => 0,
            NudgeFormStep::Content => 1,
            NudgeFormStep::Permissions => 2,
            NudgeFormStep::Review => 3,
        }
    }

    pub const ALL: [NudgeFormStep; 4] = [
        NudgeFormStep::Basics,
        NudgeFormStep::Content,
        NudgeFormStep::Permissions,
        NudgeFormStep::Review,
    ];

    pub fn next(&self) -> Option<NudgeFormStep> {
        match self {
            NudgeFormStep::Basics => Some(NudgeFormStep::Content),
            NudgeFormStep::Content => Some(NudgeFormStep::Permissions),
            NudgeFormStep::Permissions => Some(NudgeFormStep::Review),
            NudgeFormStep::Review => None,
        }
    }

    pub fn prev(&self) -> Option<NudgeFormStep> {
        match self {
            NudgeFormStep::Basics => None,
            NudgeFormStep::Content => Some(NudgeFormStep::Basics),
            NudgeFormStep::Permissions => Some(NudgeFormStep::Content),
            NudgeFormStep::Review => Some(NudgeFormStep::Permissions),
        }
    }
}

/// Which field is focused in the basics step
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NudgeFormFocus {
    Title,
    Description,
    Hashtags,
}

impl NudgeFormFocus {
    pub fn next(&self) -> Self {
        match self {
            NudgeFormFocus::Title => NudgeFormFocus::Description,
            NudgeFormFocus::Description => NudgeFormFocus::Hashtags,
            NudgeFormFocus::Hashtags => NudgeFormFocus::Title,
        }
    }

    pub fn prev(&self) -> Self {
        match self {
            NudgeFormFocus::Title => NudgeFormFocus::Hashtags,
            NudgeFormFocus::Description => NudgeFormFocus::Title,
            NudgeFormFocus::Hashtags => NudgeFormFocus::Description,
        }
    }
}

/// Permission editing mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionMode {
    /// Viewing/navigating the tool list
    Browse,
    /// Adding a tool to allow list
    AddAllow,
    /// Adding a tool to deny list
    AddDeny,
}

/// State for the nudge creation form
#[derive(Debug, Clone)]
pub struct NudgeFormState {
    /// Current wizard step
    pub step: NudgeFormStep,
    /// Focus within basics step
    pub focus: NudgeFormFocus,

    // Basics fields
    pub title: String,
    pub description: String,
    pub hashtags: Vec<String>,
    /// Current hashtag being typed
    pub hashtag_input: String,

    // Content field
    pub content: String,
    /// Scroll offset for content view
    pub content_scroll: usize,
    /// Cursor position in content (line, col)
    pub content_cursor: (usize, usize),

    // Permissions
    pub permissions: ToolPermissions,
    /// Permission editing mode
    pub permission_mode: PermissionMode,
    /// Current filter for tool search
    pub tool_filter: String,
    /// Selected index in filtered tool list
    pub tool_index: usize,
    /// Scroll offset for tool list
    pub tool_scroll: usize,

    // Review
    pub review_scroll: usize,
}

impl NudgeFormState {
    /// Create new state for creating a nudge
    pub fn new() -> Self {
        Self {
            step: NudgeFormStep::Basics,
            focus: NudgeFormFocus::Title,
            title: String::new(),
            description: String::new(),
            hashtags: Vec::new(),
            hashtag_input: String::new(),
            content: String::new(),
            content_scroll: 0,
            content_cursor: (0, 0),
            permissions: ToolPermissions::new(),
            permission_mode: PermissionMode::Browse,
            tool_filter: String::new(),
            tool_index: 0,
            tool_scroll: 0,
            review_scroll: 0,
        }
    }

    /// Create state for copying an existing nudge (creates a NEW nudge with pre-populated data)
    ///
    /// Note: Nostr events are immutable, so we can't edit them. Instead, we copy the nudge's
    /// data and allow the user to create a new nudge with modifications.
    pub fn copy_from_nudge(nudge: &Nudge) -> Self {
        let mut permissions = ToolPermissions::new();
        for tool in &nudge.allowed_tools {
            permissions.add_allow_tool(tool.clone());
        }
        for tool in &nudge.denied_tools {
            permissions.add_deny_tool(tool.clone());
        }

        let first_line_len = nudge.content.lines().next().map(|l| l.len()).unwrap_or(0);

        Self {
            step: NudgeFormStep::Basics,
            focus: NudgeFormFocus::Title,
            title: nudge.title.clone(),
            description: nudge.description.clone(),
            hashtags: nudge.hashtags.clone(),
            hashtag_input: String::new(),
            content: nudge.content.clone(),
            content_scroll: 0,
            content_cursor: (0, first_line_len),
            permissions,
            permission_mode: PermissionMode::Browse,
            tool_filter: String::new(),
            tool_index: 0,
            tool_scroll: 0,
            review_scroll: 0,
        }
    }

    /// Get the mode label for the form title
    pub fn mode_label(&self) -> &'static str {
        "Create Nudge"
    }

    /// Check if current step can proceed to next
    pub fn can_proceed(&self) -> bool {
        match self.step {
            NudgeFormStep::Basics => !self.title.trim().is_empty(),
            NudgeFormStep::Content => !self.content.trim().is_empty(),
            NudgeFormStep::Permissions => true, // Permissions are optional
            NudgeFormStep::Review => true,
        }
    }

    /// Check if form is ready to submit
    pub fn can_submit(&self) -> bool {
        !self.title.trim().is_empty() && !self.content.trim().is_empty()
    }

    /// Move to next step if possible
    pub fn next_step(&mut self) -> bool {
        if !self.can_proceed() {
            return false;
        }
        if let Some(next) = self.step.next() {
            self.step = next;
            // Reset scroll/focus for new step
            match self.step {
                NudgeFormStep::Basics => self.focus = NudgeFormFocus::Title,
                NudgeFormStep::Content => self.content_scroll = 0,
                NudgeFormStep::Permissions => {
                    self.permission_mode = PermissionMode::Browse;
                    self.tool_index = 0;
                    self.tool_filter.clear();
                }
                NudgeFormStep::Review => self.review_scroll = 0,
            }
            true
        } else {
            false
        }
    }

    /// Move to previous step if possible
    pub fn prev_step(&mut self) -> bool {
        if let Some(prev) = self.step.prev() {
            self.step = prev;
            true
        } else {
            false
        }
    }

    /// Add current hashtag input to list
    pub fn add_hashtag(&mut self) {
        let tag = self.hashtag_input.trim().to_string();
        if !tag.is_empty() && !self.hashtags.contains(&tag) {
            self.hashtags.push(tag);
        }
        self.hashtag_input.clear();
    }

    /// Remove last hashtag
    pub fn remove_last_hashtag(&mut self) {
        if self.hashtag_input.is_empty() {
            self.hashtags.pop();
        }
    }

    /// Get content line count
    pub fn content_line_count(&self) -> usize {
        self.content.lines().count().max(1)
    }

    /// Insert character at cursor position in content
    pub fn insert_content_char(&mut self, c: char) {
        let lines: Vec<&str> = self.content.lines().collect();
        let (line_idx, col_idx) = self.content_cursor;

        if line_idx >= lines.len() {
            // Append to end
            if c == '\n' {
                self.content.push('\n');
                self.content_cursor = (line_idx + 1, 0);
            } else {
                self.content.push(c);
                self.content_cursor.1 += 1;
            }
        } else {
            // Insert at position
            let mut new_content = String::new();
            for (i, line) in self.content.lines().enumerate() {
                if i > 0 {
                    new_content.push('\n');
                }
                if i == line_idx {
                    let col = col_idx.min(line.len());
                    new_content.push_str(&line[..col]);
                    if c == '\n' {
                        new_content.push('\n');
                        new_content.push_str(&line[col..]);
                        self.content_cursor = (line_idx + 1, 0);
                    } else {
                        new_content.push(c);
                        new_content.push_str(&line[col..]);
                        self.content_cursor.1 = col + 1;
                    }
                } else {
                    new_content.push_str(line);
                }
            }
            self.content = new_content;
        }
    }

    /// Delete character before cursor in content
    pub fn backspace_content(&mut self) {
        let (line_idx, col_idx) = self.content_cursor;

        if col_idx > 0 {
            // Delete within line
            let lines: Vec<&str> = self.content.lines().collect();
            if line_idx < lines.len() {
                let line = lines[line_idx];
                let col = col_idx.min(line.len());
                let new_line = format!("{}{}", &line[..col - 1], &line[col..]);

                let mut new_content = String::new();
                for (i, l) in self.content.lines().enumerate() {
                    if i > 0 {
                        new_content.push('\n');
                    }
                    if i == line_idx {
                        new_content.push_str(&new_line);
                    } else {
                        new_content.push_str(l);
                    }
                }
                self.content = new_content;
                self.content_cursor.1 = col - 1;
            }
        } else if line_idx > 0 {
            // Merge with previous line
            let lines: Vec<&str> = self.content.lines().collect();
            let prev_line_len = lines[line_idx - 1].len();

            let mut new_content = String::new();
            for (i, l) in self.content.lines().enumerate() {
                if i == line_idx {
                    // Skip newline - content already merged
                    continue;
                }
                if i > 0 && i != line_idx {
                    new_content.push('\n');
                }
                if i == line_idx - 1 {
                    new_content.push_str(l);
                    if line_idx < lines.len() {
                        new_content.push_str(lines[line_idx]);
                    }
                } else {
                    new_content.push_str(l);
                }
            }
            self.content = new_content;
            self.content_cursor = (line_idx - 1, prev_line_len);
        }
    }

    /// Move content cursor up
    pub fn move_content_up(&mut self) {
        if self.content_cursor.0 > 0 {
            self.content_cursor.0 -= 1;
            // Clamp column to line length
            let lines: Vec<&str> = self.content.lines().collect();
            if self.content_cursor.0 < lines.len() {
                self.content_cursor.1 = self.content_cursor.1.min(lines[self.content_cursor.0].len());
            }
        }
    }

    /// Move content cursor down
    pub fn move_content_down(&mut self) {
        let line_count = self.content_line_count();
        if self.content_cursor.0 + 1 < line_count {
            self.content_cursor.0 += 1;
            // Clamp column to line length
            let lines: Vec<&str> = self.content.lines().collect();
            if self.content_cursor.0 < lines.len() {
                self.content_cursor.1 = self.content_cursor.1.min(lines[self.content_cursor.0].len());
            }
        }
    }

    /// Move content cursor left
    pub fn move_content_left(&mut self) {
        if self.content_cursor.1 > 0 {
            self.content_cursor.1 -= 1;
        } else if self.content_cursor.0 > 0 {
            // Move to end of previous line
            self.content_cursor.0 -= 1;
            let lines: Vec<&str> = self.content.lines().collect();
            if self.content_cursor.0 < lines.len() {
                self.content_cursor.1 = lines[self.content_cursor.0].len();
            }
        }
    }

    /// Move content cursor right
    pub fn move_content_right(&mut self) {
        let lines: Vec<&str> = self.content.lines().collect();
        if self.content_cursor.0 < lines.len() {
            let line_len = lines[self.content_cursor.0].len();
            if self.content_cursor.1 < line_len {
                self.content_cursor.1 += 1;
            } else if self.content_cursor.0 + 1 < lines.len() {
                // Move to start of next line
                self.content_cursor.0 += 1;
                self.content_cursor.1 = 0;
            }
        }
    }

    /// Filter available tools based on current filter
    pub fn filter_tools<'a>(&self, available_tools: &'a [String]) -> Vec<&'a str> {
        if self.tool_filter.is_empty() {
            available_tools.iter().map(|s| s.as_str()).collect()
        } else {
            let filter_lower = self.tool_filter.to_lowercase();
            available_tools
                .iter()
                .filter(|t| t.to_lowercase().contains(&filter_lower))
                .map(|s| s.as_str())
                .collect()
        }
    }
}

impl Default for NudgeFormState {
    fn default() -> Self {
        Self::new()
    }
}
