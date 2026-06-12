use crate::auth::AuthWithDb;
use crate::errors::{AppError, AppResult};
use crate::models::*;
use crate::routes::book_routes::{can_edit_book, check_book_access};
use axum::{
    extract::Path,
    Json,
};

/// GET /api/books/:id/sheets - list sheets in a book
pub async fn list_sheets(
    AuthWithDb { user, db, .. }: AuthWithDb,
    Path(book_id): Path<i64>,
) -> AppResult<Vec<Sheet>> {
    let conn = db.get().map_err(AppError::internal)?;

    check_book_access(&conn, book_id, user.id)?;

    let mut stmt = conn.prepare(
        "SELECT s.id, s.book_id, s.name, s.description, s.created_at,
                (SELECT COUNT(*) FROM passwords WHERE sheet_id = s.id) as password_count
         FROM sheets s
         WHERE s.book_id = ?1
         ORDER BY s.created_at ASC",
    ).map_err(AppError::internal)?;

    let sheets = stmt
        .query_map([book_id], |row| {
            Ok(Sheet {
                id: row.get(0)?,
                book_id: row.get(1)?,
                name: row.get(2)?,
                description: row.get::<_, Option<String>>(3)?,
                created_at: row.get(4)?,
                password_count: row.get(5)?,
            })
        })
        .map_err(AppError::internal)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(AppError::internal)?;

    Ok(Json(sheets))
}

/// POST /api/books/:id/sheets - create a new sheet (edit permission required)
pub async fn create_sheet(
    AuthWithDb { user, db, .. }: AuthWithDb,
    Path(book_id): Path<i64>,
    Json(req): Json<CreateSheetRequest>,
) -> AppResult<Sheet> {
    let conn = db.get().map_err(AppError::internal)?;

    check_book_access(&conn, book_id, user.id)?;
    if !can_edit_book(&conn, book_id, user.id)? {
        return Err(AppError::forbidden("You don't have edit permission in this project"));
    }

    if req.name.trim().is_empty() {
        return Err(AppError::bad_request("Sheet name is required"));
    }

    let description = req.description.unwrap_or_default();

    conn.execute(
        "INSERT INTO sheets (book_id, name, description) VALUES (?1, ?2, ?3)",
        rusqlite::params![book_id, req.name.trim(), description],
    ).map_err(AppError::internal)?;

    let sheet_id = conn.last_insert_rowid();

    Ok(Json(Sheet {
        id: sheet_id,
        book_id,
        name: req.name.trim().to_string(),
        description: Some(description).filter(|d| !d.is_empty()),
        created_at: (chrono::Utc::now() + chrono::Duration::hours(8)).format("%Y-%m-%d %H:%M:%S").to_string(),
        password_count: 0,
    }))
}

/// PUT /api/sheets/:id - update a sheet (edit permission required)
pub async fn update_sheet(
    AuthWithDb { user, db, .. }: AuthWithDb,
    Path(sheet_id): Path<i64>,
    Json(req): Json<UpdateSheetRequest>,
) -> AppResult<serde_json::Value> {
    let conn = db.get().map_err(AppError::internal)?;

    let book_id: i64 = conn
        .query_row(
            "SELECT book_id FROM sheets WHERE id = ?1",
            [sheet_id],
            |row| row.get(0),
        )
        .map_err(|_| AppError::not_found("Sheet not found"))?;

    check_book_access(&conn, book_id, user.id)?;
    if !can_edit_book(&conn, book_id, user.id)? {
        return Err(AppError::forbidden("You don't have edit permission in this project"));
    }

    let mut updates = Vec::new();
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    if let Some(name) = &req.name {
        updates.push("name = ?");
        params.push(Box::new(name.trim().to_string()));
    }
    if let Some(desc) = &req.description {
        updates.push("description = ?");
        params.push(Box::new(desc));
    }

    if updates.is_empty() {
        return Err(AppError::bad_request("No fields to update"));
    }

    params.push(Box::new(sheet_id));

    let sql = format!(
        "UPDATE sheets SET {} WHERE id = ?",
        updates.join(", ")
    );

    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();

    conn.execute(&sql, param_refs.as_slice())
        .map_err(AppError::internal)?;

    Ok(Json(serde_json::json!({"success": true})))
}

/// DELETE /api/sheets/:id - delete a sheet (edit permission required)
pub async fn delete_sheet(
    AuthWithDb { user, db, .. }: AuthWithDb,
    Path(sheet_id): Path<i64>,
) -> AppResult<serde_json::Value> {
    let conn = db.get().map_err(AppError::internal)?;

    let book_id: i64 = conn
        .query_row(
            "SELECT book_id FROM sheets WHERE id = ?1",
            [sheet_id],
            |row| row.get(0),
        )
        .map_err(|_| AppError::not_found("Sheet not found"))?;

    check_book_access(&conn, book_id, user.id)?;
    if !can_edit_book(&conn, book_id, user.id)? {
        return Err(AppError::forbidden("You don't have edit permission in this project"));
    }

    conn.execute("DELETE FROM sheets WHERE id = ?1", [sheet_id])
        .map_err(AppError::internal)?;

    Ok(Json(serde_json::json!({"success": true})))
}
