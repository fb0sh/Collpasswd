use crate::auth::AuthWithDb;
use crate::crypto::EccCrypto;
use crate::errors::{AppError, AppResult};
use crate::models::*;
use axum::{
    extract::Path,
    Json,
};

/// GET /api/books - list books accessible to the current user
pub async fn list_books(
    AuthWithDb { user, db, .. }: AuthWithDb,
) -> AppResult<Vec<BookSummary>> {
    let conn = db.get().map_err(AppError::internal)?;

    let sql = if user.role == "admin" {
        "SELECT b.id, b.name, b.description, b.created_at, b.created_by,
                (SELECT COUNT(*) FROM book_members WHERE book_id = b.id) as member_count
         FROM books b
         ORDER BY b.created_at DESC"
    } else {
        "SELECT b.id, b.name, b.description, b.created_at, b.created_by,
                (SELECT COUNT(*) FROM book_members WHERE book_id = b.id) as member_count
         FROM books b
         JOIN book_members bm ON b.id = bm.book_id
         WHERE bm.user_id = ?1
         ORDER BY b.created_at DESC"
    };

    let mut stmt = conn.prepare(sql).map_err(AppError::internal)?;

    let books = if user.role == "admin" {
        stmt
            .query_map([], |row| {
                Ok(BookSummary {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    description: row.get::<_, Option<String>>(2)?,
                    created_at: row.get(3)?,
                    created_by: row.get(4)?,
                    member_count: row.get(5)?,
                    is_holder: row.get::<_, i64>(4)? == user.id,
                })
            })
            .map_err(AppError::internal)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(AppError::internal)?
    } else {
        stmt
            .query_map([user.id], |row| {
                Ok(BookSummary {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    description: row.get::<_, Option<String>>(2)?,
                    created_at: row.get(3)?,
                    created_by: row.get(4)?,
                    member_count: row.get(5)?,
                    is_holder: row.get::<_, i64>(4)? == user.id,
                })
            })
            .map_err(AppError::internal)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(AppError::internal)?
    };

    Ok(Json(books))
}

/// POST /api/books - create a new book (any user can create)
pub async fn create_book(
    AuthWithDb { user, db, master_key }: AuthWithDb,
    Json(req): Json<CreateBookRequest>,
) -> AppResult<BookDetail> {
    if req.name.trim().is_empty() {
        return Err(AppError::bad_request("Book name is required"));
    }

    let conn = db.get().map_err(AppError::internal)?;

    // Generate ECC key pair for this book
    let (sk, pk) = EccCrypto::generate_keypair().map_err(AppError::internal)?;
    let sk_pem = EccCrypto::secret_key_to_pem(&sk).map_err(AppError::internal)?;
    let pk_pem = EccCrypto::public_key_to_pem(&pk).map_err(AppError::internal)?;

    // Encrypt private key with master key before storing in DB
    let sk_encrypted = master_key
        .encrypt_private_key(&sk_pem)
        .map_err(|e| AppError::internal(format!("Failed to encrypt private key: {}", e)))?;

    let description = req.description.unwrap_or_default();

    conn.execute(
        "INSERT INTO books (name, description, ecc_private_key, ecc_public_key, created_by) VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![req.name.trim(), description, sk_encrypted, pk_pem, user.id],
    ).map_err(AppError::internal)?;

    let book_id = conn.last_insert_rowid();

    // Add creator as a member with 'edit' role (holder status is from created_by)
    conn.execute(
        "INSERT INTO book_members (book_id, user_id, role) VALUES (?1, ?2, 'edit')",
        rusqlite::params![book_id, user.id],
    ).map_err(AppError::internal)?;

    Ok(Json(BookDetail {
        id: book_id,
        name: req.name.trim().to_string(),
        description: Some(description).filter(|d| !d.is_empty()),
        created_at: chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        created_by: user.id,
        member_count: 1,
        is_holder: true,
    }))
}

/// GET /api/books/:id - get book details
pub async fn get_book(
    AuthWithDb { user, db, .. }: AuthWithDb,
    Path(book_id): Path<i64>,
) -> AppResult<BookDetail> {
    let conn = db.get().map_err(AppError::internal)?;

    check_book_access(&conn, book_id, user.id)?;

    let book = conn.query_row(
        "SELECT b.id, b.name, b.description, b.created_at, b.created_by,
                (SELECT COUNT(*) FROM book_members WHERE book_id = b.id) as member_count
         FROM books b WHERE b.id = ?1",
        [book_id],
        |row| {
            Ok(BookDetail {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get::<_, Option<String>>(2)?,
                created_at: row.get(3)?,
                created_by: row.get(4)?,
                member_count: row.get(5)?,
                is_holder: row.get::<_, i64>(4)? == user.id,
            })
        },
    ).map_err(|_| AppError::not_found("Book not found"))?;

    Ok(Json(book))
}

/// PUT /api/books/:id - update a book's name/description (holder or global admin only)
pub async fn update_book(
    AuthWithDb { user, db, .. }: AuthWithDb,
    Path(book_id): Path<i64>,
    Json(req): Json<UpdateBookRequest>,
) -> AppResult<serde_json::Value> {
    let conn = db.get().map_err(AppError::internal)?;

    let created_by: i64 = conn
        .query_row(
            "SELECT created_by FROM books WHERE id = ?1",
            [book_id],
            |row| row.get(0),
        )
        .map_err(|_| AppError::not_found("Book not found"))?;

    if created_by != user.id && user.role != "admin" {
        return Err(AppError::forbidden("Only the book holder or admin can update this project"));
    }

    let mut updates = Vec::new();
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    if let Some(name) = &req.name {
        if !name.trim().is_empty() {
            updates.push("name = ?");
            params.push(Box::new(name.trim().to_string()));
        }
    }
    if let Some(desc) = &req.description {
        updates.push("description = ?");
        params.push(Box::new(desc));
    }

    if updates.is_empty() {
        return Err(AppError::bad_request("No fields to update"));
    }

    params.push(Box::new(book_id));

    let sql = format!(
        "UPDATE books SET {} WHERE id = ?",
        updates.join(", ")
    );

    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();

    conn.execute(&sql, param_refs.as_slice())
        .map_err(AppError::internal)?;

    Ok(Json(serde_json::json!({"success": true})))
}

/// DELETE /api/books/:id - delete a book (holder or global admin only)
pub async fn delete_book(
    AuthWithDb { user, db, .. }: AuthWithDb,
    Path(book_id): Path<i64>,
) -> AppResult<serde_json::Value> {
    let conn = db.get().map_err(AppError::internal)?;

    let created_by: i64 = conn
        .query_row(
            "SELECT created_by FROM books WHERE id = ?1",
            [book_id],
            |row| row.get(0),
        )
        .map_err(|_| AppError::not_found("Book not found"))?;

    if created_by != user.id && user.role != "admin" {
        return Err(AppError::forbidden("Only the book holder or admin can delete this book"));
    }

    conn.execute("DELETE FROM books WHERE id = ?1", [book_id])
        .map_err(AppError::internal)?;

    Ok(Json(serde_json::json!({"success": true})))
}

/// GET /api/books/:id/members - list members of a book
pub async fn list_members(
    AuthWithDb { user, db, .. }: AuthWithDb,
    Path(book_id): Path<i64>,
) -> AppResult<Vec<BookMember>> {
    let conn = db.get().map_err(AppError::internal)?;

    check_book_access(&conn, book_id, user.id)?;

    let mut stmt = conn.prepare(
        "SELECT bm.id, bm.user_id, u.username, bm.role
         FROM book_members bm
         JOIN users u ON bm.user_id = u.id
         WHERE bm.book_id = ?1
         ORDER BY u.username",
    ).map_err(AppError::internal)?;

    let members = stmt
        .query_map([book_id], |row| {
            Ok(BookMember {
                id: row.get(0)?,
                user_id: row.get(1)?,
                username: row.get(2)?,
                role: row.get(3)?,
            })
        })
        .map_err(AppError::internal)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(AppError::internal)?;

    Ok(Json(members))
}

/// POST /api/books/:id/members - add a member to a book (holder or global admin only)
pub async fn add_member(
    AuthWithDb { user, db, .. }: AuthWithDb,
    Path(book_id): Path<i64>,
    Json(req): Json<AddMemberRequest>,
) -> AppResult<BookMember> {
    let conn = db.get().map_err(AppError::internal)?;

    // Allow only if user is holder or global admin
    if !is_book_holder(&conn, book_id, user.id).map_err(AppError::internal)? && user.role != "admin" {
        return Err(AppError::forbidden("Only the book holder can add members"));
    }

    if req.role != "edit" && req.role != "view" {
        return Err(AppError::bad_request("Role must be 'edit' or 'view'"));
    }

    let target_user = conn.query_row(
        "SELECT id, username FROM users WHERE username = ?1",
        [&req.username],
        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
    ).map_err(|_| AppError::not_found("User not found"))?;

    let existing: Result<i64, _> = conn.query_row(
        "SELECT id FROM book_members WHERE book_id = ?1 AND user_id = ?2",
        rusqlite::params![book_id, target_user.0],
        |row| row.get(0),
    );

    if existing.is_ok() {
        return Err(AppError::bad_request("User is already a member of this book"));
    }

    conn.execute(
        "INSERT INTO book_members (book_id, user_id, role) VALUES (?1, ?2, ?3)",
        rusqlite::params![book_id, target_user.0, req.role],
    ).map_err(AppError::internal)?;

    let member_id = conn.last_insert_rowid();

    Ok(Json(BookMember {
        id: member_id,
        user_id: target_user.0,
        username: target_user.1,
        role: req.role,
    }))
}

/// DELETE /api/books/:id/members/:uid - remove a member from a book
pub async fn remove_member(
    AuthWithDb { user, db, .. }: AuthWithDb,
    Path((book_id, member_user_id)): Path<(i64, i64)>,
) -> AppResult<serde_json::Value> {
    let conn = db.get().map_err(AppError::internal)?;

    // Only holder or global admin can remove members
    if !is_book_holder(&conn, book_id, user.id).map_err(AppError::internal)? && user.role != "admin" {
        return Err(AppError::forbidden("Only the book holder can remove members"));
    }

    // Cannot remove the holder
    if is_book_holder(&conn, book_id, member_user_id).map_err(AppError::internal)? {
        return Err(AppError::forbidden("Cannot remove the book holder from the project"));
    }

    let affected = conn
        .execute(
            "DELETE FROM book_members WHERE book_id = ?1 AND user_id = ?2",
            rusqlite::params![book_id, member_user_id],
        )
        .map_err(AppError::internal)?;

    if affected == 0 {
        return Err(AppError::not_found("Member not found"));
    }

    Ok(Json(serde_json::json!({"success": true})))
}

/// PUT /api/books/:id/members/:uid — update a member's role in a book
pub async fn update_member_role(
    AuthWithDb { user, db, .. }: AuthWithDb,
    Path((book_id, member_user_id)): Path<(i64, i64)>,
    Json(req): Json<UpdateMemberRoleRequest>,
) -> AppResult<serde_json::Value> {
    let conn = db.get().map_err(AppError::internal)?;

    // Only holder or global admin can change member roles
    if !is_book_holder(&conn, book_id, user.id).map_err(AppError::internal)? && user.role != "admin" {
        return Err(AppError::forbidden("Only the book holder can change member roles"));
    }

    if req.role != "edit" && req.role != "view" {
        return Err(AppError::bad_request("Role must be 'edit' or 'view'"));
    }

    // Cannot change the holder's role
    if is_book_holder(&conn, book_id, member_user_id).map_err(AppError::internal)? {
        return Err(AppError::forbidden("Cannot change the book holder's role"));
    }

    let affected = conn
        .execute(
            "UPDATE book_members SET role = ?1 WHERE book_id = ?2 AND user_id = ?3",
            rusqlite::params![req.role, book_id, member_user_id],
        )
        .map_err(AppError::internal)?;

    if affected == 0 {
        return Err(AppError::not_found("Member not found"));
    }

    Ok(Json(serde_json::json!({"success": true, "role": req.role})))
}

// ─── Helpers ───────────────────────────────────────────────────────────────

pub fn check_book_access(
    conn: &rusqlite::Connection,
    book_id: i64,
    user_id: i64,
) -> Result<(), AppError> {
    let exists: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM book_members WHERE book_id = ?1 AND user_id = ?2",
            rusqlite::params![book_id, user_id],
            |row| row.get(0),
        )
        .map_err(|_| AppError::internal("Database error"))?;

    if !exists {
        return Err(AppError::not_found("Book not found"));
    }
    Ok(())
}

/// Check if a user is the holder (created_by) of a book
pub fn is_book_holder(conn: &rusqlite::Connection, book_id: i64, user_id: i64) -> Result<bool, rusqlite::Error> {
    conn.query_row(
        "SELECT COUNT(*) > 0 FROM books WHERE id = ?1 AND created_by = ?2",
        rusqlite::params![book_id, user_id],
        |row| row.get(0),
    )
}

/// Check if a user can edit (holder or member with 'edit' role)
pub fn can_edit_book(conn: &rusqlite::Connection, book_id: i64, user_id: i64) -> Result<bool, AppError> {
    // Holder can always edit
    if is_book_holder(conn, book_id, user_id).map_err(AppError::internal)? {
        return Ok(true);
    }

    // Check if member has edit role
    let role: Result<String, _> = conn.query_row(
        "SELECT role FROM book_members WHERE book_id = ?1 AND user_id = ?2",
        rusqlite::params![book_id, user_id],
        |row| row.get(0),
    );

    match role {
        Ok(r) => Ok(r == "edit" || r == "admin" || r == "editor"),
        Err(_) => Ok(false),
    }
}

/// GET /api/books/:id/search?q=keyword — search all passwords in a book
pub async fn search_book(
    AuthWithDb { user, db, .. }: AuthWithDb,
    Path(book_id): Path<i64>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> AppResult<Vec<serde_json::Value>> {
    let conn = db.get().map_err(AppError::internal)?;
    check_book_access(&conn, book_id, user.id)?;

    let q = params.get("q").map(|s| s.trim()).unwrap_or("");
    if q.is_empty() {
        return Ok(Json(Vec::new()));
    }

    let pattern = format!("%{}%", q);

    let mut stmt = conn.prepare(
        "SELECT p.id, p.title, p.username, p.url, s.id as sheet_id, s.name as sheet_name
         FROM passwords p
         JOIN sheets s ON p.sheet_id = s.id
         WHERE s.book_id = ?1
           AND (p.title LIKE ?2 OR p.username LIKE ?2 OR p.url LIKE ?2 OR p.notes LIKE ?2)
         ORDER BY s.name, p.title
         LIMIT 100"
    ).map_err(AppError::internal)?;

    let results = stmt
        .query_map(rusqlite::params![book_id, pattern], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, i64>(0)?,
                "title": row.get::<_, String>(1)?,
                "username": row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                "url": row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                "sheet_id": row.get::<_, i64>(4)?,
                "sheet_name": row.get::<_, String>(5)?,
            }))
        })
        .map_err(AppError::internal)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(AppError::internal)?;

    Ok(Json(results))
}
