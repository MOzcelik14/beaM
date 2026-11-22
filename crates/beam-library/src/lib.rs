//! SQLite library catalog and photo management for beaM.

use beam_core::error::{BeamError, Result};
use beam_core::sidecar::{ColorLabel, Sidecar};
use beam_media::{
    extract_exif_metadata, extract_image_dimensions, generate_thumbnail_with_edits, is_raster_file,
    save_thumbnail, thumbnail_cache_path,
};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

pub const SUPPORTED_EXTENSIONS: &[&str] = &[
    "arw", "cr2", "cr3", "nef", "dng", "raf", "rw2", "orf", "pef", "jpg", "jpeg", "png", "tif",
    "tiff", "webp",
];

pub fn is_supported_image(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| {
            let lower = ext.to_ascii_lowercase();
            SUPPORTED_EXTENSIONS.contains(&lower.as_str())
        })
        .unwrap_or(false)
}

pub fn default_catalog_path() -> PathBuf {
    if let Ok(data_home) = std::env::var("XDG_DATA_HOME") {
        PathBuf::from(data_home).join("beam").join("catalog.db")
    } else if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".local/share/beam/catalog.db")
    } else if let Ok(appdata) = std::env::var("APPDATA") {
        PathBuf::from(appdata).join("beam/catalog.db")
    } else {
        PathBuf::from("beam_catalog.db")
    }
}

pub fn default_thumbnail_cache_dir() -> PathBuf {
    if let Ok(cache_home) = std::env::var("XDG_CACHE_HOME") {
        PathBuf::from(cache_home).join("beam/thumbnails")
    } else if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".cache/beam/thumbnails")
    } else if let Ok(localappdata) = std::env::var("LOCALAPPDATA") {
        PathBuf::from(localappdata).join("beam/thumbnails")
    } else {
        PathBuf::from("beam_thumbnails")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhotoRecord {
    pub id: i64,
    pub file_path: String,
    pub file_name: String,
    pub file_size: u64,
    pub created_at: Option<String>,
    pub modified_at: Option<String>,
    pub camera_make: Option<String>,
    pub camera_model: Option<String>,
    pub width: u32,
    pub height: u32,
    pub rating: u8,
    pub color_label: ColorLabel,
    pub thumbnail_path: Option<String>,
    pub sidecar_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VirtualCopyRecord {
    pub id: i64,
    pub photo_id: i64,
    pub copy_name: String,
    pub sidecar_path: String,
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortOrder {
    #[default]
    DateTakenDesc,
    DateTakenAsc,
    RatingDesc,
    FileNameAsc,
}

impl SortOrder {
    pub fn translation_key(&self) -> &'static str {
        match self {
            Self::DateTakenDesc => "library_sort_date_desc",
            Self::DateTakenAsc => "library_sort_date_asc",
            Self::RatingDesc => "library_sort_rating_desc",
            Self::FileNameAsc => "library_sort_filename_asc",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FileTypeFilter {
    #[default]
    All,
    Raw,
    Jpeg,
    Png,
    Tiff,
    WebP,
}

impl FileTypeFilter {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Raw => "RAW",
            Self::Jpeg => "JPEG",
            Self::Png => "PNG",
            Self::Tiff => "TIFF",
            Self::WebP => "WebP",
        }
    }

    pub fn translation_key(&self) -> &'static str {
        match self {
            Self::All => "library_format_all",
            Self::Raw => "library_format_raw",
            Self::Jpeg => "library_format_jpeg",
            Self::Png => "library_format_png",
            Self::Tiff => "library_format_tiff",
            Self::WebP => "library_format_webp",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct LibraryFilter {
    pub min_rating: u8,
    pub color_label: Option<ColorLabel>,
    pub file_type: FileTypeFilter,
    pub search_query: Option<String>,
    pub sort_by: SortOrder,
    pub folder_path: Option<PathBuf>,
    pub recursive: bool,
    pub tag: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmartCollection {
    pub id: i64,
    pub name: String,
    pub min_rating: u8,
    pub color_label: Option<ColorLabel>,
    pub file_type: FileTypeFilter,
    pub search_query: Option<String>,
    pub tag: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct PhotoMetadata {
    pub title: String,
    pub caption: String,
    pub copyright: String,
    pub creator: String,
    pub gps_lat: Option<f64>,
    pub gps_lon: Option<f64>,
}

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
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
        )
        .map_err(|e| BeamError::Database(e.to_string()))?;

        let db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    pub const CURRENT_SCHEMA_VERSION: u32 = 1;
    pub const MAX_IMPORT_DEPTH: usize = 32;
    pub const MAX_FOLDER_IMPORT_COUNT: usize = 100_000;

    /// Apply schema migrations for the library catalog.
    fn migrate(&self) -> Result<()> {
        self.conn
            .execute_batch(
                r#"
            PRAGMA foreign_keys = ON;
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            PRAGMA busy_timeout = 5000;

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
                width INTEGER DEFAULT 0,
                height INTEGER DEFAULT 0,
                rating INTEGER DEFAULT 0,
                color_label TEXT DEFAULT 'none',
                thumbnail_path TEXT,
                sidecar_path TEXT
            );

            CREATE INDEX IF NOT EXISTS idx_photos_rating ON photos(rating);
            CREATE INDEX IF NOT EXISTS idx_photos_color ON photos(color_label);
            CREATE INDEX IF NOT EXISTS idx_photos_created_at ON photos(created_at DESC, id DESC);
            CREATE INDEX IF NOT EXISTS idx_photos_file_size ON photos(file_size);
            CREATE INDEX IF NOT EXISTS idx_photos_file_name ON photos(file_name);

            CREATE TABLE IF NOT EXISTS virtual_copies (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                photo_id INTEGER NOT NULL REFERENCES photos(id) ON DELETE CASCADE,
                copy_name TEXT NOT NULL,
                sidecar_path TEXT NOT NULL,
                created_at TEXT DEFAULT (datetime('now'))
            );

            CREATE INDEX IF NOT EXISTS idx_virtual_copies_photo ON virtual_copies(photo_id);

            CREATE TABLE IF NOT EXISTS tags (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL UNIQUE
            );

            CREATE TABLE IF NOT EXISTS photo_tags (
                photo_id INTEGER NOT NULL REFERENCES photos(id) ON DELETE CASCADE,
                tag_id INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
                PRIMARY KEY (photo_id, tag_id)
            );

            CREATE INDEX IF NOT EXISTS idx_photo_tags_tag ON photo_tags(tag_id);

            CREATE TABLE IF NOT EXISTS smart_collections (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL UNIQUE,
                min_rating INTEGER NOT NULL DEFAULT 0,
                color_label TEXT,
                file_type TEXT NOT NULL DEFAULT 'All',
                search_query TEXT,
                tag TEXT
            );

            CREATE TABLE IF NOT EXISTS photo_metadata (
                photo_id INTEGER PRIMARY KEY REFERENCES photos(id) ON DELETE CASCADE,
                title TEXT NOT NULL DEFAULT '',
                caption TEXT NOT NULL DEFAULT '',
                copyright TEXT NOT NULL DEFAULT '',
                creator TEXT NOT NULL DEFAULT '',
                gps_lat REAL,
                gps_lon REAL
            );
            "#,
            )
            .map_err(|e| BeamError::Database(e.to_string()))?;

        let version: Option<u32> = self
            .conn
            .query_row(
                "SELECT version FROM schema_version ORDER BY version DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| BeamError::Database(e.to_string()))?;

        match version {
            Some(v) if v > Self::CURRENT_SCHEMA_VERSION => {
                return Err(BeamError::Database(format!(
                    "Unsupported database schema version {v}. Maximum supported version is {}.",
                    Self::CURRENT_SCHEMA_VERSION
                )));
            }
            Some(_v) => {
                // Schema version is current
            }
            None => {
                self.conn
                    .execute(
                        "INSERT INTO schema_version (version) VALUES (?1)",
                        params![Self::CURRENT_SCHEMA_VERSION],
                    )
                    .map_err(|e| BeamError::Database(e.to_string()))?;
            }
        }

        Ok(())
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    /// Import a single image file into the library catalog.
    /// Also generates and caches the thumbnail and synchronizes sidecar metadata.
    pub fn import_file(&self, path: &Path, thumb_dir: &Path) -> Result<PhotoRecord> {
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        let file_path = canonical.to_string_lossy().to_string();
        let file_name = path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown".to_string());

        let metadata = fs::metadata(path)?;
        let file_size = metadata.len();

        let modified_at = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs().to_string());

        // Check if sidecar already exists on disk
        let sidecar_p = Sidecar::find_sidecar_for(path).unwrap_or_else(|| Sidecar::path_for(path));
        let sidecar_path_str = if sidecar_p.exists() {
            Some(sidecar_p.to_string_lossy().to_string())
        } else {
            None
        };

        let mut rating = 0u8;
        let mut color_label = ColorLabel::None;
        if sidecar_p.exists() {
            if let Ok(sc) = Sidecar::load_from_file(&sidecar_p) {
                rating = sc.rating;
                color_label = sc.color_label;
            }
        }

        // Fast thumbnail handling: reuse cached disk thumbnail if present.
        // For un-cached raster files, quickly generate a lightweight thumbnail.
        // For RAW files, defer thumbnail generation to the lazy background worker to avoid blocking the scan.
        let thumb_path = thumbnail_cache_path(path, thumb_dir);
        let thumb_path_str = if thumb_path.exists() {
            Some(thumb_path.to_string_lossy().to_string())
        } else if is_raster_file(path) {
            let edit_stack = if sidecar_p.exists() {
                Sidecar::load_from_file(&sidecar_p)
                    .map(|sc| sc.edit_stack)
                    .ok()
            } else {
                None
            };
            match generate_thumbnail_with_edits(path, edit_stack.as_ref(), 256) {
                Ok(img) => match save_thumbnail(&img, &thumb_path) {
                    Ok(()) => Some(thumb_path.to_string_lossy().to_string()),
                    Err(e) => {
                        log::warn!("Failed to save thumbnail for {file_name}: {e}");
                        None
                    }
                },
                Err(e) => {
                    log::warn!("Failed to generate thumbnail for {file_name}: {e}");
                    None
                }
            }
        } else {
            None
        };

        // Fast metadata extraction: reads only container headers & EXIF block (<0.1ms, zero full decode)
        let mut width = 0u32;
        let mut height = 0u32;
        let mut camera_make = None;
        let mut camera_model = None;
        let created_at = modified_at.clone();

        if let Some((w, h)) = extract_image_dimensions(path) {
            width = w;
            height = h;
        }

        let exif = extract_exif_metadata(path);
        if let Some(m) = exif.make {
            camera_make = Some(m);
        }
        if let Some(m) = exif.model {
            camera_model = Some(m);
        }

        let id = self
            .conn
            .query_row(
                r#"
            INSERT INTO photos (
                file_path, file_name, file_size, created_at, modified_at,
                camera_make, camera_model, width, height, rating, color_label,
                thumbnail_path, sidecar_path
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
            ON CONFLICT(file_path) DO UPDATE SET
                file_name=excluded.file_name,
                file_size=excluded.file_size,
                modified_at=excluded.modified_at,
                width=excluded.width,
                height=excluded.height,
                rating=CASE WHEN excluded.sidecar_path IS NOT NULL THEN excluded.rating ELSE photos.rating END,
                color_label=CASE WHEN excluded.sidecar_path IS NOT NULL THEN excluded.color_label ELSE photos.color_label END,
                thumbnail_path=COALESCE(excluded.thumbnail_path, photos.thumbnail_path),
                sidecar_path=COALESCE(excluded.sidecar_path, photos.sidecar_path)
            RETURNING id
            "#,
                params![
                    file_path,
                    file_name,
                    file_size as i64,
                    created_at,
                    modified_at,
                    camera_make,
                    camera_model,
                    width as i64,
                    height as i64,
                    rating as i64,
                    color_label.as_str(),
                    thumb_path_str,
                    sidecar_path_str,
                ],
                |row| row.get::<_, i64>(0),
            )
            .map_err(|e| BeamError::Database(e.to_string()))?;

        Ok(PhotoRecord {
            id,
            file_path,
            file_name,
            file_size,
            created_at,
            modified_at,
            camera_make,
            camera_model,
            width,
            height,
            rating,
            color_label,
            thumbnail_path: thumb_path_str,
            sidecar_path: sidecar_path_str,
        })
    }

    /// Recursively scan a folder and import all supported photos.
    /// Safely prevents symlink loops, caps traversal depth, and bounds maximum imported items.
    pub fn scan_and_import_folder(
        &self,
        folder: &Path,
        thumb_dir: &Path,
        recursive: bool,
    ) -> Result<usize> {
        let mut count = 0;
        let mut visited_dirs = HashSet::new();

        let canonical_root = folder
            .canonicalize()
            .unwrap_or_else(|_| folder.to_path_buf());
        visited_dirs.insert(canonical_root.clone());

        // Stack contains (directory_path, current_depth)
        let mut stack = vec![(folder.to_path_buf(), 0usize)];
        let in_tx = self.conn.execute("BEGIN IMMEDIATE", []).is_ok();

        while let Some((dir, depth)) = stack.pop() {
            if count >= Self::MAX_FOLDER_IMPORT_COUNT {
                log::warn!(
                    "Reached maximum folder import limit of {} items.",
                    Self::MAX_FOLDER_IMPORT_COUNT
                );
                break;
            }

            let entries = match fs::read_dir(&dir) {
                Ok(e) => e,
                Err(err) => {
                    log::warn!("Could not read directory {}: {err}", dir.display());
                    continue;
                }
            };

            for entry in entries.flatten() {
                if count >= Self::MAX_FOLDER_IMPORT_COUNT {
                    break;
                }

                let path = entry.path();
                let file_type = entry.file_type().ok();
                let is_symlink = file_type
                    .as_ref()
                    .map(|ft| ft.is_symlink())
                    .unwrap_or(false);

                // Isolation policy: Skip any symlink resolving outside the imported folder root
                if is_symlink {
                    if let Ok(target) = path.canonicalize() {
                        if !target.starts_with(&canonical_root) {
                            log::debug!(
                                "Skipping symlink pointing outside library root: {}",
                                path.display()
                            );
                            continue;
                        }
                    } else {
                        // Broken or unresolvable symlink
                        continue;
                    }
                }

                if recursive && depth < Self::MAX_IMPORT_DEPTH && !is_symlink && path.is_dir() {
                    let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
                    if visited_dirs.insert(canonical) {
                        stack.push((path, depth + 1));
                    }
                } else if path.is_file() && is_supported_image(&path) {
                    if let Ok(_photo) = self.import_file(&path, thumb_dir) {
                        count += 1;
                    }
                }
            }
        }

        if in_tx {
            let _ = self.conn.execute("COMMIT", []);
        }

        Ok(count)
    }

    /// Query photos matching the given filter criteria.
    pub fn query_photos(&self, filter: &LibraryFilter) -> Result<Vec<PhotoRecord>> {
        let mut sql = String::from(
            r#"
            SELECT id, file_path, file_name, file_size, created_at, modified_at,
                   camera_make, camera_model, width, height, rating, color_label,
                   thumbnail_path, sidecar_path
            FROM photos
            WHERE rating >= ?1
            "#,
        );

        let mut params_vec: Vec<rusqlite::types::Value> = vec![filter.min_rating.into()];

        if let Some(col) = filter.color_label {
            if col != ColorLabel::None {
                sql.push_str(" AND color_label = ?");
                params_vec.push(col.as_str().to_string().into());
            }
        }

        let escape_like = |s: &str| -> String {
            s.replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_")
        };

        if let Some(query) = &filter.search_query {
            let q = query.trim();
            if !q.is_empty() {
                sql.push_str(
                    " AND (file_name LIKE ? ESCAPE '\\' OR camera_model LIKE ? ESCAPE '\\')",
                );
                let pattern = format!("%{}%", escape_like(q));
                params_vec.push(pattern.clone().into());
                params_vec.push(pattern.into());
            }
        }

        match filter.file_type {
            FileTypeFilter::All => {}
            FileTypeFilter::Raw => {
                sql.push_str(
                    " AND (lower(file_name) LIKE '%.arw' OR lower(file_name) LIKE '%.cr2' OR lower(file_name) LIKE '%.cr3' OR lower(file_name) LIKE '%.nef' OR lower(file_name) LIKE '%.dng' OR lower(file_name) LIKE '%.raf' OR lower(file_name) LIKE '%.rw2' OR lower(file_name) LIKE '%.orf' OR lower(file_name) LIKE '%.pef')",
                );
            }
            FileTypeFilter::Jpeg => {
                sql.push_str(
                    " AND (lower(file_name) LIKE '%.jpg' OR lower(file_name) LIKE '%.jpeg')",
                );
            }
            FileTypeFilter::Png => {
                sql.push_str(" AND lower(file_name) LIKE '%.png'");
            }
            FileTypeFilter::Tiff => {
                sql.push_str(
                    " AND (lower(file_name) LIKE '%.tif' OR lower(file_name) LIKE '%.tiff')",
                );
            }
            FileTypeFilter::WebP => {
                sql.push_str(" AND lower(file_name) LIKE '%.webp'");
            }
        }

        if let Some(folder) = &filter.folder_path {
            let folder_str = folder.to_string_lossy().to_string();
            let sep = if folder_str.contains('\\') && !folder_str.contains('/') {
                '\\'
            } else {
                '/'
            };
            let prefix = if folder_str.ends_with('/') || folder_str.ends_with('\\') {
                folder_str
            } else {
                format!("{folder_str}{sep}")
            };
            let escaped_prefix = escape_like(&prefix);
            if filter.recursive {
                sql.push_str(" AND file_path LIKE ? ESCAPE '\\'");
                params_vec.push(format!("{escaped_prefix}%").into());
            } else {
                sql.push_str(
                    " AND file_path LIKE ? ESCAPE '\\' AND file_path NOT LIKE ? ESCAPE '\\' AND file_path NOT LIKE ? ESCAPE '\\'",
                );
                params_vec.push(format!("{escaped_prefix}%").into());
                params_vec.push(format!("{escaped_prefix}%/%").into());
                params_vec.push(format!("{escaped_prefix}%\\\\%").into());
            }
        }

        if let Some(tag) = &filter.tag {
            let trimmed = tag.trim();
            if !trimmed.is_empty() {
                sql.push_str(
                    " AND EXISTS (SELECT 1 FROM photo_tags pt JOIN tags t ON t.id = pt.tag_id WHERE pt.photo_id = photos.id AND t.name = ?)",
                );
                params_vec.push(trimmed.to_string().into());
            }
        }

        match filter.sort_by {
            SortOrder::DateTakenDesc => sql.push_str(" ORDER BY created_at DESC, id DESC"),
            SortOrder::DateTakenAsc => sql.push_str(" ORDER BY created_at ASC, id ASC"),
            SortOrder::RatingDesc => sql.push_str(" ORDER BY rating DESC, id DESC"),
            SortOrder::FileNameAsc => sql.push_str(" ORDER BY file_name ASC"),
        }

        let mut stmt = self
            .conn
            .prepare(&sql)
            .map_err(|e| BeamError::Database(e.to_string()))?;

        let rows = stmt
            .query_map(rusqlite::params_from_iter(params_vec), |row| {
                let col_label_str: String = row.get(11)?;
                Ok(PhotoRecord {
                    id: row.get(0)?,
                    file_path: row.get(1)?,
                    file_name: row.get(2)?,
                    file_size: row.get::<_, i64>(3)? as u64,
                    created_at: row.get(4)?,
                    modified_at: row.get(5)?,
                    camera_make: row.get(6)?,
                    camera_model: row.get(7)?,
                    width: row.get::<_, i64>(8)? as u32,
                    height: row.get::<_, i64>(9)? as u32,
                    rating: row.get::<_, i64>(10)? as u8,
                    color_label: ColorLabel::from_str_name(&col_label_str),
                    thumbnail_path: row.get(12)?,
                    sidecar_path: row.get(13)?,
                })
            })
            .map_err(|e| BeamError::Database(e.to_string()))?;

        let mut photos = Vec::new();
        for r in rows {
            photos.push(r.map_err(|e| BeamError::Database(e.to_string()))?);
        }

        Ok(photos)
    }

    /// Retrieve a single photo record by its database ID.
    pub fn get_photo(&self, photo_id: i64) -> Result<Option<PhotoRecord>> {
        let mut stmt = self
            .conn
            .prepare(
                r#"
            SELECT id, file_path, file_name, file_size, created_at, modified_at,
                   camera_make, camera_model, width, height, rating, color_label,
                   thumbnail_path, sidecar_path
            FROM photos
            WHERE id = ?1
            "#,
            )
            .map_err(|e| BeamError::Database(e.to_string()))?;

        let mut rows = stmt
            .query_map([photo_id], |row| {
                let col_label_str: String = row.get(11)?;
                Ok(PhotoRecord {
                    id: row.get(0)?,
                    file_path: row.get(1)?,
                    file_name: row.get(2)?,
                    file_size: row.get::<_, i64>(3)? as u64,
                    created_at: row.get(4)?,
                    modified_at: row.get(5)?,
                    camera_make: row.get(6)?,
                    camera_model: row.get(7)?,
                    width: row.get::<_, i64>(8)? as u32,
                    height: row.get::<_, i64>(9)? as u32,
                    rating: row.get::<_, i64>(10)? as u8,
                    color_label: ColorLabel::from_str_name(&col_label_str),
                    thumbnail_path: row.get(12)?,
                    sidecar_path: row.get(13)?,
                })
            })
            .map_err(|e| BeamError::Database(e.to_string()))?;

        if let Some(r) = rows.next() {
            Ok(Some(r.map_err(|e| BeamError::Database(e.to_string()))?))
        } else {
            Ok(None)
        }
    }

    fn sync_photo_sidecar(
        &self,
        photo_id: i64,
        update_fn: impl FnOnce(&mut Sidecar),
    ) -> Result<()> {
        if let Some(photo) = self.get_photo(photo_id)? {
            let img_p = Path::new(&photo.file_path);
            let sidecar_p =
                Sidecar::find_sidecar_for(img_p).unwrap_or_else(|| Sidecar::path_for(img_p));
            let mut sidecar = if sidecar_p.exists() {
                Sidecar::load_from_file(&sidecar_p)?
            } else {
                Sidecar::new(&photo.file_name)
            };
            update_fn(&mut sidecar);
            sidecar.save_to_file(&sidecar_p)?;
        }
        Ok(())
    }

    /// Update star rating (0-5) for a photo and synchronize to its sidecar.
    pub fn update_rating(&self, photo_id: i64, rating: u8) -> Result<()> {
        let clamped = rating.min(5);
        self.conn
            .execute(
                "UPDATE photos SET rating = ?1 WHERE id = ?2",
                params![clamped as i64, photo_id],
            )
            .map_err(|e| BeamError::Database(e.to_string()))?;

        self.sync_photo_sidecar(photo_id, |sidecar| sidecar.set_rating(clamped))
    }

    /// Update color label for a photo and synchronize to its sidecar.
    pub fn update_color_label(&self, photo_id: i64, label: ColorLabel) -> Result<()> {
        self.conn
            .execute(
                "UPDATE photos SET color_label = ?1 WHERE id = ?2",
                params![label.as_str(), photo_id],
            )
            .map_err(|e| BeamError::Database(e.to_string()))?;

        self.sync_photo_sidecar(photo_id, |sidecar| sidecar.set_color_label(label))
    }

    /// Update the cached thumbnail path for a photo in the database.
    pub fn update_thumbnail_path(&self, photo_id: i64, thumb_path: &str) -> Result<()> {
        self.conn
            .execute(
                "UPDATE photos SET thumbnail_path = ?1 WHERE id = ?2",
                params![thumb_path, photo_id],
            )
            .map_err(|e| BeamError::Database(e.to_string()))?;
        Ok(())
    }

    /// Clear all thumbnail paths in the database (e.g. for rebuilding thumbnails).
    pub fn clear_all_thumbnails(&self) -> Result<()> {
        self.conn
            .execute("UPDATE photos SET thumbnail_path = NULL", [])
            .map_err(|e| BeamError::Database(e.to_string()))?;
        Ok(())
    }

    /// Update the file path and file name for a photo (e.g. after moving or relocating a file).
    pub fn update_photo_path(&self, photo_id: i64, new_path: &Path) -> Result<()> {
        let new_path_str = new_path.to_string_lossy().to_string();
        let new_name = new_path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown".to_string());

        let sidecar_p =
            Sidecar::find_sidecar_for(new_path).unwrap_or_else(|| Sidecar::path_for(new_path));
        let sidecar_str = if sidecar_p.exists() {
            Some(sidecar_p.to_string_lossy().to_string())
        } else {
            None
        };

        self.conn
            .execute(
                "UPDATE photos SET file_path = ?1, file_name = ?2, sidecar_path = ?3 WHERE id = ?4",
                params![new_path_str, new_name, sidecar_str, photo_id],
            )
            .map_err(|e| BeamError::Database(e.to_string()))?;
        Ok(())
    }

    /// Delete a single photo record from the database.
    pub fn delete_photo(&self, photo_id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM photos WHERE id = ?1", params![photo_id])
            .map_err(|e| BeamError::Database(e.to_string()))?;
        Ok(())
    }

    /// Delete multiple photo records from the database inside a single transaction.
    pub fn delete_photos(&self, photo_ids: &[i64]) -> Result<usize> {
        if photo_ids.is_empty() {
            return Ok(0);
        }
        self.conn
            .execute_batch("BEGIN IMMEDIATE")
            .map_err(|e| BeamError::Database(e.to_string()))?;
        let mut count = 0;
        for &id in photo_ids {
            if self.delete_photo(id).is_ok() {
                count += 1;
            }
        }
        self.conn
            .execute_batch("COMMIT")
            .map_err(|e| BeamError::Database(e.to_string()))?;
        Ok(count)
    }

    /// Scan all photo records in the catalog and delete any whose file does not exist on disk.
    pub fn remove_missing_photos(&self) -> Result<usize> {
        let all_photos = self.query_photos(&LibraryFilter::default())?;
        self.conn
            .execute_batch("BEGIN IMMEDIATE")
            .map_err(|e| BeamError::Database(e.to_string()))?;
        let mut removed = 0;
        for photo in all_photos {
            if !Path::new(&photo.file_path).exists() && self.delete_photo(photo.id).is_ok() {
                removed += 1;
            }
        }
        self.conn
            .execute_batch("COMMIT")
            .map_err(|e| BeamError::Database(e.to_string()))?;
        Ok(removed)
    }

    /// Relink all photos residing under an old directory to a new directory.
    /// If a file exists in `new_dir` matching the relative path from `old_dir`, updates the database.
    /// Returns the number of successfully remapped photos.
    pub fn relink_folder(&self, old_dir: &Path, new_dir: &Path) -> Result<usize> {
        let all_photos = self.query_photos(&LibraryFilter::default())?;
        let mut remapped = 0;

        for photo in all_photos {
            let current_p = Path::new(&photo.file_path);
            if let Ok(rel) = current_p.strip_prefix(old_dir) {
                let candidate = new_dir.join(rel);
                if candidate.is_file() && self.update_photo_path(photo.id, &candidate).is_ok() {
                    remapped += 1;
                }
            }
        }

        Ok(remapped)
    }

    /// Create a new non-destructive virtual copy for an existing photo, cloning its current sidecar edits.
    pub fn create_virtual_copy(&self, photo_id: i64) -> Result<VirtualCopyRecord> {
        let photo = self
            .get_photo(photo_id)?
            .ok_or_else(|| BeamError::Database(format!("Photo id {photo_id} not found")))?;

        let existing_count: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM virtual_copies WHERE photo_id = ?1",
                params![photo_id],
                |row| row.get(0),
            )
            .map_err(|e| BeamError::Database(e.to_string()))?;

        let copy_num = existing_count + 1;
        let copy_name = format!("Copy {copy_num}");
        let src_path = PathBuf::from(&photo.file_path);
        let file_name = src_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("photo")
            .to_string();

        // Generate unique sidecar path: <photo>.v<n>.beam
        let mut idx = copy_num;
        let vc_sidecar_path = loop {
            let candidate = PathBuf::from(format!("{}.v{idx}.beam", photo.file_path));
            if !candidate.exists() {
                break candidate;
            }
            idx += 1;
        };

        let master_sidecar_path = Sidecar::path_for(&src_path);
        let sidecar = if master_sidecar_path.exists() {
            Sidecar::load_from_file(&master_sidecar_path)
                .unwrap_or_else(|_| Sidecar::new(file_name))
        } else {
            Sidecar::new(file_name)
        };
        sidecar.save_to_file(&vc_sidecar_path)?;

        let vc_path_str = vc_sidecar_path.to_string_lossy().to_string();
        self.conn
            .execute(
                "INSERT INTO virtual_copies (photo_id, copy_name, sidecar_path) VALUES (?1, ?2, ?3)",
                params![photo_id, copy_name, vc_path_str],
            )
            .map_err(|e| BeamError::Database(e.to_string()))?;

        let id = self.conn.last_insert_rowid();
        Ok(VirtualCopyRecord {
            id,
            photo_id,
            copy_name,
            sidecar_path: vc_path_str,
            created_at: None,
        })
    }

    /// List all virtual copies associated with a photo.
    pub fn list_virtual_copies(&self, photo_id: i64) -> Result<Vec<VirtualCopyRecord>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, photo_id, copy_name, sidecar_path, created_at FROM virtual_copies WHERE photo_id = ?1 ORDER BY id ASC",
            )
            .map_err(|e| BeamError::Database(e.to_string()))?;

        let rows = stmt
            .query_map(params![photo_id], |row| {
                Ok(VirtualCopyRecord {
                    id: row.get(0)?,
                    photo_id: row.get(1)?,
                    copy_name: row.get(2)?,
                    sidecar_path: row.get(3)?,
                    created_at: row.get(4)?,
                })
            })
            .map_err(|e| BeamError::Database(e.to_string()))?;

        let mut list = Vec::new();
        for row in rows {
            list.push(row.map_err(|e| BeamError::Database(e.to_string()))?);
        }
        Ok(list)
    }

    /// Delete a virtual copy and its sidecar file.
    pub fn delete_virtual_copy(&self, copy_id: i64) -> Result<()> {
        let sidecar_path: Option<String> = self
            .conn
            .query_row(
                "SELECT sidecar_path FROM virtual_copies WHERE id = ?1",
                params![copy_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| BeamError::Database(e.to_string()))?;

        if let Some(p) = sidecar_path {
            let path = PathBuf::from(p);
            if path.exists() {
                let _ = fs::remove_file(path);
            }
        }

        self.conn
            .execute("DELETE FROM virtual_copies WHERE id = ?1", params![copy_id])
            .map_err(|e| BeamError::Database(e.to_string()))?;
        Ok(())
    }

    /// Add a keyword/tag to a photo (trimmed, case-preserving, deduplicated).
    pub fn add_tag(&self, photo_id: i64, tag_name: &str) -> Result<()> {
        let trimmed = tag_name.trim();
        if trimmed.is_empty() {
            return Ok(());
        }
        self.conn
            .execute(
                "INSERT OR IGNORE INTO tags (name) VALUES (?1)",
                params![trimmed],
            )
            .map_err(|e| BeamError::Database(e.to_string()))?;
        self.conn
            .execute(
                "INSERT OR IGNORE INTO photo_tags (photo_id, tag_id)
                 SELECT ?1, id FROM tags WHERE name = ?2",
                params![photo_id, trimmed],
            )
            .map_err(|e| BeamError::Database(e.to_string()))?;
        Ok(())
    }

    /// Remove a keyword/tag from a photo.
    pub fn remove_tag(&self, photo_id: i64, tag_name: &str) -> Result<()> {
        let trimmed = tag_name.trim();
        if trimmed.is_empty() {
            return Ok(());
        }
        self.conn
            .execute(
                "DELETE FROM photo_tags
                 WHERE photo_id = ?1 AND tag_id IN (SELECT id FROM tags WHERE name = ?2)",
                params![photo_id, trimmed],
            )
            .map_err(|e| BeamError::Database(e.to_string()))?;
        Ok(())
    }

    /// List all tags attached to a specific photo, sorted alphabetically.
    pub fn get_photo_tags(&self, photo_id: i64) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT t.name FROM tags t
                 JOIN photo_tags pt ON pt.tag_id = t.id
                 WHERE pt.photo_id = ?1
                 ORDER BY t.name ASC",
            )
            .map_err(|e| BeamError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(params![photo_id], |row| row.get::<_, String>(0))
            .map_err(|e| BeamError::Database(e.to_string()))?;
        let mut tags = Vec::new();
        for row in rows {
            tags.push(row.map_err(|e| BeamError::Database(e.to_string()))?);
        }
        Ok(tags)
    }

    /// List all tags currently assigned to at least one photo, with photo counts.
    pub fn list_all_tags(&self) -> Result<Vec<(String, usize)>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT t.name, COUNT(pt.photo_id)
                 FROM tags t
                 JOIN photo_tags pt ON pt.tag_id = t.id
                 GROUP BY t.id, t.name
                 ORDER BY t.name ASC",
            )
            .map_err(|e| BeamError::Database(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as usize))
            })
            .map_err(|e| BeamError::Database(e.to_string()))?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row.map_err(|e| BeamError::Database(e.to_string()))?);
        }
        Ok(result)
    }

    /// Save or update a named Smart Collection (saved LibraryFilter).
    pub fn save_smart_collection(&self, name: &str, filter: &LibraryFilter) -> Result<i64> {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Err(BeamError::Database(
                "Smart collection name cannot be empty".into(),
            ));
        }
        let color_str = filter.color_label.map(|c| c.as_str().to_string());
        let id: i64 = self
            .conn
            .query_row(
                "INSERT INTO smart_collections (name, min_rating, color_label, file_type, search_query, tag)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(name) DO UPDATE SET
                    min_rating = excluded.min_rating,
                    color_label = excluded.color_label,
                    file_type = excluded.file_type,
                    search_query = excluded.search_query,
                    tag = excluded.tag
                 RETURNING id",
                params![
                    trimmed,
                    filter.min_rating as i64,
                    color_str,
                    filter.file_type.as_str(),
                    filter.search_query,
                    filter.tag
                ],
                |row| row.get(0),
            )
            .map_err(|e| BeamError::Database(e.to_string()))?;
        Ok(id)
    }

    /// List all saved Smart Collections sorted alphabetically.
    pub fn list_smart_collections(&self) -> Result<Vec<SmartCollection>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, name, min_rating, color_label, file_type, search_query, tag
                 FROM smart_collections
                 ORDER BY name ASC",
            )
            .map_err(|e| BeamError::Database(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                let col_str: Option<String> = row.get(3)?;
                let ft_str: String = row.get(4)?;
                let file_type = match ft_str.as_str() {
                    "RAW" => FileTypeFilter::Raw,
                    "JPEG" => FileTypeFilter::Jpeg,
                    "PNG" => FileTypeFilter::Png,
                    "TIFF" => FileTypeFilter::Tiff,
                    "WebP" => FileTypeFilter::WebP,
                    _ => FileTypeFilter::All,
                };
                Ok(SmartCollection {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    min_rating: (row.get::<_, i64>(2)?).clamp(0, 5) as u8,
                    color_label: col_str.map(|s| ColorLabel::from_str_name(&s)),
                    file_type,
                    search_query: row.get(5)?,
                    tag: row.get(6)?,
                })
            })
            .map_err(|e| BeamError::Database(e.to_string()))?;
        let mut list = Vec::new();
        for row in rows {
            list.push(row.map_err(|e| BeamError::Database(e.to_string()))?);
        }
        Ok(list)
    }

    /// Delete a Smart Collection by ID.
    pub fn delete_smart_collection(&self, id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM smart_collections WHERE id = ?1", params![id])
            .map_err(|e| BeamError::Database(e.to_string()))?;
        Ok(())
    }

    /// Get IPTC/XMP metadata and GPS coordinates for a photo (returns empty defaults if not yet set).
    pub fn get_photo_metadata(&self, photo_id: i64) -> Result<PhotoMetadata> {
        let meta = self
            .conn
            .query_row(
                "SELECT title, caption, copyright, creator, gps_lat, gps_lon FROM photo_metadata WHERE photo_id = ?1",
                params![photo_id],
                |row| {
                    Ok(PhotoMetadata {
                        title: row.get(0)?,
                        caption: row.get(1)?,
                        copyright: row.get(2)?,
                        creator: row.get(3)?,
                        gps_lat: row.get(4)?,
                        gps_lon: row.get(5)?,
                    })
                },
            )
            .optional()
            .map_err(|e| BeamError::Database(e.to_string()))?;
        Ok(meta.unwrap_or_default())
    }

    /// Save IPTC/XMP metadata and GPS coordinates for a photo.
    pub fn set_photo_metadata(&self, photo_id: i64, meta: &PhotoMetadata) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO photo_metadata (photo_id, title, caption, copyright, creator, gps_lat, gps_lon)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(photo_id) DO UPDATE SET
                    title = excluded.title,
                    caption = excluded.caption,
                    copyright = excluded.copyright,
                    creator = excluded.creator,
                    gps_lat = excluded.gps_lat,
                    gps_lon = excluded.gps_lon",
                params![
                    photo_id,
                    meta.title,
                    meta.caption,
                    meta.copyright,
                    meta.creator,
                    meta.gps_lat,
                    meta.gps_lon
                ],
            )
            .map_err(|e| BeamError::Database(e.to_string()))?;
        Ok(())
    }

    /// Detect exact duplicate files by grouping on file_size and verifying streaming 128-bit content fingerprint.
    pub fn find_duplicates(&self) -> Result<Vec<Vec<PhotoRecord>>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, file_path, file_name, file_size, created_at, modified_at,
                        camera_make, camera_model, width, height, rating, color_label,
                        thumbnail_path, sidecar_path
                 FROM photos
                 WHERE file_size IN (
                     SELECT file_size FROM photos WHERE file_size > 0
                     GROUP BY file_size HAVING COUNT(*) > 1
                 )
                 ORDER BY file_size ASC, id ASC",
            )
            .map_err(|e| BeamError::Database(e.to_string()))?;

        let rows: Vec<PhotoRecord> = stmt
            .query_map([], |row| {
                let col_str: String = row.get(11)?;
                Ok(PhotoRecord {
                    id: row.get(0)?,
                    file_path: row.get(1)?,
                    file_name: row.get(2)?,
                    file_size: row.get::<_, i64>(3)? as u64,
                    created_at: row.get(4)?,
                    modified_at: row.get(5)?,
                    camera_make: row.get(6)?,
                    camera_model: row.get(7)?,
                    width: row.get::<_, i64>(8)? as u32,
                    height: row.get::<_, i64>(9)? as u32,
                    rating: row.get::<_, i64>(10)? as u8,
                    color_label: ColorLabel::from_str_name(&col_str),
                    thumbnail_path: row.get(12)?,
                    sidecar_path: row.get(13)?,
                })
            })
            .map_err(|e| BeamError::Database(e.to_string()))?
            .filter_map(|r| r.ok())
            .collect();

        let mut by_size_and_hash: std::collections::BTreeMap<(u64, u128), Vec<PhotoRecord>> =
            std::collections::BTreeMap::new();
        for rec in rows {
            if let Ok(fingerprint) = stream_file_fingerprint_128(Path::new(&rec.file_path)) {
                by_size_and_hash
                    .entry((rec.file_size, fingerprint))
                    .or_default()
                    .push(rec);
            }
        }

        let groups = by_size_and_hash
            .into_values()
            .filter(|g| g.len() > 1)
            .collect();
        Ok(groups)
    }
}

fn stream_file_fingerprint_128(path: &Path) -> std::io::Result<u128> {
    use std::io::{BufReader, Read};
    const FNV_OFFSET_128: u128 = 0x6c62272e07bb014262b821756295c58d;
    const FNV_PRIME_128: u128 = 0x0000000001000000000000000000013b;

    let file = fs::File::open(path)?;
    let mut reader = BufReader::with_capacity(64 * 1024, file);
    let mut buf = [0u8; 64 * 1024];
    let mut hash = FNV_OFFSET_128;
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        for &b in &buf[..n] {
            hash ^= b as u128;
            hash = hash.wrapping_mul(FNV_PRIME_128);
        }
    }
    Ok(hash)
}

/// List immediate subdirectories within a directory, ignoring hidden (dot) folders and symlinks, sorted alphabetically.
pub fn list_subdirectories(folder: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(entries) = fs::read_dir(folder) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Ok(meta) = fs::symlink_metadata(&path) {
                if !meta.file_type().is_symlink() && meta.is_dir() {
                    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                        if !name.starts_with('.') {
                            dirs.push(path);
                        }
                    }
                }
            }
        }
    }
    dirs.sort_by(|a, b| {
        let name_a = a.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let name_b = b.file_name().and_then(|n| n.to_str()).unwrap_or("");
        name_a.to_lowercase().cmp(&name_b.to_lowercase())
    });
    dirs
}

/// Return the standard XMP sidecar path for a photo (`<stem>.xmp` next to the photo).
pub fn xmp_path_for(photo_path: &Path) -> PathBuf {
    photo_path.with_extension("xmp")
}

fn escape_xml(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(ch),
        }
    }
    out
}

/// Write an industry-standard XMP sidecar file (`xmp:Rating`, `xmp:Label`, `dc:title`,
/// `dc:description`, `dc:creator`, `dc:rights`, `dc:subject`) atomically next to `photo_path`.
pub fn write_xmp_sidecar(
    photo_path: &Path,
    meta: &PhotoMetadata,
    rating: u8,
    color_label: ColorLabel,
    tags: &[String],
) -> Result<PathBuf> {
    let xmp_path = xmp_path_for(photo_path);
    let label_str = match color_label {
        ColorLabel::None => "",
        ColorLabel::Red => "Red",
        ColorLabel::Yellow => "Yellow",
        ColorLabel::Green => "Green",
        ColorLabel::Blue => "Blue",
        ColorLabel::Purple => "Purple",
    };

    let mut xml = String::from(
        "<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n\
         <x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\n\
         \x20<rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n\
         \x20\x20<rdf:Description rdf:about=\"\"\n\
         \x20\x20\x20\x20xmlns:xmp=\"http://ns.adobe.com/xap/1.0/\"\n\
         \x20\x20\x20\x20xmlns:dc=\"http://purl.org/dc/elements/1.1/\"\n",
    );
    xml.push_str(&format!("    xmp:Rating=\"{}\"\n", rating.min(5)));
    if !label_str.is_empty() {
        xml.push_str(&format!("    xmp:Label=\"{}\"\n", label_str));
    }
    xml.push_str("  >\n");

    if !meta.title.trim().is_empty() {
        xml.push_str(&format!(
            "   <dc:title><rdf:Alt><rdf:li xml:lang=\"x-default\">{}</rdf:li></rdf:Alt></dc:title>\n",
            escape_xml(&meta.title)
        ));
    }
    if !meta.caption.trim().is_empty() {
        xml.push_str(&format!(
            "   <dc:description><rdf:Alt><rdf:li xml:lang=\"x-default\">{}</rdf:li></rdf:Alt></dc:description>\n",
            escape_xml(&meta.caption)
        ));
    }
    if !meta.creator.trim().is_empty() {
        xml.push_str(&format!(
            "   <dc:creator><rdf:Seq><rdf:li>{}</rdf:li></rdf:Seq></dc:creator>\n",
            escape_xml(&meta.creator)
        ));
    }
    if !meta.copyright.trim().is_empty() {
        xml.push_str(&format!(
            "   <dc:rights><rdf:Alt><rdf:li xml:lang=\"x-default\">{}</rdf:li></rdf:Alt></dc:rights>\n",
            escape_xml(&meta.copyright)
        ));
    }
    if !tags.is_empty() {
        xml.push_str("   <dc:subject><rdf:Bag>\n");
        for tag in tags {
            if !tag.trim().is_empty() {
                xml.push_str(&format!("    <rdf:li>{}</rdf:li>\n", escape_xml(tag)));
            }
        }
        xml.push_str("   </rdf:Bag></dc:subject>\n");
    }

    xml.push_str(
        "  </rdf:Description>\n\
         \x20</rdf:RDF>\n\
         </x:xmpmeta>\n\
         <?xpacket end=\"w\"?>\n",
    );

    beam_core::fs_util::atomic_write(&xmp_path, xml.as_bytes())?;
    Ok(xmp_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgba};

    #[test]
    fn test_library_db_in_memory_migration() {
        let db = LibraryDb::open_in_memory().expect("Failed to open in-memory db");
        let count: i64 = db
            .connection()
            .query_row("SELECT count(*) FROM photos", [], |row| row.get(0))
            .expect("Failed to query photos");
        assert_eq!(count, 0);
    }

    #[test]
    fn test_library_import_and_filtering() {
        let db = LibraryDb::open_in_memory().expect("Failed to open db");
        let temp_dir = std::env::temp_dir().join("beam_library_test");
        let _ = fs::create_dir_all(&temp_dir);

        let img_path1 = temp_dir.join("photo1.png");
        let img_path2 = temp_dir.join("photo2.png");
        let thumb_dir = temp_dir.join("thumbs");

        let img1: ImageBuffer<Rgba<u8>, Vec<u8>> =
            ImageBuffer::from_pixel(100, 100, Rgba([200, 50, 50, 255]));
        img1.save(&img_path1).unwrap();

        let img2: ImageBuffer<Rgba<u8>, Vec<u8>> =
            ImageBuffer::from_pixel(100, 100, Rgba([50, 200, 50, 255]));
        img2.save(&img_path2).unwrap();

        let p1 = db.import_file(&img_path1, &thumb_dir).unwrap();
        let p2 = db.import_file(&img_path2, &thumb_dir).unwrap();

        // Update ratings and labels
        db.update_rating(p1.id, 4).unwrap();
        db.update_color_label(p1.id, ColorLabel::Red).unwrap();

        db.update_rating(p2.id, 2).unwrap();
        db.update_color_label(p2.id, ColorLabel::Green).unwrap();

        // Filter: min 3 stars
        let filter_stars = LibraryFilter {
            min_rating: 3,
            ..Default::default()
        };
        let res_stars = db.query_photos(&filter_stars).unwrap();
        assert_eq!(res_stars.len(), 1);
        assert_eq!(res_stars[0].id, p1.id);

        // Filter: color label Green
        let filter_green = LibraryFilter {
            color_label: Some(ColorLabel::Green),
            ..Default::default()
        };
        let res_green = db.query_photos(&filter_green).unwrap();
        assert_eq!(res_green.len(), 1);
        assert_eq!(res_green[0].id, p2.id);

        // Filter: search query "photo1"
        let filter_search = LibraryFilter {
            search_query: Some("photo1".to_string()),
            ..Default::default()
        };
        let res_search = db.query_photos(&filter_search).unwrap();
        assert_eq!(res_search.len(), 1);
        assert_eq!(res_search[0].id, p1.id);

        // Cleanup
        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn test_library_thumbnail_updates() {
        let db = LibraryDb::open_in_memory().expect("Failed to open db");
        let temp_dir = std::env::temp_dir().join("beam_library_thumb_test");
        let _ = fs::create_dir_all(&temp_dir);

        let img_path = temp_dir.join("photo.png");
        let thumb_dir = temp_dir.join("thumbs");

        let img: ImageBuffer<Rgba<u8>, Vec<u8>> =
            ImageBuffer::from_pixel(50, 50, Rgba([100, 100, 100, 255]));
        img.save(&img_path).unwrap();

        let photo = db.import_file(&img_path, &thumb_dir).unwrap();
        assert!(photo.thumbnail_path.is_some());

        // Update thumbnail path
        db.update_thumbnail_path(photo.id, "/new/thumb/path.jpg")
            .unwrap();
        let updated = db.get_photo(photo.id).unwrap().unwrap();
        assert_eq!(
            updated.thumbnail_path.as_deref(),
            Some("/new/thumb/path.jpg")
        );

        // Clear all thumbnails
        db.clear_all_thumbnails().unwrap();
        let cleared = db.get_photo(photo.id).unwrap().unwrap();
        assert_eq!(cleared.thumbnail_path, None);

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn test_library_file_type_filtering() {
        let db = LibraryDb::open_in_memory().expect("Failed to open db");
        let temp_dir = std::env::temp_dir().join("beam_library_ft_test");
        let _ = fs::create_dir_all(&temp_dir);

        let p_png = temp_dir.join("photo.png");
        let p_jpg = temp_dir.join("photo.jpg");
        let p_webp = temp_dir.join("photo.webp");
        let thumb_dir = temp_dir.join("thumbs");

        let img: ImageBuffer<Rgba<u8>, Vec<u8>> =
            ImageBuffer::from_pixel(10, 10, Rgba([100, 100, 100, 255]));
        img.save(&p_png).unwrap();
        image::DynamicImage::ImageRgba8(img.clone())
            .to_rgb8()
            .save(&p_jpg)
            .unwrap();
        img.save(&p_webp).unwrap();

        db.import_file(&p_png, &thumb_dir).unwrap();
        db.import_file(&p_jpg, &thumb_dir).unwrap();
        db.import_file(&p_webp, &thumb_dir).unwrap();

        // 1. All
        let all = db.query_photos(&LibraryFilter::default()).unwrap();
        assert_eq!(all.len(), 3);

        // 2. PNG only
        let filter_png = LibraryFilter {
            file_type: FileTypeFilter::Png,
            ..Default::default()
        };
        let png_only = db.query_photos(&filter_png).unwrap();
        assert_eq!(png_only.len(), 1);
        assert!(png_only[0].file_name.ends_with(".png"));

        // 3. JPEG only
        let filter_jpg = LibraryFilter {
            file_type: FileTypeFilter::Jpeg,
            ..Default::default()
        };
        let jpg_only = db.query_photos(&filter_jpg).unwrap();
        assert_eq!(jpg_only.len(), 1);
        assert!(jpg_only[0].file_name.ends_with(".jpg"));

        // 4. WebP only
        let filter_webp = LibraryFilter {
            file_type: FileTypeFilter::WebP,
            ..Default::default()
        };
        let webp_only = db.query_photos(&filter_webp).unwrap();
        assert_eq!(webp_only.len(), 1);
        assert!(webp_only[0].file_name.ends_with(".webp"));

        // 5. RAW only
        let filter_raw = LibraryFilter {
            file_type: FileTypeFilter::Raw,
            ..Default::default()
        };
        let raw_only = db.query_photos(&filter_raw).unwrap();
        assert_eq!(raw_only.len(), 0);

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn test_schema_version_validation() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE schema_version (version INTEGER PRIMARY KEY);
            INSERT INTO schema_version (version) VALUES (999);
            "#,
        )
        .unwrap();

        let db = LibraryDb { conn };
        let err = db.migrate();
        assert!(err.is_err(), "Must reject future schema versions");
        let msg = err.err().unwrap().to_string();
        assert!(msg.contains("Unsupported database schema version 999"));
    }

    #[test]
    fn test_scan_folder_symlink_cycle_protection() {
        let temp_dir = std::env::temp_dir().join("beam_library_symlink_test");
        let _ = fs::remove_dir_all(&temp_dir);
        let sub_dir = temp_dir.join("sub");
        fs::create_dir_all(&sub_dir).unwrap();

        let img_path = temp_dir.join("test.png");
        let img: ImageBuffer<Rgba<u8>, Vec<u8>> =
            ImageBuffer::from_pixel(10, 10, Rgba([10, 20, 30, 255]));
        img.save(&img_path).unwrap();

        #[cfg(unix)]
        {
            let loop_link = sub_dir.join("cycle_link");
            let _ = std::os::unix::fs::symlink(&temp_dir, &loop_link);
        }

        let db = LibraryDb::open_in_memory().unwrap();
        let thumb_dir = temp_dir.join("thumbs");
        let count = db
            .scan_and_import_folder(&temp_dir, &thumb_dir, true)
            .unwrap();
        assert_eq!(
            count, 1,
            "Must import exactly 1 image despite symlink cycle"
        );

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn test_sql_injection_and_metacharacters() {
        let db = LibraryDb::open_in_memory().unwrap();
        let temp_dir = std::env::temp_dir().join("beam_library_sqli_test");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        let img_path = temp_dir.join("target.png");
        let img: ImageBuffer<Rgba<u8>, Vec<u8>> =
            ImageBuffer::from_pixel(10, 10, Rgba([0, 0, 0, 255]));
        img.save(&img_path).unwrap();

        let thumb_dir = temp_dir.join("thumbs");
        db.import_file(&img_path, &thumb_dir).unwrap();

        // 1. Classic SQL injection attempt
        let sqli_filter = LibraryFilter {
            search_query: Some("'; DROP TABLE photos; --".into()),
            ..Default::default()
        };
        let res = db.query_photos(&sqli_filter).unwrap();
        assert_eq!(res.len(), 0, "Injection search must not match anything");

        // Verify photos table is still intact and readable
        let count_filter = LibraryFilter::default();
        assert_eq!(
            db.query_photos(&count_filter).unwrap().len(),
            1,
            "Table must not be dropped!"
        );

        // 2. OR injection attempt
        let or_filter = LibraryFilter {
            search_query: Some("' OR '1'='1".into()),
            ..Default::default()
        };
        let res_or = db.query_photos(&or_filter).unwrap();
        assert_eq!(
            res_or.len(),
            0,
            "Must treat search literally, not boolean OR"
        );

        // 3. Metacharacter % (must be escaped as literal, not wildcard)
        let wildcard_filter = LibraryFilter {
            search_query: Some("%".into()),
            ..Default::default()
        };
        let res_wc = db.query_photos(&wildcard_filter).unwrap();
        assert_eq!(res_wc.len(), 0, "Literal % must not act as SQL wildcard");

        // 4. Very long search query (10,000 characters)
        let long_query = "x".repeat(10_000);
        let long_filter = LibraryFilter {
            search_query: Some(long_query),
            ..Default::default()
        };
        let res_long = db.query_photos(&long_filter).unwrap();
        assert_eq!(res_long.len(), 0, "Oversized query handled safely");

        // 5. Unicode and special characters
        let unicode_filter = LibraryFilter {
            search_query: Some("öçşğü_日本語_Ω".into()),
            ..Default::default()
        };
        let res_uni = db.query_photos(&unicode_filter).unwrap();
        assert_eq!(res_uni.len(), 0);

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn test_corrupted_database_handling() {
        let temp_dir = std::env::temp_dir().join("beam_library_corrupt_test");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        let corrupt_db_path = temp_dir.join("corrupted.db");
        // Write invalid garbage bytes that do not form a valid SQLite database header
        fs::write(
            &corrupt_db_path,
            b"NOT_A_SQLITE_DATABASE_INVALID_GARBAGE_BYTES_12345",
        )
        .unwrap();

        let res = LibraryDb::open(&corrupt_db_path);
        assert!(res.is_err(), "Must return error on corrupted database file");
        match res {
            Err(BeamError::Database(msg)) => {
                assert!(!msg.is_empty(), "Error message must be present");
            }
            Err(e) => panic!("Expected BeamError::Database, got {e:?}"),
            Ok(_) => panic!("Expected error, but database opened successfully"),
        }

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn test_locked_database_handling() {
        let temp_dir = std::env::temp_dir().join("beam_library_lock_test");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        let db_path = temp_dir.join("library.db");
        let db1 = LibraryDb::open(&db_path).unwrap();

        // Lock database exclusively with db1
        db1.conn.execute("BEGIN EXCLUSIVE", []).unwrap();

        // Open second connection and try to write; must receive busy/locked error without panic
        let db2 = Connection::open(&db_path).unwrap();
        let res = db2.execute("INSERT INTO schema_version (version) VALUES (2)", []);
        assert!(
            res.is_err(),
            "Second connection must fail gracefully when locked"
        );

        // Rollback first connection to release lock cleanly
        db1.conn.execute("ROLLBACK", []).unwrap();

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn test_symlinks_outside_folder_skipped() {
        let temp_dir = std::env::temp_dir().join("beam_library_symlink_ext_test");
        let _ = fs::remove_dir_all(&temp_dir);
        let lib_dir = temp_dir.join("library_root");
        let ext_dir = temp_dir.join("external_dir");
        fs::create_dir_all(&lib_dir).unwrap();
        fs::create_dir_all(&ext_dir).unwrap();

        let outside_img = ext_dir.join("secret.png");
        let img1: ImageBuffer<Rgba<u8>, Vec<u8>> =
            ImageBuffer::from_pixel(10, 10, Rgba([1, 1, 1, 255]));
        img1.save(&outside_img).unwrap();

        let inside_img = lib_dir.join("normal.png");
        let img2: ImageBuffer<Rgba<u8>, Vec<u8>> =
            ImageBuffer::from_pixel(10, 10, Rgba([2, 2, 2, 255]));
        img2.save(&inside_img).unwrap();

        #[cfg(unix)]
        {
            let symlink_path = lib_dir.join("link_to_outside.png");
            let _ = std::os::unix::fs::symlink(&outside_img, &symlink_path);
        }

        let db = LibraryDb::open_in_memory().unwrap();
        let thumb_dir = temp_dir.join("thumbs");
        let count = db
            .scan_and_import_folder(&lib_dir, &thumb_dir, true)
            .unwrap();

        // Must import exactly 1 image (normal.png); the external symlink must be skipped
        assert_eq!(
            count, 1,
            "Must skip symlinks resolving outside the library root folder"
        );

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn test_deeply_nested_directory_tree() {
        let temp_dir = std::env::temp_dir().join("beam_library_depth_test");
        let _ = fs::remove_dir_all(&temp_dir);

        let mut current = temp_dir.clone();
        for i in 0..40 {
            current = current.join(format!("level_{i}"));
        }
        fs::create_dir_all(&current).unwrap();

        // Place an image at depth 10
        let mut depth_10 = temp_dir.clone();
        for i in 0..10 {
            depth_10 = depth_10.join(format!("level_{i}"));
        }
        let img_path = depth_10.join("photo.png");
        let img: ImageBuffer<Rgba<u8>, Vec<u8>> =
            ImageBuffer::from_pixel(10, 10, Rgba([5, 5, 5, 255]));
        img.save(&img_path).unwrap();

        let db = LibraryDb::open_in_memory().unwrap();
        let thumb_dir = temp_dir.join("thumbs");
        let count = db
            .scan_and_import_folder(&temp_dir, &thumb_dir, true)
            .unwrap();
        assert_eq!(count, 1, "Must scan deep tree without stack overflow");

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn test_photo_path_update_delete_and_relink() {
        let temp_dir = std::env::temp_dir().join("beam_library_relink_test");
        let _ = fs::remove_dir_all(&temp_dir);
        let old_folder = temp_dir.join("old_desktop_folder");
        let new_folder = temp_dir.join("new_desktop_folder");
        fs::create_dir_all(&old_folder).unwrap();
        fs::create_dir_all(&new_folder).unwrap();

        let img_path1 = old_folder.join("image_a.png");
        let img_path2 = old_folder.join("image_b.png");
        let thumb_dir = temp_dir.join("thumbs");

        let img: ImageBuffer<Rgba<u8>, Vec<u8>> =
            ImageBuffer::from_pixel(10, 10, Rgba([10, 20, 30, 255]));
        img.save(&img_path1).unwrap();
        img.save(&img_path2).unwrap();

        let db = LibraryDb::open_in_memory().unwrap();
        let p1 = db.import_file(&img_path1, &thumb_dir).unwrap();
        let p2 = db.import_file(&img_path2, &thumb_dir).unwrap();

        assert_eq!(db.query_photos(&LibraryFilter::default()).unwrap().len(), 2);

        // 1. Move image_a to new location and test update_photo_path
        let relocated_a = new_folder.join("image_a_renamed.png");
        fs::rename(&img_path1, &relocated_a).unwrap();
        db.update_photo_path(p1.id, &relocated_a).unwrap();

        let updated_p1 = db.get_photo(p1.id).unwrap().unwrap();
        assert_eq!(updated_p1.file_path, relocated_a.to_string_lossy());
        assert_eq!(updated_p1.file_name, "image_a_renamed.png");

        // 2. Move image_b to new_folder/image_b.png and test relink_folder
        let moved_b = new_folder.join("image_b.png");
        fs::rename(&img_path2, &moved_b).unwrap();
        let remapped_count = db.relink_folder(&old_folder, &new_folder).unwrap();
        assert_eq!(remapped_count, 1);

        let updated_p2 = db.get_photo(p2.id).unwrap().unwrap();
        assert_eq!(updated_p2.file_path, moved_b.to_string_lossy());
        assert!(Path::new(&updated_p2.file_path).exists());

        // 3. Test remove_missing_photos
        // Delete image_b from disk so it's missing
        fs::remove_file(&moved_b).unwrap();
        let missing_removed = db.remove_missing_photos().unwrap();
        assert_eq!(missing_removed, 1);
        assert_eq!(db.query_photos(&LibraryFilter::default()).unwrap().len(), 1);

        // 4. Test delete_photo
        db.delete_photo(p1.id).unwrap();
        assert_eq!(db.query_photos(&LibraryFilter::default()).unwrap().len(), 0);

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn test_list_subdirectories_and_folder_filter() {
        let temp_dir = std::env::temp_dir().join("beam_library_folder_browse_test");
        let _ = fs::remove_dir_all(&temp_dir);
        let root = temp_dir.join("photos_root");
        let sub_a = root.join("vacation");
        let sub_b = root.join("portraits");
        let hidden = root.join(".hidden_folder");
        fs::create_dir_all(&sub_a).unwrap();
        fs::create_dir_all(&sub_b).unwrap();
        fs::create_dir_all(&hidden).unwrap();

        let subdirs = list_subdirectories(&root);
        assert_eq!(subdirs.len(), 2);
        assert_eq!(subdirs[0].file_name().unwrap(), "portraits");
        assert_eq!(subdirs[1].file_name().unwrap(), "vacation");

        let img: ImageBuffer<Rgba<u8>, Vec<u8>> =
            ImageBuffer::from_pixel(10, 10, Rgba([10, 20, 30, 255]));
        let file_root = root.join("root_photo.png");
        let file_sub = sub_a.join("beach.png");
        img.save(&file_root).unwrap();
        img.save(&file_sub).unwrap();

        let thumb_dir = temp_dir.join("thumbs");
        let db = LibraryDb::open_in_memory().unwrap();
        db.import_file(&file_root, &thumb_dir).unwrap();
        db.import_file(&file_sub, &thumb_dir).unwrap();

        // 1. Filter non-recursive root
        let filter_non_rec = LibraryFilter {
            folder_path: Some(root.clone()),
            recursive: false,
            ..Default::default()
        };
        let res_non_rec = db.query_photos(&filter_non_rec).unwrap();
        assert_eq!(res_non_rec.len(), 1);
        assert_eq!(res_non_rec[0].file_name, "root_photo.png");

        // 2. Filter recursive root
        let filter_rec = LibraryFilter {
            folder_path: Some(root.clone()),
            recursive: true,
            ..Default::default()
        };
        let res_rec = db.query_photos(&filter_rec).unwrap();
        assert_eq!(res_rec.len(), 2);

        // 3. Filter subfolder
        let filter_sub = LibraryFilter {
            folder_path: Some(sub_a.clone()),
            recursive: false,
            ..Default::default()
        };
        let res_sub = db.query_photos(&filter_sub).unwrap();
        assert_eq!(res_sub.len(), 1);
        assert_eq!(res_sub[0].file_name, "beach.png");

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn test_photo_tags_and_filtering() {
        let temp_dir = std::env::temp_dir().join(format!("beam_tag_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        let img: ImageBuffer<Rgba<u8>, Vec<u8>> =
            ImageBuffer::from_pixel(10, 10, Rgba([50, 100, 150, 255]));
        let f1 = temp_dir.join("sunset.png");
        let f2 = temp_dir.join("portrait.png");
        img.save(&f1).unwrap();
        img.save(&f2).unwrap();

        let thumb_dir = temp_dir.join("thumbs");
        let db = LibraryDb::open_in_memory().unwrap();
        let id1 = db.import_file(&f1, &thumb_dir).unwrap().id;
        let id2 = db.import_file(&f2, &thumb_dir).unwrap().id;

        db.add_tag(id1, "landscape").unwrap();
        db.add_tag(id1, "golden-hour").unwrap();
        db.add_tag(id1, "  landscape  ").unwrap(); // duplicate should be ignored
        db.add_tag(id2, "portrait").unwrap();

        let tags1 = db.get_photo_tags(id1).unwrap();
        assert_eq!(tags1, vec!["golden-hour", "landscape"]);

        let all_tags = db.list_all_tags().unwrap();
        assert_eq!(
            all_tags,
            vec![
                ("golden-hour".to_string(), 1),
                ("landscape".to_string(), 1),
                ("portrait".to_string(), 1),
            ]
        );

        let filter = LibraryFilter {
            tag: Some("landscape".to_string()),
            ..Default::default()
        };
        let matched = db.query_photos(&filter).unwrap();
        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0].id, id1);

        db.remove_tag(id1, "landscape").unwrap();
        assert_eq!(db.get_photo_tags(id1).unwrap(), vec!["golden-hour"]);
        let matched_after = db.query_photos(&filter).unwrap();
        assert!(matched_after.is_empty());

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn test_smart_collections_metadata_and_duplicates() {
        let temp_dir =
            std::env::temp_dir().join(format!("beam_phase3_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        let img: ImageBuffer<Rgba<u8>, Vec<u8>> =
            ImageBuffer::from_pixel(12, 12, Rgba([80, 120, 160, 255]));
        let f1 = temp_dir.join("orig.png");
        let f2 = temp_dir.join("copy.png");
        img.save(&f1).unwrap();
        fs::copy(&f1, &f2).unwrap();

        let thumb_dir = temp_dir.join("thumbs");
        let db = LibraryDb::open_in_memory().unwrap();
        let id1 = db.import_file(&f1, &thumb_dir).unwrap().id;
        let _id2 = db.import_file(&f2, &thumb_dir).unwrap().id;

        // 1. Smart Collections
        let filter = LibraryFilter {
            min_rating: 4,
            file_type: FileTypeFilter::Png,
            tag: Some("best".to_string()),
            ..Default::default()
        };
        let sc_id = db.save_smart_collection("Top PNGs", &filter).unwrap();
        let cols = db.list_smart_collections().unwrap();
        assert_eq!(cols.len(), 1);
        assert_eq!(cols[0].name, "Top PNGs");
        assert_eq!(cols[0].min_rating, 4);
        assert_eq!(cols[0].file_type, FileTypeFilter::Png);
        assert_eq!(cols[0].tag.as_deref(), Some("best"));
        db.delete_smart_collection(sc_id).unwrap();
        assert!(db.list_smart_collections().unwrap().is_empty());

        // 2. IPTC/XMP Metadata + GPS
        let meta = PhotoMetadata {
            title: "Mountain Dawn".into(),
            caption: "Alpenglow over ridge".into(),
            copyright: "2026 beaM".into(),
            creator: "Photographer".into(),
            gps_lat: Some(46.5775),
            gps_lon: Some(7.9052),
        };
        db.set_photo_metadata(id1, &meta).unwrap();
        assert_eq!(db.get_photo_metadata(id1).unwrap(), meta);

        // 3. Duplicate Detection
        let dups = db.find_duplicates().unwrap();
        assert_eq!(dups.len(), 1);
        assert_eq!(dups[0].len(), 2);

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn test_xmp_sidecar_export_escapes_xml() {
        let temp_dir = std::env::temp_dir().join(format!("beam_xmp_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        let photo = temp_dir.join("sample.dng");
        let meta = PhotoMetadata {
            title: "Dawn & <Ridge>".into(),
            caption: "Line \"1\" '2'".into(),
            copyright: "2026 beaM".into(),
            creator: "M. Author".into(),
            gps_lat: None,
            gps_lon: None,
        };
        let tags = vec!["landscape".to_string(), "raw<test>".to_string()];
        let xmp_path = write_xmp_sidecar(&photo, &meta, 5, ColorLabel::Green, &tags).unwrap();
        assert_eq!(xmp_path, temp_dir.join("sample.xmp"));

        let content = fs::read_to_string(&xmp_path).unwrap();
        assert!(content.contains("xmp:Rating=\"5\""));
        assert!(content.contains("xmp:Label=\"Green\""));
        assert!(content.contains("Dawn &amp; &lt;Ridge&gt;"));
        assert!(content.contains("Line &quot;1&quot; &apos;2&apos;"));
        assert!(content.contains("<rdf:li>raw&lt;test&gt;</rdf:li>"));

        let _ = fs::remove_dir_all(temp_dir);
    }
}
