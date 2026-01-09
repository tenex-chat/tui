use iced::alignment::{Horizontal, Vertical};
use iced::widget::{Column, Row, Space, container, scrollable, text};
use iced::{Background, Border, Color, Element, Length, Sandbox, Settings, Theme};

const BG_APP: Color = rgb(18, 20, 24);
const BG_PANEL: Color = rgb(24, 26, 31);
const BG_CARD: Color = rgb(28, 31, 37);
const BG_CARD_HI: Color = rgb(34, 38, 46);
const TEXT_PRIMARY: Color = rgb(226, 230, 235);
const TEXT_MUTED: Color = rgb(150, 156, 168);
const TEXT_DIM: Color = rgb(114, 120, 134);
const ACCENT_BLUE: Color = rgb(79, 134, 255);
const ACCENT_GREEN: Color = rgb(62, 200, 124);
const ACCENT_YELLOW: Color = rgb(222, 168, 54);
const ACCENT_ORANGE: Color = rgb(222, 124, 66);

fn main() -> iced::Result {
    TenexMock::run(Settings {
        window: iced::window::Settings {
            size: iced::Size::new(1500.0, 900.0),
            ..Default::default()
        },
        ..Default::default()
    })
}

const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color {
        r: r as f32 / 255.0,
        g: g as f32 / 255.0,
        b: b as f32 / 255.0,
        a: 1.0,
    }
}

struct TenexMock {
    sidebar_projects: Vec<ProjectItem>,
    columns: Vec<ColumnData>,
}

impl Sandbox for TenexMock {
    type Message = ();

    fn new() -> Self {
        Self {
            sidebar_projects: vec![
                ProjectItem::new("DDD", ACCENT_BLUE),
                ProjectItem::new("TENEX Management", ACCENT_GREEN),
                ProjectItem::new("TENEX Backend", ACCENT_ORANGE),
                ProjectItem::new("TENEX Web Svelte", ACCENT_YELLOW),
                ProjectItem::new("TENEX iOS Client", Color::from_rgb8(125, 102, 255)),
                ProjectItem::new("TENEX TUI Client", Color::from_rgb8(84, 172, 255)),
                ProjectItem::new("Agents", Color::from_rgb8(132, 117, 255)),
            ],
            columns: build_mock_columns(),
        }
    }

    fn title(&self) -> String {
        "TENEX GUI Mock - iced".to_string()
    }

    fn theme(&self) -> Theme {
        Theme::Dark
    }

    fn update(&mut self, _message: Self::Message) {}

    fn view(&self) -> Element<'_, Self::Message> {
        let sidebar = container(sidebar_content(&self.sidebar_projects))
            .width(Length::Fixed(230.0))
            .height(Length::Fill)
            .style(panel_style(BG_PANEL, Color::from_rgb8(32, 36, 44), 10.0));

        let mut columns = Row::new().spacing(12).width(Length::Fill).height(Length::Fill);
        for (idx, col) in self.columns.iter().enumerate() {
            let accent = match idx {
                0 => ACCENT_BLUE,
                1 => ACCENT_GREEN,
                _ => ACCENT_ORANGE,
            };
            columns = columns.push(column_panel(col, accent));
        }

        let detail = container(detail_panel())
            .width(Length::Fixed(360.0))
            .height(Length::Fill)
            .style(panel_style(BG_PANEL, Color::from_rgb8(32, 36, 44), 10.0));

        let content = Row::new()
            .spacing(12)
            .height(Length::Fill)
            .push(sidebar)
            .push(columns)
            .push(detail);

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(12)
            .style(panel_style(BG_APP, BG_APP, 0.0))
            .into()
    }
}

fn sidebar_content(items: &[ProjectItem]) -> Element<'static, ()> {
    let header = Row::new()
        .align_items(iced::Alignment::Center)
        .push(text("TENEX").size(18).style(TEXT_PRIMARY))
        .push(Space::with_width(Length::Fill))
        .push(text("88").size(12).style(TEXT_DIM));

    let chips = Row::new()
        .spacing(6)
        .push(color_chip(ACCENT_BLUE))
        .push(color_chip(ACCENT_GREEN))
        .push(color_chip(ACCENT_ORANGE));

    let mut list = Column::new()
        .spacing(6)
        .push(header)
        .push(Space::with_height(Length::Fixed(6.0)))
        .push(chips)
        .push(Space::with_height(Length::Fixed(16.0)))
        .push(text("TENEX").size(12).style(TEXT_DIM))
        .push(Space::with_height(Length::Fixed(6.0)));

    for item in items {
        list = list.push(sidebar_item(item));
    }

    list = list.push(Space::with_height(Length::Fill));

    list.push(user_block())
        .padding(12)
        .spacing(6)
        .into()
}

fn sidebar_item(item: &ProjectItem) -> Element<'static, ()> {
    let body = Row::new()
        .spacing(8)
        .align_items(iced::Alignment::Center)
        .push(color_chip(item.color))
        .push(text(item.name.clone()).size(12).style(TEXT_PRIMARY));

    container(body)
        .width(Length::Fill)
        .padding(8)
        .style(panel_style(BG_CARD, Color::from_rgb8(38, 42, 52), 8.0))
        .into()
}

fn user_block() -> Element<'static, ()> {
    let body = Row::new()
        .spacing(8)
        .align_items(iced::Alignment::Center)
        .push(avatar_chip(ACCENT_BLUE, "P"))
        .push(text("Pablo Testing Pubkey").size(12).style(TEXT_PRIMARY));

    container(body)
        .width(Length::Fill)
        .padding(8)
        .style(panel_style(BG_PANEL, Color::from_rgb8(38, 42, 52), 8.0))
        .into()
}

fn column_panel(col: &ColumnData, accent: Color) -> Element<'static, ()> {
    let header = Row::new()
        .align_items(iced::Alignment::Center)
        .spacing(6)
        .push(text(col.title.clone()).size(14).style(TEXT_PRIMARY))
        .push(pill(&col.count.to_string(), accent, Color::from_rgb8(15, 18, 22)))
        .push(Space::with_width(Length::Fill))
        .push(pill("+", BG_CARD_HI, TEXT_MUTED));

    let mut list = Column::new();
    for item in &col.items {
        list = list.push(card(item));
    }

    let scroll = scrollable(list.spacing(8))
        .height(Length::Fill)
        .width(Length::Fill);

    let body = Column::new()
        .spacing(6)
        .push(header)
        .push(Space::with_height(Length::Fixed(6.0)))
        .push(scroll);

    container(body)
        .padding(10)
        .style(panel_style(BG_PANEL, Color::from_rgb8(32, 36, 44), 10.0))
        .height(Length::Fill)
        .width(Length::Fill)
        .into()
}

fn card(item: &CardData) -> Element<'static, ()> {
    let status = match item.status {
        Status::Asking => ("Asking", ACCENT_ORANGE),
        Status::InProgress => ("In progress", ACCENT_BLUE),
        Status::Complete => ("Complete", ACCENT_GREEN),
    };

    let status_row = Row::new()
        .spacing(6)
        .align_items(iced::Alignment::Center)
        .push(pill(status.0, status.1, Color::from_rgb8(15, 18, 22)))
        .push(Space::with_width(Length::Fill))
        .push(pill(&item.time, Color::from_rgb8(88, 94, 106), TEXT_MUTED));

    let mut badges_row = Row::new().spacing(6).align_items(iced::Alignment::Center);
    for badge in &item.badges {
        badges_row = badges_row.push(pill(badge, Color::from_rgb8(66, 70, 82), TEXT_MUTED));
    }

    let content = Column::new()
        .spacing(4)
        .push(text(item.title.clone()).size(13).style(TEXT_PRIMARY))
        .push(text(item.subtitle.clone()).size(12).style(TEXT_MUTED))
        .push(Space::with_height(Length::Fixed(4.0)))
        .push(status_row)
        .push(Space::with_height(Length::Fixed(4.0)))
        .push(badges_row);

    container(content)
        .padding(8)
        .style(panel_style(BG_CARD, Color::from_rgb8(38, 42, 52), 8.0))
        .into()
}

fn detail_panel() -> Element<'static, ()> {
    let header = Row::new()
        .align_items(iced::Alignment::Center)
        .push(text("New Conversation").size(14).style(TEXT_MUTED))
        .push(Space::with_width(Length::Fill))
        .push(pill("...", BG_CARD_HI, TEXT_MUTED));

    let summary = Column::new()
        .spacing(6)
        .push(header)
        .push(Space::with_height(Length::Fixed(8.0)))
        .push(text("Executive Summary").size(18).style(TEXT_PRIMARY))
        .push(text("Recent conversation highlights and outcomes.").size(12).style(TEXT_MUTED))
        .push(Space::with_height(Length::Fixed(10.0)))
        .push(text("Timeline").size(12).style(TEXT_DIM))
        .push(text("Jan 06 - Onboarded the web-tester agent").size(12).style(TEXT_MUTED))
        .push(text("Jan 07 - First automation pass complete").size(12).style(TEXT_MUTED))
        .push(Space::with_height(Length::Fixed(10.0)))
        .push(text("Key Details").size(12).style(TEXT_DIM))
        .push(text("- No-code access, uses browser automation.").size(12).style(TEXT_MUTED))
        .push(text("- Delivers manual test reports and summaries.").size(12).style(TEXT_MUTED))
        .push(text("- Signals outcomes and issues.").size(12).style(TEXT_MUTED))
        .push(Space::with_height(Length::Fixed(12.0)))
        .push(text("Messages").size(12).style(TEXT_DIM))
        .push(message_row("Pablo", "Can you confirm scope for this agent?"))
        .push(message_row("Web Tester", "Confirmed: web UI only. No backend access."));

    let composer = container(
        Row::new()
            .align_items(iced::Alignment::Center)
            .push(text("Type a message...").size(12).style(TEXT_DIM))
            .push(Space::with_width(Length::Fill))
            .push(pill("Send", ACCENT_BLUE, Color::from_rgb8(15, 18, 22)))
    )
    .padding(8)
    .style(panel_style(BG_CARD, Color::from_rgb8(38, 42, 52), 8.0));

    let content = Column::new()
        .spacing(6)
        .push(summary)
        .push(Space::with_height(Length::Fill))
        .push(composer);

    let scroll = scrollable(content)
        .height(Length::Fill)
        .width(Length::Fill);

    container(scroll)
        .padding(10)
        .style(panel_style(BG_PANEL, Color::from_rgb8(32, 36, 44), 10.0))
        .into()
}

fn message_row(name: &str, message: &str) -> Element<'static, ()> {
    let header = Row::new()
        .spacing(6)
        .align_items(iced::Alignment::Center)
        .push(avatar_chip(ACCENT_GREEN, &name[0..1]))
        .push(text(name).size(12).style(TEXT_PRIMARY));

    let body = Column::new()
        .spacing(4)
        .push(header)
        .push(text(message).size(12).style(TEXT_MUTED));

    container(body)
        .padding(8)
        .style(panel_style(BG_CARD, Color::from_rgb8(38, 42, 52), 8.0))
        .into()
}

fn pill(label: &str, bg: Color, text_color: Color) -> Element<'static, ()> {
    container(text(label).size(11).style(text_color))
        .padding([4, 8])
        .align_x(Horizontal::Center)
        .align_y(Vertical::Center)
        .style(pill_style(bg))
        .into()
}

fn color_chip(color: Color) -> Element<'static, ()> {
    container(Space::with_width(Length::Fixed(14.0)))
        .height(Length::Fixed(14.0))
        .width(Length::Fixed(14.0))
        .style(pill_style(color))
        .into()
}

fn avatar_chip(color: Color, label: &str) -> Element<'static, ()> {
    container(text(label).size(11).style(TEXT_PRIMARY))
        .height(Length::Fixed(20.0))
        .width(Length::Fixed(20.0))
        .align_x(Horizontal::Center)
        .align_y(Vertical::Center)
        .style(pill_style(color))
        .into()
}

fn panel_style(bg: Color, border: Color, radius: f32) -> iced::theme::Container {
    iced::theme::Container::Custom(Box::new(PanelStyle { bg, border, radius }))
}

fn pill_style(bg: Color) -> iced::theme::Container {
    iced::theme::Container::Custom(Box::new(PillStyle { bg }))
}

#[derive(Clone, Copy)]
struct PanelStyle {
    bg: Color,
    border: Color,
    radius: f32,
}

impl iced::widget::container::StyleSheet for PanelStyle {
    type Style = Theme;

    fn appearance(&self, _style: &Theme) -> iced::widget::container::Appearance {
        iced::widget::container::Appearance {
            text_color: None,
            background: Some(Background::Color(self.bg)),
            border: Border {
                radius: self.radius.into(),
                width: 1.0,
                color: self.border,
            },
            ..Default::default()
        }
    }
}

#[derive(Clone, Copy)]
struct PillStyle {
    bg: Color,
}

impl iced::widget::container::StyleSheet for PillStyle {
    type Style = Theme;

    fn appearance(&self, _style: &Theme) -> iced::widget::container::Appearance {
        iced::widget::container::Appearance {
            text_color: None,
            background: Some(Background::Color(self.bg)),
            border: Border {
                radius: 6.0.into(),
                width: 0.0,
                color: self.bg,
            },
            ..Default::default()
        }
    }
}

#[derive(Clone)]
struct ProjectItem {
    name: String,
    color: Color,
}

impl ProjectItem {
    fn new(name: &str, color: Color) -> Self {
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
