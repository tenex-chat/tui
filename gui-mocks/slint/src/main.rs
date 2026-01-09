use slint::{ModelRc, SharedString, VecModel};
use std::rc::Rc;

slint::include_modules!();

fn main() -> Result<(), slint::PlatformError> {
    let app = AppWindow::new()?;

    let workspace_items = vec![
        SidebarItem { name: "DDD".into(), color: rgb(79, 134, 255), active: true },
        SidebarItem { name: "TENEX Management".into(), color: rgb(62, 200, 124), active: false },
        SidebarItem { name: "TENEX Backend".into(), color: rgb(222, 124, 66), active: false },
        SidebarItem { name: "TENEX Web Svelte".into(), color: rgb(222, 168, 54), active: false },
        SidebarItem { name: "TENEX iOS Client".into(), color: rgb(125, 102, 255), active: false },
        SidebarItem { name: "TENEX TUI Client".into(), color: rgb(84, 172, 255), active: false },
        SidebarItem { name: "Agents".into(), color: rgb(132, 117, 255), active: false },
        SidebarItem { name: "Agents Web".into(), color: rgb(94, 188, 255), active: false },
    ];

    let inbox_cards = vec![
        make_card("Connection Error - Localhost:5005", "Web Tester cannot access the app", "Asking", rgb(222, 124, 66), "2h", &["Web Tester", "TENEX Web"]),
        make_card("Go to Parent Navigation Details", "Manual steps for delegation navigation", "In progress", rgb(79, 134, 255), "6h", &["PM-WIP", "Web Tester"]),
        make_card("Sample Questions", "Ask modal with multi-select fields", "Complete", rgb(62, 200, 124), "1d", &["PM-WIP", "TENEX Web"]),
    ];

    let web_cards = vec![
        make_card("Testing 'Go to Parent' Feature", "Validate parent jump flow", "In progress", rgb(79, 134, 255), "3h", &["PM-WIP", "Web Tester"]),
        make_card("Fixing 'Go to Parent' Navigation", "Navigation regression fixes", "Complete", rgb(62, 200, 124), "6h", &["PM-WIP", "Claude"]),
        make_card("Getting Started Guide Creation", "Docs and onboarding guide", "Complete", rgb(62, 200, 124), "10h", &["PM-WIP", "Writer"]),
    ];

    let backend_cards = vec![
        make_card("Discussion on Testing Tenex Agent", "Plan for QA automation", "In progress", rgb(79, 134, 255), "3h", &["PM", "Project Historian"]),
        make_card("Tool Description Updates Implementation", "Update tool metadata and guidelines", "Complete", rgb(62, 200, 124), "8h", &["PM", "Claude"]),
        make_card("Improving Tool Descriptions and Guides", "Refine tool documentation", "In progress", rgb(79, 134, 255), "1d", &["PM-WIP", "Reporter"]),
    ];

    let ddd_cards = vec![
        make_card("Current Time & Date", "Time sync for DDD project", "Asking", rgb(222, 168, 54), "2h", &["Transparent", "DDD"]),
        make_card("Testing Response Interaction", "Validate response input style", "Complete", rgb(62, 200, 124), "4h", &["Writer", "DDD"]),
        make_card("Initial User Interaction", "Confirm greeting flow", "In progress", rgb(79, 134, 255), "8h", &["PM", "DDD"]),
    ];

    let home_columns = vec![
        make_column("Inbox", "17", rgb(79, 134, 255), inbox_cards),
        make_column("TENEX Web Svelte", "12", rgb(62, 200, 124), web_cards),
        make_column("DDD", "9", rgb(79, 134, 255), ddd_cards),
        make_column("TENEX Backend", "10", rgb(222, 124, 66), backend_cards),
    ];

    let status_columns = vec![
        make_column("Asking", "6", rgb(222, 124, 66), vec![
            make_card("OpenAPI Audit", "Spec review and gaps", "Asking", rgb(222, 124, 66), "2h", &["PM"]),
            make_card("Delegation Handoff", "Need clarifications", "Asking", rgb(222, 124, 66), "5h", &["PM"]),
        ]),
        make_column("In progress", "8", rgb(79, 134, 255), vec![
            make_card("Inbox Sorting", "Optimizing filters", "In progress", rgb(79, 134, 255), "4h", &["Claude"]),
            make_card("Thread Viewer", "Inline summary work", "In progress", rgb(79, 134, 255), "6h", &["Writer"]),
        ]),
        make_column("Complete", "11", rgb(62, 200, 124), vec![
            make_card("MCP Tool Rollout", "Docs updated", "Complete", rgb(62, 200, 124), "1d", &["PM"]),
            make_card("Agent Pack Review", "QA finished", "Complete", rgb(62, 200, 124), "2d", &["Reporter"]),
        ]),
    ];

    let agents = vec![
        AgentItem {
            name: "Web Tester".into(),
            role: "qa".into(),
            description: "Explores flows, writes reproducible steps, flags regressions".into(),
            version: "2.4".into(),
        },
        AgentItem {
            name: "Project Historian".into(),
            role: "research".into(),
            description: "Summarizes conversations, extracts decisions, tracks context".into(),
            version: "1.8".into(),
        },
        AgentItem {
            name: "Execution Coordinator".into(),
            role: "pm".into(),
            description: "Breaks down work, assigns agents, keeps status aligned".into(),
            version: "3.1".into(),
        },
    ];

    let packs = vec![
        PackItem {
            title: "Product Launch".into(),
            description: "PM, QA, research, and documentation agents".into(),
            count: "6".into(),
        },
        PackItem {
            title: "Infra Operations".into(),
            description: "Reliability, incident response, tooling".into(),
            count: "4".into(),
        },
    ];

    let tools = vec![
        ToolItem {
            name: "Codebase Search".into(),
            description: "Search files and symbols".into(),
            command: "mcp__code_search__query".into(),
            caps: make_model_list(&["read", "glob", "ripgrep"]),
        },
        ToolItem {
            name: "Report Writer".into(),
            description: "Generate structured reports".into(),
            command: "mcp__reports__write".into(),
            caps: make_model_list(&["write", "json"]),
        },
        ToolItem {
            name: "Shell Runner".into(),
            description: "Execute sandboxed commands".into(),
            command: "mcp__shell__run".into(),
            caps: make_model_list(&["exec", "env", "fs"]),
        },
    ];

    let nudges = vec![
        NudgeItem {
            title: "Summarize decisions".into(),
            description: "Capture final decisions and next steps".into(),
            author: "Pablo".into(),
            tags: make_model_list(&["summary", "pm"]),
        },
        NudgeItem {
            title: "Regression check".into(),
            description: "List regression risks before merge".into(),
            author: "Web Tester".into(),
            tags: make_model_list(&["qa", "release"]),
        },
    ];

    let lessons = vec![
        LessonItem {
            title: "Nostr tag parsing".into(),
            summary: "Use a-tag and e-tag to resolve threads".into(),
            author: "Claude".into(),
        },
        LessonItem {
            title: "Agent routing".into(),
            summary: "Mentions override default agent selection".into(),
            author: "Project Historian".into(),
        },
    ];

    let threads = vec![
        ThreadItem {
            title: "Testing Conversation ID Search".into(),
            subtitle: "Conversation ID search".into(),
            status_text: "Asking".into(),
            status_color: rgb(222, 124, 66),
            time: "2h".into(),
            tags: make_model_list(&["PM-WIP", "Web Tester"]),
        },
        ThreadItem {
            title: "Debugging Conversation ID Search".into(),
            subtitle: "Search flow improvements".into(),
            status_text: "In progress".into(),
            status_color: rgb(79, 134, 255),
            time: "6h".into(),
            tags: make_model_list(&["Execution C", "Claude"]),
        },
        ThreadItem {
            title: "Sample Questions".into(),
            subtitle: "Ask modal flow".into(),
            status_text: "Complete".into(),
            status_color: rgb(62, 200, 124),
            time: "1d".into(),
            tags: make_model_list(&["PM-WIP", "Agent"]),
        },
    ];

    let messages = vec![
        MessageItem {
            author: "Pablo".into(),
            content: "Can you confirm scope for this agent?".into(),
            role: "user".into(),
            time: "Jan 8".into(),
        },
        MessageItem {
            author: "Web Tester".into(),
            content: "Confirmed: web UI only. No backend access.".into(),
            role: "assistant".into(),
            time: "Jan 8".into(),
        },
        MessageItem {
            author: "Project Historian".into(),
            content: "Summary updated with latest findings.".into(),
            role: "assistant".into(),
            time: "Jan 9".into(),
        },
    ];

    let docs = vec![
        DocItem {
            title: "Manual: Go to Parent Navigation".into(),
            summary: "Steps to navigate from delegation threads".into(),
            updated: "Updated Jan 8".into(),
        },
        DocItem {
            title: "Delegation UI Spec".into(),
            summary: "UI elements and expected behaviors".into(),
            updated: "Updated Jan 7".into(),
        },
    ];

    let feeds = vec![
        FeedItem {
            title: "Agent check-in".into(),
            summary: "PM requested clarification on tools".into(),
            time: "3h".into(),
        },
        FeedItem {
            title: "Tool update".into(),
            summary: "MCP metadata patch applied".into(),
            time: "6h".into(),
        },
    ];

    let agent_options = vec![
        AgentOption { name: "Execution Coordinator".into(), model: "gpt-4.1".into(), online: true },
        AgentOption { name: "Project Historian".into(), model: "claude-4".into(), online: true },
        AgentOption { name: "Web Tester".into(), model: "gemini".into(), online: true },
        AgentOption { name: "Design Reviewer".into(), model: "claude-3.5".into(), online: false },
    ];

    let tool_groups = vec![
        ToolGroup {
            name: "MCP: shell".into(),
            tools: make_model_list(&["shell:run", "shell:read"]),
        },
        ToolGroup {
            name: "MCP: docs".into(),
            tools: make_model_list(&["docs:search", "docs:write"]),
        },
        ToolGroup {
            name: "Core".into(),
            tools: make_model_list(&["todo:add", "todo:update", "report:write"]),
        },
    ];

    app.set_workspace_items(to_model(workspace_items));
    app.set_home_columns(to_model(home_columns));
    app.set_status_columns(to_model(status_columns));
    app.set_agents(to_model(agents));
    app.set_packs(to_model(packs));
    app.set_tools(to_model(tools));
    app.set_nudges(to_model(nudges));
    app.set_lessons(to_model(lessons));
    app.set_threads(to_model(threads));
    app.set_messages(to_model(messages));
    app.set_docs(to_model(docs));
    app.set_feeds(to_model(feeds));
    app.set_agent_options(to_model(agent_options));
    app.set_tool_groups(to_model(tool_groups));

    app.run()
}

fn make_card(
    title: &str,
    subtitle: &str,
    status_text: &str,
    status_color: slint::Color,
    time: &str,
    tags: &[&str],
) -> CardItem {
    CardItem {
        title: title.into(),
        subtitle: subtitle.into(),
        status_text: status_text.into(),
        status_color,
        time: time.into(),
        tags: make_model_list(tags),
    }
}

fn make_column(title: &str, count: &str, accent: slint::Color, cards: Vec<CardItem>) -> ColumnItem {
    ColumnItem {
        title: title.into(),
        count: count.into(),
        accent,
        cards: to_model(cards),
    }
}

fn make_model_list(items: &[&str]) -> ModelRc<SharedString> {
    ModelRc::from(Rc::new(VecModel::from(
        items.iter().map(|item| (*item).into()).collect::<Vec<SharedString>>(),
    )))
}

fn to_model<T: Clone + 'static>(items: Vec<T>) -> ModelRc<T> {
    ModelRc::from(Rc::new(VecModel::from(items)))
}

fn rgb(r: u8, g: u8, b: u8) -> slint::Color {
    slint::Color::from_rgb_u8(r, g, b)
}
