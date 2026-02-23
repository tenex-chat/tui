use crate::models::Thread;
use std::collections::{HashMap, HashSet};

/// Q-tag relationships: parent thread ID -> list of child conversation IDs
pub type QTagRelationships = HashMap<String, Vec<String>>;

/// Hierarchical thread item with depth for nesting display
#[derive(Clone)]
pub struct HierarchicalThread {
    pub thread: Thread,
    pub a_tag: String,
    pub depth: usize,
    pub has_children: bool,
    pub child_count: usize,
    pub is_collapsed: bool,
}

/// Build hierarchical thread list from flat list with delegation relationships
///
/// Uses two sources to determine parent-child relationships:
/// 1. Primary: `delegation` tag on child conversation root (thread.parent_conversation_id)
/// 2. Fallback: `q-tags` in parent conversation messages pointing to child conversations
///
/// When `default_collapsed` is true, threads with children will be collapsed by default
/// unless explicitly expanded (not in collapsed_ids is treated as collapsed).
pub fn build_thread_hierarchy(
    threads: &[(Thread, String)],
    collapsed_ids: &HashSet<String>,
    q_tag_relationships: &QTagRelationships,
    default_collapsed: bool,
) -> Vec<HierarchicalThread> {
    // Build reverse lookup: child_conversation_id -> parent_thread_id from q-tags
    let mut q_tag_child_to_parent: HashMap<&str, &str> = HashMap::new();
    for (parent_id, children) in q_tag_relationships {
        for child_id in children {
            q_tag_child_to_parent.insert(child_id.as_str(), parent_id.as_str());
        }
    }

    // Map from parent ID -> array of child threads (thread, a_tag)
    let mut parent_to_children: HashMap<&str, Vec<(&Thread, &String)>> = HashMap::new();
    // Set of threads that are children (have a parent)
    let mut child_ids: HashSet<&str> = HashSet::new();

    // Build the mappings using both delegation tags and q-tags
    for (thread, a_tag) in threads {
        // Primary: use delegation tag if present
        let parent_id = thread
            .parent_conversation_id
            .as_deref()
            // Fallback: use q-tag relationship if no delegation tag
            .or_else(|| q_tag_child_to_parent.get(thread.id.as_str()).copied());

        if let Some(parent_id) = parent_id {
            child_ids.insert(&thread.id);
            parent_to_children
                .entry(parent_id)
                .or_default()
                .push((thread, a_tag));
        }
    }

    // Sort children by most recent activity (same as parent sorting)
    for children in parent_to_children.values_mut() {
        children.sort_by(|a, b| b.0.last_activity.cmp(&a.0.last_activity));
    }

    // Count all descendants (recursive) for a thread
    fn count_descendants(
        thread_id: &str,
        parent_to_children: &HashMap<&str, Vec<(&Thread, &String)>>,
    ) -> usize {
        let children = parent_to_children.get(thread_id);
        match children {
            None => 0,
            Some(children) => {
                let mut count = children.len();
                for (child, _) in children {
                    count += count_descendants(&child.id, parent_to_children);
                }
                count
            }
        }
    }

    // Build flattened hierarchical list with depth information
    let mut result: Vec<HierarchicalThread> = Vec::new();

    fn add_thread_with_children(
        thread: &Thread,
        a_tag: &String,
        depth: usize,
        collapsed_ids: &HashSet<String>,
        parent_to_children: &HashMap<&str, Vec<(&Thread, &String)>>,
        result: &mut Vec<HierarchicalThread>,
        default_collapsed: bool,
    ) {
        let children = parent_to_children.get(thread.id.as_str());
        let has_children = children.map(|c| !c.is_empty()).unwrap_or(false);
        let child_count = count_descendants(&thread.id, parent_to_children);

        // Determine collapsed state:
        // - If default_collapsed is true: collapsed unless explicitly in collapsed_ids (inverted: presence means expanded)
        // - If default_collapsed is false: expanded unless explicitly in collapsed_ids (presence means collapsed)
        let is_collapsed = if default_collapsed && has_children {
            // When default collapsed, being in the set means EXPANDED (user toggled to expand)
            !collapsed_ids.contains(&thread.id)
        } else {
            // When default expanded, being in the set means COLLAPSED (user toggled to collapse)
            collapsed_ids.contains(&thread.id)
        };

        result.push(HierarchicalThread {
            thread: thread.clone(),
            a_tag: a_tag.clone(),
            depth,
            has_children,
            child_count,
            is_collapsed,
        });

        // Only add children if this thread is not collapsed
        if !is_collapsed {
            if let Some(children) = children {
                for (child, child_a_tag) in children {
                    add_thread_with_children(
                        child,
                        child_a_tag,
                        depth + 1,
                        collapsed_ids,
                        parent_to_children,
                        result,
                        default_collapsed,
                    );
                }
            }
        }
    }

    // Build set of all thread IDs in the visible list so we can detect cross-project parents
    let thread_ids: HashSet<&str> = threads.iter().map(|(t, _)| t.id.as_str()).collect();

    // Start with root threads: those with no parent, OR whose parent is not in this visible list
    // (cross-project delegations: the child lives in Project B but its parent is in Project A)
    let root_threads: Vec<(&Thread, &String)> = threads
        .iter()
        .filter(|(t, _)| {
            if !child_ids.contains(t.id.as_str()) {
                return true;
            }
            // Thread has a parent — only nest it if the parent is actually visible here
            let parent_id = t
                .parent_conversation_id
                .as_deref()
                .or_else(|| q_tag_child_to_parent.get(t.id.as_str()).copied());
            !parent_id
                .map(|pid| thread_ids.contains(pid))
                .unwrap_or(false)
        })
        .map(|(t, a)| (t, a))
        .collect();

    for (thread, a_tag) in root_threads {
        add_thread_with_children(
            thread,
            a_tag,
            0,
            collapsed_ids,
            &parent_to_children,
            &mut result,
            default_collapsed,
        );
    }

    result
}
