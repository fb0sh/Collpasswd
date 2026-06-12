use serde::{Deserialize, Serialize};

// ─── Auth ───────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LoginResponse {
    pub token: String,
    pub user: UserInfo,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UserInfo {
    pub id: i64,
    pub username: String,
    pub role: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: i64,       // user id
    pub username: String,
    pub role: String,
    pub exp: usize,
}

// ─── Books (projects) ──────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BookSummary {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub created_at: String,
    pub created_by: i64,
    pub member_count: i64,
    pub is_holder: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BookDetail {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub created_at: String,
    pub created_by: i64,
    pub member_count: i64,
    pub is_holder: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateBookRequest {
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateBookRequest {
    pub name: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BookMember {
    pub id: i64,
    pub user_id: i64,
    pub username: String,
    pub role: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AddMemberRequest {
    pub username: String,
    pub role: String, // "admin", "editor", "viewer"
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateMemberRoleRequest {
    pub role: String, // "edit" or "view"
}

// ─── Sheets ─────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Sheet {
    pub id: i64,
    pub book_id: i64,
    pub name: String,
    pub description: Option<String>,
    pub created_at: String,
    pub password_count: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateSheetRequest {
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateSheetRequest {
    pub name: Option<String>,
    pub description: Option<String>,
}

// ─── Passwords ──────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PasswordEntry {
    pub id: i64,
    pub sheet_id: i64,
    pub title: String,
    pub username: Option<String>,
    pub url: Option<String>,
    pub notes: Option<String>,
    pub updated_at: String,
    pub updated_by_username: String,
    /// encrypted_password is NEVER included in list responses
    pub has_password: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PasswordDetail {
    pub id: i64,
    pub sheet_id: i64,
    pub title: String,
    pub username: Option<String>,
    pub password: String,
    pub url: Option<String>,
    pub notes: Option<String>,
    pub updated_at: String,
    pub updated_by_username: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreatePasswordRequest {
    pub title: String,
    pub username: Option<String>,
    pub password: String,
    pub url: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
    pub struct UpdatePasswordRequest {
    pub title: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub url: Option<String>,
    pub notes: Option<String>,
}

// ─── Admin ──────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateUserRequest {
    pub username: String,
    pub password: String,
    pub role: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateRoleRequest {
    pub role: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UserRecord {
    pub id: i64,
    pub username: String,
    pub role: String,
    pub created_at: String,
}

// ─── Audit ──────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct AuditLogEntry {
    pub id: i64,
    pub book_id: i64,
    pub sheet_id: Option<i64>,
    pub password_id: Option<i64>,
    pub user_id: i64,
    pub username: String,
    pub action: String,
    pub detail: String,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AuditQuery {
    pub book_id: Option<i64>,
    pub action: Option<String>,
    pub limit: Option<i64>,
}

// ─── Generic response ──────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
#[allow(dead_code)]
pub struct ApiResponse<T: Serialize> {
    pub success: bool,
    pub data: Option<T>,
    pub error: Option<String>,
}

impl<T: Serialize> ApiResponse<T> {
    pub fn ok(data: T) -> Self {
        Self { success: true, data: Some(data), error: None }
    }

    pub fn err(msg: &str) -> Self {
        Self { success: false, data: None, error: Some(msg.to_string()) }
    }
}
