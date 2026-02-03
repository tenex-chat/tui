use std::time::Duration;
use tenex_core::ffi::TenexCore;
use tenex_core::ffi::{EventCallback, DataChangeType};

struct DebugCallback;

impl EventCallback for DebugCallback {
    fn on_data_changed(&self, change_type: DataChangeType) {
        match change_type {
            DataChangeType::ProjectStatusChanged { project_id, is_online, .. } => {
                eprintln!("[RECEIVED] ProjectStatusChanged: {} online={}", project_id, is_online);
            }
            DataChangeType::PendingBackendApproval { backend_pubkey, project_a_tag } => {
                eprintln!("[RECEIVED] PendingBackendApproval: {} for {}", backend_pubkey, project_a_tag);
            }
            _ => {}
        }
    }
}

fn main() {
    eprintln!("=== TenexCore State Debugger ===\n");

    let core = TenexCore::new();

    if !core.init() {
        eprintln!("ERROR: Failed to initialize TenexCore");
        return;
    }
    eprintln!("✓ TenexCore initialized\n");

    // Register callback to see when events arrive
    let callback = Box::new(DebugCallback);
    core.set_event_callback(callback);
    eprintln!("✓ Event callback registered\n");

    // Try to login (will use stored credentials if available)
    match std::env::var("TENEX_NSEC").or_else(|_| std::env::var("NOSTR_NSEC")) {
        Ok(nsec) => {
            eprintln!("Attempting login with NSEC from environment...");
            match core.login(nsec) {
                Ok(_) => eprintln!("✓ Login successful\n"),
                Err(e) => eprintln!("⚠ Login failed: {:?}\n", e),
            }
        }
        Err(_) => {
            eprintln!("⚠ No NSEC in environment");
            return;
        }
    }

    eprintln!("Waiting for initial data sync...");
    std::thread::sleep(Duration::from_secs(5));

    // Poll state every 5 seconds
    for iteration in 1..=100 {
        eprintln!("\n============================================================");
        eprintln!("ITERATION {}", iteration);
        eprintln!("============================================================");

        // Refresh to process any new events
        if !core.refresh() {
            eprintln!("⚠ Refresh returned false (throttled or error)");
        }

        // Approve any pending backends
        match core.approve_all_pending_backends() {
            Ok(count) => {
                if count > 0 {
                    eprintln!("✓ Auto-approved {} pending backend(s)", count);
                }
            }
            Err(e) => eprintln!("⚠ Backend approval error: {:?}", e),
        }

        // Get projects
        let projects = core.get_projects();
        eprintln!("\n📁 Projects in store: {}", projects.len());
        for project in &projects {
            eprintln!("  - {} (id: {})", project.title, project.id);

            // Try to get agents for this project
            match core.get_online_agents(project.id.clone()) {
                Ok(agents) => {
                    if agents.is_empty() {
                        eprintln!("    ⚠ 0 online agents");
                    } else {
                        eprintln!("    ✓ {} online agents:", agents.len());
                        for agent in &agents {
                            eprintln!("      - {} ({})", agent.name,
                                if agent.is_pm { "PM" } else { "agent" });
                        }
                    }
                }
                Err(e) => eprintln!("    ⚠ Error getting agents: {:?}", e),
            }
        }

        std::thread::sleep(Duration::from_secs(5));
    }
}
