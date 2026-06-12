use crate::auth::AuthWithDb;
use crate::crypto::EccCrypto;
use crate::errors::{AppError, AppResult};
use crate::models::*;
use crate::routes::audit_routes::log_action;
use crate::routes::book_routes::{can_edit_book, check_book_access};
use axum::{
    extract::Path,
    Json,
};
use zeroize::Zeroize;

/// GET /api/sheets/:id/passwords - list passwords (titles only, NO plaintext passwords)
pub async fn list_passwords(
    AuthWithDb { user, db, .. }: AuthWithDb,
    Path(sheet_id): Path<i64>,
) -> AppResult<Vec<PasswordEntry>> {
    let conn = db.get().map_err(AppError::internal)?;

    let book_id: i64 = conn
        .query_row(
            "SELECT book_id FROM sheets WHERE id = ?1",
            [sheet_id],
            |row| row.get(0),
        )
        .map_err(|_| AppError::not_found("Sheet not found"))?;

    check_book_access(&conn, book_id, user.id)?;

    let mut stmt = conn.prepare(
        "SELECT id, sheet_id, title, username, url, notes, updated_at, COALESCE(updated_by_username, '')
         FROM passwords
         WHERE sheet_id = ?1
         ORDER BY updated_at DESC",
    ).map_err(AppError::internal)?;

    let passwords = stmt
        .query_map([sheet_id], |row| {
            Ok(PasswordEntry {
                id: row.get(0)?,
                sheet_id: row.get(1)?,
                title: row.get(2)?,
                username: row.get::<_, Option<String>>(3)?.filter(|s| !s.is_empty()),
                url: row.get::<_, Option<String>>(4)?.filter(|s| !s.is_empty()),
                notes: row.get::<_, Option<String>>(5)?.filter(|s| !s.is_empty()),
                updated_at: row.get(6)?,
                updated_by_username: row.get(7)?,
                has_password: true,
            })
        })
        .map_err(AppError::internal)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(AppError::internal)?;

    Ok(Json(passwords))
}

/// GET /api/sheets/:id/passwords/:pid - get a specific password (ECC-decrypted on-demand)
pub async fn get_password(
    AuthWithDb { user, db, master_key }: AuthWithDb,
    Path((sheet_id, password_id)): Path<(i64, i64)>,
) -> AppResult<PasswordDetail> {
    let conn = db.get().map_err(AppError::internal)?;

    let book_id: i64 = conn
        .query_row(
            "SELECT book_id FROM sheets WHERE id = ?1",
            [sheet_id],
            |row| row.get(0),
        )
        .map_err(|_| AppError::not_found("Sheet not found"))?;

    check_book_access(&conn, book_id, user.id)?;

    let ecc_private_key: String = conn
        .query_row(
            "SELECT ecc_private_key FROM books WHERE id = ?1",
            [book_id],
            |row| row.get(0),
        )
        .map_err(|_| AppError::not_found("Book not found"))?;

    let (id, sheet_id_val, title, username, encrypted_pass, url, notes, updated_at, updated_by_username): (
        i64, i64, String, Option<String>, String, Option<String>, Option<String>, String, String
    ) = conn.query_row(
        "SELECT id, sheet_id, title, username, encrypted_password, url, notes, updated_at, COALESCE(updated_by_username, '')
         FROM passwords WHERE id = ?1 AND sheet_id = ?2",
        rusqlite::params![password_id, sheet_id],
        |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get(7)?,
                row.get(8)?,
            ))
        },
    ).map_err(|_| AppError::not_found("Password entry not found"))?;

    // Decrypt the book's ECC private key with the master key
    let mut sk_pem = master_key
        .decrypt_private_key(&ecc_private_key)
        .map_err(|e| AppError::internal(format!("Failed to decrypt book key: {}", e)))?;

    let sk = EccCrypto::secret_key_from_pem(&sk_pem)
        .map_err(|e| AppError::internal(format!("Failed to load book key: {}", e)))?;
    sk_pem.zeroize(); // ← ECC 私钥 PEM 不再需要，立即归零

    let decrypted = EccCrypto::decrypt(&sk, &encrypted_pass)
        .map_err(|e| AppError::internal(format!("Failed to decrypt password: {}", e)))?;
    // sk (SecretKey) dropped here — p256 的 Drop 自带 zeroize

    let password_str = String::from_utf8(decrypted)
        .map_err(|_| AppError::internal("Decrypted password is not valid UTF-8"))?;

    // Audit log: password viewed
    let detail = format!("{}：{}", title, username.as_deref().unwrap_or("—"));
    log_action(&conn, book_id, Some(sheet_id), Some(id), user.id, &user.username, "view_password", &detail);

    Ok(Json(PasswordDetail {
        id,
        sheet_id: sheet_id_val,
        title,
        username: username.filter(|s| !s.is_empty()),
        password: password_str,
        url: url.filter(|s| !s.is_empty()),
        notes: notes.filter(|s| !s.is_empty()),
        updated_at,
        updated_by_username,
    }))
}

/// POST /api/sheets/:id/passwords - create a new password entry
pub async fn create_password(
    AuthWithDb { user, db, .. }: AuthWithDb,
    Path(sheet_id): Path<i64>,
    Json(req): Json<CreatePasswordRequest>,
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

    if req.title.trim().is_empty() {
        return Err(AppError::bad_request("Title is required"));
    }

    if req.password.is_empty() {
        return Err(AppError::bad_request("Password is required"));
    }

    let ecc_public_key: String = conn
        .query_row(
            "SELECT ecc_public_key FROM books WHERE id = ?1",
            [book_id],
            |row| row.get(0),
        )
        .map_err(|_| AppError::not_found("Book not found"))?;

    let pk = EccCrypto::public_key_from_pem(&ecc_public_key)
        .map_err(|e| AppError::internal(format!("Failed to load book public key: {}", e)))?;

    let encrypted = EccCrypto::encrypt(&pk, req.password.as_bytes())
        .map_err(|e| AppError::internal(format!("Failed to encrypt password: {}", e)))?;

    let username = req.username.unwrap_or_default();
    let url = req.url.unwrap_or_default();
    let notes = req.notes.unwrap_or_default();

    conn.execute(
        "INSERT INTO passwords (sheet_id, title, username, encrypted_password, url, notes, updated_by_user_id, updated_by_username, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, datetime('now', '+8 hours'))",
        rusqlite::params![sheet_id, req.title.trim(), username, encrypted, url, notes, user.id, &user.username],
    ).map_err(AppError::internal)?;

    let password_id = conn.last_insert_rowid();

    // Audit log: password created
    let create_detail = format!("{}：{}", req.title.trim(), if username.is_empty() { "—" } else { &username });
    log_action(&conn, book_id, Some(sheet_id), Some(password_id), user.id, &user.username, "create_password", &create_detail);

    Ok(Json(serde_json::json!({
        "success": true,
        "id": password_id,
    })))
}

/// PUT /api/passwords/:id - update a password entry
pub async fn update_password(
    AuthWithDb { user, db, .. }: AuthWithDb,
    Path(password_id): Path<i64>,
    Json(req): Json<UpdatePasswordRequest>,
) -> AppResult<serde_json::Value> {
    let conn = db.get().map_err(AppError::internal)?;

    let (_sheet_id, book_id): (i64, i64) = conn
        .query_row(
            "SELECT p.sheet_id, s.book_id FROM passwords p JOIN sheets s ON p.sheet_id = s.id WHERE p.id = ?1",
            [password_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|_| AppError::not_found("Password entry not found"))?;

    check_book_access(&conn, book_id, user.id)?;
    if !can_edit_book(&conn, book_id, user.id)? {
        return Err(AppError::forbidden("You don't have edit permission in this project"));
    }

    let mut updates = Vec::new();
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    if let Some(title) = &req.title {
        updates.push("title = ?");
        params.push(Box::new(title.trim().to_string()));
    }
    if let Some(username) = &req.username {
        updates.push("username = ?");
        params.push(Box::new(username.clone()));
    }
    if let Some(password) = &req.password {
        let ecc_public_key: String = conn
            .query_row(
                "SELECT ecc_public_key FROM books WHERE id = ?1",
                [book_id],
                |row| row.get(0),
            )
            .map_err(|_| AppError::not_found("Book not found"))?;

        let pk = EccCrypto::public_key_from_pem(&ecc_public_key)
            .map_err(|e| AppError::internal(format!("Failed to load book public key: {}", e)))?;

        let encrypted = EccCrypto::encrypt(&pk, password.as_bytes())
            .map_err(|e| AppError::internal(format!("Failed to encrypt password: {}", e)))?;

        updates.push("encrypted_password = ?");
        params.push(Box::new(encrypted));
    }
    if let Some(url) = &req.url {
        updates.push("url = ?");
        params.push(Box::new(url.clone()));
    }
    if let Some(notes) = &req.notes {
        updates.push("notes = ?");
        params.push(Box::new(notes.clone()));
    }

    if updates.is_empty() {
        return Err(AppError::bad_request("No fields to update"));
    }

    // Get current title and username for audit log
    let (current_title, current_pw_user): (String, Option<String>) = conn
        .query_row(
            "SELECT title, username FROM passwords WHERE id = ?1",
            [password_id],
            |row| Ok((row.get(0)?, row.get::<_, Option<String>>(1)?)),
        )
        .map_err(|_| AppError::not_found("Password not found"))?;

    updates.push("updated_at = datetime('now', '+8 hours')");
    updates.push("updated_by_user_id = ?");
    params.push(Box::new(user.id));
    updates.push("updated_by_username = ?");
    params.push(Box::new(user.username.clone()));
    params.push(Box::new(password_id));

    let sql = format!(
        "UPDATE passwords SET {} WHERE id = ?",
        updates.join(", ")
    );

    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();

    conn.execute(&sql, param_refs.as_slice())
        .map_err(AppError::internal)?;

    // Audit log: password updated
    let update_detail = format!("{}：{}", current_title, current_pw_user.as_deref().unwrap_or("—"));
    log_action(&conn, book_id, Some(_sheet_id), Some(password_id), user.id, &user.username, "update_password", &update_detail);

    Ok(Json(serde_json::json!({"success": true})))
}

/// DELETE /api/passwords/:id - delete a password entry
pub async fn delete_password(
    AuthWithDb { user, db, .. }: AuthWithDb,
    Path(password_id): Path<i64>,
) -> AppResult<serde_json::Value> {
    let conn = db.get().map_err(AppError::internal)?;

    let (_sheet_id, book_id): (i64, i64) = conn
        .query_row(
            "SELECT p.sheet_id, s.book_id FROM passwords p JOIN sheets s ON p.sheet_id = s.id WHERE p.id = ?1",
            [password_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|_| AppError::not_found("Password entry not found"))?;

    check_book_access(&conn, book_id, user.id)?;
    if !can_edit_book(&conn, book_id, user.id)? {
        return Err(AppError::forbidden("You don't have edit permission in this project"));
    }

    // Get title and username for audit log before deleting
    let (del_title, del_user): (String, Option<String>) = conn
        .query_row(
            "SELECT title, username FROM passwords WHERE id = ?1",
            [password_id],
            |row| Ok((row.get(0)?, row.get::<_, Option<String>>(1)?)),
        )
        .map_err(|_| AppError::not_found("Password not found"))?;

    conn.execute("DELETE FROM passwords WHERE id = ?1", [password_id])
        .map_err(AppError::internal)?;

    // Audit log: password deleted
    let delete_detail = format!("{}：{}", del_title, del_user.as_deref().unwrap_or("—"));
    log_action(&conn, book_id, Some(_sheet_id), Some(password_id), user.id, &user.username, "delete_password", &delete_detail);

    Ok(Json(serde_json::json!({"success": true})))
}
