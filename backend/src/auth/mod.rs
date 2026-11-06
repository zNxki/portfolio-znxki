pub mod cookies;
pub mod extractors;
pub mod github;
pub mod session;
pub mod states;

use uuid::Uuid;

use crate::models::Role;

#[derive(Debug, Clone)]
pub struct CurrentUser {
    pub id: Uuid,
    pub github_id: i64,
    pub login: String,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub role: Role,
    pub session_id: Uuid,
    pub csrf: String,
}

impl CurrentUser {
    pub fn is_admin(&self) -> bool {
        self.role == Role::Admin
    }
}