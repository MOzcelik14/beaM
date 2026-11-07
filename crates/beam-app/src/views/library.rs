//! Photo library grid view for beaM.

use beam_core::sidecar::ColorLabel;
use beam_core::t;
use beam_library::{LibraryDb, LibraryFilter, PhotoRecord, SortOrder};
use eframe::egui::{self, Color32, Panel, RichText, Sense, Stroke, Vec2};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub enum ThumbnailStatus {
    Loading,
    Ready(egui::TextureHandle),
    Failed(String),
}

impl std::fmt::Debug for ThumbnailStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Loading => write!(f, "Loading"),
            Self::Ready(_) => write!(f, "Ready(TextureHandle)"),
            Self::Failed(e) => write!(f, "Failed({e})"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ThumbnailRequest {
    pub photo_id: i64,
    pub source_path: PathBuf,
    pub sidecar_path: Option<PathBuf>,
}

pub enum LibraryAction {
    None,
    OpenPhoto(PathBuf),
    SelectFolderDialog,
    ChangeFolder(PathBuf),
    ToggleBookmark(PathBuf),
    ToggleRecursive(bool),
    ExportPhotos(Vec<PathBuf>),
    RebuildThumbnails,
    SyncSettings,
    OpenFileDialog,
    RelocatePhoto(i64, PathBuf),
    RelinkFolder,
    CleanMissingPhotos,
    RemovePhoto(i64),
}

/// System places (Home, Pictures, Desktop, Downloads) for quick navigation
pub fn system_places() -> Vec<(&'static str, PathBuf)> {
    let mut places = Vec::new();
    let home_opt = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()
        .map(PathBuf::from);

    if let Some(home) = home_opt {
        places.push(("browser_home", home.clone()));

        let pictures = home.join("Pictures");
        if pictures.exists() {
            places.push(("browser_pictures", pictures));
        } else {
            let resimler = home.join("Resimler");
            if resimler.exists() {
                places.push(("browser_pictures", resimler));
            }
        }

        let desktop = home.join("Desktop");
        if desktop.exists() {
            places.push(("browser_desktop", desktop));
        } else {
            let masaustu = home.join("Masaüstü");
            if masaustu.exists() {
                places.push(("browser_desktop", masaustu));
            }
        }

        let downloads = home.join("Downloads");
        if downloads.exists() {
            places.push(("browser_downloads", downloads));
        } else {
            let indirilenler = home.join("İndirilenler");
            if indirilenler.exists() {
                places.push(("browser_downloads", indirilenler));
            }
        }
    }
    places
}

/// Helper to truncate a long filename nicely while preserving extension and readability
pub fn truncate_filename(name: &str, max_len: usize) -> String {
    let char_count = name.chars().count();
    if char_count <= max_len {
        return name.to_string();
    }
    if let Some(dot_idx) = name.rfind('.') {
        let ext = &name[dot_idx..];
        let stem = &name[..dot_idx];
        let stem_chars = stem.chars().count();
        let ext_chars = ext.chars().count();
        if ext_chars < max_len.saturating_sub(6) {
            let keep_stem = max_len - ext_chars - 3;
            let half_stem = keep_stem / 2;
            let head: String = stem.chars().take(half_stem + keep_stem % 2).collect();
            let tail: String = stem
                .chars()
                .skip(stem_chars.saturating_sub(half_stem))
                .collect();
            return format!("{head}...{tail}{ext}");
        }
    }
    let half = max_len.saturating_sub(3) / 2;
    let head: String = name.chars().take(half).collect();
    let tail: String = name.chars().skip(char_count.saturating_sub(half)).collect();
    format!("{head}...{tail}")
}

pub struct LibraryViewState {
    pub photos: Vec<PhotoRecord>,
    pub filter: LibraryFilter,
    pub selected_photo_id: Option<i64>,
    pub selected_photo_ids: HashSet<i64>,
    pub thumbnail_status: HashMap<i64, ThumbnailStatus>,
    pub pending_thumbnail_requests: Vec<ThumbnailRequest>,
    pub is_importing: bool,
    pub import_status: Option<String>,
    pub needs_refresh: bool,
    pub logo_texture: Option<egui::TextureHandle>,
    pub missing_photo_ids: HashSet<i64>,
    pub active_folder: Option<PathBuf>,
    pub bookmarked_folders: Vec<PathBuf>,
    pub subdirectories: Vec<PathBuf>,
}

impl Default for LibraryViewState {
    fn default() -> Self {
        Self {
            photos: Vec::new(),
            filter: LibraryFilter::default(),
            selected_photo_id: None,
            selected_photo_ids: HashSet::new(),
            thumbnail_status: HashMap::new(),
            pending_thumbnail_requests: Vec::new(),
            is_importing: false,
            import_status: None,
            needs_refresh: true,
            logo_texture: None,
            missing_photo_ids: HashSet::new(),
            active_folder: None,
            bookmarked_folders: Vec::new(),
            subdirectories: Vec::new(),
        }
    }
}

pub const MAX_IN_MEMORY_THUMBNAILS: usize = 1000;
pub const MAX_PENDING_THUMBNAILS: usize = 200;

impl LibraryViewState {
    /// Inserts a thumbnail status, enforcing the in-memory cache capacity bound.
    pub fn insert_thumbnail(&mut self, photo_id: i64, status: ThumbnailStatus) {
        if self.thumbnail_status.len() >= MAX_IN_MEMORY_THUMBNAILS
            && !self.thumbnail_status.contains_key(&photo_id)
        {
            if let Some(&old_key) = self.thumbnail_status.keys().next() {
                self.thumbnail_status.remove(&old_key);
            }
        }
        self.thumbnail_status.insert(photo_id, status);
    }

    /// Refresh photos list from SQLite catalog matching current filter and identify missing files.
    pub fn refresh(&mut self, db: &LibraryDb) {
        if let Some(folder) = &self.active_folder {
            self.filter.folder_path = Some(folder.clone());
            self.subdirectories = beam_library::list_subdirectories(folder);
        } else {
            self.filter.folder_path = None;
            self.subdirectories.clear();
        }

        if let Ok(photos) = db.query_photos(&self.filter) {
            self.missing_photo_ids = photos
                .iter()
                .filter(|p| !Path::new(&p.file_path).exists())
                .map(|p| p.id)
                .collect();
            self.photos = photos;
            self.needs_refresh = false;
        }
    }
}

/// Helper to load an egui TextureHandle from an existing disk thumbnail.
fn load_texture_from_path(
    ctx: &egui::Context,
    photo_id: i64,
    path: &Path,
) -> Result<egui::TextureHandle, String> {
    let img = image::open(path).map_err(|e| e.to_string())?;
    let rgba = img.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    let pixels = rgba.into_raw();

    let color_image = egui::ColorImage::from_rgba_unmultiplied(size, &pixels);
    let tex = ctx.load_texture(
        format!("thumb_{photo_id}"),
        color_image,
        egui::TextureOptions::LINEAR,
    );
    Ok(tex)
}

/// Helper to draw a modern pill-shaped filter chip.
#[allow(dead_code)]
pub fn draw_chip(ui: &mut egui::Ui, selected: bool, text: RichText) -> egui::Response {
    draw_chip_sized(ui, selected, text, Vec2::new(0.0, 20.0))
}

/// Helper to draw an exact sized filter chip.
pub fn draw_chip_sized(
    ui: &mut egui::Ui,
    selected: bool,
    text: RichText,
    size: Vec2,
) -> egui::Response {
    let (bg, stroke) = if selected {
        (
            Color32::from_rgb(46, 56, 78),
            Stroke::new(1.0, Color32::from_rgb(70, 92, 138)),
        )
    } else {
        (
            Color32::from_rgb(22, 24, 30),
            Stroke::new(1.0, Color32::from_rgb(38, 42, 52)),
        )
    };
    ui.add(
        egui::Button::new(text)
            .fill(bg)
            .stroke(stroke)
            .corner_radius(5.0)
            .min_size(size),
    )
}

/// Render the complete Library View (Sidebar + Responsive Grid).
pub fn show_library_view(
    ui: &mut egui::Ui,
    state: &mut LibraryViewState,
    db: &LibraryDb,
) -> LibraryAction {
    let mut action = LibraryAction::None;
    let ctx = ui.ctx().clone();

    if state.needs_refresh {
        state.refresh(db);
    }

    // Keyboard shortcuts for selected photo rating: keys 0-5
    if let Some(selected_id) = state.selected_photo_id {
        const RATING_KEYS: [(u8, egui::Key); 6] = [
            (0, egui::Key::Num0),
            (1, egui::Key::Num1),
            (2, egui::Key::Num2),
            (3, egui::Key::Num3),
            (4, egui::Key::Num4),
            (5, egui::Key::Num5),
        ];
        for (num, key) in RATING_KEYS {
            if ctx.input(|i| i.key_pressed(key) && !i.modifiers.command && !i.modifiers.ctrl) {
                let _ = db.update_rating(selected_id, num);
                state.needs_refresh = true;
            }
        }
    }

    // Left Sidebar: Filters & Import Controls
    Panel::left("library_sidebar")
        .default_size(270.0)
        .min_size(240.0)
        .resizable(true)
        .show(ui, |ui| {
            ui.vertical(|ui| {
                // Header with Select Folder button
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(t!("browser_title"))
                            .size(14.0)
                            .strong()
                            .color(Color32::from_rgb(240, 242, 248)),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let btn = egui::Button::new(
                            RichText::new(t!("browser_choose_folder"))
                                .size(11.0)
                                .color(Color32::from_rgb(190, 215, 255)),
                        )
                        .fill(Color32::from_rgb(26, 36, 52))
                        .stroke(Stroke::new(1.0, Color32::from_rgb(46, 70, 110)))
                        .corner_radius(4.0);
                        if ui.add(btn).clicked() {
                            action = LibraryAction::SelectFolderDialog;
                        }
                    });
                });
                ui.add_space(4.0);
                ui.separator();
                ui.add_space(4.0);

                // Path navigation bar:
                // [ ↑ .. ] [ ⟳ ] [ ★ / ☆ ]
                if let Some(current_folder) = &state.active_folder {
                    egui::Frame::new()
                        .fill(Color32::from_rgb(18, 19, 25))
                        .stroke(Stroke::new(1.0, Color32::from_rgb(34, 38, 48)))
                        .corner_radius(6.0)
                        .inner_margin(egui::Margin::symmetric(6, 6))
                        .show(ui, |ui| {
                            ui.vertical(|ui| {
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 4.0;
                                    let has_parent = current_folder.parent().is_some();
                                    let up_btn = egui::Button::new(
                                        RichText::new("↑ ..").size(11.0).strong(),
                                    )
                                    .fill(Color32::from_rgb(26, 28, 36))
                                    .stroke(Stroke::new(1.0, Color32::from_rgb(44, 48, 60)))
                                    .corner_radius(4.0)
                                    .min_size(Vec2::new(42.0, 22.0));
                                    if ui
                                        .add_enabled(has_parent, up_btn)
                                        .on_hover_text(t!("browser_parent"))
                                        .clicked()
                                    {
                                        if let Some(parent) = current_folder.parent() {
                                            action =
                                                LibraryAction::ChangeFolder(parent.to_path_buf());
                                        }
                                    }

                                    let refresh_btn =
                                        egui::Button::new(RichText::new("⟳").size(12.0))
                                            .fill(Color32::from_rgb(26, 28, 36))
                                            .stroke(Stroke::new(1.0, Color32::from_rgb(44, 48, 60)))
                                            .corner_radius(4.0)
                                            .min_size(Vec2::new(26.0, 22.0));
                                    if ui
                                        .add(refresh_btn)
                                        .on_hover_text(t!("browser_refresh"))
                                        .clicked()
                                    {
                                        action =
                                            LibraryAction::ChangeFolder(current_folder.clone());
                                    }

                                    let is_bookmarked =
                                        state.bookmarked_folders.contains(current_folder);
                                    let (bm_icon, bm_color) = if is_bookmarked {
                                        ("★", Color32::from_rgb(240, 195, 45))
                                    } else {
                                        ("☆", Color32::from_rgb(160, 165, 175))
                                    };
                                    let bookmark_tip = if is_bookmarked {
                                        t!("browser_bookmark_remove")
                                    } else {
                                        t!("browser_bookmark_add")
                                    };
                                    let bm_btn = egui::Button::new(
                                        RichText::new(bm_icon).size(12.0).color(bm_color),
                                    )
                                    .fill(Color32::from_rgb(26, 28, 36))
                                    .stroke(Stroke::new(1.0, Color32::from_rgb(44, 48, 60)))
                                    .corner_radius(4.0)
                                    .min_size(Vec2::new(26.0, 22.0));
                                    if ui.add(bm_btn).on_hover_text(bookmark_tip).clicked() {
                                        action =
                                            LibraryAction::ToggleBookmark(current_folder.clone());
                                    }
                                });

                                ui.add_space(4.0);
                                let folder_disp = current_folder.to_string_lossy();
                                egui::Frame::new()
                                    .fill(Color32::from_rgb(13, 14, 18))
                                    .stroke(Stroke::new(1.0, Color32::from_rgb(28, 30, 38)))
                                    .corner_radius(4.0)
                                    .inner_margin(egui::Margin::symmetric(6, 4))
                                    .show(ui, |ui| {
                                        ui.add_sized(
                                            [ui.available_width(), 16.0],
                                            egui::Label::new(
                                                RichText::new(truncate_filename(&folder_disp, 30))
                                                    .size(11.0)
                                                    .color(Color32::from_rgb(180, 205, 245)),
                                            ),
                                        )
                                        .on_hover_text(&*folder_disp);
                                    });
                            });
                        });

                    ui.add_space(4.0);
                    // Recursive checkbox: "Alt klasörleri dahil et"
                    let mut rec = state.filter.recursive;
                    if ui.checkbox(&mut rec, t!("browser_recursive")).changed() {
                        action = LibraryAction::ToggleRecursive(rec);
                    }
                } else {
                    let choose_btn = egui::Button::new(
                        RichText::new(t!("browser_choose_folder"))
                            .size(12.0)
                            .strong()
                            .color(Color32::from_rgb(220, 235, 255)),
                    )
                    .fill(Color32::from_rgb(32, 45, 68))
                    .stroke(Stroke::new(1.0, Color32::from_rgb(52, 85, 138)))
                    .corner_radius(6.0)
                    .min_size(Vec2::new(ui.available_width(), 28.0));
                    if ui.add(choose_btn).clicked() {
                        action = LibraryAction::SelectFolderDialog;
                    }
                }

                ui.add_space(6.0);
                ui.separator();
                ui.add_space(4.0);

                // Places & Bookmarks
                ui.label(
                    RichText::new(t!("browser_places").to_uppercase())
                        .size(10.5)
                        .strong()
                        .color(Color32::from_rgb(130, 136, 150)),
                );
                ui.add_space(3.0);
                let places = system_places();
                for (trans_key, path) in places {
                    let is_active = state.active_folder.as_ref() == Some(&path);
                    let (fill, stroke, text_color) = if is_active {
                        (
                            Color32::from_rgb(34, 46, 68),
                            Stroke::new(1.0, Color32::from_rgb(52, 85, 138)),
                            Color32::from_rgb(235, 242, 255),
                        )
                    } else {
                        (
                            Color32::TRANSPARENT,
                            Stroke::NONE,
                            Color32::from_rgb(180, 186, 198),
                        )
                    };
                    let btn = egui::Button::new(
                        RichText::new(format!("›  {}", t!(trans_key)))
                            .size(11.5)
                            .color(text_color),
                    )
                    .fill(fill)
                    .stroke(stroke)
                    .corner_radius(4.0)
                    .min_size(Vec2::new(ui.available_width(), 24.0));
                    if ui.add(btn).clicked() {
                        action = LibraryAction::ChangeFolder(path);
                    }
                }

                if !state.bookmarked_folders.is_empty() {
                    ui.add_space(3.0);
                    let bookmarks = state.bookmarked_folders.clone();
                    for b_path in bookmarks {
                        let is_active = state.active_folder.as_ref() == Some(&b_path);
                        let folder_name = b_path
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("Folder")
                            .to_string();
                        let b_path_lossy = b_path.to_string_lossy().into_owned();
                        let b_path_target = b_path.clone();
                        let (fill, stroke, text_color) = if is_active {
                            (
                                Color32::from_rgb(34, 46, 68),
                                Stroke::new(1.0, Color32::from_rgb(52, 85, 138)),
                                Color32::from_rgb(235, 242, 255),
                            )
                        } else {
                            (
                                Color32::TRANSPARENT,
                                Stroke::NONE,
                                Color32::from_rgb(180, 186, 198),
                            )
                        };
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 2.0;
                            let row_w = (ui.available_width() - 24.0).max(10.0);
                            let btn = egui::Button::new(
                                RichText::new(format!("★  {folder_name}"))
                                    .size(11.5)
                                    .color(text_color),
                            )
                            .fill(fill)
                            .stroke(stroke)
                            .corner_radius(4.0)
                            .min_size(Vec2::new(row_w, 24.0));
                            if ui.add(btn).on_hover_text(&b_path_lossy).clicked() {
                                action = LibraryAction::ChangeFolder(b_path_target.clone());
                            }
                            let rm_btn = egui::Button::new(RichText::new("×").size(11.0))
                                .fill(Color32::from_rgb(24, 26, 34))
                                .stroke(Stroke::new(1.0, Color32::from_rgb(42, 46, 58)))
                                .corner_radius(3.0)
                                .min_size(Vec2::new(20.0, 20.0));
                            if ui
                                .add(rm_btn)
                                .on_hover_text(t!("browser_bookmark_remove"))
                                .clicked()
                            {
                                action = LibraryAction::ToggleBookmark(b_path_target);
                            }
                        });
                    }
                }

                ui.add_space(6.0);
                ui.separator();
                ui.add_space(4.0);

                // Subdirectories (Folders)
                ui.label(
                    RichText::new(t!("browser_folders").to_uppercase())
                        .size(10.5)
                        .strong()
                        .color(Color32::from_rgb(130, 136, 150)),
                );
                ui.add_space(3.0);
                if state.subdirectories.is_empty() {
                    ui.label(RichText::new(t!("browser_no_subfolders")).weak().size(11.0));
                } else {
                    egui::ScrollArea::vertical()
                        .id_salt("subdirectories_scroll")
                        .max_height(140.0)
                        .show(ui, |ui| {
                            let subdirs = state.subdirectories.clone();
                            for sub in subdirs {
                                let name = sub
                                    .file_name()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or("folder")
                                    .to_string();
                                let sub_lossy = sub.to_string_lossy().into_owned();
                                let sub_btn = egui::Button::new(
                                    RichText::new(format!("▸  {name}"))
                                        .size(11.5)
                                        .color(Color32::from_rgb(180, 185, 195)),
                                )
                                .fill(Color32::TRANSPARENT)
                                .stroke(Stroke::NONE)
                                .corner_radius(4.0)
                                .min_size(Vec2::new(ui.available_width(), 22.0));
                                if ui.add(sub_btn).on_hover_text(&sub_lossy).clicked() {
                                    action = LibraryAction::ChangeFolder(sub);
                                }
                            }
                        });
                }

                if !state.missing_photo_ids.is_empty() {
                    let missing_count = state.missing_photo_ids.len();
                    ui.add_space(4.0);
                    egui::Frame::new()
                        .fill(Color32::from_rgb(38, 28, 18))
                        .stroke(Stroke::new(1.0, Color32::from_rgb(190, 115, 35)))
                        .corner_radius(6.0)
                        .inner_margin(egui::Margin::same(8))
                        .show(ui, |ui| {
                            ui.vertical(|ui| {
                                ui.colored_label(
                                    Color32::from_rgb(245, 175, 70),
                                    t!("library_missing_banner", "count" => missing_count),
                                );
                                ui.add_space(6.0);
                                ui.horizontal(|ui| {
                                    if ui
                                        .button(
                                            RichText::new(t!("library_relink_folder"))
                                                .size(11.5)
                                                .strong(),
                                        )
                                        .on_hover_text(t!("library_relink_folder_title"))
                                        .clicked()
                                    {
                                        action = LibraryAction::RelinkFolder;
                                    }
                                    if ui
                                        .button(
                                            RichText::new(t!("library_clean_missing"))
                                                .size(11.5)
                                                .color(Color32::from_rgb(240, 120, 120)),
                                        )
                                        .clicked()
                                    {
                                        action = LibraryAction::CleanMissingPhotos;
                                    }
                                });
                            });
                        });
                    ui.add_space(4.0);
                }

                let selected_count = state.selected_photo_ids.len();
                let export_btn_text = if selected_count > 1 {
                    t!("library_export_selected", "count" => selected_count)
                } else if selected_count == 1 {
                    t!("library_export_selected_one")
                } else {
                    t!("library_export_all", "count" => state.photos.len())
                };

                let can_export = !state.photos.is_empty();
                ui.add_space(6.0);
                let export_btn = egui::Button::new(
                    RichText::new(export_btn_text)
                        .size(12.0)
                        .color(Color32::from_rgb(240, 245, 255))
                        .strong(),
                )
                .fill(Color32::from_rgb(38, 68, 124))
                .stroke(Stroke::new(1.0, Color32::from_rgb(58, 96, 162)))
                .corner_radius(6.0)
                .min_size(Vec2::new(ui.available_width(), 28.0));

                if ui.add_enabled(can_export, export_btn).clicked() {
                    let paths = if selected_count > 0 {
                        state
                            .photos
                            .iter()
                            .filter(|p| state.selected_photo_ids.contains(&p.id))
                            .map(|p| PathBuf::from(&p.file_path))
                            .collect()
                    } else {
                        state
                            .photos
                            .iter()
                            .map(|p| PathBuf::from(&p.file_path))
                            .collect()
                    };
                    action = LibraryAction::ExportPhotos(paths);
                }

                if selected_count > 1 {
                    ui.add_space(4.0);
                    let sync_btn = egui::Button::new(
                        RichText::new(t!("library_sync_settings"))
                            .size(11.5)
                            .color(Color32::from_rgb(215, 220, 235)),
                    )
                    .fill(Color32::from_rgb(28, 32, 42))
                    .stroke(Stroke::new(1.0, Color32::from_rgb(45, 52, 68)))
                    .corner_radius(6.0)
                    .min_size(Vec2::new(ui.available_width(), 26.0));

                    if ui
                        .add(sync_btn)
                        .on_hover_text(t!("sync_dialog_title"))
                        .clicked()
                    {
                        action = LibraryAction::SyncSettings;
                    }
                }

                ui.add_space(4.0);
                let rebuild_btn = egui::Button::new(
                    RichText::new(t!("library_rebuild_thumbnails"))
                        .size(11.5)
                        .color(Color32::from_rgb(175, 180, 195)),
                )
                .fill(Color32::from_rgb(22, 24, 30))
                .stroke(Stroke::new(1.0, Color32::from_rgb(38, 42, 52)))
                .corner_radius(6.0)
                .min_size(Vec2::new(ui.available_width(), 26.0));

                if ui
                    .add(rebuild_btn)
                    .on_hover_text(t!("library_rebuild_tooltip"))
                    .clicked()
                {
                    action = LibraryAction::RebuildThumbnails;
                }

                if state.is_importing {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.spinner();
                        if let Some(status) = &state.import_status {
                            ui.label(RichText::new(status).weak().size(11.0));
                        } else {
                            ui.label(t!("library_importing"));
                        }
                    });
                }

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(4.0);
                ui.label(
                    RichText::new(t!("library_filters").to_uppercase())
                        .size(10.5)
                        .strong()
                        .color(Color32::from_rgb(130, 136, 150)),
                );
                ui.add_space(4.0);

                // Star rating filter buttons: 6 columns spanning exactly 100% width
                ui.label(
                    RichText::new(t!("library_rating"))
                        .size(11.0)
                        .color(Color32::from_rgb(140, 145, 160)),
                );
                ui.add_space(2.0);
                let star_w = ((ui.available_width() - 15.0) / 6.0).floor().max(16.0);
                let star_size = Vec2::new(star_w, 22.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 3.0;
                    for r in 0..=5u8 {
                        let label = if r == 0 {
                            t!("library_rating_any")
                        } else {
                            format!("★{r}")
                        };
                        let is_selected = state.filter.min_rating == r;
                        let text = RichText::new(label).size(10.5).color(if is_selected {
                            Color32::from_rgb(240, 245, 255)
                        } else {
                            Color32::from_rgb(170, 175, 185)
                        });
                        let tooltip = if r == 0 {
                            t!("library_rating_any")
                        } else {
                            format!("★{r}+")
                        };
                        if draw_chip_sized(ui, is_selected, text, star_size)
                            .on_hover_text(tooltip)
                            .clicked()
                        {
                            state.filter.min_rating = r;
                            state.needs_refresh = true;
                        }
                    }
                });

                ui.add_space(6.0);

                // Color label filter: 6 columns spanning exactly 100% width
                ui.label(
                    RichText::new(t!("library_color_label"))
                        .size(11.0)
                        .color(Color32::from_rgb(140, 145, 160)),
                );
                ui.add_space(2.0);
                let labels: [(Option<ColorLabel>, String, Color32); 6] = [
                    (None, t!("library_rating_any"), Color32::WHITE),
                    (
                        Some(ColorLabel::Red),
                        t!(ColorLabel::Red.translation_key()),
                        Color32::from_rgb(235, 60, 60),
                    ),
                    (
                        Some(ColorLabel::Yellow),
                        t!(ColorLabel::Yellow.translation_key()),
                        Color32::from_rgb(240, 215, 50),
                    ),
                    (
                        Some(ColorLabel::Green),
                        t!(ColorLabel::Green.translation_key()),
                        Color32::from_rgb(60, 200, 80),
                    ),
                    (
                        Some(ColorLabel::Blue),
                        t!(ColorLabel::Blue.translation_key()),
                        Color32::from_rgb(65, 130, 245),
                    ),
                    (
                        Some(ColorLabel::Purple),
                        t!(ColorLabel::Purple.translation_key()),
                        Color32::from_rgb(160, 80, 240),
                    ),
                ];

                let color_w = ((ui.available_width() - 15.0) / 6.0).floor().max(16.0);
                let color_size = Vec2::new(color_w, 22.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 3.0;
                    for (col, name, color) in labels {
                        let is_selected = state.filter.color_label == col;
                        let text = if col.is_some() {
                            RichText::new("●").size(12.0).color(color)
                        } else {
                            RichText::new(name.clone())
                                .size(10.5)
                                .color(if is_selected {
                                    Color32::from_rgb(240, 245, 255)
                                } else {
                                    Color32::from_rgb(170, 175, 185)
                                })
                        };
                        if draw_chip_sized(ui, is_selected, text, color_size)
                            .on_hover_text(name)
                            .clicked()
                        {
                            state.filter.color_label = col;
                            state.needs_refresh = true;
                        }
                    }
                });

                ui.add_space(6.0);

                // File format filter: Strict 3x2 grid
                ui.label(
                    RichText::new(t!("library_format"))
                        .size(11.0)
                        .color(Color32::from_rgb(140, 145, 160)),
                );
                ui.add_space(2.0);
                let fmt_w = ((ui.available_width() - 8.0) / 3.0).floor().max(20.0);
                let fmt_size = Vec2::new(fmt_w, 22.0);

                let formats_row1 = [
                    beam_library::FileTypeFilter::All,
                    beam_library::FileTypeFilter::Raw,
                    beam_library::FileTypeFilter::Jpeg,
                ];
                let formats_row2 = [
                    beam_library::FileTypeFilter::Png,
                    beam_library::FileTypeFilter::Tiff,
                    beam_library::FileTypeFilter::WebP,
                ];

                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    for ft in formats_row1 {
                        let is_selected = state.filter.file_type == ft;
                        let text = RichText::new(t!(ft.translation_key())).size(11.0).color(
                            if is_selected {
                                Color32::from_rgb(240, 245, 255)
                            } else {
                                Color32::from_rgb(170, 175, 185)
                            },
                        );
                        if draw_chip_sized(ui, is_selected, text, fmt_size).clicked() {
                            state.filter.file_type = ft;
                            state.needs_refresh = true;
                        }
                    }
                });
                ui.add_space(3.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    for ft in formats_row2 {
                        let is_selected = state.filter.file_type == ft;
                        let text = RichText::new(t!(ft.translation_key())).size(11.0).color(
                            if is_selected {
                                Color32::from_rgb(240, 245, 255)
                            } else {
                                Color32::from_rgb(170, 175, 185)
                            },
                        );
                        if draw_chip_sized(ui, is_selected, text, fmt_size).clicked() {
                            state.filter.file_type = ft;
                            state.needs_refresh = true;
                        }
                    }
                });

                ui.add_space(8.0);

                // Search box
                ui.label(
                    RichText::new(t!("library_search"))
                        .size(11.0)
                        .color(Color32::from_rgb(140, 145, 160)),
                );
                ui.add_space(2.0);
                let mut search_text = state.filter.search_query.clone().unwrap_or_default();
                let resp = ui.add_sized(
                    [ui.available_width(), 24.0],
                    egui::TextEdit::singleline(&mut search_text).hint_text(t!("library_search")),
                );
                if resp.changed() {
                    state.filter.search_query = if search_text.trim().is_empty() {
                        None
                    } else {
                        Some(search_text)
                    };
                    state.needs_refresh = true;
                }

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(4.0);

                // Sort order selector
                ui.label(
                    RichText::new(t!("library_sort_by"))
                        .size(11.0)
                        .color(Color32::from_rgb(140, 145, 160)),
                );
                ui.add_space(2.0);
                egui::ComboBox::from_id_salt("library_sort_order")
                    .width(ui.available_width())
                    .selected_text(t!(state.filter.sort_by.translation_key()))
                    .show_ui(ui, |ui| {
                        let mut changed = false;
                        changed |= ui
                            .selectable_value(
                                &mut state.filter.sort_by,
                                SortOrder::DateTakenDesc,
                                t!(SortOrder::DateTakenDesc.translation_key()),
                            )
                            .changed();
                        changed |= ui
                            .selectable_value(
                                &mut state.filter.sort_by,
                                SortOrder::DateTakenAsc,
                                t!(SortOrder::DateTakenAsc.translation_key()),
                            )
                            .changed();
                        changed |= ui
                            .selectable_value(
                                &mut state.filter.sort_by,
                                SortOrder::RatingDesc,
                                t!(SortOrder::RatingDesc.translation_key()),
                            )
                            .changed();
                        changed |= ui
                            .selectable_value(
                                &mut state.filter.sort_by,
                                SortOrder::FileNameAsc,
                                t!(SortOrder::FileNameAsc.translation_key()),
                            )
                            .changed();
                        if changed {
                            state.needs_refresh = true;
                        }
                    });

                ui.add_space(16.0);
                ui.label(
                    RichText::new(t!("library_showing_count", "count" => state.photos.len()))
                        .weak()
                        .size(11.0),
                );
            });
        });

    // Central Area: Multi-Column Thumbnail Grid
    egui::CentralPanel::default().show(ui, |ui| {
        if state.photos.is_empty() {
            ui.centered_and_justified(|ui| {
                egui::Frame::window(ui.style())
                    .fill(Color32::from_rgb(22, 23, 27))
                    .stroke(Stroke::new(1.0, Color32::from_rgb(45, 48, 56)))
                    .corner_radius(12.0)
                    .inner_margin(egui::Margin::same(36))
                    .show(ui, |ui| {
                        ui.set_max_width(520.0);
                        ui.vertical_centered(|ui| {
                            if let Some(logo) = &state.logo_texture {
                                ui.image(egui::load::SizedTexture::new(
                                    logo.id(),
                                    Vec2::new(100.0, 100.0),
                                ));
                                ui.add_space(16.0);
                            }
                            if let Some(ref folder) = state.active_folder {
                                ui.label(
                                    RichText::new(t!("browser_no_photos"))
                                        .size(18.0)
                                        .strong()
                                        .color(Color32::from_rgb(240, 240, 245)),
                                );
                                ui.add_space(6.0);
                                ui.label(
                                    RichText::new(folder.to_string_lossy())
                                        .size(12.0)
                                        .weak()
                                        .color(Color32::from_rgb(160, 165, 175)),
                                );
                                ui.add_space(16.0);
                                ui.label(
                                    RichText::new(t!("browser_empty_tip"))
                                        .size(12.0)
                                        .weak()
                                        .color(Color32::from_rgb(130, 135, 145)),
                                );
                                ui.add_space(20.0);
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 12.0;
                                    if !state.filter.recursive
                                        && ui
                                            .add_sized(
                                                [180.0, 36.0],
                                                egui::Button::new(
                                                    RichText::new(t!("browser_recursive"))
                                                        .size(13.0)
                                                        .strong(),
                                                ),
                                            )
                                            .clicked()
                                    {
                                        action = LibraryAction::ToggleRecursive(true);
                                    }
                                    if ui
                                        .add_sized(
                                            [180.0, 36.0],
                                            egui::Button::new(
                                                RichText::new(t!("browser_choose_folder"))
                                                    .size(13.0),
                                            ),
                                        )
                                        .clicked()
                                    {
                                        action = LibraryAction::SelectFolderDialog;
                                    }
                                });
                            } else {
                                ui.label(
                                    RichText::new(t!("welcome_title"))
                                        .size(24.0)
                                        .strong()
                                        .color(Color32::from_rgb(240, 240, 245)),
                                );
                                ui.add_space(4.0);
                                ui.label(
                                    RichText::new(t!("welcome_subtitle"))
                                        .size(13.0)
                                        .color(Color32::from_rgb(160, 165, 175)),
                                );
                                ui.add_space(16.0);
                                ui.label(
                                    RichText::new(t!("welcome_drag_drop"))
                                        .size(12.0)
                                        .weak()
                                        .color(Color32::from_rgb(130, 135, 145)),
                                );
                                ui.add_space(20.0);
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 12.0;
                                    if ui
                                        .add_sized(
                                            [180.0, 36.0],
                                            egui::Button::new(
                                                RichText::new(t!("welcome_open_file"))
                                                    .size(13.0)
                                                    .strong(),
                                            ),
                                        )
                                        .clicked()
                                    {
                                        action = LibraryAction::OpenFileDialog;
                                    }
                                    if ui
                                        .add_sized(
                                            [180.0, 36.0],
                                            egui::Button::new(
                                                RichText::new(t!("welcome_import_folder"))
                                                    .size(13.0),
                                            ),
                                        )
                                        .clicked()
                                    {
                                        action = LibraryAction::SelectFolderDialog;
                                    }
                                });
                            }
                            ui.add_space(24.0);
                            ui.separator();
                            ui.add_space(8.0);
                            ui.label(
                                RichText::new(t!("welcome_tip"))
                                    .size(11.0)
                                    .weak()
                                    .italics()
                                    .color(Color32::from_rgb(120, 125, 135)),
                            );
                        });
                    });
            });
            return;
        }

        egui::ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                let card_width = 210.0f32;
                let card_height = 228.0f32;
                let spacing = 14.0f32;

                let avail_width = ui.available_width();
                let columns = ((avail_width / (card_width + spacing)).floor() as usize).max(1);

                egui::Grid::new("photos_thumbnail_grid")
                    .spacing([spacing, spacing])
                    .show(ui, |ui| {
                        let mut col = 0;
                        let mut photo_to_open = None;
                        let mut rating_change = None;
                        let mut color_change = None;
                        let mut loaded_this_frame = 0;

                        let photos_clone = state.photos.clone();
                        for photo in &photos_clone {
                            let is_missing = state.missing_photo_ids.contains(&photo.id);
                            let is_selected = state.selected_photo_id == Some(photo.id)
                                || state.selected_photo_ids.contains(&photo.id);

                            ui.vertical(|ui| {
                                let (rect, mut response) = ui.allocate_exact_size(
                                    Vec2::new(card_width, card_height),
                                    Sense::click(),
                                );
                                let is_visible = ui.is_rect_visible(rect);

                                if is_missing {
                                    response = response.on_hover_text(t!(
                                        "library_missing_tooltip",
                                        "path" => &photo.file_path
                                    ));
                                } else {
                                    response = response.on_hover_text(&photo.file_path);
                                }

                                if response.clicked() {
                                    let ctrl =
                                        ctx.input(|i| i.modifiers.command || i.modifiers.ctrl);
                                    if ctrl {
                                        if state.selected_photo_ids.contains(&photo.id) {
                                            state.selected_photo_ids.remove(&photo.id);
                                            if state.selected_photo_id == Some(photo.id) {
                                                state.selected_photo_id =
                                                    state.selected_photo_ids.iter().next().copied();
                                            }
                                        } else {
                                            state.selected_photo_ids.insert(photo.id);
                                            state.selected_photo_id = Some(photo.id);
                                        }
                                    } else {
                                        state.selected_photo_ids.clear();
                                        state.selected_photo_ids.insert(photo.id);
                                        state.selected_photo_id = Some(photo.id);
                                    }
                                }
                                if response.double_clicked() {
                                    if is_missing {
                                        action = LibraryAction::RelocatePhoto(
                                            photo.id,
                                            PathBuf::from(&photo.file_path),
                                        );
                                    } else {
                                        photo_to_open = Some(PathBuf::from(&photo.file_path));
                                    }
                                }

                                response.context_menu(|ui| {
                                    if is_missing {
                                        if ui.button(t!("library_relink_file")).clicked() {
                                            action = LibraryAction::RelocatePhoto(
                                                photo.id,
                                                PathBuf::from(&photo.file_path),
                                            );
                                            ui.close();
                                        }
                                    } else {
                                        if ui.button(t!("library_context_open")).clicked() {
                                            photo_to_open = Some(PathBuf::from(&photo.file_path));
                                            ui.close();
                                        }
                                    }
                                    if ui.button(t!("library_context_export")).clicked() {
                                        action = LibraryAction::ExportPhotos(vec![PathBuf::from(
                                            &photo.file_path,
                                        )]);
                                        ui.close();
                                    }
                                    ui.separator();
                                    if ui.button(t!("library_remove_from_catalog")).clicked() {
                                        action = LibraryAction::RemovePhoto(photo.id);
                                        ui.close();
                                    }
                                });

                                // Card background and border
                                let bg_col = if is_selected {
                                    Color32::from_rgb(26, 36, 54)
                                } else if response.hovered() {
                                    Color32::from_rgb(28, 31, 39)
                                } else {
                                    Color32::from_rgb(20, 22, 28)
                                };

                                let stroke_col = if is_selected {
                                    Color32::from_rgb(59, 130, 246)
                                } else if is_missing {
                                    Color32::from_rgb(140, 70, 40)
                                } else if response.hovered() {
                                    Color32::from_rgb(55, 60, 75)
                                } else {
                                    Color32::from_rgb(34, 37, 46)
                                };

                                ui.painter().rect_filled(rect, 7.0, bg_col);
                                ui.painter().rect_stroke(
                                    rect,
                                    7.0,
                                    Stroke::new(if is_selected { 2.0 } else { 1.0 }, stroke_col),
                                    egui::StrokeKind::Inside,
                                );

                                // Render thumbnail image in top portion
                                let thumb_rect = egui::Rect::from_min_size(
                                    rect.min + Vec2::new(7.0, 7.0),
                                    Vec2::new(card_width - 14.0, 142.0),
                                );

                                if is_visible && !state.thumbnail_status.contains_key(&photo.id) {
                                    let disk_file_valid = photo
                                        .thumbnail_path
                                        .as_ref()
                                        .map(|p| Path::new(p).exists())
                                        .unwrap_or(false);

                                    if disk_file_valid {
                                        if loaded_this_frame < 8 {
                                            if let Some(thumb_p) = photo.thumbnail_path.as_deref() {
                                                match load_texture_from_path(
                                                    &ctx,
                                                    photo.id,
                                                    Path::new(thumb_p),
                                                ) {
                                                    Ok(tex) => {
                                                        state.insert_thumbnail(
                                                            photo.id,
                                                            ThumbnailStatus::Ready(tex),
                                                        );
                                                        loaded_this_frame += 1;
                                                        ctx.request_repaint();
                                                    }
                                                    Err(_) => {
                                                        state.insert_thumbnail(
                                                            photo.id,
                                                            ThumbnailStatus::Loading,
                                                        );
                                                        if state.pending_thumbnail_requests.len()
                                                            < MAX_PENDING_THUMBNAILS
                                                        {
                                                            state.pending_thumbnail_requests.push(
                                                                ThumbnailRequest {
                                                                    photo_id: photo.id,
                                                                    source_path: PathBuf::from(
                                                                        &photo.file_path,
                                                                    ),
                                                                    sidecar_path: photo
                                                                        .sidecar_path
                                                                        .as_ref()
                                                                        .map(PathBuf::from),
                                                                },
                                                            );
                                                        }
                                                    }
                                                }
                                            }
                                        } else {
                                            ctx.request_repaint();
                                        }
                                    } else {
                                        state.insert_thumbnail(photo.id, ThumbnailStatus::Loading);
                                        if state.pending_thumbnail_requests.len()
                                            < MAX_PENDING_THUMBNAILS
                                        {
                                            state.pending_thumbnail_requests.push(
                                                ThumbnailRequest {
                                                    photo_id: photo.id,
                                                    source_path: PathBuf::from(&photo.file_path),
                                                    sidecar_path: photo
                                                        .sidecar_path
                                                        .as_ref()
                                                        .map(PathBuf::from),
                                                },
                                            );
                                        }
                                    }
                                }

                                match state.thumbnail_status.get(&photo.id) {
                                    Some(ThumbnailStatus::Ready(tex)) => {
                                        let img_aspect =
                                            tex.size_vec2().x / tex.size_vec2().y.max(1.0);
                                        let box_aspect = thumb_rect.width() / thumb_rect.height();
                                        let fitted_size = if img_aspect > box_aspect {
                                            Vec2::new(
                                                thumb_rect.width(),
                                                thumb_rect.width() / img_aspect,
                                            )
                                        } else {
                                            Vec2::new(
                                                thumb_rect.height() * img_aspect,
                                                thumb_rect.height(),
                                            )
                                        };
                                        let fitted_rect = egui::Rect::from_center_size(
                                            thumb_rect.center(),
                                            fitted_size,
                                        );
                                        ui.painter().rect_filled(
                                            thumb_rect,
                                            4.0,
                                            Color32::from_rgb(15, 15, 18),
                                        );
                                        ui.painter().image(
                                            tex.id(),
                                            fitted_rect,
                                            egui::Rect::from_min_max(
                                                egui::pos2(0.0, 0.0),
                                                egui::pos2(1.0, 1.0),
                                            ),
                                            Color32::WHITE,
                                        );
                                    }
                                    Some(ThumbnailStatus::Loading) => {
                                        ui.painter().rect_filled(
                                            thumb_rect,
                                            4.0,
                                            Color32::from_rgb(20, 20, 24),
                                        );
                                        let mut child_ui = ui.new_child(
                                            egui::UiBuilder::new().max_rect(thumb_rect).layout(
                                                egui::Layout::centered_and_justified(
                                                    egui::Direction::TopDown,
                                                ),
                                            ),
                                        );
                                        child_ui.spinner();
                                    }
                                    Some(ThumbnailStatus::Failed(err)) => {
                                        ui.painter().rect_filled(
                                            thumb_rect,
                                            4.0,
                                            Color32::from_rgb(32, 18, 18),
                                        );
                                        let mut child_ui = ui.new_child(
                                            egui::UiBuilder::new().max_rect(thumb_rect).layout(
                                                egui::Layout::centered_and_justified(
                                                    egui::Direction::TopDown,
                                                ),
                                            ),
                                        );
                                        let err_label = child_ui.add(
                                            egui::Label::new(
                                                RichText::new(t!("library_thumb_failed"))
                                                    .color(Color32::from_rgb(240, 100, 100))
                                                    .size(12.0),
                                            )
                                            .sense(Sense::hover()),
                                        );
                                        err_label.on_hover_text(
                                            t!("library_thumb_failed_tooltip", "err" => err),
                                        );
                                    }
                                    None => {
                                        ui.painter().rect_filled(
                                            thumb_rect,
                                            4.0,
                                            Color32::from_rgb(18, 18, 22),
                                        );
                                    }
                                }

                                if is_missing {
                                    let badge_rect = egui::Rect::from_min_size(
                                        thumb_rect.min + Vec2::new(6.0, 6.0),
                                        Vec2::new(56.0, 18.0),
                                    );
                                    ui.painter().rect_filled(
                                        badge_rect,
                                        3.0,
                                        Color32::from_rgba_unmultiplied(190, 85, 25, 230),
                                    );
                                    ui.painter().text(
                                        badge_rect.center(),
                                        egui::Align2::CENTER_CENTER,
                                        t!("library_missing_badge"),
                                        egui::FontId::proportional(10.0),
                                        Color32::WHITE,
                                    );
                                }

                                // Format badge in top-right corner of thumbnail
                                let ext_str = Path::new(&photo.file_name)
                                    .extension()
                                    .and_then(|e| e.to_str())
                                    .unwrap_or("")
                                    .to_uppercase();
                                if !ext_str.is_empty() {
                                    let badge_w = (ext_str.len() as f32 * 6.5 + 8.0).max(26.0);
                                    let ext_badge_rect = egui::Rect::from_min_size(
                                        egui::pos2(
                                            thumb_rect.max.x - badge_w - 5.0,
                                            thumb_rect.min.y + 5.0,
                                        ),
                                        Vec2::new(badge_w, 15.0),
                                    );
                                    ui.painter().rect_filled(
                                        ext_badge_rect,
                                        3.0,
                                        Color32::from_rgba_unmultiplied(14, 16, 22, 210),
                                    );
                                    ui.painter().text(
                                        ext_badge_rect.center(),
                                        egui::Align2::CENTER_CENTER,
                                        &ext_str,
                                        egui::FontId::proportional(9.0),
                                        Color32::from_rgb(180, 185, 205),
                                    );
                                }

                                // Filename (truncated cleanly and hard-clipped to card boundary)
                                let text_rect = egui::Rect::from_min_size(
                                    rect.min + Vec2::new(8.0, 155.0),
                                    Vec2::new(card_width - 16.0, 17.0),
                                );
                                let display_name = truncate_filename(&photo.file_name, 19);
                                let name_color = if is_selected {
                                    Color32::from_rgb(245, 248, 255)
                                } else if is_missing {
                                    Color32::from_rgb(240, 160, 110)
                                } else {
                                    Color32::from_rgb(215, 220, 230)
                                };
                                ui.painter().with_clip_rect(text_rect).text(
                                    text_rect.min,
                                    egui::Align2::LEFT_TOP,
                                    display_name,
                                    egui::FontId::proportional(11.5),
                                    name_color,
                                );

                                // Resolution or file size metadata subtitle
                                let sub_rect = egui::Rect::from_min_size(
                                    rect.min + Vec2::new(8.0, 174.0),
                                    Vec2::new(card_width - 16.0, 14.0),
                                );
                                let sub_text = if photo.width > 0 && photo.height > 0 {
                                    let mp =
                                        (photo.width as f64 * photo.height as f64) / 1_000_000.0;
                                    format!("{mp:.1} MP · {}×{}", photo.width, photo.height)
                                } else if photo.file_size > 0 {
                                    let mb = photo.file_size as f64 / (1024.0 * 1024.0);
                                    format!("{mb:.1} MB")
                                } else {
                                    String::new()
                                };
                                if !sub_text.is_empty() {
                                    ui.painter().with_clip_rect(sub_rect).text(
                                        sub_rect.min,
                                        egui::Align2::LEFT_TOP,
                                        sub_text,
                                        egui::FontId::proportional(10.0),
                                        Color32::from_rgb(130, 135, 148),
                                    );
                                }

                                // Star rating & color label on bottom row
                                let bottom_row_rect = egui::Rect::from_min_size(
                                    rect.min + Vec2::new(7.0, 194.0),
                                    Vec2::new(card_width - 14.0, 24.0),
                                );

                                let mut child_ui = ui.new_child(
                                    egui::UiBuilder::new()
                                        .max_rect(bottom_row_rect)
                                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                                );
                                child_ui.spacing_mut().item_spacing = Vec2::new(1.0, 0.0);

                                // 5 clickable rating stars
                                for s in 1..=5u8 {
                                    let star_char = if s <= photo.rating { "★" } else { "☆" };
                                    let star_col = if s <= photo.rating {
                                        Color32::from_rgb(245, 195, 45)
                                    } else {
                                        Color32::from_rgb(70, 75, 88)
                                    };

                                    let star_btn = child_ui.add(
                                        egui::Button::new(
                                            RichText::new(star_char).color(star_col).size(12.5),
                                        )
                                        .frame(false),
                                    );

                                    if star_btn.clicked() {
                                        let new_r = if photo.rating == s { 0 } else { s };
                                        rating_change = Some((photo.id, new_r));
                                    }
                                }

                                // Color label badge button
                                child_ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        let (col_color, next_label) = match photo.color_label {
                                            ColorLabel::None => {
                                                (Color32::from_rgb(52, 56, 68), ColorLabel::Red)
                                            }
                                            ColorLabel::Red => {
                                                (Color32::from_rgb(235, 60, 60), ColorLabel::Yellow)
                                            }
                                            ColorLabel::Yellow => {
                                                (Color32::from_rgb(240, 215, 50), ColorLabel::Green)
                                            }
                                            ColorLabel::Green => {
                                                (Color32::from_rgb(60, 200, 80), ColorLabel::Blue)
                                            }
                                            ColorLabel::Blue => (
                                                Color32::from_rgb(65, 130, 245),
                                                ColorLabel::Purple,
                                            ),
                                            ColorLabel::Purple => {
                                                (Color32::from_rgb(160, 80, 240), ColorLabel::None)
                                            }
                                        };

                                        let label_btn = ui
                                            .add(
                                                egui::Button::new(
                                                    RichText::new("●").color(col_color).size(13.0),
                                                )
                                                .frame(false),
                                            )
                                            .on_hover_text(t!(
                                                "library_color_cycle_tooltip",
                                                "label" => t!(photo.color_label.translation_key())
                                            ));

                                        if label_btn.clicked() {
                                            color_change = Some((photo.id, next_label));
                                        }
                                    },
                                );
                            });

                            col += 1;
                            if col >= columns {
                                ui.end_row();
                                col = 0;
                            }
                        }

                        if let Some((id, new_r)) = rating_change {
                            let _ = db.update_rating(id, new_r);
                            state.needs_refresh = true;
                        }
                        if let Some((id, new_l)) = color_change {
                            let _ = db.update_color_label(id, new_l);
                            state.needs_refresh = true;
                        }
                        if let Some(path) = photo_to_open {
                            action = LibraryAction::OpenPhoto(path);
                        }
                    });
            });
    });

    action
}
