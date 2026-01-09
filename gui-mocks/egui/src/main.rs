use eframe::egui::{self, Color32, Frame, RichText, ScrollArea, Stroke, Vec2};

const BG_APP: Color32 = Color32::from_rgb(18, 20, 24);
const BG_PANEL: Color32 = Color32::from_rgb(24, 26, 31);
const BG_CARD: Color32 = Color32::from_rgb(28, 31, 37);
const BG_CARD_HI: Color32 = Color32::from_rgb(34, 38, 46);
const TEXT_PRIMARY: Color32 = Color32::from_rgb(226, 230, 235);
const TEXT_MUTED: Color32 = Color32::from_rgb(150, 156, 168);
const TEXT_DIM: Color32 = Color32::from_rgb(114, 120, 134);
const ACCENT_BLUE: Color32 = Color32::from_rgb(79, 134, 255);
const ACCENT_GREEN: Color32 = Color32::from_rgb(62, 200, 124);
const ACCENT_YELLOW: Color32 = Color32::from_rgb(222, 168, 54);
const ACCENT_ORANGE: Color32 = Color32::from_rgb(222, 124, 66);

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size(Vec2::new(1500.0, 900.0)),
        ..Default::default()
    };

    eframe::run_native(
        "TENEX GUI Mock - egui",
        options,
        Box::new(|cc| Ok(Box::new(TenexMockApp::new(cc)))),
    )
}

struct TenexMockApp {
    columns: Vec<ColumnData>,
    sidebar_projects: Vec<ProjectItem>,
}

impl TenexMockApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut visuals = egui::Visuals::dark();
        visuals.override_text_color = Some(TEXT_PRIMARY);
        visuals.panel_fill = BG_APP;
        visuals.window_fill = BG_PANEL;
        visuals.faint_bg_color = BG_PANEL;
        cc.egui_ctx.set_visuals(visuals);

        Self {
            sidebar_projects: vec![
                ProjectItem::new("DDD", ACCENT_BLUE),
                ProjectItem::new("TENEX Management", ACCENT_GREEN),
                ProjectItem::new("TENEX Backend", ACCENT_ORANGE),
                ProjectItem::new("TENEX Web Svelte", ACCENT_YELLOW),
                ProjectItem::new("TENEX iOS Client", Color32::from_rgb(125, 102, 255)),
                ProjectItem::new("TENEX TUI Client", Color32::from_rgb(84, 172, 255)),
                ProjectItem::new("Agents", Color32::from_rgb(132, 117, 255)),
            ],
            columns: build_mock_columns(),
        }
    }
}

impl eframe::App for TenexMockApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("top_spacer")
            .exact_height(6.0)
            .show(ctx, |_| {});

        egui::SidePanel::left("sidebar")
            .resizable(false)
            .default_width(230.0)
            .show(ctx, |ui| {
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("TENEX").size(18.0).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new("88").size(12.0).color(TEXT_DIM));
                    });
                });
                ui.add_space(6.0);

                ui.horizontal(|ui| {
                    for color in [ACCENT_BLUE, ACCENT_GREEN, ACCENT_ORANGE] {
                        let (rect, _) = ui.allocate_exact_size(Vec2::new(14.0, 14.0), egui::Sense::hover());
                        ui.painter().rect_filled(rect, 3.0, color);
                    }
                });

                ui.add_space(14.0);
                ui.label(RichText::new("TENEX").size(12.0).color(TEXT_DIM));
                ui.add_space(8.0);

                for item in &self.sidebar_projects {
                    sidebar_item(ui, item);
                    ui.add_space(4.0);
                }

                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.add_space(12.0);
                    user_block(ui);
                });
            });

        egui::SidePanel::right("detail")
            .resizable(false)
            .default_width(360.0)
            .show(ctx, |ui| {
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("New Conversation").size(14.0).color(TEXT_MUTED));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        small_icon_button(ui, "...");
                    });
                });
                ui.add_space(10.0);

                ScrollArea::vertical().show(ui, |ui| {
                    ui.add_space(6.0);
                    ui.label(RichText::new("Executive Summary").size(18.0).strong());
                    ui.add_space(6.0);
                    ui.label(RichText::new("Recent conversation highlights and outcomes.").color(TEXT_MUTED));
                    ui.add_space(12.0);

                    section_title(ui, "Timeline");
                    ui.label(RichText::new("Jan 06 - Onboarded the web-tester agent").color(TEXT_MUTED));
                    ui.label(RichText::new("Jan 07 - First automation pass complete").color(TEXT_MUTED));
                    ui.add_space(10.0);

                    section_title(ui, "Key Details");
                    bullet(ui, "No-code access, uses browser automation.");
                    bullet(ui, "Delivers: manual test reports.");
                    bullet(ui, "Signals: outcome summaries and issues.");
                    ui.add_space(14.0);

                    section_title(ui, "Messages");
                    message_row(ui, "Pablo", "Can you confirm scope for this agent?");
                    message_row(ui, "Web Tester", "Confirmed: web UI only. No backend access.");
                    ui.add_space(10.0);
                });

                ui.add_space(8.0);
                Frame::none()
                    .fill(BG_CARD)
                    .rounding(8.0)
                    .stroke(Stroke::new(1.0, Color32::from_rgb(38, 42, 52)))
                    .show(ui, |ui| {
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Type a message...").color(TEXT_DIM));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                small_pill(ui, "Send", ACCENT_BLUE);
                            });
                        });
                        ui.add_space(6.0);
                    });
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(8.0);
            header_row(ui);
            ui.add_space(10.0);

            ui.columns(3, |cols| {
                for (idx, col) in self.columns.iter().enumerate() {
                    let accent = match idx {
                        0 => ACCENT_BLUE,
                        1 => ACCENT_GREEN,
                        _ => ACCENT_ORANGE,
                    };
                    column_panel(&mut cols[idx], col, accent);
                }
            });
        });
    }
}

fn header_row(ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("Inbox").size(16.0).strong());
        small_pill(ui, "17", ACCENT_ORANGE);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            small_pill(ui, "Filter", Color32::from_rgb(90, 92, 98));
        });
    });
}

fn column_panel(ui: &mut egui::Ui, col: &ColumnData, accent: Color32) {
    Frame::none()
        .fill(BG_PANEL)
        .rounding(10.0)
        .stroke(Stroke::new(1.0, Color32::from_rgb(32, 36, 44)))
        .show(ui, |ui| {
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                let title = RichText::new(&col.title).size(14.0).strong();
                ui.label(title);
                small_pill(ui, &col.count.to_string(), accent);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    small_icon_button(ui, "+");
                });
            });
            ui.add_space(8.0);

            ScrollArea::vertical().show(ui, |ui| {
                for item in &col.items {
                    card(ui, item);
                    ui.add_space(8.0);
                }
            });
            ui.add_space(6.0);
        });
}

fn card(ui: &mut egui::Ui, item: &CardData) {
    Frame::none()
        .fill(BG_CARD)
        .rounding(8.0)
        .stroke(Stroke::new(1.0, Color32::from_rgb(38, 42, 52)))
        .show(ui, |ui| {
            ui.add_space(8.0);
            ui.label(RichText::new(&item.title).strong());
            ui.label(RichText::new(&item.subtitle).size(12.0).color(TEXT_MUTED));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                status_pill(ui, item.status);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    small_pill(ui, &item.time, Color32::from_rgb(88, 94, 106));
                });
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                for badge in &item.badges {
                    small_pill(ui, badge, Color32::from_rgb(66, 70, 82));
                }
            });
            ui.add_space(6.0);
        });
}

fn sidebar_item(ui: &mut egui::Ui, item: &ProjectItem) {
    Frame::none()
        .fill(BG_CARD)
        .rounding(8.0)
        .stroke(Stroke::new(1.0, Color32::from_rgb(38, 42, 52)))
        .show(ui, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(Vec2::new(14.0, 14.0), egui::Sense::hover());
                ui.painter().rect_filled(rect, 3.0, item.color);
                ui.label(RichText::new(&item.name).size(12.0));
            });
            ui.add_space(6.0);
        });
}

fn user_block(ui: &mut egui::Ui) {
    Frame::none()
        .fill(BG_PANEL)
        .rounding(8.0)
        .stroke(Stroke::new(1.0, Color32::from_rgb(38, 42, 52)))
        .show(ui, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(Vec2::new(22.0, 22.0), egui::Sense::hover());
                ui.painter().rect_filled(rect, 11.0, ACCENT_BLUE);
                ui.label(RichText::new("Pablo Testing Pubkey").size(12.0));
            });
            ui.add_space(6.0);
        });
}

fn section_title(ui: &mut egui::Ui, title: &str) {
    ui.label(RichText::new(title).size(13.0).color(TEXT_DIM));
    ui.add_space(4.0);
}

fn bullet(ui: &mut egui::Ui, text: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("-"));
        ui.label(RichText::new(text).color(TEXT_MUTED));
    });
}

fn message_row(ui: &mut egui::Ui, name: &str, text: &str) {
    Frame::none()
        .fill(BG_CARD)
        .rounding(8.0)
        .stroke(Stroke::new(1.0, Color32::from_rgb(38, 42, 52)))
        .show(ui, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(Vec2::new(18.0, 18.0), egui::Sense::hover());
                ui.painter().rect_filled(rect, 9.0, ACCENT_GREEN);
                ui.label(RichText::new(name).size(12.0).strong());
            });
            ui.label(RichText::new(text).size(12.0).color(TEXT_MUTED));
            ui.add_space(6.0);
        });
}

fn status_pill(ui: &mut egui::Ui, status: Status) {
    let (label, color) = match status {
        Status::Asking => ("Asking", ACCENT_ORANGE),
        Status::InProgress => ("In progress", ACCENT_BLUE),
        Status::Complete => ("Complete", ACCENT_GREEN),
    };
    small_pill(ui, label, color);
}

fn small_pill(ui: &mut egui::Ui, text: &str, color: Color32) {
    Frame::none()
        .fill(color)
        .rounding(6.0)
        .inner_margin(egui::Margin::symmetric(6.0, 3.0))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(11.0).color(Color32::BLACK));
        });
}

fn small_icon_button(ui: &mut egui::Ui, label: &str) {
    Frame::none()
        .fill(BG_CARD_HI)
        .rounding(6.0)
        .inner_margin(egui::Margin::symmetric(6.0, 3.0))
        .show(ui, |ui| {
            ui.label(RichText::new(label).size(11.0).color(TEXT_MUTED));
        });
}

#[derive(Clone)]
struct ProjectItem {
    name: String,
    color: Color32,
}

impl ProjectItem {
    fn new(name: &str, color: Color32) -> Self {
        Self {
            name: name.to_string(),
            color,
        }
    }
}

#[derive(Clone, Copy)]
enum Status {
    Asking,
    InProgress,
    Complete,
}

#[derive(Clone)]
struct CardData {
    title: String,
    subtitle: String,
    status: Status,
    time: String,
    badges: Vec<String>,
}

#[derive(Clone)]
struct ColumnData {
    title: String,
    count: usize,
    items: Vec<CardData>,
}

fn build_mock_columns() -> Vec<ColumnData> {
    vec![
        ColumnData {
            title: "Inbox".to_string(),
            count: 17,
            items: vec![
                CardData {
                    title: "Go to Parent Navigation Details".to_string(),
                    subtitle: "Testing plan to validate parent jump".to_string(),
                    status: Status::Asking,
                    time: "3h".to_string(),
                    badges: vec!["Web Tester".to_string(), "TENEX Web".to_string()],
                },
                CardData {
                    title: "Current Time & Date".to_string(),
                    subtitle: "Time sync for DDD project".to_string(),
                    status: Status::InProgress,
                    time: "6h".to_string(),
                    badges: vec!["Transparent".to_string(), "DDD".to_string()],
                },
                CardData {
                    title: "Sample Questions".to_string(),
                    subtitle: "Multi-select question UI".to_string(),
                    status: Status::Complete,
                    time: "8h".to_string(),
                    badges: vec!["PM".to_string(), "TENEX Web".to_string()],
                },
            ],
        },
        ColumnData {
            title: "TENEX Web Svelte".to_string(),
            count: 12,
            items: vec![
                CardData {
                    title: "Testing Response Interaction".to_string(),
                    subtitle: "Validate response input style".to_string(),
                    status: Status::Complete,
                    time: "4h".to_string(),
                    badges: vec!["Writer".to_string(), "DDD".to_string()],
                },
                CardData {
                    title: "Delegations and Web Search".to_string(),
                    subtitle: "Status on search flow".to_string(),
                    status: Status::InProgress,
                    time: "7h".to_string(),
                    badges: vec!["Transparent".to_string(), "TENEX Web".to_string()],
                },
                CardData {
                    title: "Overnight Nostr Protocol".to_string(),
                    subtitle: "Quick report on relays".to_string(),
                    status: Status::Complete,
                    time: "10h".to_string(),
                    badges: vec!["Claude".to_string(), "TENEX".to_string()],
                },
            ],
        },
        ColumnData {
            title: "TENEX Backend".to_string(),
            count: 10,
            items: vec![
                CardData {
                    title: "Tool Description Updates".to_string(),
                    subtitle: "Patch MCP metadata".to_string(),
                    status: Status::Complete,
                    time: "2h".to_string(),
                    badges: vec!["PM".to_string(), "Claude".to_string()],
                },
                CardData {
                    title: "Nostr Event Fetch Error".to_string(),
                    subtitle: "Investigate relay fallback".to_string(),
                    status: Status::InProgress,
                    time: "9h".to_string(),
                    badges: vec!["PM".to_string(), "TENEX".to_string()],
                },
                CardData {
                    title: "Delegation UI polish".to_string(),
                    subtitle: "Spacing and overflow fixes".to_string(),
                    status: Status::Asking,
                    time: "1d".to_string(),
                    badges: vec!["Reporter".to_string(), "TENEX".to_string()],
                },
            ],
        },
    ]
}
