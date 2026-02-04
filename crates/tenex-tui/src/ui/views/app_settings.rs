//! App Settings modal view - global application settings accessible via comma key

use crate::ui::components::{Modal, ModalSize};
use crate::ui::modal::{AiSetting, AppSettingsState, GeneralSetting, SettingsTab};
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
    }

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

/// Render AI tab content - all 6 AI audio settings
fn render_ai_tab(f: &mut Frame, area: Rect, state: &AppSettingsState) {
    let mut y_offset = area.y;

    // Section header: Audio Notifications
    let header_area = Rect::new(area.x, y_offset, area.width, 1);
    let header = Paragraph::new(Line::from(vec![Span::styled(
        "Audio Notifications",
        Style::default()
            .fg(theme::ACCENT_WARNING)
            .add_modifier(Modifier::ITALIC),
    )]));
    f.render_widget(header, header_area);
    y_offset += 2;

    // Note: AI audio settings are fetched from secure storage and preferences
    // For now, we show placeholder values. Future: fetch when modal opens and cache in state

    // 1. Enabled toggle
    let is_enabled_selected = state.current_tab == SettingsTab::AI
        && state.selected_ai_setting() == Some(AiSetting::Enabled);
    render_setting_row(
        f,
        area.x,
        y_offset,
        area.width,
        "Enabled:",
        "(not yet implemented)",
        is_enabled_selected,
        false,
    );
    y_offset += 3;

    // Section header: API Keys
    let header_area = Rect::new(area.x, y_offset, area.width, 1);
    let header = Paragraph::new(Line::from(vec![Span::styled(
        "API Keys",
        Style::default()
            .fg(theme::ACCENT_WARNING)
            .add_modifier(Modifier::ITALIC),
    )]));
    f.render_widget(header, header_area);
    y_offset += 2;

    // Check secure storage for existing keys
    let elevenlabs_key_exists =
        tenex_core::SecureStorage::exists(tenex_core::SecureKey::ElevenLabsApiKey);
    let openrouter_key_exists =
        tenex_core::SecureStorage::exists(tenex_core::SecureKey::OpenRouterApiKey);

    // 2. ElevenLabs API Key
    let is_elevenlabs_selected = state.current_tab == SettingsTab::AI
        && state.selected_ai_setting() == Some(AiSetting::ElevenLabsApiKey);
    render_api_key_row(
        f,
        area.x,
        y_offset,
        area.width,
        "ElevenLabs API Key:",
        "API key for ElevenLabs voice synthesis",
        &state.ai.elevenlabs_key_input,
        is_elevenlabs_selected,
        state.editing_elevenlabs_key(),
        elevenlabs_key_exists,
    );
    y_offset += 3;

    // 3. OpenRouter API Key
    let is_openrouter_selected = state.current_tab == SettingsTab::AI
        && state.selected_ai_setting() == Some(AiSetting::OpenRouterApiKey);
    render_api_key_row(
        f,
        area.x,
        y_offset,
        area.width,
        "OpenRouter API Key:",
        "API key for OpenRouter LLM access",
        &state.ai.openrouter_key_input,
        is_openrouter_selected,
        state.editing_openrouter_key(),
        openrouter_key_exists,
    );
    y_offset += 3;

    // Section header: Voice & Model Configuration
    let header_area = Rect::new(area.x, y_offset, area.width, 1);
    let header = Paragraph::new(Line::from(vec![Span::styled(
        "Voice & Model Configuration",
        Style::default()
            .fg(theme::ACCENT_WARNING)
            .add_modifier(Modifier::ITALIC),
    )]));
    f.render_widget(header, header_area);
    y_offset += 2;

    // 4. Selected Voices
    let is_voices_selected = state.current_tab == SettingsTab::AI
        && state.selected_ai_setting() == Some(AiSetting::SelectedVoices);
    render_setting_row(
        f,
        area.x,
        y_offset,
        area.width,
        "Selected Voices:",
        "(not yet implemented)",
        is_voices_selected,
        false,
    );
    y_offset += 3;

    // 5. OpenRouter Model
    let is_model_selected = state.current_tab == SettingsTab::AI
        && state.selected_ai_setting() == Some(AiSetting::OpenRouterModel);
    render_setting_row(
        f,
        area.x,
        y_offset,
        area.width,
        "LLM Model:",
        "(not yet implemented)",
        is_model_selected,
        false,
    );
    y_offset += 3;

    // 6. Audio Prompt
    let is_prompt_selected = state.current_tab == SettingsTab::AI
        && state.selected_ai_setting() == Some(AiSetting::AudioPrompt);
    render_setting_row(
        f,
        area.x,
        y_offset,
        area.width,
        "Prompt Template:",
        "(not yet implemented)",
        is_prompt_selected,
        false,
    );
}

/// Helper to render a simple read-only setting row
fn render_setting_row(
    f: &mut Frame,
    x: u16,
    y: u16,
    width: u16,
    label: &str,
    value: &str,
    is_selected: bool,
    _is_editing: bool,
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

    // Value
    spans.push(Span::styled(
        format!(" {}", value),
        Style::default().fg(theme::ACCENT_SPECIAL),
    ));

    let row = Paragraph::new(Line::from(spans)).block(Block::default().borders(Borders::NONE));
    f.render_widget(row, row_area);
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

    let hint_spans = if state.editing {
        vec![
            Span::styled("Enter", Style::default().fg(theme::ACCENT_WARNING)),
            Span::styled(" save", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled("Esc", Style::default().fg(theme::ACCENT_WARNING)),
            Span::styled(" cancel", Style::default().fg(theme::TEXT_MUTED)),
        ]
    } else {
        vec![
            Span::styled("Tab", Style::default().fg(theme::ACCENT_WARNING)),
            Span::styled(" switch tab", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled("↑↓", Style::default().fg(theme::ACCENT_WARNING)),
            Span::styled(" navigate", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled("Enter", Style::default().fg(theme::ACCENT_WARNING)),
            Span::styled(" edit", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled("Esc", Style::default().fg(theme::ACCENT_WARNING)),
            Span::styled(" close", Style::default().fg(theme::TEXT_MUTED)),
        ]
    };

    let hints = Paragraph::new(Line::from(hint_spans));
    f.render_widget(hints, hints_area);
}
