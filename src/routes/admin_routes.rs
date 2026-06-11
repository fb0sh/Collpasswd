use crate::auth::{hash_password, AdminUser, AppState, AuthenticatedUser};
use crate::errors::{AppError, AppResult};
use crate::models::{CreateUserRequest, UserRecord};
use axum::{extract::State, Json};
use serde::Serialize;
use std::sync::Arc;

/// Lightweight user info for the member picker
#[derive(Serialize)]
pub struct UserBrief {
    pub id: i64,
    pub username: String,
}

/// GET /api/users — list all users (any authenticated user, for member picker dropdown)
pub async fn list_users_brief(
    _user: AuthenticatedUser,
    State(state): State<Arc<AppState>>,
) -> AppResult<Vec<UserBrief>> {
    let conn = state.db.get().map_err(AppError::internal)?;

    let mut stmt = conn
        .prepare("SELECT id, username FROM users ORDER BY username ASC")
        .map_err(AppError::internal)?;

    let users = stmt
        .query_map([], |row| {
            Ok(UserBrief {
                id: row.get(0)?,
                username: row.get(1)?,
            })
        })
        .map_err(AppError::internal)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(AppError::internal)?;

    Ok(Json(users))
}

/// GET /api/admin/users - list all users (admin only)
pub async fn list_users(
    _admin: AdminUser,
    State(state): State<Arc<AppState>>,
) -> AppResult<Vec<UserRecord>> {
    let conn = state.db.get().map_err(AppError::internal)?;

    let mut stmt = conn
        .prepare("SELECT id, username, role, created_at FROM users ORDER BY id ASC")
        .map_err(AppError::internal)?;

    let users = stmt
        .query_map([], |row| {
            Ok(UserRecord {
                id: row.get(0)?,
                username: row.get(1)?,
                role: row.get(2)?,
                created_at: row.get(3)?,
            })
        })
        .map_err(AppError::internal)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(AppError::internal)?;

    Ok(Json(users))
}

/// POST /api/admin/users - create a user (admin only)
pub async fn create_user(
    _admin: AdminUser,
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateUserRequest>,
) -> AppResult<UserRecord> {
    if req.username.trim().is_empty() {
        return Err(AppError::bad_request("Username is required"));
    }
    if req.password.len() < 6 {
        return Err(AppError::bad_request("Password must be at least 6 characters"));
    }

    // All created users are regular users; only the built-in admin account has admin role.
    let role = "user";

    let conn = state.db.get().map_err(AppError::internal)?;

    let password_hash = hash_password(&req.password).map_err(AppError::internal)?;

    match conn.execute(
        "INSERT INTO users (username, password_hash, role) VALUES (?1, ?2, ?3)",
        rusqlite::params![req.username.trim(), &password_hash, role],
    ) {
        Ok(_) => {
            let id = conn.last_insert_rowid();
            let record = UserRecord {
                id,
                username: req.username.trim().to_string(),
                role: role.to_string(),
                created_at: chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            };
            Ok(Json(record))
        }
        Err(e) if e.to_string().contains("UNIQUE") => {
            Err(AppError::bad_request("Username already exists"))
        }
        Err(e) => Err(AppError::internal(e.to_string())),
    }
}

/// DELETE /api/admin/users/:id - delete a user (admin only)
pub async fn delete_user(
    _admin: AdminUser,
    State(state): State<Arc<AppState>>,
    axum::extract::Path(user_id): axum::extract::Path<i64>,
) -> AppResult<serde_json::Value> {
    // Don't allow deleting yourself
    if user_id == _admin.0.id {
        return Err(AppError::bad_request("Cannot delete yourself"));
    }

    let conn = state.db.get().map_err(AppError::internal)?;

    // Protect the built-in admin account (created at first startup)
    let username: String = conn
        .query_row("SELECT username FROM users WHERE id = ?1", [user_id], |row| row.get(0))
        .map_err(|_| AppError::not_found("User not found"))?;
    if username == "admin" {
        return Err(AppError::forbidden("Cannot delete the built-in admin account"));
    }

    let affected = conn
        .execute("DELETE FROM users WHERE id = ?1", [user_id])
        .map_err(AppError::internal)?;

    if affected == 0 {
        return Err(AppError::not_found("User not found"));
    }

    Ok(Json(serde_json::json!({"success": true})))
}

/// GET /api/admin/settings — get platform settings (admin only)
pub async fn get_settings(
    _admin: AdminUser,
    State(state): State<Arc<AppState>>,
) -> AppResult<serde_json::Value> {
    let conn = state.db.get().map_err(AppError::internal)?;

    let jwt_expiry_hours = conn
        .query_row("SELECT value FROM config WHERE key = 'jwt_expiry_hours'", [], |row| row.get::<_, String>(0))
        .ok()
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(24);

    Ok(Json(serde_json::json!({
        "jwt_expiry_hours": jwt_expiry_hours,
    })))
}

/// PUT /api/admin/settings — update platform settings (admin only)
pub async fn update_settings(
    _admin: AdminUser,
    State(state): State<Arc<AppState>>,
    Json(body): Json<serde_json::Value>,
) -> AppResult<serde_json::Value> {
    let conn = state.db.get().map_err(AppError::internal)?;

    if let Some(hours) = body.get("jwt_expiry_hours").and_then(|v| v.as_i64()) {
        if hours < 1 || hours > 720 {
            return Err(AppError::bad_request("Login expiry must be between 1 and 720 hours"));
        }
        conn.execute(
            "INSERT OR REPLACE INTO config (key, value) VALUES ('jwt_expiry_hours', ?1)",
            rusqlite::params![hours.to_string()],
        ).map_err(AppError::internal)?;

        // Update in-memory value
        state.jwt_expiry_hours.store(hours, std::sync::atomic::Ordering::Relaxed);
    }

    // Return updated settings
    let jwt_expiry_hours = state.jwt_expiry_hours.load(std::sync::atomic::Ordering::Relaxed);

    Ok(Json(serde_json::json!({
        "success": true,
        "jwt_expiry_hours": jwt_expiry_hours,
    })))
}
