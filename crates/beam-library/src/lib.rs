//! SQLite library manager for beaM.

use beam_core::error::{BeamError, Result};
use rusqlite::{Connection, OpenFlags};
use std::path::Path;

pub struct LibraryDb {
    conn: Connection,
}

impl LibraryDb {
    /// Open or create an in-memory database (useful for testing or ephemeral sessions).
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory().map_err(|e| BeamError::Database(e.to_string()))?;
        let db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    /// Open or create a database file at the specified path.
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_URI,
        )
        .map_err(|e| BeamError::Database(e.to_string()))?;

        let db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    /// Apply schema migrations for the library catalog.
    fn migrate(&self) -> Result<()> {
        self.conn
            .execute_batch(
                r#"
            PRAGMA foreign_keys = ON;
            PRAGMA journal_mode = WAL;

            CREATE TABLE IF NOT EXISTS schema_version (
                version INTEGER PRIMARY KEY
            );

            CREATE TABLE IF NOT EXISTS photos (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                file_path TEXT NOT NULL UNIQUE,
                file_name TEXT NOT NULL,
                file_size INTEGER NOT NULL,
                created_at TEXT,
                modified_at TEXT,
                camera_make TEXT,
                camera_model TEXT,
                width INTEGER,
                height INTEGER,
                rating INTEGER DEFAULT 0,
                color_label TEXT DEFAULT 'none',
                sidecar_path TEXT
            );

            CREATE INDEX IF NOT EXISTS idx_photos_path ON photos(file_path);
            CREATE INDEX IF NOT EXISTS idx_photos_rating ON photos(rating);
            "#,
            )
            .map_err(|e| BeamError::Database(e.to_string()))?;

        Ok(())
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_library_db_in_memory_migration() {
        let db = LibraryDb::open_in_memory().expect("Failed to open in-memory db");
        let count: i64 = db
            .connection()
            .query_row("SELECT count(*) FROM photos", [], |row| row.get(0))
            .expect("Failed to query photos");
        assert_eq!(count, 0);
    }
}
