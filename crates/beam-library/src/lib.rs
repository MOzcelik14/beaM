//! SQLite library catalog and photo management for beaM.

use beam_core::error::{BeamError, Result};
use beam_core::sidecar::{ColorLabel, Sidecar};
use beam_media::{
    decode_image, generate_thumbnail_with_edits, save_thumbnail, thumbnail_cache_path,
};
use rusqlite::{params, Connection, OpenFlags};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortOrder {
    #[default]
    DateTakenDesc,
    DateTakenAsc,
    RatingDesc,
    FileNameAsc,
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
}

#[derive(Debug, Clone, Default)]
pub struct LibraryFilter {
    pub min_rating: u8,
    pub color_label: Option<ColorLabel>,
    pub file_type: FileTypeFilter,
    pub search_query: Option<String>,
    pub sort_by: SortOrder,
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
                width INTEGER DEFAULT 0,
                height INTEGER DEFAULT 0,
                rating INTEGER DEFAULT 0,
                color_label TEXT DEFAULT 'none',
                thumbnail_path TEXT,
                sidecar_path TEXT
            );

            CREATE INDEX IF NOT EXISTS idx_photos_path ON photos(file_path);
            CREATE INDEX IF NOT EXISTS idx_photos_rating ON photos(rating);
            CREATE INDEX IF NOT EXISTS idx_photos_color ON photos(color_label);
            "#,
            )
            .map_err(|e| BeamError::Database(e.to_string()))?;

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
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        let metadata = fs::metadata(path)?;
        let file_size = metadata.len();

        let modified_at = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs().to_string());

        // Check if sidecar already exists on disk
        let sidecar_p = Sidecar::path_for(path);
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

        // Generate thumbnail
        let thumb_path = thumbnail_cache_path(path, thumb_dir);
        let thumb_path_str = if thumb_path.exists() {
            Some(thumb_path.to_string_lossy().to_string())
        } else {
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
        };

        // Try extracting camera/dimension metadata
        let mut width = 0u32;
        let mut height = 0u32;
        let mut camera_make = None;
        let mut camera_model = None;
        let mut created_at = None;

        if let Ok(decoded) = decode_image(path) {
            width = decoded.width;
            height = decoded.height;
            camera_make = decoded.metadata.camera_make;
            camera_model = decoded.metadata.camera_model;
            created_at = modified_at.clone();
        }

        self.conn
            .execute(
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
                thumbnail_path=COALESCE(excluded.thumbnail_path, photos.thumbnail_path),
                sidecar_path=COALESCE(excluded.sidecar_path, photos.sidecar_path)
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
            )
            .map_err(|e| BeamError::Database(e.to_string()))?;

        let id = self.conn.last_insert_rowid();

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
    pub fn scan_and_import_folder(
        &self,
        folder: &Path,
        thumb_dir: &Path,
        recursive: bool,
    ) -> Result<usize> {
        let mut count = 0;
        let mut stack = vec![folder.to_path_buf()];

        while let Some(dir) = stack.pop() {
            let entries = match fs::read_dir(&dir) {
                Ok(e) => e,
                Err(err) => {
                    log::warn!("Could not read directory {}: {err}", dir.display());
                    continue;
                }
            };

            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() && recursive {
                    stack.push(path);
                } else if path.is_file() && is_supported_image(&path) {
                    if let Ok(_photo) = self.import_file(&path, thumb_dir) {
                        count += 1;
                    }
                }
            }
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

        if let Some(query) = &filter.search_query {
            let q = query.trim();
            if !q.is_empty() {
                sql.push_str(" AND (file_name LIKE ? OR camera_model LIKE ?)");
                let pattern = format!("%{q}%");
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

    /// Update star rating (0-5) for a photo and synchronize to its sidecar.
    pub fn update_rating(&self, photo_id: i64, rating: u8) -> Result<()> {
        let clamped = rating.min(5);
        self.conn
            .execute(
                "UPDATE photos SET rating = ?1 WHERE id = ?2",
                params![clamped as i64, photo_id],
            )
            .map_err(|e| BeamError::Database(e.to_string()))?;

        if let Ok(Some(photo)) = self.get_photo(photo_id) {
            let img_p = Path::new(&photo.file_path);
            let sidecar_p = Sidecar::path_for(img_p);
            let mut sidecar = if sidecar_p.exists() {
                Sidecar::load_from_file(&sidecar_p)
                    .unwrap_or_else(|_| Sidecar::new(&photo.file_name))
            } else {
                Sidecar::new(&photo.file_name)
            };
            sidecar.set_rating(clamped);
            let _ = sidecar.save_to_file(&sidecar_p);
        }

        Ok(())
    }

    /// Update color label for a photo and synchronize to its sidecar.
    pub fn update_color_label(&self, photo_id: i64, label: ColorLabel) -> Result<()> {
        self.conn
            .execute(
                "UPDATE photos SET color_label = ?1 WHERE id = ?2",
                params![label.as_str(), photo_id],
            )
            .map_err(|e| BeamError::Database(e.to_string()))?;

        if let Ok(Some(photo)) = self.get_photo(photo_id) {
            let img_p = Path::new(&photo.file_path);
            let sidecar_p = Sidecar::path_for(img_p);
            let mut sidecar = if sidecar_p.exists() {
                Sidecar::load_from_file(&sidecar_p)
                    .unwrap_or_else(|_| Sidecar::new(&photo.file_name))
            } else {
                Sidecar::new(&photo.file_name)
            };
            sidecar.set_color_label(label);
            let _ = sidecar.save_to_file(&sidecar_p);
        }

        Ok(())
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
}
