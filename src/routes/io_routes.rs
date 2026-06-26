use crate::auth::AuthWithDb;
use crate::crypto::EccCrypto;
use crate::errors::{AppError, AppResult};
use crate::models::*;
use crate::routes::audit_routes::log_action;
use crate::routes::book_routes::{can_edit_book, check_book_access};
use axum::{
    extract::multipart::Multipart,
    extract::Path,
    http::header,
    response::IntoResponse,
    Json,
};
use calamine::{open_workbook_from_rs, Reader, Xlsx};
use rust_xlsxwriter::*;
use serde::{Deserialize, Serialize};
use std::io::Cursor;
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

/// Maximum uploaded XLSX file size (10 MB)
const MAX_XLSX_SIZE: usize = 10 * 1024 * 1024;

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

fn entry_detail(entry: &IoPassword) -> String {
    let mut parts = Vec::new();
    if !entry.title.is_empty() {
        parts.push(format!("标题={}", entry.title));
    }
    if let Some(ref u) = entry.username {
        if !u.is_empty() {
            parts.push(format!("用户名={}", u));
        }
    }
    if let Some(ref u) = entry.url {
        if !u.is_empty() {
            parts.push(format!("网址={}", u));
        }
    }
    parts.join(" | ")
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
    let mut error_details: Vec<String> = Vec::new();

    for (idx, entry) in passwords.iter().enumerate() {
        let identity = entry_detail(entry);
        let label = if identity.is_empty() { format!("#{}", idx + 1) } else { format!("#{}: {}", idx + 1, identity) };

        if entry.title.trim().is_empty() || entry.password.is_empty() {
            let reason = if entry.title.trim().is_empty() { "标题为空" } else { "密码为空" };
            error_details.push(format!("{}（{}）", label, reason));
            continue;
        }

        match EccCrypto::encrypt(&pk, entry.password.as_bytes()) {
            Ok(encrypted) => {
                let username = entry.username.as_deref().unwrap_or("");
                let url = entry.url.as_deref().unwrap_or("");
                let notes = entry.notes.as_deref().unwrap_or("");

                if let Ok(_) = conn.execute(
                    "INSERT INTO passwords (sheet_id, title, username, encrypted_password, url, notes, updated_by_user_id, updated_by_username, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, datetime('now', '+8 hours'))",
                    rusqlite::params![sheet_id, entry.title.trim(), username, encrypted, url, notes, user.id, &user.username],
                ) {
                    let pid = conn.last_insert_rowid();
                    log_action(&conn, book_id, Some(sheet_id), Some(pid), user.id, &user.username, "create_password", &format!("{}：{}", entry.title, username));
                    imported += 1;
                } else {
                    error_details.push(format!("{}（数据库写入失败）", label));
                }
            }
            Err(e) => {
                tracing::error!("Import encrypt failed: {}", e);
                error_details.push(format!("{}（加密失败）", label));
            }
        }
    }

    Ok(Json(serde_json::json!({
        "success": true,
        "imported": imported,
        "errors": error_details.len(),
        "error_details": error_details,
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
    let mut total_error_details: Vec<String> = Vec::new();
    let mut entry_counter = 0i64;

    for sheet_entry in &data {
        let sheet_name = sheet_entry.get("sheet_name").and_then(|v| v.as_str()).unwrap_or("导入的密码").to_string();
        let passwords: Vec<IoPassword> = match serde_json::from_value(sheet_entry.get("passwords").unwrap_or(&serde_json::Value::Null).clone()) {
            Ok(p) => p,
            Err(_) => { total_error_details.push(format!("表「{}」格式错误", &sheet_name)); continue; }
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
            entry_counter += 1;
            let identity = entry_detail(entry);
            let label = if identity.is_empty() { format!("#{}", entry_counter) } else { format!("#{}: {}", entry_counter, identity) };

            if entry.title.trim().is_empty() || entry.password.is_empty() {
                let reason = if entry.title.trim().is_empty() { "标题为空" } else { "密码为空" };
                total_error_details.push(format!("{}（{}）", label, reason));
                continue;
            }

            match EccCrypto::encrypt(&pk, entry.password.as_bytes()) {
                Ok(encrypted) => {
                    let username = entry.username.as_deref().unwrap_or("");
                    let url = entry.url.as_deref().unwrap_or("");
                    let notes = entry.notes.as_deref().unwrap_or("");

                    if let Ok(_) = conn.execute(
                        "INSERT INTO passwords (sheet_id, title, username, encrypted_password, url, notes, updated_by_user_id, updated_by_username, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, datetime('now', '+8 hours'))",
                        rusqlite::params![sheet_id, entry.title.trim(), username, encrypted, url, notes, user.id, &user.username],
                    ) {
                        let pid = conn.last_insert_rowid();
                        log_action(&conn, book_id, Some(sheet_id), Some(pid), user.id, &user.username, "create_password", &format!("{}：{}", entry.title, username));
                        total_imported += 1;
                    } else {
                        total_error_details.push(format!("{}（数据库写入失败）", label));
                    }
                }
                Err(e) => {
                    tracing::error!("Import encrypt failed: {}", e);
                    total_error_details.push(format!("{}（加密失败）", label));
                }
            }
        }
    }

    Ok(Json(serde_json::json!({
        "success": true,
        "imported": total_imported,
        "errors": total_error_details.len(),
        "error_details": total_error_details,
    })))
}

/// Parse XLSX bytes into Vec<IoPassword> from the first worksheet
fn parse_xlsx_passwords(data: Vec<u8>) -> Result<Vec<IoPassword>, AppError> {
    if data.len() > MAX_XLSX_SIZE {
        return Err(AppError::bad_request(format!("文件过大（{}MB 限制），请上传较小的文件", MAX_XLSX_SIZE / 1024 / 1024)));
    }
    let cursor = Cursor::new(data);
    let mut workbook: Xlsx<Cursor<Vec<u8>>> =
        open_workbook_from_rs(cursor).map_err(|e| AppError::bad_request(format!("无法解析 XLSX: {}", e)))?;

    let sheet_name = workbook
        .sheet_names()
        .first()
        .cloned()
        .ok_or_else(|| AppError::bad_request("XLSX 中没有工作表"))?;

    let range = workbook
        .worksheet_range(&sheet_name)
        .map_err(|e| AppError::bad_request(format!("读取工作表失败: {}", e)))?;

    let mut rows = range.rows();
    let header = match rows.next() {
        Some(r) => r,
        None => return Ok(Vec::new()),
    };

    // Map header names to column indices (case-insensitive)
    let headers: Vec<String> = header.iter().map(|c| c.to_string().to_lowercase()).collect();
    let title_idx = headers.iter().position(|h| h == "title");
    let user_idx = headers.iter().position(|h| h == "username");
    let pass_idx = headers.iter().position(|h| h == "password");
    let url_idx = headers.iter().position(|h| h == "url" || h == "website");
    let notes_idx = headers.iter().position(|h| h == "notes" || h == "note" || h == "备注");

    let title_idx = title_idx.ok_or_else(|| AppError::bad_request("XLSX 缺少 title 列"))?;
    let pass_idx = pass_idx.ok_or_else(|| AppError::bad_request("XLSX 缺少 password 列"))?;

    let mut result = Vec::new();
    for row in rows {
        let title = row.get(title_idx).map(|c| c.to_string().trim().to_string()).unwrap_or_default();
        let password = row.get(pass_idx).map(|c| c.to_string().trim().to_string()).unwrap_or_default();
        if title.is_empty() && password.is_empty() {
            continue;
        }
        result.push(IoPassword {
            title,
            username: user_idx.and_then(|i| row.get(i)).map(|c| c.to_string().trim().to_string()).filter(|s| !s.is_empty()),
            password,
            url: url_idx.and_then(|i| row.get(i)).map(|c| c.to_string().trim().to_string()).filter(|s| !s.is_empty()),
            notes: notes_idx.and_then(|i| row.get(i)).map(|c| c.to_string().trim().to_string()).filter(|s| !s.is_empty()),
        });
    }

    Ok(result)
}

/// POST /api/sheets/:id/preview-xlsx — upload XLSX, parse and return preview as JSON
pub async fn preview_sheet_xlsx(
    AuthWithDb { user, db, .. }: AuthWithDb,
    Path(sheet_id): Path<i64>,
    mut multipart: Multipart,
) -> AppResult<Vec<IoPassword>> {
    let conn = db.get().map_err(AppError::internal)?;

    let book_id: i64 = conn
        .query_row("SELECT book_id FROM sheets WHERE id = ?1", [sheet_id], |row| row.get(0))
        .map_err(|_| AppError::not_found("Sheet not found"))?;
    check_book_access(&conn, book_id, user.id)?;

    let mut file_bytes: Vec<u8> = Vec::new();
    while let Some(field) = multipart.next_field().await.map_err(|e| AppError::bad_request(e.to_string()))? {
        let data = field.bytes().await.map_err(|e| AppError::bad_request(e.to_string()))?;
        file_bytes = data.to_vec();
        break; // only read first file field
    }

    if file_bytes.is_empty() {
        return Err(AppError::bad_request("未上传文件"));
    }

    let passwords = parse_xlsx_passwords(file_bytes)?;
    Ok(Json(passwords))
}

/// POST /api/books/:id/preview-xlsx — upload XLSX, parse all sheets and return grouped preview
pub async fn preview_book_xlsx(
    AuthWithDb { user, db, .. }: AuthWithDb,
    Path(book_id): Path<i64>,
    mut multipart: Multipart,
) -> AppResult<Vec<serde_json::Value>> {
    let conn = db.get().map_err(AppError::internal)?;
    check_book_access(&conn, book_id, user.id)?;

    let mut file_bytes: Vec<u8> = Vec::new();
    while let Some(field) = multipart.next_field().await.map_err(|e| AppError::bad_request(e.to_string()))? {
        let data = field.bytes().await.map_err(|e| AppError::bad_request(e.to_string()))?;
        file_bytes = data.to_vec();
        break;
    }

    if file_bytes.is_empty() {
        return Err(AppError::bad_request("未上传文件"));
    }

    if file_bytes.len() > MAX_XLSX_SIZE {
        return Err(AppError::bad_request(format!("文件过大（{}MB 限制），请上传较小的文件", MAX_XLSX_SIZE / 1024 / 1024)));
    }

    let cursor = Cursor::new(file_bytes);
    let mut workbook: Xlsx<Cursor<Vec<u8>>> =
        open_workbook_from_rs(cursor).map_err(|e| AppError::bad_request(format!("无法解析 XLSX: {}", e)))?;

    let mut result = Vec::new();
    for sname in workbook.sheet_names().to_vec() {
        let range = workbook
            .worksheet_range(&sname)
            .map_err(|e| AppError::bad_request(format!("读取工作表「{}」失败: {}", sname, e)))?;

        let mut rows = range.rows();
        let header = match rows.next() {
            Some(r) => r,
            None => continue,
        };

        let headers: Vec<String> = header.iter().map(|c| c.to_string().to_lowercase()).collect();
        let title_idx = match headers.iter().position(|h| h == "title") {
            Some(i) => i,
            None => continue,
        };
        let pass_idx = match headers.iter().position(|h| h == "password") {
            Some(i) => i,
            None => continue,
        };
        let user_idx = headers.iter().position(|h| h == "username");
        let url_idx = headers.iter().position(|h| h == "url");
        let notes_idx = headers.iter().position(|h| h == "notes");

        let mut entries = Vec::new();
        for row in rows {
            let title = row.get(title_idx).map(|c| c.to_string().trim().to_string()).unwrap_or_default();
            let password = row.get(pass_idx).map(|c| c.to_string().trim().to_string()).unwrap_or_default();
            if title.is_empty() && password.is_empty() {
                continue;
            }
            entries.push(IoPassword {
                title,
                username: user_idx.and_then(|i| row.get(i)).map(|c| c.to_string().trim().to_string()).filter(|s| !s.is_empty()),
                password,
                url: url_idx.and_then(|i| row.get(i)).map(|c| c.to_string().trim().to_string()).filter(|s| !s.is_empty()),
                notes: notes_idx.and_then(|i| row.get(i)).map(|c| c.to_string().trim().to_string()).filter(|s| !s.is_empty()),
            });
        }

        if !entries.is_empty() {
            result.push(serde_json::json!({
                "sheet_name": sname,
                "passwords": entries,
            }));
        }
    }

    if result.is_empty() {
        return Err(AppError::bad_request("XLSX 中没有找到有效数据（需要包含 title 和 password 列的工作表）"));
    }

    Ok(Json(result))
}

fn export_sheet_decrypted(
    conn: &rusqlite::Connection,
    master_key: &crate::crypto::MasterKey,
    sheet_id: i64,
    book_id: i64,
) -> Result<Vec<IoPassword>, AppError> {
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
        "SELECT title, username, encrypted_password, url, notes FROM passwords WHERE sheet_id = ?1 ORDER BY title",
    )
    .map_err(AppError::internal)?;

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
    Ok(result)
}

/// GET /api/sheets/:id/export-xlsx — download sheet as XLSX
pub async fn export_sheet_xlsx(
    AuthWithDb { user, db, master_key }: AuthWithDb,
    Path(sheet_id): Path<i64>,
) -> Result<impl IntoResponse, AppError> {
    let conn = db.get().map_err(AppError::internal)?;

    let book_id: i64 = conn
        .query_row("SELECT book_id FROM sheets WHERE id = ?1", [sheet_id], |row| row.get(0))
        .map_err(|_| AppError::not_found("Sheet not found"))?;
    check_book_access(&conn, book_id, user.id)?;

    let entries = export_sheet_decrypted(&conn, &master_key, sheet_id, book_id)?;

    let mut wb = Workbook::new();
    let ws = wb.add_worksheet();
    let bold = Format::new().set_bold();

    let headers = ["title", "username", "password", "url", "notes"];
    for (col, h) in headers.iter().enumerate() {
        ws.write_string_with_format(0, col as u16, *h, &bold)
            .map_err(|e| AppError::internal(e.to_string()))?;
    }

    for (i, entry) in entries.iter().enumerate() {
        let r = (i + 1) as u32;
        ws.write_string(r, 0, &entry.title).map_err(|e| AppError::internal(e.to_string()))?;
        ws.write_string(r, 1, entry.username.as_deref().unwrap_or("")).map_err(|e| AppError::internal(e.to_string()))?;
        ws.write_string(r, 2, &entry.password).map_err(|e| AppError::internal(e.to_string()))?;
        ws.write_string(r, 3, entry.url.as_deref().unwrap_or("")).map_err(|e| AppError::internal(e.to_string()))?;
        ws.write_string(r, 4, entry.notes.as_deref().unwrap_or("")).map_err(|e| AppError::internal(e.to_string()))?;
    }

    let data = wb.save_to_buffer().map_err(|e| AppError::internal(e.to_string()))?;
    Ok(([(header::CONTENT_TYPE, "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet")], data))
}

/// GET /api/books/:id/export-xlsx — download entire project as XLSX (one sheet per tab)
pub async fn export_book_xlsx(
    AuthWithDb { user, db, master_key }: AuthWithDb,
    Path(book_id): Path<i64>,
) -> Result<impl IntoResponse, AppError> {
    let conn = db.get().map_err(AppError::internal)?;
    check_book_access(&conn, book_id, user.id)?;

    let mut sheet_stmt = conn
        .prepare("SELECT id, name FROM sheets WHERE book_id = ?1 ORDER BY name")
        .map_err(AppError::internal)?;

    let sheets: Vec<(i64, String)> = sheet_stmt
        .query_map([book_id], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))
        .map_err(AppError::internal)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(AppError::internal)?;

    let mut wb = Workbook::new();
    let bold = Format::new().set_bold();
    let headers = ["title", "username", "password", "url", "notes"];

    for (sid, sname) in &sheets {
        let entries = export_sheet_decrypted(&conn, &master_key, *sid, book_id)?;
        if entries.is_empty() {
            continue;
        }

        let ws = wb.add_worksheet();
        let safe_name: String = sname.chars().take(31).collect();
        let _ = ws.set_name(&safe_name);

        for (col, h) in headers.iter().enumerate() {
            ws.write_string_with_format(0, col as u16, *h, &bold)
                .map_err(|e| AppError::internal(e.to_string()))?;
        }

        for (i, entry) in entries.iter().enumerate() {
            let r = (i + 1) as u32;
            ws.write_string(r, 0, &entry.title).map_err(|e| AppError::internal(e.to_string()))?;
            ws.write_string(r, 1, entry.username.as_deref().unwrap_or("")).map_err(|e| AppError::internal(e.to_string()))?;
            ws.write_string(r, 2, &entry.password).map_err(|e| AppError::internal(e.to_string()))?;
            ws.write_string(r, 3, entry.url.as_deref().unwrap_or("")).map_err(|e| AppError::internal(e.to_string()))?;
            ws.write_string(r, 4, entry.notes.as_deref().unwrap_or("")).map_err(|e| AppError::internal(e.to_string()))?;
        }
    }

    if wb.worksheets().is_empty() {
        let ws = wb.add_worksheet();
        let _ = ws.write_string(0, 0, "无数据");
    }

    let data = wb.save_to_buffer().map_err(|e| AppError::internal(e.to_string()))?;
    Ok(([(header::CONTENT_TYPE, "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet")], data))
}

/// GET /api/template/sheet-xlsx — download an import template XLSX (single sheet)
pub async fn template_sheet_xlsx() -> Result<impl IntoResponse, AppError> {
    let mut wb = Workbook::new();
    let ws = wb.add_worksheet();
    let bold = Format::new().set_bold();

    let headers = ["title", "username", "password", "url", "notes"];
    for (col, h) in headers.iter().enumerate() {
        ws.write_string_with_format(0, col as u16, *h, &bold)
            .map_err(|e| AppError::internal(e.to_string()))?;
    }

    ws.write_string(1, 0, "生产服务器 SSH").map_err(|e| AppError::internal(e.to_string()))?;
    ws.write_string(1, 1, "root").map_err(|e| AppError::internal(e.to_string()))?;
    ws.write_string(1, 2, "请输入密码").map_err(|e| AppError::internal(e.to_string()))?;
    ws.write_string(1, 3, "192.168.1.1").map_err(|e| AppError::internal(e.to_string()))?;
    ws.write_string(1, 4, "root 用户").map_err(|e| AppError::internal(e.to_string()))?;
    ws.write_string(2, 0, "测试服务器").map_err(|e| AppError::internal(e.to_string()))?;
    ws.write_string(2, 1, "admin").map_err(|e| AppError::internal(e.to_string()))?;
    ws.write_string(2, 2, "请输入密码").map_err(|e| AppError::internal(e.to_string()))?;
    ws.write_string(2, 3, "10.0.0.1").map_err(|e| AppError::internal(e.to_string()))?;
    ws.write_string(2, 4, "开发环境").map_err(|e| AppError::internal(e.to_string()))?;

    let data = wb.save_to_buffer().map_err(|e| AppError::internal(e.to_string()))?;
    Ok(([(header::CONTENT_TYPE, "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet")], data))
}

/// GET /api/template/book-xlsx — download an import template XLSX (multi-sheet)
pub async fn template_book_xlsx() -> Result<impl IntoResponse, AppError> {
    let mut wb = Workbook::new();
    let bold = Format::new().set_bold();
    let headers = ["title", "username", "password", "url", "notes"];

    let ws1 = wb.add_worksheet();
    ws1.set_name("服务器密码").map_err(|e| AppError::internal(e.to_string()))?;
    for (col, h) in headers.iter().enumerate() {
        ws1.write_string_with_format(0, col as u16, *h, &bold)
            .map_err(|e| AppError::internal(e.to_string()))?;
    }
    ws1.write_string(1, 0, "生产服务器 SSH").map_err(|e| AppError::internal(e.to_string()))?;
    ws1.write_string(1, 1, "root").map_err(|e| AppError::internal(e.to_string()))?;
    ws1.write_string(1, 2, "请输入密码").map_err(|e| AppError::internal(e.to_string()))?;
    ws1.write_string(1, 3, "192.168.1.1").map_err(|e| AppError::internal(e.to_string()))?;
    ws1.write_string(1, 4, "root 用户").map_err(|e| AppError::internal(e.to_string()))?;
    ws1.write_string(2, 0, "测试服务器").map_err(|e| AppError::internal(e.to_string()))?;
    ws1.write_string(2, 1, "admin").map_err(|e| AppError::internal(e.to_string()))?;
    ws1.write_string(2, 2, "请输入密码").map_err(|e| AppError::internal(e.to_string()))?;
    ws1.write_string(2, 3, "10.0.0.1").map_err(|e| AppError::internal(e.to_string()))?;
    ws1.write_string(2, 4, "开发环境").map_err(|e| AppError::internal(e.to_string()))?;

    let ws2 = wb.add_worksheet();
    ws2.set_name("数据库密码").map_err(|e| AppError::internal(e.to_string()))?;
    for (col, h) in headers.iter().enumerate() {
        ws2.write_string_with_format(0, col as u16, *h, &bold)
            .map_err(|e| AppError::internal(e.to_string()))?;
    }
    ws2.write_string(1, 0, "MySQL 生产").map_err(|e| AppError::internal(e.to_string()))?;
    ws2.write_string(1, 1, "dbadmin").map_err(|e| AppError::internal(e.to_string()))?;
    ws2.write_string(1, 2, "请输入密码").map_err(|e| AppError::internal(e.to_string()))?;
    ws2.write_string(1, 3, "db01.example.com").map_err(|e| AppError::internal(e.to_string()))?;
    ws2.write_string(1, 4, "主库").map_err(|e| AppError::internal(e.to_string()))?;

    let data = wb.save_to_buffer().map_err(|e| AppError::internal(e.to_string()))?;
    Ok(([(header::CONTENT_TYPE, "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet")], data))
}
