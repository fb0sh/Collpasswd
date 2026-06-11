use anyhow::{Context, Result};
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use std::path::Path;

pub type DbPool = Pool<SqliteConnectionManager>;

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
    }

    Ok(pool)
}
