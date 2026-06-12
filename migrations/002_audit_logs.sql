-- Audit log — records every password view, create, update, delete
CREATE TABLE IF NOT EXISTS audit_logs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    book_id INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
    sheet_id INTEGER REFERENCES sheets(id) ON DELETE SET NULL,
    password_id INTEGER REFERENCES passwords(id) ON DELETE SET NULL,
    user_id INTEGER NOT NULL REFERENCES users(id),
    username TEXT NOT NULL,
    action TEXT NOT NULL,  -- 'view_password' | 'create_password' | 'update_password' | 'delete_password'
    detail TEXT DEFAULT '', -- JSON with extra context (e.g. password title)
    created_at TEXT NOT NULL DEFAULT (datetime('now', '+8 hours'))
);

CREATE INDEX IF NOT EXISTS idx_audit_logs_book ON audit_logs(book_id);
CREATE INDEX IF NOT EXISTS idx_audit_logs_user ON audit_logs(user_id);
CREATE INDEX IF NOT EXISTS idx_audit_logs_action ON audit_logs(action);
CREATE INDEX IF NOT EXISTS idx_audit_logs_created ON audit_logs(created_at);
