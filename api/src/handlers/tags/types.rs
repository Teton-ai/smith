use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct Tag {
    pub id: i32,
    pub name: String,
    pub color: Option<String>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct NewTag {
    pub name: String,
    pub color: Option<String>,
}
