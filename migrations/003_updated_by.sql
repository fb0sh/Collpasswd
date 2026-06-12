-- Add updated_by tracking to passwords table
ALTER TABLE passwords ADD COLUMN updated_by_user_id INTEGER DEFAULT NULL REFERENCES users(id);
ALTER TABLE passwords ADD COLUMN updated_by_username TEXT DEFAULT '';
