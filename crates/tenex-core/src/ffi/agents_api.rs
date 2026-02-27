use super::*;

#[uniffi::export]
impl TenexCore {
    /// Get agents for a project.
    ///
    /// Returns agents configured for the specified project.
    /// Returns an error if the store cannot be accessed.
    pub fn get_agents(&self, project_id: String) -> Result<Vec<AgentDefinition>, TenexError> {
        let store_guard = self.store.read().map_err(|e| TenexError::Internal {
            message: format!("Failed to acquire store lock: {}", e),
        })?;

        let store = store_guard.as_ref().ok_or_else(|| TenexError::Internal {
            message: "Store not initialized - call init() first".to_string(),
        })?;

        // Find the project by ID and get its agent IDs (event IDs of kind:4199 definitions)
        let project = store
            .get_projects()
            .iter()
            .find(|p| p.id == project_id)
            .cloned();
        let agent_definition_ids: Vec<String> = match project {
            Some(p) => p.agent_definition_ids,
            None => return Ok(Vec::new()), // Project not found = empty agents (not an error)
        };

        // Get agent definitions for these IDs
        Ok(store
            .content
            .get_agent_definitions()
            .into_iter()
            .filter(|agent| agent_definition_ids.contains(&agent.id))
            .cloned()
            .collect())
    }

    /// Get all available agents (not filtered by project).
    ///
    /// Returns all known agent definitions.
    /// Returns an error if the store cannot be accessed.
    pub fn get_all_agents(&self) -> Result<Vec<AgentDefinition>, TenexError> {
        let store_guard = self.store.read().map_err(|e| TenexError::Internal {
            message: format!("Failed to acquire store lock: {}", e),
        })?;

        let store = store_guard.as_ref().ok_or_else(|| TenexError::Internal {
            message: "Store not initialized - call init() first".to_string(),
        })?;

        Ok(store
            .content
            .get_agent_definitions()
            .into_iter()
            .cloned()
            .collect())
    }

    /// Get all available team packs (kind:34199), deduped to latest by `pubkey + d_tag`.
    ///
    /// Includes computed social metrics from comments (kind:1111) and reactions (kind:7)
    /// matched with dual anchors (`a`/`A` coordinate + `e`/`E` event id).
    pub fn get_all_teams(&self) -> Result<Vec<TeamInfo>, TenexError> {
        let store_guard = self.store.read().map_err(|e| TenexError::Internal {
            message: format!("Failed to acquire store lock: {}", e),
        })?;
        let store = store_guard.as_ref().ok_or_else(|| TenexError::Internal {
            message: "Store not initialized - call init() first".to_string(),
        })?;

        let mut latest_by_key: HashMap<String, TeamPack> = HashMap::new();
        for team in store.content.get_team_packs() {
            let identifier = if team.d_tag.is_empty() {
                team.id.clone()
            } else {
                team.d_tag.clone()
            };
            let key = format!(
                "{}:{}",
                team.pubkey.to_lowercase(),
                identifier.to_lowercase()
            );
            match latest_by_key.get(&key) {
                Some(existing)
                    if existing.created_at > team.created_at
                        || (existing.created_at == team.created_at && existing.id >= team.id) => {}
                _ => {
                    latest_by_key.insert(key, team.clone());
                }
            }
        }

        let mut teams: Vec<TeamPack> = latest_by_key.into_values().collect();
        teams.sort_by(|a, b| {
            b.created_at
                .cmp(&a.created_at)
                .then_with(|| b.id.cmp(&a.id))
        });

        let ndb = {
            let ndb_guard = self.ndb.read().map_err(|e| TenexError::Internal {
                message: format!("Failed to acquire ndb lock: {}", e),
            })?;
            ndb_guard
                .as_ref()
                .cloned()
                .ok_or(TenexError::CoreNotInitialized)?
        };

        let txn = Transaction::new(ndb.as_ref()).map_err(|e| TenexError::Internal {
            message: format!("Failed to create transaction: {}", e),
        })?;
        let social_filter = nostrdb::Filter::new().kinds([7, 1111]).build();
        let social_notes =
            ndb.query(&txn, &[social_filter], 50_000)
                .map_err(|e| TenexError::Internal {
                    message: format!("Failed querying social events: {}", e),
                })?;

        let current_user_pubkey = self.get_current_user().map(|u| u.pubkey);

        #[derive(Default)]
        struct TeamSocial {
            comment_count: u64,
            reactions_by_pubkey: HashMap<String, (u64, bool)>,
        }

        let mut social_by_team: HashMap<String, TeamSocial> = HashMap::new();
        for team in &teams {
            social_by_team.insert(team.id.clone(), TeamSocial::default());
        }

        for result in social_notes {
            let Ok(note) = ndb.get_note_by_key(&txn, result.note_key) else {
                continue;
            };

            for team in &teams {
                let identifier = if team.d_tag.is_empty() {
                    team.id.clone()
                } else {
                    team.d_tag.clone()
                };
                let coordinate = format!("34199:{}:{}", team.pubkey, identifier);
                if !note_matches_team_context(&note, &coordinate, &team.id) {
                    continue;
                }

                if let Some(social) = social_by_team.get_mut(&team.id) {
                    if note.kind() == 1111 {
                        social.comment_count += 1;
                    } else if note.kind() == 7 {
                        let reactor = hex::encode(note.pubkey());
                        let is_positive = reaction_is_positive(note.content());
                        let created_at = note.created_at();
                        match social.reactions_by_pubkey.get(&reactor) {
                            Some((existing_ts, _)) if *existing_ts > created_at => {}
                            _ => {
                                social
                                    .reactions_by_pubkey
                                    .insert(reactor, (created_at, is_positive));
                            }
                        }
                    }
                }
                break;
            }
        }

        Ok(teams
            .into_iter()
            .map(|team| {
                let identifier = if team.d_tag.is_empty() {
                    team.id.clone()
                } else {
                    team.d_tag.clone()
                };
                let coordinate = format!("34199:{}:{}", team.pubkey, identifier);
                let social = social_by_team.remove(&team.id).unwrap_or_default();
                let like_count = social
                    .reactions_by_pubkey
                    .values()
                    .filter(|(_, is_positive)| *is_positive)
                    .count() as u64;
                let liked_by_me = current_user_pubkey
                    .as_ref()
                    .and_then(|pk| social.reactions_by_pubkey.get(pk))
                    .map(|(_, is_positive)| *is_positive)
                    .unwrap_or(false);

                TeamInfo {
                    id: team.id,
                    pubkey: team.pubkey,
                    d_tag: team.d_tag,
                    coordinate,
                    title: team.title,
                    description: team.description,
                    image: team.image,
                    agent_definition_ids: team.agent_definition_ids,
                    categories: team.categories,
                    tags: team.tags,
                    created_at: team.created_at,
                    like_count,
                    comment_count: social.comment_count,
                    liked_by_me,
                }
            })
            .collect())
    }

    /// Get team comments (kind:1111) for one team using dual-anchor matching.
    pub fn get_team_comments(
        &self,
        team_coordinate: String,
        team_event_id: String,
    ) -> Result<Vec<TeamCommentInfo>, TenexError> {
        let store_guard = self.store.read().map_err(|e| TenexError::Internal {
            message: format!("Failed to acquire store lock: {}", e),
        })?;
        let store = store_guard.as_ref().ok_or_else(|| TenexError::Internal {
            message: "Store not initialized - call init() first".to_string(),
        })?;

        let ndb = {
            let ndb_guard = self.ndb.read().map_err(|e| TenexError::Internal {
                message: format!("Failed to acquire ndb lock: {}", e),
            })?;
            ndb_guard
                .as_ref()
                .cloned()
                .ok_or(TenexError::CoreNotInitialized)?
        };

        let txn = Transaction::new(ndb.as_ref()).map_err(|e| TenexError::Internal {
            message: format!("Failed to create transaction: {}", e),
        })?;
        let filter = nostrdb::Filter::new().kinds([1111]).build();
        let notes = ndb
            .query(&txn, &[filter], 20_000)
            .map_err(|e| TenexError::Internal {
                message: format!("Failed querying comments: {}", e),
            })?;

        let mut comments: Vec<TeamCommentInfo> = Vec::new();
        for result in notes {
            let Ok(note) = ndb.get_note_by_key(&txn, result.note_key) else {
                continue;
            };
            if !note_matches_team_context(&note, &team_coordinate, &team_event_id) {
                continue;
            }

            let pubkey = hex::encode(note.pubkey());
            comments.push(TeamCommentInfo {
                id: hex::encode(note.id()),
                pubkey: pubkey.clone(),
                author: store.get_profile_name(&pubkey),
                content: note.content().to_string(),
                created_at: note.created_at(),
                parent_comment_id: parse_parent_comment_id(&note, &team_event_id),
            });
        }

        comments.sort_by(|a, b| {
            a.created_at
                .cmp(&b.created_at)
                .then_with(|| a.id.cmp(&b.id))
        });
        Ok(comments)
    }

    /// Publish a team reaction (kind:7 NIP-25) and return reaction event ID.
    pub fn react_to_team(
        &self,
        team_coordinate: String,
        team_event_id: String,
        team_pubkey: String,
        is_like: bool,
    ) -> Result<String, TenexError> {
        let core_handle = get_core_handle(&self.core_handle)?;
        let (response_tx, response_rx) = std::sync::mpsc::sync_channel::<String>(1);

        core_handle
            .send(NostrCommand::ReactToTeam {
                team_coordinate,
                team_event_id,
                team_pubkey,
                is_like,
                response_tx: Some(response_tx),
            })
            .map_err(|e| TenexError::Internal {
                message: format!("Failed to send react_to_team command: {}", e),
            })?;

        response_rx
            .recv_timeout(Duration::from_secs(10))
            .map_err(|_| TenexError::Internal {
                message: "Timed out waiting for team reaction publish confirmation".to_string(),
            })
    }

    /// Publish a team comment (kind:1111 NIP-22) and return comment event ID.
    pub fn post_team_comment(
        &self,
        team_coordinate: String,
        team_event_id: String,
        team_pubkey: String,
        content: String,
        parent_comment_id: Option<String>,
        parent_comment_pubkey: Option<String>,
    ) -> Result<String, TenexError> {
        if content.trim().is_empty() {
            return Err(TenexError::Internal {
                message: "Comment content cannot be empty".to_string(),
            });
        }

        let core_handle = get_core_handle(&self.core_handle)?;
        let (response_tx, response_rx) = std::sync::mpsc::sync_channel::<String>(1);

        core_handle
            .send(NostrCommand::PostTeamComment {
                team_coordinate,
                team_event_id,
                team_pubkey,
                content,
                parent_comment_id,
                parent_comment_pubkey,
                response_tx: Some(response_tx),
            })
            .map_err(|e| TenexError::Internal {
                message: format!("Failed to send post_team_comment command: {}", e),
            })?;

        response_rx
            .recv_timeout(Duration::from_secs(10))
            .map_err(|_| TenexError::Internal {
                message: "Timed out waiting for team comment publish confirmation".to_string(),
            })
    }

    /// Get all nudges (kind:4201 events).
    ///
    /// Returns nudges deduplicated by `author + d-tag`, sorted by created_at
    /// descending (most recent first).
    /// Used by iOS for nudge selection in new conversations.
    pub fn get_nudges(&self) -> Result<Vec<Nudge>, TenexError> {
        let store_guard = self.store.read().map_err(|e| TenexError::Internal {
            message: format!("Failed to acquire store lock: {}", e),
        })?;

        let store = store_guard.as_ref().ok_or_else(|| TenexError::Internal {
            message: "Store not initialized - call init() first".to_string(),
        })?;

        Ok(store.content.get_nudges().into_iter().cloned().collect())
    }

    /// Get all skills (kind:4202 events).
    ///
    /// Returns skills deduplicated by `author + d-tag`, sorted by created_at
    /// descending (most recent first).
    /// Used by iOS/CLI for skill selection in new conversations.
    pub fn get_skills(&self) -> Result<Vec<Skill>, TenexError> {
        let store_guard = self.store.read().map_err(|e| TenexError::Internal {
            message: format!("Failed to acquire store lock: {}", e),
        })?;

        let store = store_guard.as_ref().ok_or_else(|| TenexError::Internal {
            message: "Store not initialized - call init() first".to_string(),
        })?;

        Ok(store.content.get_skills().into_iter().cloned().collect())
    }

    // MARK: - Nudge CRUD Methods

    pub fn create_nudge(
        &self,
        title: String,
        description: String,
        content: String,
        hashtags: Vec<String>,
        allow_tools: Vec<String>,
        deny_tools: Vec<String>,
        only_tools: Vec<String>,
    ) -> Result<(), TenexError> {
        if title.trim().is_empty() {
            return Err(TenexError::Internal {
                message: "Nudge title cannot be empty".to_string(),
            });
        }

        if content.trim().is_empty() {
            return Err(TenexError::Internal {
                message: "Nudge content cannot be empty".to_string(),
            });
        }

        let core_handle = get_core_handle(&self.core_handle)?;
        core_handle
            .send(NostrCommand::CreateNudge {
                title,
                description,
                content,
                hashtags,
                allow_tools,
                deny_tools,
                only_tools,
            })
            .map_err(|e| TenexError::Internal {
                message: format!("Failed to send create nudge command: {}", e),
            })?;

        Ok(())
    }

    /// Delete a nudge (kind:4201) via NIP-09 kind:5 deletion.
    ///
    /// Only the nudge author can delete it.
    pub fn delete_nudge(&self, nudge_id: String) -> Result<(), TenexError> {
        let store_guard = self.store.read().map_err(|e| TenexError::Internal {
            message: format!("Failed to acquire store lock: {}", e),
        })?;
        let store = store_guard.as_ref().ok_or_else(|| TenexError::Internal {
            message: "Store not initialized - call init() first".to_string(),
        })?;

        let nudge = store
            .content
            .get_nudge(&nudge_id)
            .ok_or_else(|| TenexError::Internal {
                message: format!("Nudge not found: {}", nudge_id),
            })?;

        let current_user = self
            .get_current_user()
            .ok_or_else(|| TenexError::Internal {
                message: "No logged-in user".to_string(),
            })?;

        if !nudge.pubkey.eq_ignore_ascii_case(&current_user.pubkey) {
            return Err(TenexError::Internal {
                message: "Only the author can delete this nudge".to_string(),
            });
        }

        let core_handle = get_core_handle(&self.core_handle)?;
        core_handle
            .send(NostrCommand::DeleteNudge { nudge_id })
            .map_err(|e| TenexError::Internal {
                message: format!("Failed to send delete nudge command: {}", e),
            })?;

        Ok(())
    }

    // MARK: - Bookmark Methods (kind:14202)

    /// Check if a nudge or skill is bookmarked by the current user.
    pub fn is_bookmarked(&self, item_id: String) -> bool {
        let user_pubkey = match self.get_current_user() {
            Some(u) => u.pubkey,
            None => return false,
        };

        let store_guard = match self.store.read() {
            Ok(g) => g,
            Err(_) => return false,
        };

        store_guard
            .as_ref()
            .map(|s| s.is_bookmarked(&user_pubkey, &item_id))
            .unwrap_or(false)
    }

    /// Get all bookmarked nudge/skill IDs for the current user.
    pub fn get_bookmarked_ids(&self) -> Result<Vec<String>, TenexError> {
        let user_pubkey = self
            .get_current_user()
            .ok_or_else(|| TenexError::Internal {
                message: "Not logged in".to_string(),
            })?
            .pubkey;

        let store_guard = self.store.read().map_err(|e| TenexError::Internal {
            message: format!("Failed to acquire store lock: {}", e),
        })?;

        let store = store_guard.as_ref().ok_or_else(|| TenexError::Internal {
            message: "Store not initialized - call init() first".to_string(),
        })?;

        Ok(store
            .get_bookmarks(&user_pubkey)
            .map(|bl| bl.bookmarked_ids.iter().cloned().collect())
            .unwrap_or_default())
    }

    /// Toggle bookmark status for a nudge or skill.
    pub fn toggle_bookmark(&self, item_id: String) -> Result<Vec<String>, TenexError> {
        use std::time::{SystemTime, UNIX_EPOCH};

        let user_pubkey = self
            .get_current_user()
            .ok_or_else(|| TenexError::Internal {
                message: "Not logged in".to_string(),
            })?
            .pubkey;

        let new_ids: Vec<String> = {
            let mut store_guard = self.store.write().map_err(|e| TenexError::Internal {
                message: format!("Failed to acquire store lock: {}", e),
            })?;

            let store = store_guard.as_mut().ok_or_else(|| TenexError::Internal {
                message: "Store not initialized - call init() first".to_string(),
            })?;

            let mut bookmarked_ids: std::collections::HashSet<String> = store
                .get_bookmarks(&user_pubkey)
                .map(|bl| bl.bookmarked_ids.clone())
                .unwrap_or_default();

            if bookmarked_ids.contains(&item_id) {
                bookmarked_ids.remove(&item_id);
            } else {
                bookmarked_ids.insert(item_id.clone());
            }

            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);

            let updated_list = crate::models::BookmarkList {
                pubkey: user_pubkey.clone(),
                bookmarked_ids: bookmarked_ids.clone(),
                last_updated: now,
            };
            store.set_bookmarks(&user_pubkey, updated_list);

            bookmarked_ids.into_iter().collect()
        };

        let core_handle = get_core_handle(&self.core_handle)?;
        let _ = core_handle.send(NostrCommand::PublishBookmarkList {
            bookmarked_ids: new_ids.clone(),
        });

        Ok(new_ids)
    }

    /// Get online agents for a project from the project status (kind:24010).
    ///
    /// These are actual agent instances with their own Nostr keypairs.
    /// Use these for agent selection in the message composer - the pubkeys
    /// can be used for profile picture lookups and p-tags.
    ///
    /// Returns empty if project not found or project is offline.
    pub fn get_online_agents(&self, project_id: String) -> Result<Vec<ProjectAgent>, TenexError> {
        use crate::tlog;
        tlog!(
            "FFI",
            "get_online_agents called with project_id: {}",
            project_id
        );

        let store_guard = self.store.read().map_err(|e| TenexError::Internal {
            message: format!("Failed to acquire store lock: {}", e),
        })?;

        let store = store_guard.as_ref().ok_or_else(|| TenexError::Internal {
            message: "Store not initialized - call init() first".to_string(),
        })?;

        tlog!(
            "FFI",
            "Total projects in store: {}",
            store.get_projects().len()
        );
        tlog!("FFI", "project_statuses HashMap keys:");
        for key in store.project_statuses.keys() {
            tlog!("FFI", "  - '{}'", key);
        }

        // Find the project by ID
        let project = store
            .get_projects()
            .iter()
            .find(|p| p.id == project_id)
            .cloned();
        let project = match project {
            Some(p) => {
                tlog!("FFI", "Project found: id='{}' a_tag='{}'", p.id, p.a_tag());
                p
            }
            None => {
                tlog!("FFI", "Project NOT found for id: {}", project_id);
                return Ok(Vec::new()); // Project not found = empty agents
            }
        };

        // Get agents from project status (kind:24010)
        tlog!(
            "FFI",
            "Looking up project_statuses for a_tag: '{}'",
            project.a_tag()
        );

        // Check if status exists (even if stale)
        if let Some(status) = store.project_statuses.get(&project.a_tag()) {
            use std::time::{SystemTime, UNIX_EPOCH};
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let age_secs = now.saturating_sub(status.created_at);
            tlog!(
                "FFI",
                "Status exists: created_at={} now={} age={}s is_online={}",
                status.created_at,
                now,
                age_secs,
                status.is_online()
            );
        } else {
            tlog!("FFI", "No status found in project_statuses HashMap");
        }

        let agents = store
            .get_online_agents(&project.a_tag())
            .map(|agents| {
                tlog!("FFI", "Found {} online agents", agents.len());
                for agent in agents {
                    tlog!("FFI", "  Agent: {} ({})", agent.name, agent.pubkey);
                }
                agents.iter().cloned().collect()
            })
            .unwrap_or_else(|| {
                tlog!("FFI", "No online agents (status is stale or missing)");
                Vec::new()
            });

        tlog!("FFI", "Returning {} agents", agents.len());
        Ok(agents)
    }

    /// Get available configuration options for a project.
    ///
    /// Returns all available models and tools from the project status (kind:24010).
    /// Used by iOS to populate the agent config modal with available options.
    pub fn get_project_config_options(
        &self,
        project_id: String,
    ) -> Result<ProjectConfigOptions, TenexError> {
        let store_guard = self.store.read().map_err(|e| TenexError::Internal {
            message: format!("Failed to acquire store lock: {}", e),
        })?;

        let store = store_guard.as_ref().ok_or_else(|| TenexError::Internal {
            message: "Store not initialized - call init() first".to_string(),
        })?;

        // Find the project by ID
        let project = store
            .get_projects()
            .iter()
            .find(|p| p.id == project_id)
            .cloned();
        let project = match project {
            Some(p) => p,
            None => {
                return Err(TenexError::Internal {
                    message: format!("Project not found: {}", project_id),
                })
            }
        };

        // Get project status to extract all_models and all_tools
        let status = store.get_project_status(&project.a_tag());
        match status {
            Some(s) => Ok(ProjectConfigOptions {
                all_models: s.all_models.clone(),
                all_tools: s.all_tools.to_vec(),
            }),
            None => Ok(ProjectConfigOptions {
                all_models: Vec::new(),
                all_tools: Vec::new(),
            }),
        }
    }

    /// Update an agent's configuration (model and tools).
    ///
    /// Publishes a kind:24020 event to update the agent's configuration.
    /// The backend will process this event and update the agent's config.
    pub fn update_agent_config(
        &self,
        project_id: String,
        agent_pubkey: String,
        model: Option<String>,
        tools: Vec<String>,
        tags: Vec<String>,
    ) -> Result<(), TenexError> {
        let project_a_tag = get_project_a_tag(&self.store, &project_id)?;
        let core_handle = get_core_handle(&self.core_handle)?;

        core_handle
            .send(NostrCommand::UpdateAgentConfig {
                project_a_tag,
                agent_pubkey,
                model,
                tools,
                tags,
            })
            .map_err(|e| TenexError::Internal {
                message: format!("Failed to send update agent config command: {}", e),
            })?;

        Ok(())
    }

    /// Update an agent's configuration globally (all projects).
    ///
    /// Publishes a kind:24020 event without a project a-tag.
    pub fn update_global_agent_config(
        &self,
        agent_pubkey: String,
        model: Option<String>,
        tools: Vec<String>,
        tags: Vec<String>,
    ) -> Result<(), TenexError> {
        let core_handle = get_core_handle(&self.core_handle)?;

        core_handle
            .send(NostrCommand::UpdateGlobalAgentConfig {
                agent_pubkey,
                model,
                tools,
                tags,
            })
            .map_err(|e| TenexError::Internal {
                message: format!("Failed to send update global agent config command: {}", e),
            })?;

        Ok(())
    }

    /// Create a new agent definition (kind:4199).
    ///
    /// The created definition is published through the Nostr worker and ingested locally.
    pub fn create_agent_definition(
        &self,
        name: String,
        description: String,
        role: String,
        instructions: String,
        version: String,
        source_id: Option<String>,
        is_fork: bool,
    ) -> Result<(), TenexError> {
        let core_handle = get_core_handle(&self.core_handle)?;

        core_handle
            .send(NostrCommand::CreateAgentDefinition {
                name,
                description,
                role,
                instructions,
                version,
                source_id,
                is_fork,
            })
            .map_err(|e| TenexError::Internal {
                message: format!("Failed to send create agent definition command: {}", e),
            })?;

        Ok(())
    }

    /// Delete an agent definition (kind:4199) via NIP-09 kind:5 deletion.
    pub fn delete_agent_definition(&self, agent_id: String) -> Result<(), TenexError> {
        let store_guard = self.store.read().map_err(|e| TenexError::Internal {
            message: format!("Failed to acquire store lock: {}", e),
        })?;
        let store = store_guard.as_ref().ok_or_else(|| TenexError::Internal {
            message: "Store not initialized - call init() first".to_string(),
        })?;

        let agent = store
            .content
            .get_agent_definition(&agent_id)
            .ok_or_else(|| TenexError::Internal {
                message: format!("Agent definition not found: {}", agent_id),
            })?;

        let current_user = self
            .get_current_user()
            .ok_or_else(|| TenexError::Internal {
                message: "No logged-in user".to_string(),
            })?;

        if !agent.pubkey.eq_ignore_ascii_case(&current_user.pubkey) {
            return Err(TenexError::Internal {
                message: "Only the author can delete this agent definition".to_string(),
            });
        }

        let core_handle = get_core_handle(&self.core_handle)?;

        core_handle
            .send(NostrCommand::DeleteAgentDefinition {
                agent_id,
                client: Some("tenex-ios".to_string()),
            })
            .map_err(|e| TenexError::Internal {
                message: format!("Failed to send delete agent definition command: {}", e),
            })?;

        Ok(())
    }

    /// Delete an agent from a project or globally by publishing a kind:24030 event.
    ///
    /// - `agent_pubkey`: Hex pubkey of the agent to delete.
    /// - `project_a_tag`: Optional project coordinate (`31933:<pubkey>:<d_tag>`).
    ///   - `Some(a_tag)` → scope is "project", event includes the `a` tag.
    ///   - `None` → scope is "global", no `a` tag (backend removes agent from all projects).
    /// - `reason`: Optional reason text placed in event content.
    pub fn delete_agent(
        &self,
        agent_pubkey: String,
        project_a_tag: Option<String>,
        reason: Option<String>,
    ) -> Result<(), TenexError> {
        let core_handle = get_core_handle(&self.core_handle)?;

        core_handle
            .send(NostrCommand::DeleteAgent {
                agent_pubkey,
                project_a_tag,
                reason,
                client: Some("tenex-ios".to_string()),
            })
            .map_err(|e| TenexError::Internal {
                message: format!("Failed to send delete agent command: {}", e),
            })?;

        Ok(())
    }

    /// Get all MCP tool definitions (kind:4200 events).
    ///
    /// Returns tools sorted by created_at descending (newest first).
    pub fn get_all_mcp_tools(&self) -> Result<Vec<MCPTool>, TenexError> {
        let store_guard = self.store.read().map_err(|e| TenexError::Internal {
            message: format!("Failed to acquire store lock: {}", e),
        })?;

        let store = store_guard.as_ref().ok_or_else(|| TenexError::Internal {
            message: "Store not initialized - call init() first".to_string(),
        })?;

        Ok(store.content.get_mcp_tools().into_iter().cloned().collect())
    }
}
