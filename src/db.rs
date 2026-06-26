use anyhow::{Context, Result};
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use std::path::Path;

pub type DbPool = Pool<SqliteConnectionManager>;

/// Helper: add a column if it doesn't already exist
fn ensure_column(conn: &rusqlite::Connection, table: &str, column: &str, def: &str) -> Result<()> {
    let exists: bool = conn.query_row(
        &format!("SELECT COUNT(*) > 0 FROM pragma_table_info('{}') WHERE name = ?1", table),
        [column],
        |row| row.get(0),
    ).map_err(|e| anyhow::anyhow!("Failed to check column existence: {}", e))?;

    if !exists {
        let sql = format!("ALTER TABLE {} ADD COLUMN {}", table, def);
        conn.execute_batch(&sql).context(format!("Failed to add column {} to {}", column, table))?;
    }
    Ok(())
}

/// Initialize the database pool and run migrations
pub fn init_db(db_path: &Path) -> Result<DbPool> {
    let manager = SqliteConnectionManager::file(db_path);
    let pool = Pool::builder()
        .max_size(10)
        .build(manager)
        .context("Failed to create database connection pool")?;

    // Run migrations
    {
        let conn = pool.get().context("Failed to get database connection")?;

        conn.execute_batch("PRAGMA journal_mode=WAL;")
            .context("Failed to enable WAL mode")?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")
            .context("Failed to enable foreign keys")?;

        conn.execute_batch(include_str!("../migrations/001_init.sql"))
            .context("Failed to run migration 001")?;
        conn.execute_batch(include_str!("../migrations/002_audit_logs.sql"))
            .context("Failed to run migration 002")?;

        // Migration 003: add updated_by columns to passwords
        ensure_column(&conn, "passwords", "updated_by_user_id", "updated_by_user_id INTEGER DEFAULT NULL REFERENCES users(id)")?;
        ensure_column(&conn, "passwords", "updated_by_username", "updated_by_username TEXT DEFAULT ''")?;
    }

    Ok(pool)
}
