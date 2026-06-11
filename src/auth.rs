use crate::crypto::MasterKey;
use crate::db::DbPool;
use crate::models::Claims;
use argon2::password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use axum::{
    async_trait,
    extract::FromRequestParts,
    http::{request::Parts, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};

use std::sync::Arc;

/// Secret key for JWT signing - generated at startup
pub struct JwtSecret(pub String);

/// Shared application state
#[derive(Clone)]
pub struct AppState {
    pub db: DbPool,
    pub jwt_secret: Arc<JwtSecret>,
    pub master_key: MasterKey,
    pub jwt_expiry_hours: Arc<std::sync::atomic::AtomicI64>,
}

/// Hash a password with Argon2id
pub fn hash_password(password: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let hash = argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("Password hashing failed: {:?}", e))?;
    Ok(hash.to_string())
}

/// Verify a password against its hash
pub fn verify_password(password: &str, hash: &str) -> anyhow::Result<bool> {
    let parsed_hash = PasswordHash::new(hash)
        .map_err(|e| anyhow::anyhow!("Invalid password hash: {:?}", e))?;
    let argon2 = Argon2::default();
    Ok(argon2
        .verify_password(password.as_bytes(), &parsed_hash)
        .is_ok())
}

/// Create a JWT token for a user
pub fn create_token(user: &crate::models::UserInfo, secret: &str, expiry_hours: i64) -> anyhow::Result<String> {
    let now = chrono::Utc::now();
    let exp = (now + chrono::Duration::hours(expiry_hours)).timestamp() as usize;

    let claims = Claims {
        sub: user.id,
        username: user.username.clone(),
        role: user.role.clone(),
        exp,
    };

    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )?;
    Ok(token)
}

/// Validate a JWT token and extract claims
pub fn validate_token(token: &str, secret: &str) -> anyhow::Result<Claims> {
    let token_data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )?;
    Ok(token_data.claims)
}

/// Extract a Bearer token from Authorization header
fn extract_bearer_token(parts: &Parts) -> Result<String, AuthRejection> {
    let auth_header = parts
        .headers
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .ok_or(AuthRejection::MissingToken)?;

    auth_header
        .strip_prefix("Bearer ")
        .map(|s| s.to_string())
        .ok_or(AuthRejection::InvalidFormat)
}

// ─── Auth Extractor ─────────────────────────────────────────────────────────

/// Extract the authenticated user from a request
#[derive(Debug, Clone)]
pub struct AuthenticatedUser {
    pub id: i64,
    pub username: String,
    pub role: String,
}

#[async_trait]
impl FromRequestParts<Arc<AppState>> for AuthenticatedUser {
    type Rejection = AuthRejection;

    async fn from_request_parts(parts: &mut Parts, state: &Arc<AppState>) -> Result<Self, Self::Rejection> {
        let token = extract_bearer_token(parts)?;
        let claims = validate_token(&token, &state.jwt_secret.0)
            .map_err(|_| AuthRejection::InvalidToken)?;

        Ok(AuthenticatedUser {
            id: claims.sub,
            username: claims.username,
            role: claims.role,
        })
    }
}

/// Extract the authenticated user, DB pool, and master key from a request
#[derive(Debug, Clone)]
pub struct AuthWithDb {
    pub user: AuthenticatedUser,
    pub db: DbPool,
    pub master_key: MasterKey,
}

#[async_trait]
impl FromRequestParts<Arc<AppState>> for AuthWithDb {
    type Rejection = AuthRejection;

    async fn from_request_parts(parts: &mut Parts, state: &Arc<AppState>) -> Result<Self, Self::Rejection> {
        let user = AuthenticatedUser::from_request_parts(parts, state).await?;
        Ok(AuthWithDb {
            user,
            db: state.db.clone(),
            master_key: state.master_key.clone(),
        })
    }
}

/// Admin-only extractor
#[derive(Debug, Clone)]
pub struct AdminUser(pub AuthenticatedUser);

#[async_trait]
impl FromRequestParts<Arc<AppState>> for AdminUser {
    type Rejection = AuthRejection;

    async fn from_request_parts(parts: &mut Parts, state: &Arc<AppState>) -> Result<Self, Self::Rejection> {
        let user = AuthenticatedUser::from_request_parts(parts, state).await?;
        if user.role != "admin" {
            return Err(AuthRejection::Forbidden);
        }
        Ok(AdminUser(user))
    }
}

// ─── Rejection types ───────────────────────────────────────────────────────

#[derive(Debug)]
pub enum AuthRejection {
    MissingToken,
    InvalidFormat,
    InvalidToken,
    Forbidden,
}

impl std::fmt::Display for AuthRejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthRejection::MissingToken => write!(f, "Missing authorization token"),
            AuthRejection::InvalidFormat => write!(f, "Invalid authorization format, expected Bearer"),
            AuthRejection::InvalidToken => write!(f, "Invalid or expired token"),
            AuthRejection::Forbidden => write!(f, "Forbidden: admin access required"),
        }
    }
}

impl IntoResponse for AuthRejection {
    fn into_response(self) -> Response {
        let (status, msg) = match &self {
            AuthRejection::MissingToken
            | AuthRejection::InvalidFormat
            | AuthRejection::InvalidToken => (StatusCode::UNAUTHORIZED, self.to_string()),
            AuthRejection::Forbidden => (StatusCode::FORBIDDEN, self.to_string()),
        };

        let body = serde_json::json!({
            "success": false,
            "error": msg,
        });

        (status, Json(body)).into_response()
    }
}
