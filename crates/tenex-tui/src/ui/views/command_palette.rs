use crate::ui::components::{
    modal_area, render_modal_background, render_modal_header, render_modal_overlay,
    render_modal_search, render_modal_sections, ModalItem, ModalSection, ModalSize,
};
use crate::ui::modal::CommandPaletteState;
use ratatui::{layout::Rect, Frame};
use std::collections::BTreeMap;

/// Render the command palette modal (Ctrl+T)
pub fn render_command_palette(f: &mut Frame, area: Rect, state: &CommandPaletteState) {
    render_modal_overlay(f, area);

    let size = ModalSize {
        max_width: 50,
        height_percent: 0.6,
    };

    let popup_area = modal_area(area, &size);
    render_modal_background(f, popup_area);

    // Inner area with vertical padding
    let inner_area = Rect::new(
        popup_area.x,
        popup_area.y + 1,
        popup_area.width,
        popup_area.height.saturating_sub(2),
    );

    let remaining = render_modal_header(f, inner_area, "Commands", "esc");
    let remaining = render_modal_search(f, remaining, &state.filter, "Type to filter...");

    // Get available commands and group by section
    let commands = state.available_commands();

    // Group commands by section (preserving order with BTreeMap)
    let mut sections_map: BTreeMap<&str, Vec<(&char, &str)>> = BTreeMap::new();
    for cmd in &commands {
        sections_map
            .entry(cmd.section)
            .or_default()
            .push((&cmd.key, cmd.label));
    }

    // Build sections for rendering
    let mut sections = Vec::new();
    let mut item_idx = 0usize;

    for (section_name, items) in &sections_map {
        let mut section = ModalSection::new(*section_name);
        let mut section_items = Vec::new();

        for (key, label) in items {
            let shortcut = if **key == ' ' {
                "Space".to_string()
            } else {
                key.to_string()
            };

            let item = ModalItem::new(*label)
                .with_shortcut(shortcut)
                .selected(item_idx == state.selected_index);

            section_items.push(item);
            item_idx += 1;
        }

        section = section.with_items(section_items);
        sections.push(section);
    }

    render_modal_sections(f, remaining, &sections);
}
