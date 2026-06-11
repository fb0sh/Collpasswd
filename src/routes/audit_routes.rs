use crate::auth::AuthWithDb;
use crate::errors::{AppError, AppResult};
use crate::routes::book_routes::check_book_access;
use axum::{extract::Path, extract::Query, Json};
use serde::Deserialize;

/// Record an audit log entry.
/// Called internally from other route handlers.
pub fn log_action(
    conn: &rusqlite::Connection,
    book_id: i64,
    sheet_id: Option<i64>,
    password_id: Option<i64>,
    user_id: i64,
    username: &str,
    action: &str,
    detail: &str,
) {
    let _ = conn.execute(
        "INSERT INTO audit_logs (book_id, sheet_id, password_id, user_id, username, action, detail) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![book_id, sheet_id, password_id, user_id, username, action, detail],
    );
}

#[derive(Deserialize)]
pub struct AuditQueryParams {
    pub action: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

struct AuditWhere {
    sql: String,
    params: Vec<Box<dyn rusqlite::types::ToSql>>,
}

fn build_audit_where(
    base_where: &str,
    params: &AuditQueryParams,
) -> AuditWhere {
    let mut parts = Vec::new();
    let mut sql_params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    if !base_where.is_empty() {
        parts.push(base_where.to_string());
    }
    if let Some(ref act) = params.action {
        parts.push(format!("a.action = ?{}", sql_params.len() + 1));
        sql_params.push(Box::new(act.clone()));
    }

    let where_sql = if parts.is_empty() { "1=1".to_string() } else { parts.join(" AND ") };
    AuditWhere { sql: where_sql, params: sql_params }
}

/// GET /api/audit — global audit log (admin only)
pub async fn list_audit_global(
    AuthWithDb { user, db, .. }: AuthWithDb,
    Query(params): Query<AuditQueryParams>,
) -> AppResult<serde_json::Value> {
    if user.role != "admin" {
        return Err(AppError::forbidden("Admin access required"));
    }

    let conn = db.get().map_err(AppError::internal)?;
    let limit = params.limit.unwrap_or(50).min(500);
    let offset = params.offset.unwrap_or(0);
    let w = build_audit_where("", &params);

    // Count total
    let count_sql = format!("SELECT COUNT(*) FROM audit_logs a LEFT JOIN books b ON a.book_id = b.id WHERE {}", w.sql);
    let mut stmt = conn.prepare(&count_sql).map_err(AppError::internal)?;
    let refs: Vec<&dyn rusqlite::types::ToSql> = w.params.iter().map(|p| p.as_ref()).collect();
    let total: i64 = stmt.query_row(refs.as_slice(), |row| row.get(0)).map_err(AppError::internal)?;

    // Fetch rows
    let data_sql = format!(
        "SELECT a.id, a.book_id, COALESCE(b.name, '') as book_name,
                a.sheet_id, a.password_id,
                a.user_id, a.username, a.action, a.detail, a.created_at
         FROM audit_logs a
         LEFT JOIN books b ON a.book_id = b.id
         WHERE {} ORDER BY a.created_at DESC LIMIT ?1 OFFSET ?2",
        w.sql
    );

    let mut stmt = conn.prepare(&data_sql).map_err(AppError::internal)?;

    // Build params for the data query: first the where params, then limit/offset
    let mut all_params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    for p in w.params {
        all_params.push(p);
    }
    all_params.push(Box::new(limit));
    all_params.push(Box::new(offset));

    let refs: Vec<&dyn rusqlite::types::ToSql> = all_params.iter().map(|p| p.as_ref()).collect();

    let rows = stmt
        .query_map(refs.as_slice(), |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, i64>(0)?,
                "book_id": row.get::<_, i64>(1)?,
                "book_name": row.get::<_, String>(2)?,
                "sheet_id": row.get::<_, Option<i64>>(3)?,
                "password_id": row.get::<_, Option<i64>>(4)?,
                "user_id": row.get::<_, i64>(5)?,
                "username": row.get::<_, String>(6)?,
                "action": row.get::<_, String>(7)?,
                "detail": row.get::<_, String>(8)?,
                "created_at": row.get::<_, String>(9)?,
            }))
        })
        .map_err(AppError::internal)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(AppError::internal)?;

    Ok(Json(serde_json::json!({
        "records": rows,
        "total": total,
        "limit": limit,
        "offset": offset,
    })))
}

/// GET /api/books/:id/audit — per-book audit log
pub async fn list_audit_book(
    AuthWithDb { user, db, .. }: AuthWithDb,
    Path(book_id): Path<i64>,
    Query(params): Query<AuditQueryParams>,
) -> AppResult<serde_json::Value> {
    let conn = db.get().map_err(AppError::internal)?;

    check_book_access(&conn, book_id, user.id)?;

    let limit = params.limit.unwrap_or(20).min(200);
    let offset = params.offset.unwrap_or(0);
    let base = format!("a.book_id = {}", book_id);
    let w = build_audit_where(&base, &params);

    // Count total
    let count_sql = format!("SELECT COUNT(*) FROM audit_logs a LEFT JOIN books b ON a.book_id = b.id WHERE {}", w.sql);
    let mut stmt = conn.prepare(&count_sql).map_err(AppError::internal)?;
    let refs: Vec<&dyn rusqlite::types::ToSql> = w.params.iter().map(|p| p.as_ref()).collect();
    let total: i64 = stmt.query_row(refs.as_slice(), |row| row.get(0)).map_err(AppError::internal)?;

    // Fetch rows
    let data_sql = format!(
        "SELECT a.id, a.book_id, COALESCE(b.name, '') as book_name,
                a.sheet_id, a.password_id,
                a.user_id, a.username, a.action, a.detail, a.created_at
         FROM audit_logs a
         LEFT JOIN books b ON a.book_id = b.id
         WHERE {} ORDER BY a.created_at DESC LIMIT ?1 OFFSET ?2",
        w.sql
    );

    let mut stmt = conn.prepare(&data_sql).map_err(AppError::internal)?;
    let mut all_params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    for p in w.params {
        all_params.push(p);
    }
    all_params.push(Box::new(limit));
    all_params.push(Box::new(offset));

    let refs: Vec<&dyn rusqlite::types::ToSql> = all_params.iter().map(|p| p.as_ref()).collect();

    let rows = stmt
        .query_map(refs.as_slice(), |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, i64>(0)?,
                "book_id": row.get::<_, i64>(1)?,
                "book_name": row.get::<_, String>(2)?,
                "sheet_id": row.get::<_, Option<i64>>(3)?,
                "password_id": row.get::<_, Option<i64>>(4)?,
                "user_id": row.get::<_, i64>(5)?,
                "username": row.get::<_, String>(6)?,
                "action": row.get::<_, String>(7)?,
                "detail": row.get::<_, String>(8)?,
                "created_at": row.get::<_, String>(9)?,
            }))
        })
        .map_err(AppError::internal)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(AppError::internal)?;

    Ok(Json(serde_json::json!({
        "records": rows,
        "total": total,
        "limit": limit,
        "offset": offset,
    })))
}
