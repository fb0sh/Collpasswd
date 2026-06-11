use crate::auth::AuthWithDb;
use crate::crypto::EccCrypto;
use crate::errors::{AppError, AppResult};
use crate::models::*;
use crate::routes::audit_routes::log_action;
use crate::routes::book_routes::{can_edit_book, check_book_access};
use axum::{extract::Path, Json};
use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

/// A single password entry for import/export
#[derive(Debug, Serialize, Deserialize)]
pub struct IoPassword {
    pub title: String,
    pub username: Option<String>,
    pub password: String,
    pub url: Option<String>,
    pub notes: Option<String>,
}

/// GET /api/sheets/:id/export — export passwords from a sheet as JSON
pub async fn export_sheet(
    AuthWithDb { user, db, master_key }: AuthWithDb,
    Path(sheet_id): Path<i64>,
) -> AppResult<Vec<IoPassword>> {
    let conn = db.get().map_err(AppError::internal)?;

    let book_id: i64 = conn
        .query_row("SELECT book_id FROM sheets WHERE id = ?1", [sheet_id], |row| row.get(0))
        .map_err(|_| AppError::not_found("Sheet not found"))?;

    check_book_access(&conn, book_id, user.id)?;

    let ecc_private_key: String = conn
        .query_row("SELECT ecc_private_key FROM books WHERE id = ?1", [book_id], |row| row.get(0))
        .map_err(|_| AppError::not_found("Book not found"))?;

    let mut sk_pem = master_key
        .decrypt_private_key(&ecc_private_key)
        .map_err(|e| AppError::internal(format!("Failed to decrypt book key: {}", e)))?;

    let sk = EccCrypto::secret_key_from_pem(&sk_pem)
        .map_err(|e| AppError::internal(format!("Failed to load book key: {}", e)))?;
    sk_pem.zeroize();

    let mut stmt = conn.prepare(
        "SELECT title, username, encrypted_password, url, notes FROM passwords WHERE sheet_id = ?1 ORDER BY title"
    ).map_err(AppError::internal)?;

    let rows = stmt
        .query_map([sheet_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
            ))
        })
        .map_err(AppError::internal)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(AppError::internal)?;

    let mut result = Vec::new();
    for (title, username, encrypted, url, notes) in rows {
        match EccCrypto::decrypt(&sk, &encrypted) {
            Ok(plaintext) => {
                if let Ok(pw_str) = String::from_utf8(plaintext) {
                    result.push(IoPassword {
                        title,
                        username: username.filter(|s| !s.is_empty()),
                        password: pw_str,
                        url: url.filter(|s| !s.is_empty()),
                        notes: notes.filter(|s| !s.is_empty()),
                    });
                }
            }
            Err(_) => {
                result.push(IoPassword {
                    title,
                    username: username.filter(|s| !s.is_empty()),
                    password: "⚠️ DECRYPT FAILED".to_string(),
                    url: url.filter(|s| !s.is_empty()),
                    notes: notes.filter(|s| !s.is_empty()),
                });
            }
        }
    }

    Ok(Json(result))
}

/// GET /api/books/:id/export — export all passwords from a book as JSON (grouped by sheet)
pub async fn export_book(
    AuthWithDb { user, db, master_key }: AuthWithDb,
    Path(book_id): Path<i64>,
) -> AppResult<Vec<serde_json::Value>> {
    let conn = db.get().map_err(AppError::internal)?;

    check_book_access(&conn, book_id, user.id)?;

    let ecc_private_key: String = conn
        .query_row("SELECT ecc_private_key FROM books WHERE id = ?1", [book_id], |row| row.get(0))
        .map_err(|_| AppError::not_found("Book not found"))?;

    let mut sk_pem = master_key
        .decrypt_private_key(&ecc_private_key)
        .map_err(|e| AppError::internal(format!("Failed to decrypt book key: {}", e)))?;

    let sk = EccCrypto::secret_key_from_pem(&sk_pem)
        .map_err(|e| AppError::internal(format!("Failed to load book key: {}", e)))?;
    sk_pem.zeroize();

    let mut sheet_stmt = conn.prepare(
        "SELECT id, name FROM sheets WHERE book_id = ?1 ORDER BY name"
    ).map_err(AppError::internal)?;

    let sheets: Vec<(i64, String)> = sheet_stmt
        .query_map([book_id], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))
        .map_err(AppError::internal)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(AppError::internal)?;

    let mut result = Vec::new();
    for (sid, sname) in sheets {
        let mut stmt = conn.prepare(
            "SELECT title, username, encrypted_password, url, notes FROM passwords WHERE sheet_id = ?1 ORDER BY title"
        ).map_err(AppError::internal)?;

        let rows = stmt
            .query_map([sid], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                ))
            })
            .map_err(AppError::internal)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(AppError::internal)?;

        if rows.is_empty() { continue; }

        let mut entries = Vec::new();
        for (title, username, encrypted, url, notes) in rows {
            match EccCrypto::decrypt(&sk, &encrypted) {
                Ok(plaintext) => {
                    if let Ok(pw_str) = String::from_utf8(plaintext) {
                        entries.push(IoPassword {
                            title,
                            username: username.filter(|s| !s.is_empty()),
                            password: pw_str,
                            url: url.filter(|s| !s.is_empty()),
                            notes: notes.filter(|s| !s.is_empty()),
                        });
                    }
                }
                Err(_) => {
                    entries.push(IoPassword {
                        title,
                        username: username.filter(|s| !s.is_empty()),
                        password: "⚠️ DECRYPT FAILED".to_string(),
                        url: url.filter(|s| !s.is_empty()),
                        notes: notes.filter(|s| !s.is_empty()),
                    });
                }
            }
        }

        result.push(serde_json::json!({
            "sheet_name": sname,
            "sheet_id": sid,
            "passwords": entries
        }));
    }

    Ok(Json(result))
}

/// POST /api/sheets/:id/import — import passwords into a sheet
pub async fn import_sheet(
    AuthWithDb { user, db, .. }: AuthWithDb,
    Path(sheet_id): Path<i64>,
    Json(passwords): Json<Vec<IoPassword>>,
) -> AppResult<serde_json::Value> {
    let conn = db.get().map_err(AppError::internal)?;

    let book_id: i64 = conn
        .query_row("SELECT book_id FROM sheets WHERE id = ?1", [sheet_id], |row| row.get(0))
        .map_err(|_| AppError::not_found("Sheet not found"))?;

    check_book_access(&conn, book_id, user.id)?;
    if !can_edit_book(&conn, book_id, user.id)? {
        return Err(AppError::forbidden("You don't have edit permission in this project"));
    }

    let ecc_public_key: String = conn
        .query_row("SELECT ecc_public_key FROM books WHERE id = ?1", [book_id], |row| row.get(0))
        .map_err(|_| AppError::not_found("Book not found"))?;

    let pk = EccCrypto::public_key_from_pem(&ecc_public_key)
        .map_err(|e| AppError::internal(format!("Failed to load book public key: {}", e)))?;

    let mut imported = 0i64;
    let mut errors = 0i64;

    for entry in &passwords {
        if entry.title.trim().is_empty() || entry.password.is_empty() {
            errors += 1;
            continue;
        }

        match EccCrypto::encrypt(&pk, entry.password.as_bytes()) {
            Ok(encrypted) => {
                let username = entry.username.as_deref().unwrap_or("");
                let url = entry.url.as_deref().unwrap_or("");
                let notes = entry.notes.as_deref().unwrap_or("");

                if let Ok(_) = conn.execute(
                    "INSERT INTO passwords (sheet_id, title, username, encrypted_password, url, notes) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    rusqlite::params![sheet_id, entry.title.trim(), username, encrypted, url, notes],
                ) {
                    let pid = conn.last_insert_rowid();
                    log_action(&conn, book_id, Some(sheet_id), Some(pid), user.id, &user.username, "create_password", &format!("{}：{}", entry.title, username));
                    imported += 1;
                } else {
                    errors += 1;
                }
            }
            Err(e) => {
                tracing::error!("Import encrypt failed: {}", e);
                errors += 1;
            }
        }
    }

    Ok(Json(serde_json::json!({
        "success": true,
        "imported": imported,
        "errors": errors
    })))
}

/// POST /api/books/:id/import — import passwords into a book (creates sheets if needed)
pub async fn import_book(
    AuthWithDb { user, db, .. }: AuthWithDb,
    Path(book_id): Path<i64>,
    Json(data): Json<Vec<serde_json::Value>>,
) -> AppResult<serde_json::Value> {
    let conn = db.get().map_err(AppError::internal)?;

    check_book_access(&conn, book_id, user.id)?;
    if !can_edit_book(&conn, book_id, user.id)? {
        return Err(AppError::forbidden("You don't have edit permission in this project"));
    }

    let ecc_public_key: String = conn
        .query_row("SELECT ecc_public_key FROM books WHERE id = ?1", [book_id], |row| row.get(0))
        .map_err(|_| AppError::not_found("Book not found"))?;

    let pk = EccCrypto::public_key_from_pem(&ecc_public_key)
        .map_err(|e| AppError::internal(format!("Failed to load book public key: {}", e)))?;

    let mut total_imported = 0i64;
    let mut total_errors = 0i64;

    for sheet_entry in &data {
        let sheet_name = sheet_entry.get("sheet_name").and_then(|v| v.as_str()).unwrap_or("导入的密码").to_string();
        let passwords: Vec<IoPassword> = match serde_json::from_value(sheet_entry.get("passwords").unwrap_or(&serde_json::Value::Null).clone()) {
            Ok(p) => p,
            Err(_) => { total_errors += 1; continue; }
        };

        // Find or create the sheet
        let sheet_id: i64 = match conn.query_row(
            "SELECT id FROM sheets WHERE book_id = ?1 AND name = ?2",
            rusqlite::params![book_id, &sheet_name],
            |row| row.get(0),
        ) {
            Ok(id) => id,
            Err(_) => {
                conn.execute(
                    "INSERT INTO sheets (book_id, name, description) VALUES (?1, ?2, '')",
                    rusqlite::params![book_id, &sheet_name],
                ).map_err(AppError::internal)?;
                conn.last_insert_rowid()
            }
        };

        for entry in &passwords {
            if entry.title.trim().is_empty() || entry.password.is_empty() {
                total_errors += 1;
                continue;
            }

            match EccCrypto::encrypt(&pk, entry.password.as_bytes()) {
                Ok(encrypted) => {
                    let username = entry.username.as_deref().unwrap_or("");
                    let url = entry.url.as_deref().unwrap_or("");
                    let notes = entry.notes.as_deref().unwrap_or("");

                    if let Ok(_) = conn.execute(
                        "INSERT INTO passwords (sheet_id, title, username, encrypted_password, url, notes) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                        rusqlite::params![sheet_id, entry.title.trim(), username, encrypted, url, notes],
                    ) {
                        let pid = conn.last_insert_rowid();
                        log_action(&conn, book_id, Some(sheet_id), Some(pid), user.id, &user.username, "create_password", &format!("{}：{}", entry.title, username));
                        total_imported += 1;
                    } else {
                        total_errors += 1;
                    }
                }
                Err(e) => {
                    tracing::error!("Import encrypt failed: {}", e);
                    total_errors += 1;
                }
            }
        }
    }

    Ok(Json(serde_json::json!({
        "success": true,
        "imported": total_imported,
        "errors": total_errors
    })))
}
