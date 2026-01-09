use crate::models::{Message, ProjectStatus};

#[derive(Debug)]
pub enum CoreEvent {
    Message(Message),
    ProjectStatus(ProjectStatus),
}
