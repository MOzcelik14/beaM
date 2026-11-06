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
    ImportFolder,
    ExportPhotos(Vec<PathBuf>),
    RebuildThumbnails,
    SyncSettings,
    OpenFileDialog,
    RelocatePhoto(i64, PathBuf),
    RelinkFolder,
    CleanMissingPhotos,
    RemovePhoto(i64),
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
            let tail: String = stem.chars().skip(stem_chars.saturating_sub(half_stem)).collect();
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
        }
    }
}

pub const MAX_IN_MEMORY_THUMBNAILS: usize = 500;
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
        .default_size(240.0)
        .min_size(210.0)
        .resizable(true)
        .show(ui, |ui| {
            ui.vertical(|ui| {
                ui.heading(t!("library_catalog"));
                ui.separator();

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
                    ui.add_space(6.0);
                }

                if ui
                    .button(
                        RichText::new(t!("library_import_folder"))
                            .size(14.0)
                            .strong(),
                    )
                    .clicked()
                {
                    action = LibraryAction::ImportFolder;
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
                if ui
                    .add_enabled(
                        can_export,
                        egui::Button::new(RichText::new(export_btn_text).size(14.0).strong()),
                    )
                    .clicked()
                {
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
                    if ui
                        .button(
                            RichText::new(t!("library_sync_settings"))
                                .size(13.0)
                                .strong(),
                        )
                        .on_hover_text(t!("sync_dialog_title"))
                        .clicked()
                    {
                        action = LibraryAction::SyncSettings;
                    }
                }

                ui.add_space(4.0);
                if ui
                    .button(RichText::new(t!("library_rebuild_thumbnails")).size(13.0))
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
                ui.strong(t!("library_filters"));
                ui.add_space(4.0);

                // Star rating filter buttons
                ui.label(RichText::new(t!("library_rating")).weak());
                ui.horizontal_wrapped(|ui| {
                    for r in 0..=5u8 {
                        let label = if r == 0 {
                            t!("library_rating_any")
                        } else {
                            format!("★{r}+")
                        };
                        let is_selected = state.filter.min_rating == r;
                        if ui.selectable_label(is_selected, label).clicked() {
                            state.filter.min_rating = r;
                            state.needs_refresh = true;
                        }
                    }
                });

                ui.add_space(6.0);

                // Color label filter
                ui.label(RichText::new(t!("library_color_label")).weak());
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

                ui.horizontal_wrapped(|ui| {
                    for (col, name, color) in labels {
                        let is_selected = state.filter.color_label == col;
                        let text = if col.is_some() {
                            RichText::new(format!("● {name}")).color(color)
                        } else {
                            RichText::new(name)
                        };
                        if ui.selectable_label(is_selected, text).clicked() {
                            state.filter.color_label = col;
                            state.needs_refresh = true;
                        }
                    }
                });

                ui.add_space(6.0);

                // File format filter
                ui.label(RichText::new(t!("library_format")).weak());
                ui.horizontal_wrapped(|ui| {
                    for ft in [
                        beam_library::FileTypeFilter::All,
                        beam_library::FileTypeFilter::Raw,
                        beam_library::FileTypeFilter::Jpeg,
                        beam_library::FileTypeFilter::Png,
                        beam_library::FileTypeFilter::Tiff,
                        beam_library::FileTypeFilter::WebP,
                    ] {
                        let is_selected = state.filter.file_type == ft;
                        if ui
                            .selectable_label(is_selected, t!(ft.translation_key()))
                            .clicked()
                        {
                            state.filter.file_type = ft;
                            state.needs_refresh = true;
                        }
                    }
                });

                ui.add_space(8.0);

                // Search box
                ui.label(RichText::new(t!("library_search")).weak());
                let mut search_text = state.filter.search_query.clone().unwrap_or_default();
                let resp = ui.text_edit_singleline(&mut search_text);
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

                // Sort order selector
                ui.label(RichText::new(t!("library_sort_by")).weak());
                egui::ComboBox::from_id_salt("library_sort_order")
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
                                    action = LibraryAction::ImportFolder;
                                }
                            });
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
                let card_width = 205.0f32;
                let card_height = 222.0f32;
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
                                    Color32::from_rgb(34, 42, 56)
                                } else if response.hovered() {
                                    Color32::from_rgb(30, 32, 38)
                                } else {
                                    Color32::from_rgb(22, 23, 27)
                                };

                                let stroke_col = if is_selected {
                                    Color32::from_rgb(75, 145, 245)
                                } else if is_missing {
                                    Color32::from_rgb(85, 55, 40)
                                } else {
                                    Color32::from_rgb(42, 45, 52)
                                };

                                ui.painter().rect_filled(rect, 8.0, bg_col);
                                ui.painter().rect_stroke(
                                    rect,
                                    8.0,
                                    Stroke::new(if is_selected { 2.0 } else { 1.0 }, stroke_col),
                                    egui::StrokeKind::Inside,
                                );

                                // Render thumbnail image in top portion
                                let thumb_rect = egui::Rect::from_min_size(
                                    rect.min + Vec2::new(8.0, 8.0),
                                    Vec2::new(card_width - 16.0, 142.0),
                                );

                                if is_visible && !state.thumbnail_status.contains_key(&photo.id) {
                                    let disk_file_valid = photo
                                        .thumbnail_path
                                        .as_ref()
                                        .map(|p| Path::new(p).exists())
                                        .unwrap_or(false);

                                    if disk_file_valid {
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
                                        Color32::from_rgba_unmultiplied(200, 100, 30, 230),
                                    );
                                    ui.painter().text(
                                        badge_rect.center(),
                                        egui::Align2::CENTER_CENTER,
                                        t!("library_missing_badge"),
                                        egui::FontId::proportional(10.0),
                                        Color32::WHITE,
                                    );
                                }

                                // Filename (truncated cleanly, never overflows card boundary)
                                let text_rect = egui::Rect::from_min_size(
                                    rect.min + Vec2::new(8.0, 156.0),
                                    Vec2::new(card_width - 16.0, 18.0),
                                );
                                let display_name = truncate_filename(&photo.file_name, 22);
                                let name_color = if is_missing {
                                    Color32::from_rgb(240, 160, 110)
                                } else {
                                    Color32::from_rgb(220, 222, 230)
                                };
                                ui.painter().text(
                                    text_rect.min,
                                    egui::Align2::LEFT_TOP,
                                    display_name,
                                    egui::FontId::proportional(12.0),
                                    name_color,
                                );

                                // Star rating & color label on bottom row
                                let bottom_row_rect = egui::Rect::from_min_size(
                                    rect.min + Vec2::new(8.0, 184.0),
                                    Vec2::new(card_width - 16.0, 26.0),
                                );

                                let mut child_ui = ui.new_child(
                                    egui::UiBuilder::new()
                                        .max_rect(bottom_row_rect)
                                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                                );

                                // 5 clickable rating stars
                                for s in 1..=5u8 {
                                    let star_char = if s <= photo.rating { "★" } else { "☆" };
                                    let star_col = if s <= photo.rating {
                                        Color32::from_rgb(240, 195, 45)
                                    } else {
                                        Color32::from_rgb(70, 70, 78)
                                    };

                                    let star_btn = child_ui.add(
                                        egui::Button::new(
                                            RichText::new(star_char).color(star_col).size(13.0),
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
                                                (Color32::from_rgb(50, 50, 55), ColorLabel::Red)
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
                                                    RichText::new("●").color(col_color).size(14.0),
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
