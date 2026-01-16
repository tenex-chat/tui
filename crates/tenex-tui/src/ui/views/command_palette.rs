use crate::ui::components::{render_modal_sections, Modal, ModalItem, ModalSection, ModalSize};
use crate::ui::modal::CommandPaletteState;
use ratatui::{layout::Rect, Frame};
use std::collections::BTreeMap;

/// Render the command palette modal (Ctrl+T)
pub fn render_command_palette(f: &mut Frame, area: Rect, state: &CommandPaletteState) {
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

    Modal::new("Commands")
        .size(ModalSize {
            max_width: 50,
            height_percent: 0.6,
        })
        .render(f, area, |f, content_area| {
            render_modal_sections(f, content_area, &sections);
        });
}
