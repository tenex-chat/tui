//! App Settings modal view - global application settings accessible via comma key

use crate::ui::components::{Modal, ModalSize};
use crate::ui::modal::{AiSetting, AppSettingsState, AppearanceSetting, GeneralSetting, ModelBrowserState, SettingsTab, VoiceBrowserState};
use crate::ui::{theme, App};
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

/// Render the app settings modal
pub fn render_app_settings(f: &mut Frame, app: &App, area: Rect, state: &AppSettingsState) {
    let (popup_area, content_area) = Modal::new("Settings")
        .size(ModalSize {
            max_width: 80,
            height_percent: 0.7,
        })
        .render_frame(f, area);

    // Content area with horizontal padding
    let remaining = Rect::new(
        content_area.x + 2,
        content_area.y,
        content_area.width.saturating_sub(4),
        content_area.height,
    );

    // Render tab bar at top
    render_tab_bar(f, remaining, state);

    // Render content based on active tab
    let content_y = remaining.y + 2;
    let content_area = Rect::new(
        remaining.x,
        content_y,
        remaining.width,
        remaining.height.saturating_sub(4), // Reserve space for tabs and hints
    );

    match state.current_tab {
        SettingsTab::General => render_general_tab(f, app, content_area, state),
        SettingsTab::AI => render_ai_tab(f, content_area, state),
        SettingsTab::Appearance => render_appearance_tab(f, app, content_area, state),
    };

    // Hints at bottom
    render_hints(f, popup_area, state);
}

/// Render the tab bar
fn render_tab_bar(f: &mut Frame, area: Rect, state: &AppSettingsState) {
    let tab_area = Rect::new(area.x, area.y, area.width, 1);

    let mut tab_spans = vec![];
    for (i, tab) in SettingsTab::ALL.iter().enumerate() {
        if i > 0 {
            tab_spans.push(Span::styled(" │ ", Style::default().fg(theme::TEXT_MUTED)));
        }

        let is_active = *tab == state.current_tab;
        let style = if is_active {
            Style::default()
                .fg(theme::ACCENT_PRIMARY)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::TEXT_MUTED)
        };

        tab_spans.push(Span::styled(tab.label(), style));
    }

    let tabs_line = Line::from(tab_spans);
    let tabs_widget = Paragraph::new(tabs_line);
    f.render_widget(tabs_widget, tab_area);
}

/// Render General tab content
fn render_general_tab(f: &mut Frame, app: &App, area: Rect, state: &AppSettingsState) {
    // Section header: Trace Viewer
    let header_area = Rect::new(area.x, area.y, area.width, 1);
    let header = Paragraph::new(Line::from(vec![Span::styled(
        "Trace Viewer",
        Style::default()
            .fg(theme::ACCENT_WARNING)
            .add_modifier(Modifier::ITALIC),
    )]));
    f.render_widget(header, header_area);

    // Jaeger endpoint setting row
    let row_y = area.y + 2;
    let row_area = Rect::new(area.x, row_y, area.width, 3);

    let is_selected = state.current_tab == SettingsTab::General
        && state.selected_general_setting() == Some(GeneralSetting::JaegerEndpoint);

    // Left border indicator
    let border_char = if is_selected { "▌" } else { "│" };
    let border_color = if is_selected {
        theme::ACCENT_PRIMARY
    } else {
        theme::TEXT_MUTED
    };

    let mut spans = vec![Span::styled(border_char, Style::default().fg(border_color))];

    // Label
    let label_style = if is_selected {
        Style::default()
            .fg(theme::TEXT_PRIMARY)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::TEXT_MUTED)
    };
    spans.push(Span::styled(" Jaeger Endpoint: ", label_style));

    // Value (editable)
    if state.editing_jaeger_endpoint() {
        // Show input with cursor
        let input = &state.jaeger_endpoint_input;
        spans.push(Span::styled(
            format!("{}_", input),
            Style::default()
                .fg(theme::ACCENT_PRIMARY)
                .add_modifier(Modifier::UNDERLINED),
        ));
    } else {
        let current_value = app.preferences.borrow().jaeger_endpoint().to_string();
        spans.push(Span::styled(
            current_value,
            Style::default().fg(theme::ACCENT_SPECIAL),
        ));
    }

    let row = Paragraph::new(Line::from(spans)).block(Block::default().borders(Borders::NONE));
    f.render_widget(row, row_area);

    // Description below
    let desc_area = Rect::new(area.x + 2, row_y + 1, area.width.saturating_sub(2), 1);
    let desc = Paragraph::new("URL for opening trace links (e.g., http://localhost:16686)")
        .style(Style::default().fg(theme::TEXT_DIM));
    f.render_widget(desc, desc_area);
}

/// Render AI tab content
fn render_ai_tab(f: &mut Frame, area: Rect, state: &AppSettingsState) {
    // Voice browser overlay takes over the entire tab area
    if let Some(ref browser) = state.voice_browser {
        render_voice_browser(f, area, browser);
        return;
    }

    // Model browser overlay takes over the entire tab area
    if let Some(ref browser) = state.model_browser {
        render_model_browser(f, area, browser);
        return;
    }

    let mut y_offset = area.y;

    // Section header: API Keys
    render_section_header(f, area.x, y_offset, area.width, "API Keys");
    y_offset += 2;

    // 1. ElevenLabs API Key
    let is_elevenlabs_selected = state.selected_ai_setting() == Some(AiSetting::ElevenLabsApiKey);
    render_api_key_row(
        f,
        area.x,
        y_offset,
        area.width,
        "ElevenLabs API Key:",
        "API key for ElevenLabs voice synthesis (Enter to set, Delete to clear)",
        &state.ai.elevenlabs_key_input,
        is_elevenlabs_selected,
        state.editing_elevenlabs_key(),
        state.ai.elevenlabs_key_exists,
    );
    y_offset += 3;

    // 2. OpenRouter API Key
    let is_openrouter_selected = state.selected_ai_setting() == Some(AiSetting::OpenRouterApiKey);
    render_api_key_row(
        f,
        area.x,
        y_offset,
        area.width,
        "OpenRouter API Key:",
        "API key for OpenRouter LLM access (Enter to set, Delete to clear)",
        &state.ai.openrouter_key_input,
        is_openrouter_selected,
        state.editing_openrouter_key(),
        state.ai.openrouter_key_exists,
    );
    y_offset += 3;

    // Section header: Audio Notifications
    render_section_header(f, area.x, y_offset, area.width, "Audio Notifications");
    y_offset += 2;

    // 3. Audio Enabled toggle
    let is_audio_selected = state.selected_ai_setting() == Some(AiSetting::AudioEnabled);
    render_toggle_row(
        f,
        area.x,
        y_offset,
        area.width,
        "Audio Notifications:",
        "Enable/disable AI audio notifications (Enter to toggle)",
        state.ai.audio_enabled,
        is_audio_selected,
    );
    y_offset += 3;

    // 4. Selected Voice IDs
    let is_voices_selected = state.selected_ai_setting() == Some(AiSetting::SelectedVoiceIds);
    render_text_setting_row(
        f,
        area.x,
        y_offset,
        area.width,
        "Voice IDs:",
        "Enter to browse voices · or edit IDs manually",
        &state.ai.voice_ids_input,
        is_voices_selected,
        state.editing_voice_ids(),
    );
    y_offset += 3;

    // 5. OpenRouter Model
    let is_model_selected = state.selected_ai_setting() == Some(AiSetting::OpenRouterModel);
    render_text_setting_row(
        f,
        area.x,
        y_offset,
        area.width,
        "OpenRouter Model:",
        "Enter to browse models · or edit ID manually",
        &state.ai.openrouter_model_input,
        is_model_selected,
        state.editing_openrouter_model(),
    );
    y_offset += 3;

    // 6. Audio Prompt
    let is_prompt_selected = state.selected_ai_setting() == Some(AiSetting::AudioPrompt);
    render_text_setting_row(
        f,
        area.x,
        y_offset,
        area.width,
        "Audio Prompt:",
        "System prompt for making text audio-friendly",
        &state.ai.audio_prompt_input,
        is_prompt_selected,
        state.editing_audio_prompt(),
    );
}

/// Render Appearance tab content
fn render_appearance_tab(f: &mut Frame, app: &App, area: Rect, state: &AppSettingsState) {
    let mut y_offset = area.y;

    // Section header: Filters
    render_section_header(f, area.x, y_offset, area.width, "Filters");
    y_offset += 2;

    // 1. Time Filter (select field cycling through options)
    let is_time_filter_selected = state.selected_appearance_setting() == Some(AppearanceSetting::TimeFilter);
    let time_filter_label = app.home.time_filter
        .map(|tf| tf.label())
        .unwrap_or("All");
    render_select_field(
        f,
        area.x,
        y_offset,
        area.width,
        "Time Filter:",
        "Filter conversations by time (Enter to cycle)",
        time_filter_label,
        is_time_filter_selected,
    );
    y_offset += 3;

    // 2. Hide Scheduled toggle
    let is_hide_scheduled_selected = state.selected_appearance_setting() == Some(AppearanceSetting::HideScheduled);
    render_toggle_row(
        f,
        area.x,
        y_offset,
        area.width,
        "Hide Scheduled:",
        "Hide scheduled/future events from lists (Enter to toggle)",
        app.hide_scheduled,
        is_hide_scheduled_selected,
    );
}

/// Render a select field (read-only value with cycling)
fn render_select_field(
    f: &mut Frame,
    x: u16,
    y: u16,
    width: u16,
    label: &str,
    description: &str,
    value: &str,
    is_selected: bool,
) {
    let row_area = Rect::new(x, y, width, 1);

    let border_char = if is_selected { "▌" } else { "│" };
    let border_color = if is_selected {
        theme::ACCENT_PRIMARY
    } else {
        theme::TEXT_MUTED
    };

    let mut spans = vec![Span::styled(border_char, Style::default().fg(border_color))];

    let label_style = if is_selected {
        Style::default()
            .fg(theme::TEXT_PRIMARY)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::TEXT_MUTED)
    };
    spans.push(Span::styled(format!(" {}", label), label_style));

    // Display value with special styling
    spans.push(Span::styled(
        format!(" [{}]", value),
        Style::default().fg(theme::ACCENT_SPECIAL),
    ));

    let row = Paragraph::new(Line::from(spans)).block(Block::default().borders(Borders::NONE));
    f.render_widget(row, row_area);

    let desc_area = Rect::new(x + 2, y + 1, width.saturating_sub(2), 1);
    let desc = Paragraph::new(description).style(Style::default().fg(theme::TEXT_DIM));
    f.render_widget(desc, desc_area);
}

/// Render a section header
fn render_section_header(f: &mut Frame, x: u16, y: u16, width: u16, title: &str) {
    let header_area = Rect::new(x, y, width, 1);
    let header = Paragraph::new(Line::from(vec![Span::styled(
        title,
        Style::default()
            .fg(theme::ACCENT_WARNING)
            .add_modifier(Modifier::ITALIC),
    )]));
    f.render_widget(header, header_area);
}

/// Render a toggle (ON/OFF) row
fn render_toggle_row(
    f: &mut Frame,
    x: u16,
    y: u16,
    width: u16,
    label: &str,
    description: &str,
    enabled: bool,
    is_selected: bool,
) {
    let row_area = Rect::new(x, y, width, 1);

    let border_char = if is_selected { "▌" } else { "│" };
    let border_color = if is_selected {
        theme::ACCENT_PRIMARY
    } else {
        theme::TEXT_MUTED
    };

    let mut spans = vec![Span::styled(border_char, Style::default().fg(border_color))];

    let label_style = if is_selected {
        Style::default()
            .fg(theme::TEXT_PRIMARY)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::TEXT_MUTED)
    };
    spans.push(Span::styled(format!(" {}", label), label_style));

    let (toggle_text, toggle_color) = if enabled {
        (" [ON]", theme::ACCENT_SUCCESS)
    } else {
        (" [OFF]", theme::TEXT_DIM)
    };
    spans.push(Span::styled(
        toggle_text,
        Style::default().fg(toggle_color).add_modifier(Modifier::BOLD),
    ));

    let row = Paragraph::new(Line::from(spans)).block(Block::default().borders(Borders::NONE));
    f.render_widget(row, row_area);

    let desc_area = Rect::new(x + 2, y + 1, width.saturating_sub(2), 1);
    let desc = Paragraph::new(description).style(Style::default().fg(theme::TEXT_DIM));
    f.render_widget(desc, desc_area);
}

/// Render a text setting row (non-secret, shows actual value)
fn render_text_setting_row(
    f: &mut Frame,
    x: u16,
    y: u16,
    width: u16,
    label: &str,
    description: &str,
    value: &str,
    is_selected: bool,
    is_editing: bool,
) {
    let row_area = Rect::new(x, y, width, 1);

    let border_char = if is_selected { "▌" } else { "│" };
    let border_color = if is_selected {
        theme::ACCENT_PRIMARY
    } else {
        theme::TEXT_MUTED
    };

    let mut spans = vec![Span::styled(border_char, Style::default().fg(border_color))];

    let label_style = if is_selected {
        Style::default()
            .fg(theme::TEXT_PRIMARY)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::TEXT_MUTED)
    };
    spans.push(Span::styled(format!(" {}", label), label_style));

    if is_editing {
        spans.push(Span::styled(
            format!(" {}_", value),
            Style::default()
                .fg(theme::ACCENT_PRIMARY)
                .add_modifier(Modifier::UNDERLINED),
        ));
    } else {
        let display = if value.is_empty() {
            "(not set)"
        } else {
            value
        };
        // Truncate long values for display
        let max_len = width.saturating_sub(label.len() as u16 + 5) as usize;
        let truncated = if display.len() > max_len && max_len > 3 {
            format!("{}...", &display[..max_len - 3])
        } else {
            display.to_string()
        };
        spans.push(Span::styled(
            format!(" {}", truncated),
            Style::default().fg(theme::ACCENT_SPECIAL),
        ));
    }

    let row = Paragraph::new(Line::from(spans)).block(Block::default().borders(Borders::NONE));
    f.render_widget(row, row_area);

    let desc_area = Rect::new(x + 2, y + 1, width.saturating_sub(2), 1);
    let desc = Paragraph::new(description).style(Style::default().fg(theme::TEXT_DIM));
    f.render_widget(desc, desc_area);
}

/// Helper to render an API key row with masked display (always masked, even during edit)
fn render_api_key_row(
    f: &mut Frame,
    x: u16,
    y: u16,
    width: u16,
    label: &str,
    description: &str,
    key_input: &str,
    is_selected: bool,
    is_editing: bool,
    key_exists_in_storage: bool,
) {
    let row_area = Rect::new(x, y, width, 1);

    // Left border indicator
    let border_char = if is_selected { "▌" } else { "│" };
    let border_color = if is_selected {
        theme::ACCENT_PRIMARY
    } else {
        theme::TEXT_MUTED
    };

    let mut spans = vec![Span::styled(border_char, Style::default().fg(border_color))];

    // Label
    let label_style = if is_selected {
        Style::default()
            .fg(theme::TEXT_PRIMARY)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::TEXT_MUTED)
    };
    spans.push(Span::styled(format!(" {}", label), label_style));

    // Value - ALWAYS masked for security
    if is_editing {
        // Show masked input with cursor (bullet points for each character)
        let masked: String = "•".repeat(key_input.len());
        spans.push(Span::styled(
            format!(" {}_", masked),
            Style::default()
                .fg(theme::ACCENT_PRIMARY)
                .add_modifier(Modifier::UNDERLINED),
        ));
    } else {
        // Show status when not editing - check both input and secure storage
        let display_value = if key_exists_in_storage {
            "••••••••"
        } else {
            "(not set)"
        };
        spans.push(Span::styled(
            format!(" {}", display_value),
            Style::default().fg(theme::ACCENT_SPECIAL),
        ));
    }

    let row = Paragraph::new(Line::from(spans)).block(Block::default().borders(Borders::NONE));
    f.render_widget(row, row_area);

    // Description below
    let desc_area = Rect::new(x + 2, y + 1, width.saturating_sub(2), 1);
    let desc = Paragraph::new(description).style(Style::default().fg(theme::TEXT_DIM));
    f.render_widget(desc, desc_area);
}

/// Render hints at bottom of modal
fn render_hints(f: &mut Frame, popup_area: Rect, state: &AppSettingsState) {
    let hints_area = Rect::new(
        popup_area.x + 2,
        popup_area.y + popup_area.height.saturating_sub(2),
        popup_area.width.saturating_sub(4),
        1,
    );

    // Voice browser hints
    if state.voice_browser.is_some() {
        let hint_spans = vec![
            Span::styled("↑↓", Style::default().fg(theme::ACCENT_WARNING)),
            Span::styled(" navigate", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled("Space", Style::default().fg(theme::ACCENT_WARNING)),
            Span::styled(" toggle", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled("Enter", Style::default().fg(theme::ACCENT_WARNING)),
            Span::styled(" confirm", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled("Esc", Style::default().fg(theme::ACCENT_WARNING)),
            Span::styled(" cancel", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled("type to filter", Style::default().fg(theme::TEXT_DIM)),
        ];
        f.render_widget(Paragraph::new(Line::from(hint_spans)), hints_area);
        return;
    }

    // Model browser hints
    if state.model_browser.is_some() {
        let hint_spans = vec![
            Span::styled("↑↓", Style::default().fg(theme::ACCENT_WARNING)),
            Span::styled(" navigate", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled("Enter", Style::default().fg(theme::ACCENT_WARNING)),
            Span::styled(" select", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled("Esc", Style::default().fg(theme::ACCENT_WARNING)),
            Span::styled(" cancel", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled("type to filter", Style::default().fg(theme::TEXT_DIM)),
        ];
        f.render_widget(Paragraph::new(Line::from(hint_spans)), hints_area);
        return;
    }

    let hint_spans = if state.editing {
        vec![
            Span::styled("Enter", Style::default().fg(theme::ACCENT_WARNING)),
            Span::styled(" save", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled("Esc", Style::default().fg(theme::ACCENT_WARNING)),
            Span::styled(" cancel", Style::default().fg(theme::TEXT_MUTED)),
        ]
    } else {
        // Build base hints with tab-specific Enter behavior
        let mut hints = vec![
            Span::styled("Tab", Style::default().fg(theme::ACCENT_WARNING)),
            Span::styled(" switch tab", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled("↑↓", Style::default().fg(theme::ACCENT_WARNING)),
            Span::styled(" navigate", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled("Enter", Style::default().fg(theme::ACCENT_WARNING)),
        ];

        // Context-aware Enter behavior hint
        if state.current_tab == SettingsTab::Appearance {
            // Appearance tab uses toggle/cycle, not edit mode
            match state.selected_appearance_setting() {
                Some(AppearanceSetting::TimeFilter) => {
                    hints.push(Span::styled(" cycle", Style::default().fg(theme::TEXT_MUTED)));
                }
                Some(AppearanceSetting::HideScheduled) => {
                    hints.push(Span::styled(" toggle", Style::default().fg(theme::TEXT_MUTED)));
                }
                None => {
                    hints.push(Span::styled(" select", Style::default().fg(theme::TEXT_MUTED)));
                }
            }
        } else {
            hints.push(Span::styled(" edit", Style::default().fg(theme::TEXT_MUTED)));
        }

        // Show Delete hint on AI tab for clearable settings
        if state.current_tab == SettingsTab::AI {
            match state.selected_ai_setting() {
                Some(AiSetting::ElevenLabsApiKey)
                | Some(AiSetting::OpenRouterApiKey)
                | Some(AiSetting::SelectedVoiceIds)
                | Some(AiSetting::OpenRouterModel) => {
                    hints.push(Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)));
                    hints.push(Span::styled("Del", Style::default().fg(theme::ACCENT_WARNING)));
                    hints.push(Span::styled(" clear", Style::default().fg(theme::TEXT_MUTED)));
                }
                _ => {}
            }
        }

        hints.push(Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)));
        hints.push(Span::styled("Esc", Style::default().fg(theme::ACCENT_WARNING)));
        hints.push(Span::styled(" close", Style::default().fg(theme::TEXT_MUTED)));

        hints
    };

    let hints = Paragraph::new(Line::from(hint_spans));
    f.render_widget(hints, hints_area);
}

/// Render the voice browser overlay
fn render_voice_browser(f: &mut Frame, area: Rect, browser: &VoiceBrowserState) {
    // Header: title + selected count
    let selected_count = browser.selected_voice_ids.len();
    let header_area = Rect::new(area.x, area.y, area.width, 1);
    let header = Paragraph::new(Line::from(vec![
        Span::styled(
            "Select Voices",
            Style::default()
                .fg(theme::ACCENT_WARNING)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("  ({} selected)", selected_count),
            Style::default().fg(theme::TEXT_MUTED),
        ),
    ]));
    f.render_widget(header, header_area);

    // Search input
    let search_area = Rect::new(area.x, area.y + 1, area.width, 1);
    let search_display = if browser.filter.is_empty() {
        Span::styled("Type to filter...", Style::default().fg(theme::TEXT_DIM))
    } else {
        Span::styled(
            format!("Filter: {}_", browser.filter),
            Style::default().fg(theme::ACCENT_PRIMARY),
        )
    };
    f.render_widget(Paragraph::new(Line::from(vec![search_display])), search_area);

    // Loading state
    if browser.loading {
        let loading_area = Rect::new(area.x, area.y + 3, area.width, 1);
        let loading = Paragraph::new("Loading voices...")
            .style(Style::default().fg(theme::TEXT_MUTED).add_modifier(Modifier::ITALIC));
        f.render_widget(loading, loading_area);
        return;
    }

    // Voice list
    let list_y = area.y + 3;
    let list_height = area.height.saturating_sub(3) as usize;
    let filtered = browser.filtered_items();

    if filtered.is_empty() {
        let empty_area = Rect::new(area.x, list_y, area.width, 1);
        let msg = if browser.filter.is_empty() { "No voices available" } else { "No matching voices" };
        f.render_widget(
            Paragraph::new(msg).style(Style::default().fg(theme::TEXT_DIM)),
            empty_area,
        );
        return;
    }

    // Compute scroll window
    let scroll_offset = browser.scroll_offset.min(filtered.len().saturating_sub(1));
    let visible_end = (scroll_offset + list_height).min(filtered.len());

    for (i, item) in filtered[scroll_offset..visible_end].iter().enumerate() {
        let abs_index = scroll_offset + i;
        let row_y = list_y + i as u16;
        if row_y >= area.y + area.height {
            break;
        }
        let row_area = Rect::new(area.x, row_y, area.width, 1);

        let is_selected = abs_index == browser.selected_index;
        let is_checked = browser.selected_voice_ids.contains(&item.voice_id);

        let checkbox = if is_checked { "[x] " } else { "[ ] " };
        let checkbox_color = if is_checked { theme::ACCENT_SUCCESS } else { theme::TEXT_DIM };

        let cursor = if is_selected { ">" } else { " " };
        let cursor_color = if is_selected { theme::ACCENT_PRIMARY } else { theme::TEXT_DIM };

        let name_style = if is_selected {
            Style::default().fg(theme::TEXT_PRIMARY).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::TEXT_PRIMARY)
        };

        let mut spans = vec![
            Span::styled(cursor, Style::default().fg(cursor_color)),
            Span::styled(checkbox, Style::default().fg(checkbox_color)),
            Span::styled(&item.name, name_style),
        ];

        if let Some(ref cat) = item.category {
            spans.push(Span::styled(
                format!("  ({})", cat),
                Style::default().fg(theme::TEXT_DIM),
            ));
        }

        f.render_widget(Paragraph::new(Line::from(spans)), row_area);
    }
}

/// Render the model browser overlay
fn render_model_browser(f: &mut Frame, area: Rect, browser: &ModelBrowserState) {
    // Header
    let header_area = Rect::new(area.x, area.y, area.width, 1);
    let current = browser.selected_model_id.as_deref().unwrap_or("none");
    let header = Paragraph::new(Line::from(vec![
        Span::styled(
            "Select Model",
            Style::default()
                .fg(theme::ACCENT_WARNING)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("  (current: {})", current),
            Style::default().fg(theme::TEXT_MUTED),
        ),
    ]));
    f.render_widget(header, header_area);

    // Search input
    let search_area = Rect::new(area.x, area.y + 1, area.width, 1);
    let search_display = if browser.filter.is_empty() {
        Span::styled("Type to filter...", Style::default().fg(theme::TEXT_DIM))
    } else {
        Span::styled(
            format!("Filter: {}_", browser.filter),
            Style::default().fg(theme::ACCENT_PRIMARY),
        )
    };
    f.render_widget(Paragraph::new(Line::from(vec![search_display])), search_area);

    // Loading state
    if browser.loading {
        let loading_area = Rect::new(area.x, area.y + 3, area.width, 1);
        let loading = Paragraph::new("Loading models...")
            .style(Style::default().fg(theme::TEXT_MUTED).add_modifier(Modifier::ITALIC));
        f.render_widget(loading, loading_area);
        return;
    }

    // Model list
    let list_y = area.y + 3;
    let list_height = area.height.saturating_sub(3) as usize;
    let filtered = browser.filtered_items();

    if filtered.is_empty() {
        let empty_area = Rect::new(area.x, list_y, area.width, 1);
        let msg = if browser.filter.is_empty() { "No models available" } else { "No matching models" };
        f.render_widget(
            Paragraph::new(msg).style(Style::default().fg(theme::TEXT_DIM)),
            empty_area,
        );
        return;
    }

    // Compute scroll window
    let scroll_offset = browser.scroll_offset.min(filtered.len().saturating_sub(1));
    let visible_end = (scroll_offset + list_height).min(filtered.len());

    for (i, item) in filtered[scroll_offset..visible_end].iter().enumerate() {
        let abs_index = scroll_offset + i;
        let row_y = list_y + i as u16;
        if row_y >= area.y + area.height {
            break;
        }
        let row_area = Rect::new(area.x, row_y, area.width, 1);

        let is_selected = abs_index == browser.selected_index;
        let is_current = browser.selected_model_id.as_deref() == Some(&item.id);

        let radio = if is_current { "(o) " } else { "( ) " };
        let radio_color = if is_current { theme::ACCENT_SUCCESS } else { theme::TEXT_DIM };

        let cursor = if is_selected { ">" } else { " " };
        let cursor_color = if is_selected { theme::ACCENT_PRIMARY } else { theme::TEXT_DIM };

        let name_style = if is_selected {
            Style::default().fg(theme::TEXT_PRIMARY).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::TEXT_PRIMARY)
        };

        let display_name = item.name.as_deref().unwrap_or(&item.id);

        let mut spans = vec![
            Span::styled(cursor, Style::default().fg(cursor_color)),
            Span::styled(radio, Style::default().fg(radio_color)),
            Span::styled(display_name, name_style),
        ];

        // Show context length if available
        if let Some(ctx_len) = item.context_length {
            let ctx_display = if ctx_len >= 1_000_000 {
                format!("  {}M ctx", ctx_len / 1_000_000)
            } else if ctx_len >= 1_000 {
                format!("  {}K ctx", ctx_len / 1_000)
            } else {
                format!("  {} ctx", ctx_len)
            };
            spans.push(Span::styled(ctx_display, Style::default().fg(theme::TEXT_DIM)));
        }

        f.render_widget(Paragraph::new(Line::from(spans)), row_area);
    }
}
