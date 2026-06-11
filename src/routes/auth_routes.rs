use crate::auth::{create_token, hash_password, verify_password, AppState, AuthenticatedUser};
use crate::errors::{AppError, AppResult};
use crate::models::{CreateUserRequest, LoginRequest, LoginResponse, UserInfo};
use axum::{extract::State, Json};
use std::sync::atomic::Ordering;
use std::sync::Arc;

/// POST /api/auth/login
pub async fn login(
    State(state): State<Arc<AppState>>,
    Json(req): Json<LoginRequest>,
) -> AppResult<LoginResponse> {
    let db = state.db.get().map_err(AppError::internal)?;

    let result = db.query_row(
        "SELECT id, username, password_hash, role FROM users WHERE username = ?1",
        [&req.username],
        |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        },
    );

    match result {
        Ok((id, username, password_hash, role)) => {
            if !verify_password(&req.password, &password_hash)
                .map_err(AppError::internal)?
            {
                return Err(AppError::unauthorized("Invalid username or password"));
            }

            let user = UserInfo {
                id,
                username,
                role,
            };

            let token = create_token(&user, &state.jwt_secret.0, state.jwt_expiry_hours.load(Ordering::Relaxed))
                .map_err(AppError::internal)?;

            Ok(Json(LoginResponse { token, user }))
        }
        Err(_) => Err(AppError::unauthorized("Invalid username or password")),
    }
}

/// POST /api/auth/register — self-registration, always creates a regular user
pub async fn register(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateUserRequest>,
) -> AppResult<LoginResponse> {
    let db = state.db.get().map_err(AppError::internal)?;

    // Self-registration always creates a regular user, never admin
    let role = "user";

    let password_hash = hash_password(&req.password).map_err(AppError::internal)?;

    match db.execute(
        "INSERT INTO users (username, password_hash, role) VALUES (?1, ?2, ?3)",
        rusqlite::params![&req.username, &password_hash, role],
    ) {
        Ok(_) => {
            let id = db.last_insert_rowid();
            let user = UserInfo {
                id,
                username: req.username,
                role: role.to_string(),
            };
            let token = create_token(&user, &state.jwt_secret.0, state.jwt_expiry_hours.load(Ordering::Relaxed))
                .map_err(AppError::internal)?;
            Ok(Json(LoginResponse { token, user }))
        }
        Err(e) => {
            if e.to_string().contains("UNIQUE") {
                Err(AppError::bad_request("Username already exists"))
            } else {
                Err(AppError::internal(e.to_string()))
            }
        }
    }
}

/// POST /api/auth/admin-login - admin login with password only
pub async fn admin_login(
    State(state): State<Arc<AppState>>,
    Json(req): Json<LoginRequest>,
) -> AppResult<LoginResponse> {
    let db = state.db.get().map_err(AppError::internal)?;

    // Find any admin user
    let result = db.query_row(
        "SELECT id, username, password_hash, role FROM users WHERE role = 'admin' ORDER BY id ASC LIMIT 1",
        [],
        |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        },
    );

    match result {
        Ok((id, username, password_hash, role)) => {
            if !verify_password(&req.password, &password_hash)
                .map_err(AppError::internal)?
            {
                return Err(AppError::unauthorized("管理员密码错误"));
            }

            let user = UserInfo {
                id,
                username,
                role,
            };

            let token = create_token(&user, &state.jwt_secret.0, state.jwt_expiry_hours.load(Ordering::Relaxed))
                .map_err(AppError::internal)?;

            Ok(Json(LoginResponse { token, user }))
        }
        Err(_) => Err(AppError::not_found("没有找到管理员账号，请使用 --reset-password 创建")),
    }
}

/// GET /api/auth/me
pub async fn me(user: AuthenticatedUser) -> AppResult<UserInfo> {
    Ok(Json(UserInfo {
        id: user.id,
        username: user.username,
        role: user.role,
    }))
}
