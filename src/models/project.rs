use nostrdb::Note;

#[derive(Debug, Clone)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub pubkey: String,
    pub participants: Vec<String>,
}

impl Project {
    pub fn from_note(note: &Note) -> Option<Self> {
        if note.kind() != 31933 {
            return None;
        }

        let pubkey = hex::encode(note.pubkey());

        let mut id: Option<String> = None;
        let mut name: Option<String> = None;
        let mut participants = Vec::new();

        for tag in note.tags() {
            let tag_name = tag.get(0).and_then(|t| t.variant().str());
            match tag_name {
                Some("d") => {
                    id = tag.get(1).and_then(|t| t.variant().str()).map(|s| s.to_string());
                }
                Some("name") => {
                    name = tag.get(1).and_then(|t| t.variant().str()).map(|s| s.to_string());
                }
                Some("p") => {
                    if let Some(p) = tag.get(1).and_then(|t| t.variant().str()) {
                        participants.push(p.to_string());
                    }
                }
                _ => {}
            }
        }

        let id = id?;

        Some(Project {
            id: id.clone(),
            name: name.unwrap_or_else(|| id.clone()),
            pubkey,
            participants,
        })
    }

    pub fn a_tag(&self) -> String {
        format!("31933:{}:{}", self.pubkey, self.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_a_tag() {
        let project = Project {
            id: "proj1".to_string(),
            name: "Project 1".to_string(),
            pubkey: "a".repeat(64),
            participants: vec![],
        };

        assert_eq!(project.a_tag(), format!("31933:{}:proj1", "a".repeat(64)));
    }
}
