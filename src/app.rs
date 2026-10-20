//! Application state, frame loop, autosave, hotkeys and background jobs.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc;

use egui::{Context, Key, KeyboardShortcut, Modifiers};

use crate::canvas::{self, Action, Clip, Editor, Tool};
use crate::crypto::Key as CKey;
use crate::items::{self, FormatCmd};
use crate::model::*;
use crate::search::{self, Index};
use crate::settings::{Settings, Theme};
use crate::storage::{Notebook, SaveJob, Saver};
use crate::textures::Textures;
use crate::winapi::{Speller, Tts};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RightPanel {
    Search,
    Tags,
    Tasks,
    Helper,
    Comments,
    Versions,
    Reading,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ribbon {
    Home,
    Insert,
    Draw,
    View,
    Help,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RailMode {
    Notebooks,
    Search,
    Recent,
}

pub enum Dialog {
    NewNotebook(String),
    Rename { target: RenameTarget, text: String },
    Password { target: PwTarget, text: String, confirm: String, error: String },
    Settings(usize),
    Trash,
    Backups,
    Report(String, Vec<String>),
    TaskDue { item: Id, date: String, time: String },
    Link { text: String, url: String },
    Symbols,
    Templates,
    Tags,
    Merge { src: PathBuf, policy: usize },
    MergeSections { from: Id },
    Hotkeys,
    About,
    Table { rows: usize, cols: usize },
    Print(PrintOpts),
    OcrResult { title: String, text: String, from_ink: bool },
    Confirm { text: String, action: ConfirmAction },
    BackupPassword(String),
    Flashcards { cards: Vec<(String, String)>, idx: usize, flipped: bool },
    ClearInk,
}

#[derive(Clone, Debug)]
pub enum ConfirmAction {
    DeleteSection(Id),
    DeleteGroup(Id),
    EmptyTrash,
    CloseNotebook(usize),
    RestoreBackup(PathBuf),
}

#[derive(Clone, Debug)]
pub enum RenameTarget {
    Notebook(usize),
    Section(Id),
    Group(Id),
    Page(Id),
}

#[derive(Clone, Debug)]
pub enum PwTarget {
    UnlockSection(Id),
    UnlockNotebook(usize),
    SetSection(Id),
    SetNotebook(usize),
}

#[derive(Clone)]
pub struct PrintOpts {
    pub whole_section: bool,
    pub landscape: bool,
    pub margins: f32,
    pub scale: f32,
    pub numbers: bool,
    pub header: bool,
    pub print: bool,
    pub path: Option<PathBuf>,
}

impl Default for PrintOpts {
    fn default() -> Self {
        Self { whole_section: false, landscape: false, margins: 12.0, scale: 1.0, numbers: true, header: true, print: true, path: None }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExportKind {
    Pdf,
    Png,
    Jpg,
    Print,
}

pub struct ExportJob {
    pub kind: ExportKind,
    pub queue: Vec<Id>,
    pub images: Vec<image::RgbaImage>,
    pub path: PathBuf,
    pub opts: PrintOpts,
    pub started: bool,
}

pub enum BgResult {
    Ocr { page: Id, item: Option<Id>, text: Result<String, String> },
    Pdf { page: Id, at: [f32; 2], images: Result<Vec<Vec<u8>>, String>, nb: PathBuf },
    Summary(Vec<SummaryRow>),
    Math { page: Id, text: Result<String, String> },
}

#[derive(Clone)]
pub struct SummaryRow {
    pub nb: PathBuf,
    pub nb_name: String,
    pub section: String,
    pub page: Id,
    pub page_title: String,
    pub item: Id,
    pub tags: Vec<u32>,
    pub task: Option<Task>,
    pub text: String,
    pub modified: chrono::NaiveDateTime,
}

pub struct PinnedWin {
    pub id: egui::ViewportId,
    pub nb: PathBuf,
    pub page: Id,
    pub on_top: bool,
    pub editor: Editor,
    pub open: bool,
}

pub struct App {
    pub settings: Settings,
    pub notebooks: Vec<Notebook>,
    pub cur_nb: Option<usize>,
    pub cur_section: Option<Id>,
    pub cur_page: Option<Id>,
    pub pages: HashMap<Id, Page>,
    pub dirty: HashSet<Id>,
    pub dirty_since: f64,
    pub editors: HashMap<Id, Editor>,
    pub saver: Saver,
    pub index: Index,
    pub textures: Textures,
    pub speller: Speller,
    pub tts: Tts,
    pub clip: Clip,
    pub status: (String, f64),
    pub right: Option<RightPanel>,
    pub ribbon: Ribbon,
    pub focus_mode: bool,
    pub presentation: bool,
    pub dialogs: Vec<Dialog>,
    pub search_q: String,
    pub search_filters: search::Filters,
    pub search_focus: bool,
    pub summary: Vec<SummaryRow>,
    pub summary_scope: search::Scope,
    pub summary_tag: Option<u32>,
    pub summary_state: usize,
    pub bg_tx: mpsc::Sender<BgResult>,
    pub bg_rx: mpsc::Receiver<BgResult>,
    pub hotkey_rx: mpsc::Receiver<()>,
    pub last_input: f64,
    pub last_version: HashMap<Id, f64>,
    pub last_backup_check: f64,
    pub last_task_check: f64,
    pub last_inbox_check: f64,
    pub last_lock_refresh: f64,
    pub pinned: Vec<PinnedWin>,
    pub quick_note: Option<String>,
    pub export: Option<ExportJob>,
    pub print_preview: Vec<image::RgbaImage>,
    pub locked_by: Option<String>,
    pub backup_key: Option<CKey>,
    pub helper_topic: String,
    pub helper_question: String,
    pub helper_out: Vec<(String, String)>,
    pub comment_text: String,
    pub version_sel: Option<PathBuf>,
    pub version_page: Option<Page>,
    pub reading: crate::ui_panels::ReadingState,
    pub clipboard_watch: Option<(f64, u64)>,
    pub minimized_start: bool,
    pub pending_add: Vec<PathBuf>,
    pub screen_capture_prev: bool,
    pub focus_request: bool,
    pub pen_settings: Option<usize>,
    pub pen_pick_slot: Option<usize>,
    pub underlay_next: bool,
    pub ribbon_collapsed: bool,
    pub rail: RailMode,
    pub nav_hidden: bool,
    pub recent: Vec<Id>,
    pub page_filter: String,
}

impl App {
    pub fn new(cc: &eframe::CreationContext, add_files: Vec<PathBuf>, minimized: bool) -> Self {
        let ctx = cc.egui_ctx.clone();
        crate::fonts::install(&ctx);
        egui_extras_install(&ctx);
        #[cfg(windows)]
        {
            use raw_window_handle::{HasWindowHandle, RawWindowHandle};
            if let Ok(h) = cc.window_handle() {
                if let RawWindowHandle::Win32(w) = h.as_raw() {
                    crate::pen::install(w.hwnd.get(), ctx.clone());
                }
            }
        }
        let settings = Settings::load();
        let (bg_tx, bg_rx) = mpsc::channel();
        let (hk_tx, hotkey_rx) = mpsc::channel();
        crate::winapi::spawn_hotkey(ctx.clone(), hk_tx);
        let speller = Speller::new(ctx.clone());
        speller.set_language(&settings.spell_lang, &settings.user_dictionary);
        let mut app = Self {
            notebooks: Vec::new(),
            cur_nb: None,
            cur_section: None,
            cur_page: None,
            pages: HashMap::new(),
            dirty: HashSet::new(),
            dirty_since: 0.0,
            editors: HashMap::new(),
            saver: Saver::new(),
            index: Index::default(),
            textures: Textures::new(ctx.clone()),
            speller,
            tts: Tts::new(ctx.clone()),
            clip: Clip::default(),
            status: (String::new(), 0.0),
            right: None,
            ribbon: Ribbon::Home,
            focus_mode: false,
            presentation: false,
            dialogs: Vec::new(),
            search_q: String::new(),
            search_filters: search::Filters::default(),
            search_focus: false,
            summary: Vec::new(),
            summary_scope: search::Scope::All,
            summary_tag: None,
            summary_state: 0,
            bg_tx,
            bg_rx,
            hotkey_rx,
            last_input: 0.0,
            last_version: HashMap::new(),
            last_backup_check: -1000.0,
            last_task_check: -1000.0,
            last_inbox_check: 0.0,
            last_lock_refresh: 0.0,
            pinned: Vec::new(),
            quick_note: None,
            export: None,
            print_preview: Vec::new(),
            locked_by: None,
            backup_key: None,
            helper_topic: String::new(),
            helper_question: String::new(),
            helper_out: Vec::new(),
            comment_text: String::new(),
            version_sel: None,
            version_page: None,
            reading: Default::default(),
            clipboard_watch: None,
            minimized_start: minimized,
            pending_add: add_files,
            screen_capture_prev: false,
            focus_request: false,
            pen_settings: None,
            pen_pick_slot: None,
            underlay_next: false,
            ribbon_collapsed: false,
            rail: RailMode::Notebooks,
            nav_hidden: false,
            recent: Vec::new(),
            page_filter: String::new(),
            settings,
        };
        app.apply_theme(&ctx);
        app.ribbon = match app.settings.last_ribbon {
            1 => Ribbon::Insert,
            2 => Ribbon::Draw,
            3 => Ribbon::View,
            4 => Ribbon::Help,
            _ => Ribbon::Home,
        };
        let paths = app.settings.open_notebooks.clone();
        for p in paths {
            if let Ok(nb) = Notebook::open(&p) {
                app.notebooks.push(nb);
            }
        }
        if app.notebooks.is_empty() {
            app.create_first_notebook();
        }
        for nb in app.notebooks.iter_mut() {
            nb.purge_trash(app.settings.trash_days);
        }
        app.cur_nb = Some(0);
        app.select_first_page();
        app.rebuild_index(&ctx);
        if app.minimized_start {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        }
        app
    }

    fn create_first_notebook(&mut self) {
        let dir = self.settings.notebooks_dir.clone();
        let _ = std::fs::create_dir_all(&dir);
        if let Ok(mut nb) = Notebook::create(&dir, "Мой блокнот") {
            let mut sec = Section::new("Быстрые заметки", 5);
            let mut pm = PageMeta::new("Добро пожаловать", &self.settings.author);
            let page = crate::templates::welcome(pm.id, &self.settings);
            pm.title = "Добро пожаловать".into();
            crate::storage::write_page(&nb.page_path(page.id), &page, None);
            sec.pages.push(pm);
            nb.meta.tree.push(Node::Section(sec));
            nb.meta.tree.push(Node::Section(Section::new("Работа", 1)));
            nb.meta.tree.push(Node::Section(Section::new("Личное", 9)));
            nb.save_meta();
            self.settings.open_notebooks.push(nb.path.clone());
            self.settings.save();
            self.notebooks.push(nb);
        }
    }

    pub fn apply_theme(&self, ctx: &Context) {
        ctx.set_theme(match self.settings.theme {
            Theme::Light => egui::ThemePreference::Light,
            Theme::Dark => egui::ThemePreference::Dark,
            Theme::System => egui::ThemePreference::System,
        });
        let scale = if self.settings.touch_mode { 1.25 } else { 1.0 } * self.settings.ui_scale;
        ctx.set_zoom_factor(scale);
        // OneNote-like purple accent.
        for (theme, mut v) in [(egui::Theme::Light, egui::Visuals::light()), (egui::Theme::Dark, egui::Visuals::dark())] {
            let p = crate::chrome::pal(theme == egui::Theme::Dark);
            v.selection.bg_fill = p.accent.gamma_multiply(if theme == egui::Theme::Dark { 0.55 } else { 0.35 });
            v.selection.stroke = egui::Stroke::new(1.0, p.accent);
            v.hyperlink_color = p.accent;
            v.panel_fill = p.nav_bg;
            v.window_fill = if theme == egui::Theme::Dark { egui::Color32::from_rgb(43, 43, 43) } else { egui::Color32::WHITE };
            v.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, p.accent.gamma_multiply(0.7));
            v.widgets.active.bg_fill = p.accent.gamma_multiply(0.6);
            v.widgets.inactive.corner_radius = egui::CornerRadius::same(4);
            v.widgets.hovered.corner_radius = egui::CornerRadius::same(4);
            v.widgets.active.corner_radius = egui::CornerRadius::same(4);
            v.window_corner_radius = egui::CornerRadius::same(8);
            v.menu_corner_radius = egui::CornerRadius::same(6);
            ctx.set_visuals_of(theme, v);
        }
        ctx.global_style_mut(|s| {
            if self.settings.touch_mode {
                s.spacing.interact_size.y = 34.0;
                s.spacing.button_padding = egui::vec2(10.0, 6.0);
                s.spacing.item_spacing = egui::vec2(10.0, 8.0);
            } else {
                s.spacing.interact_size.y = 22.0;
                s.spacing.button_padding = egui::vec2(6.0, 3.0);
                s.spacing.item_spacing = egui::vec2(8.0, 5.0);
            }
        });
    }

    pub fn nb(&self) -> Option<&Notebook> {
        self.cur_nb.and_then(|i| self.notebooks.get(i))
    }

    pub fn nb_mut(&mut self) -> Option<&mut Notebook> {
        self.cur_nb.and_then(|i| self.notebooks.get_mut(i))
    }

    pub fn nb_index_of_page(&self, page: Id) -> Option<usize> {
        self.notebooks.iter().position(|nb| nb.meta.section_of_page(page).is_some())
    }

    pub fn set_status(&mut self, s: impl Into<String>) {
        self.status = (s.into(), now_secs());
    }

    pub fn select_first_page(&mut self) {
        let Some(nb) = self.nb() else { return };
        let secs = nb.meta.sections();
        let sec = self.cur_section.and_then(|s| secs.iter().find(|x| x.id == s).copied()).or_else(|| secs.first().copied());
        let sid = sec.map(|s| s.id);
        let pid = sec.and_then(|s| sorted_pages(s).first().map(|p| p.id));
        self.cur_section = sid;
        self.open_page(pid);
    }

    /// Switches the current page, saving the previous one.
    pub fn open_page(&mut self, page: Option<Id>) {
        if self.cur_page == page {
            return;
        }
        if let Some(old) = self.cur_page {
            self.flush_page(old, true);
            if let Some(nb) = self.nb() {
                nb.release_page_lock(old, &self.settings.author);
            }
            if let Some(ed) = self.editors.get_mut(&old) {
                ed.editing = None;
                ed.selection = Default::default();
            }
        }
        self.cur_page = page;
        if let Some(p) = page {
            self.recent.retain(|x| *x != p);
            self.recent.insert(0, p);
            self.recent.truncate(30);
        }
        self.locked_by = None;
        self.version_sel = None;
        self.version_page = None;
        if let Some(pid) = page {
            if let Some(ni) = self.nb_index_of_page(pid) {
                self.cur_nb = Some(ni);
                self.cur_section = self.notebooks[ni].meta.section_of_page(pid);
            }
            self.ensure_loaded(pid);
            let author = self.settings.author.clone();
            let locked = self.nb().and_then(|nb| nb.page_locked_by(pid, &author));
            if locked.is_none() {
                if let Some(nb) = self.nb() {
                    nb.acquire_page_lock(pid, &author);
                }
            }
            self.locked_by = locked;
            self.last_lock_refresh = now_secs();
        }
    }

    pub fn ensure_loaded(&mut self, pid: Id) -> bool {
        if self.pages.contains_key(&pid) {
            return true;
        }
        let Some(ni) = self.nb_index_of_page(pid) else { return false };
        let nb = &self.notebooks[ni];
        if !nb.page_accessible(pid) {
            return false;
        }
        match nb.load_page(pid) {
            Ok(p) => {
                self.pages.insert(pid, p);
                let mut ed = Editor::new(pid);
                if let Some(p) = self.settings.pens().first() {
                    ed.set_pen(p, Some(0));
                    ed.tool = Tool::Select;
                }
                // Keep the brush of the previous page.
                if let Some(prev) = self.cur_page.and_then(|c| self.editors.get(&c)) {
                    ed.pen = prev.pen.clone();
                    ed.pen_slot = prev.pen_slot;
                    ed.tool = if prev.tool.is_ink() { prev.tool } else { Tool::Select };
                    ed.shape = prev.shape.clone();
                    ed.eraser_mode = prev.eraser_mode;
                    ed.eraser_radius = prev.eraser_radius;
                }
                self.editors.entry(pid).or_insert(ed);
                true
            }
            Err(e) => {
                self.set_status(format!("Не удалось открыть страницу: {e}"));
                false
            }
        }
    }

    pub fn mark_dirty(&mut self, pid: Id) {
        if self.dirty.insert(pid) || self.dirty_since == 0.0 {
            self.dirty_since = now_secs();
        }
        let author = self.settings.author.clone();
        if let Some(ni) = self.nb_index_of_page(pid) {
            if let Some(pm) = self.notebooks[ni].meta.page_meta_mut(pid) {
                pm.modified = now();
                if pm.author.is_empty() {
                    pm.author = author;
                }
            }
            self.notebooks[ni].dirty = true;
        }
    }

    /// Writes a page (in the background) and optionally a version snapshot.
    pub fn flush_page(&mut self, pid: Id, version: bool) {
        let Some(ni) = self.nb_index_of_page(pid) else { return };
        let Some(page) = self.pages.get(&pid) else { return };
        let was_dirty = self.dirty.remove(&pid);
        let nb = &self.notebooks[ni];
        if was_dirty {
            self.saver.send(SaveJob::Page { path: nb.page_path(pid), page: Box::new(page.clone()), key: nb.key_for_page(pid).cloned() });
            // Index update.
            if let (Some(sec), Some(pm)) = (nb.meta.section_of_page(pid).and_then(|s| nb.meta.section(s)), nb.meta.page_meta(pid)) {
                let src = search::PageSrc {
                    nb: nb.path.clone(),
                    nb_name: nb.meta.name.clone(),
                    section: sec.id,
                    section_name: sec.name.clone(),
                    meta: pm.clone(),
                    path: nb.page_path(pid),
                    key: None,
                };
                self.index.update(search::entry_for(&src, page));
            }
        }
        let t = now_secs();
        let due = self.last_version.get(&pid).is_none_or(|lt| t - lt >= self.settings.version_minutes as f64 * 60.0);
        if version && (was_dirty || due) && (was_dirty || !self.last_version.contains_key(&pid)) {
            nb.save_version(page, self.settings.version_keep, self.settings.version_days);
            self.last_version.insert(pid, t);
        }
        if self.notebooks[ni].dirty {
            self.notebooks[ni].save_meta();
            self.notebooks[ni].dirty = false;
        }
    }

    pub fn save_all(&mut self) {
        let ids: Vec<Id> = self.dirty.iter().copied().collect();
        for id in ids {
            self.flush_page(id, true);
        }
        for nb in self.notebooks.iter_mut() {
            if nb.dirty {
                nb.save_meta();
                nb.dirty = false;
            }
        }
        self.saver.flush();
    }

    pub fn rebuild_index(&self, ctx: &Context) {
        let mut sources = Vec::new();
        for nb in &self.notebooks {
            if nb.sealed {
                continue;
            }
            for s in nb.meta.sections() {
                if !nb.section_accessible(s.id) {
                    continue;
                }
                for p in &s.pages {
                    sources.push(search::PageSrc {
                        nb: nb.path.clone(),
                        nb_name: nb.meta.name.clone(),
                        section: s.id,
                        section_name: s.name.clone(),
                        meta: p.clone(),
                        path: nb.page_path(p.id),
                        key: nb.key_for_page(p.id).cloned(),
                    });
                }
            }
        }
        self.index.rebuild(sources, ctx.clone());
    }

    pub fn lock_all(&mut self) {
        self.save_all();
        let mut drop_pages = Vec::new();
        for nb in self.notebooks.iter_mut() {
            let locked_secs: Vec<Id> = nb.meta.sections().iter().filter(|s| s.lock.is_some()).map(|s| s.id).collect();
            for sid in locked_secs {
                if nb.keys.remove(&sid).is_some() {
                    if let Some(s) = nb.meta.section(sid) {
                        drop_pages.extend(s.pages.iter().map(|p| p.id));
                    }
                }
            }
            if nb.lock.is_some() && !nb.sealed {
                drop_pages.extend(nb.meta.sections().iter().flat_map(|s| s.pages.iter().map(|p| p.id)));
                nb.seal();
            }
        }
        for p in drop_pages {
            self.pages.remove(&p);
            self.editors.remove(&p);
            self.index.remove(p);
        }
        if let Some(p) = self.cur_page {
            if !self.pages.contains_key(&p) {
                self.cur_page = None;
            }
        }
        self.set_status("Защищённые разделы заблокированы");
    }

    // ---- page actions ---------------------------------------------------------

    pub fn with_page<R>(&mut self, f: impl FnOnce(&mut Page, &mut Editor) -> R) -> Option<R> {
        let pid = self.cur_page?;
        let page = self.pages.get_mut(&pid)?;
        let ed = self.editors.get_mut(&pid)?;
        Some(f(page, ed))
    }

    pub fn format(&mut self, ctx: &Context, cmd: FormatCmd) {
        let pid = match self.cur_page {
            Some(p) => p,
            None => return,
        };
        if self.locked_by.is_some() {
            return;
        }
        let changed = self.with_page(|page, ed| items::apply_format(ctx, page, ed, cmd)).unwrap_or(false);
        if changed {
            self.mark_dirty(pid);
        }
    }

    /// Adds an item at a sensible place on the current page.
    pub fn insert_item(&mut self, kind: ItemKind, width: f32) -> Option<Id> {
        let pid = self.cur_page?;
        let author = self.settings.author.clone();
        let id = self.with_page(|page, ed| {
            ed.snapshot(page);
            let vis_center = ed.offset.to_pos2() + egui::vec2(60.0, 60.0) / ed.zoom;
            let pos = match ed.editing.and_then(|e| page.item(e)) {
                Some(it) => it.rect().left_bottom() + egui::vec2(0.0, 16.0),
                None => vis_center,
            };
            let it = Item::new(pos, width, kind, &author);
            let id = it.id;
            page.items.push(it);
            ed.editing = Some(id);
            ed.focus_item = Some(id);
            ed.tool = Tool::Select;
            if matches!(page.item(id).map(|i| &i.kind), Some(ItemKind::Table(_))) {
                ed.table_cell = Some((0, 0));
            }
            id
        })?;
        self.mark_dirty(pid);
        Some(id)
    }

    pub fn insert_text_at_cursor(&mut self, ctx: &Context, s: String) {
        let editing = self.cur_page.and_then(|p| self.editors.get(&p)).and_then(|e| e.editing);
        let is_text = editing
            .and_then(|e| self.cur_page.and_then(|p| self.pages.get(&p)).and_then(|pg| pg.item(e)))
            .is_some_and(|it| matches!(it.kind, ItemKind::Text(_)));
        if is_text {
            self.format(ctx, FormatCmd::Insert(s));
        } else {
            let size = self.settings.default_size;
            let font = self.settings.default_font.clone();
            self.insert_item(ItemKind::Text(TextBlock::new(&s, size, &font)), 420.0);
        }
    }

    pub fn import_files(&mut self, files: Vec<(Option<PathBuf>, Option<std::sync::Arc<[u8]>>, String)>, at: Option<egui::Pos2>) {
        let Some(pid) = self.cur_page else {
            self.set_status("Сначала откройте страницу");
            return;
        };
        let Some(ni) = self.nb_index_of_page(pid) else { return };
        let mut y = at.map(|p| p.y).unwrap_or(0.0);
        let x0 = at.map(|p| p.x);
        let mut new_items = Vec::new();
        for (path, bytes, name) in files {
            let data = match (&bytes, &path) {
                (Some(b), _) => b.to_vec(),
                (None, Some(p)) => match std::fs::read(p) {
                    Ok(d) => d,
                    Err(_) => continue,
                },
                _ => continue,
            };
            let ext = std::path::Path::new(&name).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
            let nb = &self.notebooks[ni];
            let is_img = matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp");
            let kind = if is_img {
                let (data, ext) = if self.settings.compress_images {
                    crate::textures::compress(&data, self.settings.image_max_dim, self.settings.jpeg_quality)
                        .filter(|(c, _)| c.len() < data.len())
                        .map(|(c, e)| (c, e.to_string()))
                        .unwrap_or((data.clone(), ext.clone()))
                } else {
                    (data.clone(), ext.clone())
                };
                let (w, h) = image::load_from_memory(&data).map(|i| (i.width(), i.height())).unwrap_or((400, 300));
                let asset = nb.import_asset(&data, &ext);
                let width = (w as f32).min(600.0);
                new_items.push((ItemKind::Image(ImageItem { asset, crop: full_crop(), ..Default::default() }), width, width * h as f32 / w.max(1) as f32));
                continue;
            } else {
                let asset = nb.import_asset(&data, if ext.is_empty() { "bin" } else { &ext });
                ItemKind::File(FileItem { asset, name: name.clone(), size: data.len() as u64, extracted_text: None })
            };
            new_items.push((kind, 320.0, 52.0));
        }
        let author = self.settings.author.clone();
        self.with_page(|page, ed| {
            ed.snapshot(page);
            let base = ed.offset.to_pos2() + egui::vec2(40.0, 40.0) / ed.zoom;
            if y == 0.0 {
                y = base.y;
            }
            for (kind, w, h) in new_items {
                let it = Item::new(egui::pos2(x0.unwrap_or(base.x), y), w, kind, &author);
                y += h + 20.0;
                page.items.push(it);
            }
        });
        self.mark_dirty(pid);
    }

    pub fn handle_action(&mut self, ctx: &Context, a: Action) {
        match a {
            Action::OpenLink(url) => self.open_link(&url),
            Action::Status(s) if s == "underlay" => self.pick_underlay(),
            Action::Status(s) => self.set_status(s),
            Action::CopyText(t) => {
                ctx.copy_text(t);
                self.set_status("Скопировано");
            }
            Action::ItemLink(id) => {
                if let Some(p) = self.cur_page {
                    ctx.copy_text(format!("note://page/{p}#{id}"));
                    self.set_status("Ссылка на абзац скопирована");
                }
            }
            Action::SetTaskDue(item) => {
                let now = chrono::Local::now();
                self.dialogs.push(Dialog::TaskDue { item, date: now.format("%d.%m.%Y").to_string(), time: "18:00".into() });
            }
            Action::OcrImage(item) => self.start_ocr_image(item),
            Action::OcrInk => self.start_ocr_ink(),
            Action::PdfPrintout(item) => self.start_pdf_printout(item),
            Action::Dirty => {
                if let Some(p) = self.cur_page {
                    self.mark_dirty(p);
                }
            }
            Action::PickColor(c) => {
                if let Some(slot) = self.pen_pick_slot.take() {
                    if let Some(p) = self.settings.pens_mut().get_mut(slot) {
                        p.color = [c[0], c[1], c[2], p.color[3]];
                    }
                    self.settings.save();
                    self.pen_settings = Some(slot);
                }
                self.set_status("Цвет взят со штриха");
            }
            Action::InkToMath => self.start_ocr_math(),
            Action::ExportSelection(fmt) => self.export_drawing(ctx, fmt),
        }
    }

    fn selected_or_all_strokes(&mut self) -> Vec<Stroke> {
        self.with_page(|page, ed| {
            if ed.selection.strokes.is_empty() {
                page.strokes.clone()
            } else {
                ed.selection.strokes.iter().filter_map(|&i| page.strokes.get(i).cloned()).collect()
            }
        })
        .unwrap_or_default()
    }

    /// PNG with transparency, SVG, clipboard image or GIF animation of the drawing.
    fn export_drawing(&mut self, ctx: &Context, fmt: &str) {
        let strokes = self.selected_or_all_strokes();
        if strokes.is_empty() {
            self.set_status("Нет рисунков");
            return;
        }
        let curve = self.settings.pressure_curve;
        match fmt {
            "png" => {
                if let Some(p) = rfd::FileDialog::new().add_filter("PNG", &["png"]).set_file_name("рисунок.png").save_file() {
                    match crate::raster::render_strokes(&strokes, 3.0, None, curve) {
                        Some((img, _)) => match img.save(&p) {
                            Ok(()) => self.set_status(format!("Сохранено: {}", p.display())),
                            Err(e) => self.set_status(format!("Ошибка: {e}")),
                        },
                        None => self.set_status("Рисунок слишком большой"),
                    }
                }
            }
            "svg" => {
                if let Some(p) = rfd::FileDialog::new().add_filter("SVG", &["svg"]).set_file_name("рисунок.svg").save_file() {
                    let _ = std::fs::write(&p, crate::raster::strokes_svg(&strokes, curve));
                    self.set_status(format!("Сохранено: {}", p.display()));
                }
            }
            "copy" => {
                if let Some((img, _)) = crate::raster::render_strokes(&strokes, 2.0, None, curve) {
                    let data = arboard::ImageData { width: img.width() as usize, height: img.height() as usize, bytes: std::borrow::Cow::Owned(img.into_raw()) };
                    match arboard::Clipboard::new().and_then(|mut c| c.set_image(data)) {
                        Ok(()) => self.set_status("Рисунок скопирован как изображение"),
                        Err(e) => self.set_status(format!("Ошибка буфера обмена: {e}")),
                    }
                }
            }
            "gif" => {
                if let Some(p) = rfd::FileDialog::new().add_filter("GIF", &["gif"]).set_file_name("рисование.gif").save_file() {
                    let bg = self.cur_page.and_then(|pid| self.pages.get(&pid)).and_then(|pg| pg.bg_color).unwrap_or([255, 255, 255, 255]);
                    let all = self.cur_page.and_then(|pid| self.pages.get(&pid)).map(|pg| pg.strokes.clone()).unwrap_or(strokes);
                    let tx = ctx.clone();
                    self.set_status("Создание GIF…");
                    let (done_tx, _) = std::sync::mpsc::channel::<()>();
                    std::thread::spawn(move || {
                        let _ = crate::raster::animation_gif(&all, 720, bg, curve, 8.0, &p);
                        let _ = done_tx.send(());
                        tx.request_repaint();
                    });
                }
            }
            _ => {}
        }
        let _ = ctx;
    }

    fn start_ocr_math(&mut self) {
        let Some(pid) = self.cur_page else { return };
        let strokes = self.selected_or_all_strokes();
        if strokes.is_empty() {
            self.set_status("Выделите рукописную формулу");
            return;
        }
        let tx = self.bg_tx.clone();
        self.set_status("Распознавание формулы…");
        std::thread::spawn(move || {
            let text = match crate::ink::rasterize(&strokes, 1.5) {
                Some((img, _)) => crate::winapi::ocr(&image::DynamicImage::ImageLuma8(img).to_rgba8(), None),
                None => Err("Пустой рисунок".into()),
            };
            let _ = tx.send(BgResult::Math { page: pid, text });
        });
    }

    pub fn pick_underlay(&mut self) {
        let Some(path) = rfd::FileDialog::new().add_filter("Изображение или PDF", &["png", "jpg", "jpeg", "bmp", "gif", "webp", "pdf"]).pick_file() else { return };
        let Some(pid) = self.cur_page else { return };
        let Some(ni) = self.nb_index_of_page(pid) else { return };
        if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf")) {
            self.underlay_next = true;
            self.pdf_printout_from(path, [0.0, 0.0]);
            return;
        }
        let Ok(data) = std::fs::read(&path) else { return };
        let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_else(|| "png".into());
        let (w, h) = image::load_from_memory(&data).map(|i| (i.width(), i.height())).unwrap_or((800, 600));
        let asset = self.notebooks[ni].import_asset(&data, &ext);
        let author = self.settings.author.clone();
        self.with_page(|page, ed| {
            ed.snapshot(page);
            let width = (w as f32).min(1100.0);
            let mut it = Item::new(egui::pos2(0.0, 0.0), width, ItemKind::Image(ImageItem { asset, crop: full_crop(), ..Default::default() }), &author);
            it.underlay = true;
            it.height = width * h as f32 / w.max(1) as f32;
            page.items.insert(0, it);
        });
        self.mark_dirty(pid);
        self.set_status("Подложка добавлена. «Фон → Редактировать подложку» — чтобы переместить её");
    }

    pub fn open_link(&mut self, url: &str) {
        if let Some(rest) = url.strip_prefix("note://") {
            // note://page/<id>[#item], note://section/<id>, note://notebook/<id>
            let (kind, rest) = rest.split_once('/').unwrap_or((rest, ""));
            let (id_s, frag) = rest.split_once('#').unwrap_or((rest, ""));
            let Ok(id) = Id::parse_str(id_s) else { return };
            match kind {
                "page" => {
                    self.open_page(Some(id));
                    if let Ok(item) = Id::parse_str(frag) {
                        let pos = self.pages.get(&id).and_then(|p| p.item(item)).map(|i| i.pos2());
                        if let (Some(pos), Some(ed)) = (pos, self.editors.get_mut(&id)) {
                            ed.scroll_to = Some(pos);
                            ed.selection.items = vec![item];
                        }
                    }
                }
                "section" => {
                    if let Some(ni) = self.notebooks.iter().position(|nb| nb.meta.section(id).is_some()) {
                        self.cur_nb = Some(ni);
                        self.cur_section = Some(id);
                        self.open_page(None);
                        self.select_first_page();
                    }
                }
                "notebook" => {
                    if let Some(ni) = self.notebooks.iter().position(|nb| nb.meta.id == id) {
                        self.cur_nb = Some(ni);
                        self.cur_section = None;
                        self.open_page(None);
                        self.select_first_page();
                    }
                }
                _ => {}
            }
        } else if let Some(d) = url.strip_prefix("date:") {
            self.set_status(format!("Дата: {d}"));
        } else if self.settings.offline_only && (url.starts_with("http://") || url.starts_with("https://")) {
            // Opening a link hands it to the browser; the app itself stays offline.
            crate::winapi::shell_open(url);
        } else {
            crate::winapi::shell_open(url);
        }
    }

    // ---- background jobs ---------------------------------------------------------

    fn start_ocr_image(&mut self, item: Id) {
        let Some(pid) = self.cur_page else { return };
        let Some(ni) = self.nb_index_of_page(pid) else { return };
        let Some(ImageItem { asset, rotation, flip_h, flip_v, .. }) = self.pages.get(&pid).and_then(|p| p.item(item)).and_then(|i| match &i.kind {
            ItemKind::Image(im) => Some(im.clone()),
            _ => None,
        }) else {
            return;
        };
        let path = self.notebooks[ni].asset_path(&asset);
        let tx = self.bg_tx.clone();
        self.set_status("Распознавание текста…");
        std::thread::spawn(move || {
            let text = crate::textures::load_oriented(&path, rotation, flip_h, flip_v)
                .ok_or_else(|| "Не удалось открыть изображение".to_string())
                .and_then(|img| crate::winapi::ocr(&img.to_rgba8(), None));
            let _ = tx.send(BgResult::Ocr { page: pid, item: Some(item), text });
        });
    }

    fn start_ocr_ink(&mut self) {
        let Some(pid) = self.cur_page else { return };
        let strokes: Vec<Stroke> = self
            .with_page(|page, ed| {
                if ed.selection.strokes.is_empty() {
                    page.strokes.clone()
                } else {
                    ed.selection.strokes.iter().filter_map(|&i| page.strokes.get(i).cloned()).collect()
                }
            })
            .unwrap_or_default();
        if strokes.is_empty() {
            self.set_status("На странице нет рукописных записей");
            return;
        }
        let tx = self.bg_tx.clone();
        self.set_status("Распознавание рукописного текста…");
        std::thread::spawn(move || {
            let text = match crate::ink::rasterize(&strokes, 1.5) {
                Some((img, _)) => {
                    let rgba = image::DynamicImage::ImageLuma8(img).to_rgba8();
                    crate::winapi::ocr(&rgba, None)
                }
                None => Err("Пустой рисунок".into()),
            };
            let _ = tx.send(BgResult::Ocr { page: pid, item: None, text });
        });
    }

    fn start_pdf_printout(&mut self, item: Id) {
        let Some(pid) = self.cur_page else { return };
        let Some(ni) = self.nb_index_of_page(pid) else { return };
        let Some((asset, pos, h)) = self.pages.get(&pid).and_then(|p| p.item(item)).and_then(|i| match &i.kind {
            ItemKind::File(f) => Some((f.asset.clone(), i.pos, i.height)),
            _ => None,
        }) else {
            return;
        };
        let path = self.notebooks[ni].asset_path(&asset);
        self.pdf_printout_from(path, [pos[0], pos[1] + h + 20.0]);
    }

    pub fn pdf_printout_from(&mut self, path: PathBuf, at: [f32; 2]) {
        let Some(pid) = self.cur_page else { return };
        let Some(ni) = self.nb_index_of_page(pid) else { return };
        let nb = self.notebooks[ni].path.clone();
        let tx = self.bg_tx.clone();
        self.set_status("Создание печатной копии PDF…");
        std::thread::spawn(move || {
            // Windows.Data.Pdf needs a real .pdf extension.
            let tmp = Settings::temp_dir().join(format!("{}.pdf", new_id()));
            let _ = std::fs::create_dir_all(tmp.parent().unwrap());
            let src = if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf")) {
                path.clone()
            } else {
                let _ = std::fs::copy(&path, &tmp);
                tmp.clone()
            };
            let images = crate::winapi::render_pdf(&src, 1400);
            let _ = std::fs::remove_file(&tmp);
            let _ = tx.send(BgResult::Pdf { page: pid, at, images, nb });
        });
    }

    fn poll_background(&mut self, ctx: &Context) {
        while let Ok(r) = self.bg_rx.try_recv() {
            match r {
                BgResult::Ocr { page, item, text } => match text {
                    Ok(t) => {
                        self.set_status("Распознавание завершено");
                        if let Some(it) = item {
                            if let Some(i) = self.pages.get_mut(&page).and_then(|p| p.item_mut(it)) {
                                if let ItemKind::Image(im) = &mut i.kind {
                                    im.ocr_text = Some(t.clone());
                                }
                            }
                            self.mark_dirty(page);
                            self.dialogs.push(Dialog::OcrResult { title: "Текст с изображения".into(), text: t, from_ink: false });
                        } else {
                            if let Some(p) = self.pages.get_mut(&page) {
                                p.ink_text = t.clone();
                            }
                            self.mark_dirty(page);
                            self.dialogs.push(Dialog::OcrResult { title: "Распознанный рукописный текст".into(), text: t, from_ink: true });
                        }
                    }
                    Err(e) => self.set_status(format!("Ошибка распознавания: {e}. Проверьте, что установлен языковой пакет с OCR.")),
                },
                BgResult::Pdf { page, at, images, nb } => match images {
                    Ok(imgs) => {
                        let Some(ni) = self.notebooks.iter().position(|n| n.path == nb) else { continue };
                        let author = self.settings.author.clone();
                        let mut y = at[1];
                        let mut items: Vec<Item> = Vec::new();
                        for png in imgs {
                            let (w, h) = image::load_from_memory(&png).map(|i| (i.width(), i.height())).unwrap_or((1400, 1980));
                            let asset = self.notebooks[ni].import_asset(&png, "png");
                            let width = 760.0;
                            let it = Item::new(egui::pos2(at[0], y), width, ItemKind::Image(ImageItem { asset, crop: full_crop(), ..Default::default() }), &author);
                            y += width * h as f32 / w.max(1) as f32 + 16.0;
                            items.push(it);
                        }
                        let n = items.len();
                        if std::mem::take(&mut self.underlay_next) {
                            for it in items.iter_mut() {
                                it.underlay = true;
                            }
                        }
                        self.ensure_loaded(page);
                        if let (Some(p), Some(ed)) = (self.pages.get_mut(&page), self.editors.get_mut(&page)) {
                            ed.snapshot(p);
                            p.items.extend(items);
                        }
                        self.mark_dirty(page);
                        self.set_status(format!("Вставлено страниц: {n}"));
                    }
                    Err(e) => self.set_status(format!("Не удалось прочитать PDF: {e}")),
                },
                BgResult::Summary(rows) => {
                    self.summary = rows;
                }
                BgResult::Math { page, text } => match text {
                    Ok(t) => {
                        let expr: String = t.lines().collect::<Vec<_>>().join(" ").replace(['×', '•'], "*").replace(['—', '–'], "-").replace('÷', "/");
                        if self.cur_page == Some(page) {
                            self.insert_item(ItemKind::Equation(expr), 420.0);
                            self.with_page(|_, ed| ed.editing = None);
                            self.set_status("Формула вставлена — щёлкните, чтобы исправить");
                        }
                    }
                    Err(e) => self.set_status(format!("Ошибка распознавания: {e}")),
                },
            }
            ctx.request_repaint();
        }
    }

    pub fn collect_summary(&self, ctx: &Context) {
        // Pages from disk plus unsaved in-memory versions.
        let mut jobs: Vec<(PathBuf, String, String, PageMeta, PathBuf, Option<CKey>, Option<Page>)> = Vec::new();
        for nb in &self.notebooks {
            if nb.sealed {
                continue;
            }
            for s in nb.meta.sections() {
                if !nb.section_accessible(s.id) {
                    continue;
                }
                for p in &s.pages {
                    jobs.push((nb.path.clone(), nb.meta.name.clone(), s.name.clone(), p.clone(), nb.page_path(p.id), nb.key_for_page(p.id).cloned(), self.pages.get(&p.id).cloned()));
                }
            }
        }
        let tx = self.bg_tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let mut rows = Vec::new();
            for (nb, nb_name, sec, pm, path, key, mem) in jobs {
                let page = mem.or_else(|| std::fs::read(&path).ok().and_then(|d| crate::storage::decode_page(&d, key.as_ref()).ok()));
                let Some(page) = page else { continue };
                for it in &page.items {
                    if it.tags.is_empty() && it.task.is_none() {
                        continue;
                    }
                    let text: String = it.plain_text().chars().take(200).collect();
                    rows.push(SummaryRow {
                        nb: nb.clone(),
                        nb_name: nb_name.clone(),
                        section: sec.clone(),
                        page: pm.id,
                        page_title: pm.title.clone(),
                        item: it.id,
                        tags: it.tags.clone(),
                        task: it.task.clone(),
                        text,
                        modified: it.modified,
                    });
                }
            }
            let _ = tx.send(BgResult::Summary(rows));
            ctx.request_repaint();
        });
    }

    fn check_tasks(&mut self) {
        let now_t = now();
        let mut fire: Vec<(Id, Id, String)> = Vec::new();
        for row in &self.summary {
            if let Some(t) = &row.task {
                if !t.done && !t.notified && t.due.is_some_and(|d| d <= now_t) {
                    fire.push((row.page, row.item, row.text.clone()));
                }
            }
        }
        for (page, item, text) in fire {
            crate::winapi::notify("Срок задачи", &text);
            if self.ensure_loaded(page) {
                if let Some(i) = self.pages.get_mut(&page).and_then(|p| p.item_mut(item)) {
                    if let Some(t) = i.task.as_mut() {
                        t.notified = true;
                    }
                }
                self.mark_dirty(page);
            }
            for r in self.summary.iter_mut().filter(|r| r.item == item) {
                if let Some(t) = r.task.as_mut() {
                    t.notified = true;
                }
            }
        }
    }

    pub fn run_backups(&mut self, force: bool) {
        if !self.settings.backup_enabled && !force {
            return;
        }
        self.save_all();
        let dir = self.settings.backup_dir.clone();
        let interval = self.settings.backup_hours.max(1) as u64 * 3600;
        let key = if self.settings.backup_encrypt { self.backup_key.clone() } else { None };
        if self.settings.backup_encrypt && key.is_none() {
            if force {
                self.dialogs.push(Dialog::BackupPassword(String::new()));
            }
            return;
        }
        let mut done = 0;
        for nb in &self.notebooks {
            let last = crate::storage::list_backups(&dir, &nb.meta.name).first().and_then(|p| std::fs::metadata(p).ok()).and_then(|m| m.modified().ok());
            let stale = last.is_none_or(|t| t.elapsed().map(|e| e.as_secs() >= interval).unwrap_or(true));
            if force || stale {
                let (path, name, keep) = (nb.path.clone(), nb.meta.name.clone(), self.settings.backup_keep);
                let d = dir.clone();
                let k = key.clone();
                std::thread::spawn(move || {
                    let _ = crate::storage::backup_notebook(&path, &name, &d, keep, k.as_ref());
                });
                done += 1;
            }
        }
        if force {
            self.set_status(format!("Резервное копирование запущено: {done} блокн."));
        }
    }

    fn poll_inbox(&mut self) {
        let dir = crate::winapi::inbox_dir();
        let Ok(rd) = std::fs::read_dir(&dir) else { return };
        for e in rd.flatten() {
            if let Ok(s) = std::fs::read_to_string(e.path()) {
                for l in s.lines() {
                    if l.trim() == "__focus__" {
                        self.focus_request = true;
                    } else if !l.trim().is_empty() {
                        self.pending_add.push(PathBuf::from(l.trim()));
                    }
                }
            }
            let _ = std::fs::remove_file(e.path());
        }
    }

    /// Files passed via "Add to notebook": a page in "Быстрые заметки" each.
    fn process_pending_add(&mut self, ctx: &Context) {
        if self.pending_add.is_empty() {
            return;
        }
        let files = std::mem::take(&mut self.pending_add);
        let pid = self.quick_notes_page("Добавленные файлы");
        if pid.is_some() {
            let list = files
                .into_iter()
                .map(|p| {
                    let name = p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                    (Some(p), None, name)
                })
                .collect();
            self.import_files(list, None);
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
    }

    /// Creates a new page in the "Быстрые заметки" section of the first notebook.
    pub fn quick_notes_page(&mut self, title: &str) -> Option<Id> {
        if self.notebooks.is_empty() {
            return None;
        }
        let ni = self.cur_nb.unwrap_or(0).min(self.notebooks.len() - 1);
        let nb = &mut self.notebooks[ni];
        let sid = match nb.meta.sections().iter().find(|s| s.name == "Быстрые заметки").map(|s| s.id) {
            Some(s) => s,
            None => {
                let s = Section::new("Быстрые заметки", 5);
                let id = s.id;
                nb.meta.tree.insert(0, Node::Section(s));
                id
            }
        };
        let pm = PageMeta::new(title, &self.settings.author);
        let pid = pm.id;
        nb.meta.section_mut(sid)?.pages.insert(0, pm);
        nb.dirty = true;
        self.cur_nb = Some(ni);
        self.cur_section = Some(sid);
        self.open_page(Some(pid));
        self.mark_dirty(pid);
        Some(pid)
    }

    fn handle_shortcuts(&mut self, ctx: &Context) {
        let sc = |m: Modifiers, k: Key| KeyboardShortcut::new(m, k);
        let ctrl = Modifiers::COMMAND;
        let ctrl_alt = Modifiers::COMMAND | Modifiers::ALT;
        let ctrl_alt_shift = Modifiers::COMMAND | Modifiers::ALT | Modifiers::SHIFT;
        let alt_shift = Modifiers::ALT | Modifiers::SHIFT;
        let mut consume = |s: KeyboardShortcut| ctx.input_mut(|i| i.consume_shortcut(&s));

        if consume(sc(ctrl_alt_shift, Key::N)) {
            self.new_page(true);
        }
        if consume(sc(ctrl, Key::N)) {
            self.new_page(false);
        }
        if consume(sc(ctrl, Key::T)) {
            self.new_section();
        }
        if consume(sc(ctrl, Key::M)) {
            self.pin_current(false);
        }
        if consume(sc(ctrl, Key::F)) {
            if let Some(ed) = self.cur_page.and_then(|p| self.editors.get_mut(&p)) {
                ed.find.open = true;
            }
        }
        if consume(sc(ctrl, Key::E)) {
            self.right = Some(RightPanel::Search);
            self.search_focus = true;
        }
        if consume(sc(ctrl_alt, Key::L)) {
            self.lock_all();
        }
        if consume(sc(ctrl, Key::P)) {
            self.dialogs.push(Dialog::Print(PrintOpts::default()));
        }
        if consume(sc(ctrl, Key::S)) {
            self.save_all();
            self.set_status("Сохранено");
        }
        // Text formatting.
        let fmt: &[(KeyboardShortcut, FormatCmd)] = &[
            (sc(ctrl, Key::B), FormatCmd::Wrap("**", "**")),
            (sc(ctrl, Key::I), FormatCmd::Wrap("*", "*")),
            (sc(ctrl, Key::U), FormatCmd::Wrap("__", "__")),
            (sc(ctrl, Key::Minus), FormatCmd::Wrap("~~", "~~")),
            (sc(ctrl, Key::Period), FormatCmd::LinePrefix("- ".into())),
            (sc(ctrl, Key::Slash), FormatCmd::LinePrefix("1. ".into())),
            (sc(ctrl_alt, Key::Num1), FormatCmd::LinePrefix("# ".into())),
            (sc(ctrl_alt, Key::Num2), FormatCmd::LinePrefix("## ".into())),
            (sc(ctrl_alt, Key::Num3), FormatCmd::LinePrefix("### ".into())),
            (sc(Modifiers::COMMAND | Modifiers::SHIFT, Key::Equals), FormatCmd::Wrap("^", "^")),
            (sc(ctrl, Key::Equals), FormatCmd::Wrap(",,", ",,")),
            (sc(ctrl, Key::Space), FormatCmd::Clear),
        ];
        for (s, cmd) in fmt {
            if consume(*s) {
                self.format(ctx, cmd.clone());
            }
        }
        if consume(sc(ctrl, Key::K)) {
            self.dialogs.push(Dialog::Link { text: String::new(), url: String::new() });
        }
        if consume(sc(alt_shift, Key::D)) {
            self.insert_text_at_cursor(ctx, chrono::Local::now().format("%d.%m.%Y").to_string());
        }
        if consume(sc(alt_shift, Key::T)) {
            self.insert_text_at_cursor(ctx, chrono::Local::now().format("%H:%M").to_string());
        }
        if consume(sc(alt_shift, Key::F)) {
            self.insert_text_at_cursor(ctx, chrono::Local::now().format("%d.%m.%Y %H:%M").to_string());
        }
        // Tags Ctrl+1..9
        let digits = [Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6, Key::Num7, Key::Num8, Key::Num9];
        for (i, k) in digits.iter().enumerate() {
            if consume(sc(ctrl, *k)) {
                let d = (i + 1) as u8;
                if let Some(t) = self.settings.tags.iter().find(|t| t.shortcut == Some(d)).cloned() {
                    self.toggle_tag_current(&t);
                }
            }
        }
        // Undo / redo (text fields have their own undo while editing text).
        let text_focus = ctx.memory(|m| m.focused().is_some()) && ctx.egui_wants_keyboard_input();
        if !text_focus {
            self.drawing_shortcuts(ctx);
            if consume(sc(ctrl, Key::Z)) {
                if let Some(p) = self.cur_page {
                    if self.with_page(|page, ed| ed.undo(page)).unwrap_or(false) {
                        self.mark_dirty(p);
                    }
                }
            }
            if consume(sc(ctrl, Key::Y)) || consume(sc(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z)) {
                if let Some(p) = self.cur_page {
                    if self.with_page(|page, ed| ed.redo(page)).unwrap_or(false) {
                        self.mark_dirty(p);
                    }
                }
            }
            let no_copy = self.current_no_copy();
            if consume(sc(ctrl, Key::D)) {
                if let Some(p) = self.cur_page {
                    if self.with_page(|page, ed| {
                        let c = ed.copy_selection(page);
                        ed.paste(page, &c, None);
                    }).is_some() {
                        self.mark_dirty(p);
                    }
                }
            }
            if ctx.input(|i| i.key_pressed(Key::Delete)) {
                if let Some(p) = self.cur_page {
                    if self.with_page(|page, ed| {
                        let had = !ed.selection.is_empty();
                        ed.delete_selection(page);
                        had
                    }) == Some(true) {
                        self.mark_dirty(p);
                    }
                }
            }
            if ctx.input(|i| i.key_pressed(Key::Escape)) {
                self.with_page(|_, ed| {
                    ed.selection = Default::default();
                    ed.live_cancel();
                });
                if self.presentation {
                    self.presentation = false;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
                }
            }
            // Copy / cut / paste of canvas selections.
            let events: Vec<egui::Event> = ctx.input(|i| i.events.clone());
            for e in events {
                match e {
                    egui::Event::Copy | egui::Event::Cut if !no_copy => {
                        let cut = matches!(e, egui::Event::Cut);
                        if let Some(p) = self.cur_page {
                            let clip = self.with_page(|page, ed| {
                                if ed.selection.is_empty() {
                                    return None;
                                }
                                let c = ed.copy_selection(page);
                                if cut {
                                    ed.delete_selection(page);
                                }
                                Some(c)
                            });
                            if let Some(Some(c)) = clip {
                                let text: String = c.items.iter().map(|i| i.plain_text()).collect::<Vec<_>>().join("\n");
                                self.clip = c;
                                ctx.copy_text(if text.is_empty() { " ".into() } else { text });
                                if cut {
                                    self.mark_dirty(p);
                                }
                            }
                        }
                    }
                    egui::Event::Paste(text) => self.paste_on_canvas(ctx, Some(text)),
                    _ => {}
                }
            }
            if consume(sc(ctrl, Key::V)) {
                self.paste_on_canvas(ctx, None);
            }
        }
        if ctx.input(|i| i.key_pressed(Key::F11)) {
            self.focus_mode = !self.focus_mode;
        }
        if ctx.input(|i| i.key_pressed(Key::F5)) {
            self.presentation = !self.presentation;
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.presentation));
        }
    }

    /// Drawing hotkeys from the specification (only when no text field is focused).
    fn drawing_shortcuts(&mut self, ctx: &Context) {
        use egui::Key as K;
        let Some(pid) = self.cur_page else { return };
        let none = Modifiers::NONE;
        let pressed = |m: Modifiers, k: K| ctx.input_mut(|i| i.consume_key(m, k));
        let pens = self.settings.pens().to_vec();
        let mut save = false;
        let mut dirty = false;
        let mut smart = false;
        let mut grid = false;
        {
            let (Some(ed), Some(page)) = (self.editors.get_mut(&pid), self.pages.get_mut(&pid)) else { return };
            if ed.editing.is_some() {
                return;
            }
            if pressed(Modifiers::ALT, K::Num1) {
                ed.tool = Tool::Select;
            }
            if pressed(Modifiers::ALT, K::Num2) {
                let p = ed.pen.clone();
                ed.set_pen(&p, ed.pen_slot);
            }
            if pressed(none, K::E) {
                ed.tool = Tool::Eraser;
            }
            if pressed(none, K::L) {
                ed.tool = Tool::Lasso;
            }
            if pressed(none, K::R) {
                ed.ruler = if ed.ruler.is_some() { None } else { Some(crate::canvas::Ruler { center: ed.view_center(), angle: 0.0, kind: crate::canvas::RulerKind::Ruler }) };
            }
            if pressed(none, K::G) {
                grid = true;
            }
            if pressed(Modifiers::SHIFT, K::S) {
                smart = true;
            }
            let digits = [K::Num1, K::Num2, K::Num3, K::Num4, K::Num5, K::Num6, K::Num7, K::Num8, K::Num9];
            for (i, k) in digits.iter().enumerate() {
                if pressed(none, *k) {
                    if let Some(p) = pens.get(i) {
                        ed.set_pen(p, Some(i));
                    }
                }
            }
            if pressed(none, K::OpenBracket) {
                if ed.tool == Tool::Eraser {
                    ed.eraser_radius = (ed.eraser_radius / 1.25).max(1.0);
                } else {
                    ed.pen.width = (ed.pen.width / 1.2).max(0.5);
                }
            }
            if pressed(none, K::CloseBracket) {
                if ed.tool == Tool::Eraser {
                    ed.eraser_radius = (ed.eraser_radius * 1.25).min(100.0);
                } else {
                    ed.pen.width = (ed.pen.width * 1.2).min(24.0 * crate::settings::MM);
                }
            }
            if pressed(Modifiers::COMMAND, K::A) {
                ed.select_all(page);
            }
            if pressed(Modifiers::COMMAND | Modifiers::SHIFT, K::G) {
                ed.snapshot(page);
                crate::selection::group(page, ed, false);
                dirty = true;
            } else if pressed(Modifiers::COMMAND, K::G) {
                ed.snapshot(page);
                crate::selection::group(page, ed, true);
                dirty = true;
            }
            if pressed(Modifiers::COMMAND | Modifiers::ALT, K::R) {
                let r = ed.last_rect;
                ed.rotate_by(r, r.center(), std::f32::consts::FRAC_PI_2);
            }
            if let Some(st) = ed.status.take() {
                self.status = (st, now_secs());
            }
        }
        if smart {
            self.settings.auto_shapes = !self.settings.auto_shapes;
            self.set_status(if self.settings.auto_shapes { "Умные фигуры включены" } else { "Умные фигуры выключены" });
            save = true;
        }
        if grid {
            self.settings.snap_grid = !self.settings.snap_grid;
            self.set_status(if self.settings.snap_grid { "Привязка к сетке включена" } else { "Привязка к сетке выключена" });
            save = true;
        }
        if save {
            self.settings.save();
        }
        if dirty {
            self.mark_dirty(pid);
        }
    }

    pub fn paste_on_canvas(&mut self, ctx: &Context, text: Option<String>) {
        let Some(pid) = self.cur_page else { return };
        if self.locked_by.is_some() {
            return;
        }
        // Our own clipboard wins when it was the last thing copied.
        let ours = !self.clip.items.is_empty() || !self.clip.strokes.is_empty();
        let sys_text = text.clone().or_else(|| arboard::Clipboard::new().ok().and_then(|mut c| c.get_text().ok()));
        let clip_text: String = self.clip.items.iter().map(|i| i.plain_text()).collect::<Vec<_>>().join("\n");
        if ours && sys_text.as_deref().is_none_or(|t| t == clip_text || t == " ") {
            let clip = self.clip.clone();
            let at = ctx.pointer_hover_pos().and_then(|_| None);
            self.with_page(|page, ed| ed.paste(page, &clip, at));
            self.mark_dirty(pid);
            return;
        }
        // Image on the clipboard.
        if let Ok(mut cb) = arboard::Clipboard::new() {
            if let Ok(img) = cb.get_image() {
                if let Some(buf) = image::RgbaImage::from_raw(img.width as u32, img.height as u32, img.bytes.into_owned()) {
                    let mut png = Vec::new();
                    if image::DynamicImage::ImageRgba8(buf).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).is_ok() {
                        self.import_files(vec![(None, Some(png.into()), "clipboard.png".into())], None);
                        return;
                    }
                }
            }
        }
        if let Some(t) = sys_text {
            if t.trim().is_empty() {
                return;
            }
            let t = match self.settings.paste_mode {
                crate::settings::PasteMode::TextOnly => crate::richtext::strip_markup(&t),
                _ => t,
            };
            let size = self.settings.default_size;
            let font = self.settings.default_font.clone();
            self.insert_item(ItemKind::Text(TextBlock::new(&t, size, &font)), 480.0);
            self.with_page(|_, ed| ed.editing = None);
        }
    }

    pub fn current_no_copy(&self) -> bool {
        let Some(pid) = self.cur_page else { return false };
        let Some(nb) = self.nb_index_of_page(pid).map(|i| &self.notebooks[i]) else { return false };
        nb.meta.section_of_page(pid).and_then(|s| nb.meta.section(s)).and_then(|s| s.lock.as_ref()).is_some_and(|l| l.no_copy)
    }

    pub fn toggle_tag_current(&mut self, t: &TagDef) {
        let Some(pid) = self.cur_page else { return };
        let done = self.with_page(|page, ed| {
            let targets: Vec<Id> = match ed.editing {
                Some(e) => vec![e],
                None => ed.selection.items.clone(),
            };
            if targets.is_empty() {
                return false;
            }
            ed.snapshot(page);
            for id in targets {
                if let Some(it) = page.item_mut(id) {
                    items::toggle_tag(it, t);
                }
            }
            true
        });
        if done == Some(true) {
            self.mark_dirty(pid);
        } else {
            self.set_status("Выберите абзац, чтобы поставить тег");
        }
    }

    pub fn pin_current(&mut self, on_top: bool) {
        let Some(pid) = self.cur_page else { return };
        let Some(ni) = self.nb_index_of_page(pid) else { return };
        let mut ed = Editor::new(pid);
        ed.tool = Tool::Select;
        self.pinned.push(PinnedWin {
            id: egui::ViewportId::from_hash_of(("pin", pid, self.pinned.len())),
            nb: self.notebooks[ni].path.clone(),
            page: pid,
            on_top,
            editor: ed,
            open: true,
        });
    }

    fn periodic(&mut self, ctx: &Context) {
        let t = now_secs();
        // Autosave.
        if !self.dirty.is_empty() && t - self.dirty_since >= self.settings.autosave_secs.max(1) as f64 {
            let ids: Vec<Id> = self.dirty.iter().copied().collect();
            for id in ids {
                let version_due = self.last_version.get(&id).is_none_or(|lt| t - lt >= self.settings.version_minutes.max(1) as f64 * 60.0);
                self.flush_page(id, version_due);
            }
            self.dirty_since = 0.0;
        }
        for nb in self.notebooks.iter_mut() {
            if nb.dirty {
                nb.save_meta();
                nb.dirty = false;
            }
        }
        // Idle lock.
        let any_input = ctx.input(|i| !i.events.is_empty() || i.pointer.is_moving());
        if any_input || self.last_input == 0.0 {
            self.last_input = t;
        }
        if self.settings.idle_lock_minutes > 0 && t - self.last_input > self.settings.idle_lock_minutes as f64 * 60.0 {
            let any_unlocked = self.notebooks.iter().any(|nb| nb.meta.sections().iter().any(|s| s.lock.is_some() && nb.keys.contains_key(&s.id)) || (nb.lock.is_some() && !nb.sealed));
            if any_unlocked {
                self.lock_all();
            }
            self.last_input = t;
        }
        // Shared-folder lock refresh.
        if t - self.last_lock_refresh > 30.0 {
            self.last_lock_refresh = t;
            if let (Some(pid), true) = (self.cur_page, self.locked_by.is_none()) {
                if let Some(nb) = self.nb() {
                    nb.acquire_page_lock(pid, &self.settings.author);
                }
            } else if let Some(pid) = self.cur_page {
                // Re-check if the other user finished.
                let other = self.nb().and_then(|nb| nb.page_locked_by(pid, &self.settings.author));
                if other.is_none() {
                    // Reload the page to pick up their changes.
                    self.pages.remove(&pid);
                    self.ensure_loaded(pid);
                    if let Some(nb) = self.nb() {
                        nb.acquire_page_lock(pid, &self.settings.author);
                    }
                    self.set_status("Страница освободилась и перезагружена");
                }
                self.locked_by = other;
            }
        }
        // Backups (checked every 10 minutes).
        if t - self.last_backup_check > 600.0 {
            self.last_backup_check = t;
            self.run_backups(false);
        }
        // Tasks (summary refresh every 2 minutes, notifications every 30 s).
        if t - self.last_task_check > 30.0 {
            if (t - self.last_task_check) > 120.0 || self.summary.is_empty() {
                self.collect_summary(ctx);
            }
            self.check_tasks();
            self.last_task_check = t;
        }
        // "Send to notes" requests from other processes.
        if t - self.last_inbox_check > 2.0 {
            self.last_inbox_check = t;
            self.poll_inbox();
        }
        // Screenshot from the snipping tool: wait for a new clipboard image.
        if let Some((started, prev_hash)) = self.clipboard_watch {
            if t - started > 90.0 {
                self.clipboard_watch = None;
            } else if let Ok(mut cb) = arboard::Clipboard::new() {
                if let Ok(img) = cb.get_image() {
                    let h = crate::storage::fnv1a(&img.bytes[..img.bytes.len().min(65536)]);
                    if h != prev_hash {
                        self.clipboard_watch = None;
                        self.paste_on_canvas(ctx, Some(String::new()));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    }
                }
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(400));
        }
        ctx.request_repaint_after(std::time::Duration::from_secs(2));
    }

    pub fn start_screenshot(&mut self, ctx: &Context) {
        let prev = arboard::Clipboard::new()
            .ok()
            .and_then(|mut c| c.get_image().ok())
            .map(|i| crate::storage::fnv1a(&i.bytes[..i.bytes.len().min(65536)]))
            .unwrap_or(0);
        self.clipboard_watch = Some((now_secs(), prev));
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        crate::winapi::start_screen_clip();
        self.set_status("Выделите область экрана — снимок будет вставлен на страницу");
    }

    pub fn hotkey_quick_note(&mut self, ctx: &Context) {
        if std::mem::take(&mut self.focus_request) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
        while self.hotkey_rx.try_recv().is_ok() {
            self.quick_note = Some(String::new());
            ctx.request_repaint();
        }
    }

    // ---- export pipeline -----------------------------------------------------------

    pub fn start_export(&mut self, kind: ExportKind, path: PathBuf, opts: PrintOpts) {
        let Some(pid) = self.cur_page else { return };
        let queue = if opts.whole_section {
            let sec = self.nb().and_then(|nb| self.cur_section.and_then(|s| nb.meta.section(s)));
            sec.map(|s| sorted_pages(s).iter().map(|p| p.id).collect()).unwrap_or_else(|| vec![pid])
        } else {
            vec![pid]
        };
        self.export = Some(ExportJob { kind, queue, images: Vec::new(), path, opts, started: false });
    }

    fn drive_export(&mut self, ctx: &Context) {
        let Some(job) = self.export.as_mut() else { return };
        if job.queue.is_empty() {
            let job = self.export.take().unwrap();
            self.finish_export(ctx, job);
            return;
        }
        let pid = job.queue[0];
        let header = job.opts.header;
        if self.cur_page != Some(pid) {
            self.open_page(Some(pid));
            return;
        }
        let title = self.nb().and_then(|nb| nb.meta.page_meta(pid)).map(|p| p.title.clone()).unwrap_or_default();
        let sens = self.pages.get(&pid).map(|p| p.sensitivity).unwrap_or_default();
        let Some(page) = self.pages.get(&pid) else {
            self.export.as_mut().unwrap().queue.remove(0);
            return;
        };
        let region = canvas::export_region(page);
        let Some(ed) = self.editors.get_mut(&pid) else { return };
        let job = self.export.as_mut().unwrap();
        if !job.started {
            let mut head = if header { title.clone() } else { String::new() };
            if sens != Sensitivity::None {
                head = format!("{head}   [{}]", sens.label());
            }
            let region = if head.is_empty() { region } else { egui::Rect::from_min_max(region.min - egui::vec2(0.0, 34.0), region.max) };
            ed.start_capture(region, 1.0, head);
            job.started = true;
        } else if ed.capture.as_ref().is_some_and(|c| c.done) {
            let cap = ed.capture.take().unwrap();
            if let Some(img) = cap.image {
                job.images.push(img);
            }
            job.queue.remove(0);
            job.started = false;
        }
        ctx.request_repaint();
    }

    fn finish_export(&mut self, _ctx: &Context, job: ExportJob) {
        let path = job.path.clone();
        let res: Result<(), String> = match job.kind {
            ExportKind::Png | ExportKind::Jpg => {
                let mut r = Ok(());
                for (i, img) in job.images.iter().enumerate() {
                    let p = if job.images.len() == 1 { path.clone() } else { path.with_file_name(format!("{}-{}.{}", path.file_stem().unwrap_or_default().to_string_lossy(), i + 1, path.extension().unwrap_or_default().to_string_lossy())) };
                    let out = if job.kind == ExportKind::Jpg { image::DynamicImage::ImageRgba8(img.clone()).to_rgb8().save(&p) } else { img.save(&p) };
                    if let Err(e) = out {
                        r = Err(e.to_string());
                    }
                }
                r
            }
            ExportKind::Pdf | ExportKind::Print => {
                let mut sheets = Vec::new();
                for img in &job.images {
                    let rgb = image::DynamicImage::ImageRgba8(img.clone()).to_rgb8();
                    sheets.extend(crate::export::slice_pages(&rgb, job.opts.landscape));
                }
                let opts = crate::export::PdfOptions { landscape: job.opts.landscape, margin_mm: job.opts.margins, scale: job.opts.scale, page_numbers: job.opts.numbers, header: String::new() };
                let data = crate::export::pdf_from_images(&sheets, &opts);
                if crate::storage::atomic_write(&path, &data) { Ok(()) } else { Err("Не удалось записать файл".into()) }
            }
        };
        match res {
            Ok(()) => {
                if job.kind == ExportKind::Print {
                    crate::winapi::shell_print(&path);
                    self.set_status("Документ отправлен на печать");
                } else {
                    self.set_status(format!("Экспортировано: {}", path.display()));
                }
            }
            Err(e) => self.set_status(format!("Ошибка экспорта: {e}")),
        }
    }

    // ---- quick note and pinned windows -----------------------------------------------

    fn viewports(&mut self, ctx: &Context) {
        if let Some(mut text) = self.quick_note.take() {
            let mut keep = true;
            let mut save = false;
            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("quick-note"),
                egui::ViewportBuilder::default()
                    .with_title("Быстрая заметка")
                    .with_inner_size([380.0, 300.0])
                    .with_always_on_top()
                    .with_active(true),
                |ui, _| {
                    egui::CentralPanel::default().show(ui, |ui| {
                        ui.label("Заметка попадёт в раздел «Быстрые заметки».");
                        let r = ui.add_sized(ui.available_size() - egui::vec2(0.0, 34.0), egui::TextEdit::multiline(&mut text).hint_text("Пишите…"));
                        if !r.has_focus() && text.is_empty() {
                            r.request_focus();
                        }
                        ui.horizontal(|ui| {
                            if ui.button("Сохранить").clicked() || ui.input(|i| i.modifiers.ctrl && i.key_pressed(Key::Enter)) {
                                save = true;
                            }
                            if ui.button("Отмена").clicked() {
                                keep = false;
                            }
                        });
                    });
                    if ui.ctx().input(|i| i.viewport().close_requested()) {
                        save = !text.trim().is_empty();
                        keep = false;
                    }
                },
            );
            if save && !text.trim().is_empty() {
                let title: String = text.lines().next().unwrap_or("Быстрая заметка").chars().take(50).collect();
                if let Some(pid) = self.quick_notes_page(&title) {
                    let size = self.settings.default_size;
                    let font = self.settings.default_font.clone();
                    let author = self.settings.author.clone();
                    if let Some(p) = self.pages.get_mut(&pid) {
                        p.items.push(Item::new(egui::pos2(0.0, 0.0), 480.0, ItemKind::Text(TextBlock::new(&text, size, &font)), &author));
                    }
                    self.mark_dirty(pid);
                    self.flush_page(pid, false);
                    self.set_status("Быстрая заметка сохранена");
                }
                keep = false;
            }
            if keep {
                self.quick_note = Some(text);
            }
        }
        // Pinned pages.
        let mut pinned = std::mem::take(&mut self.pinned);
        for w in pinned.iter_mut() {
            let pid = w.page;
            if !self.ensure_loaded(pid) {
                w.open = false;
                continue;
            }
            let title = self.notebooks.iter().find_map(|nb| nb.meta.page_meta(pid)).map(|p| p.title.clone()).unwrap_or_default();
            let Some(ni) = self.notebooks.iter().position(|n| n.path == w.nb) else {
                w.open = false;
                continue;
            };
            let mut builder = egui::ViewportBuilder::default().with_title(format!("📌 {title}")).with_inner_size([520.0, 600.0]);
            if w.on_top {
                builder = builder.with_always_on_top();
            }
            let mut changed = false;
            let mut actions = Vec::new();
            let highlights = canvas::Highlights::new();
            let dark = ctx.global_style().visuals.dark_mode;
            ctx.show_viewport_immediate(w.id, builder, |ui, _| {
                egui::Panel::top("pin_top").show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if ui.selectable_label(w.on_top, "Поверх окон").clicked() {
                            w.on_top = !w.on_top;
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::WindowLevel(if w.on_top { egui::WindowLevel::AlwaysOnTop } else { egui::WindowLevel::Normal }));
                        }
                        for (t, l) in [(Tool::Select, "Выбор"), (Tool::Draw, "Перо"), (Tool::Eraser, "Ластик")] {
                            if ui.selectable_label(w.editor.tool == t, l).clicked() {
                                w.editor.tool = t;
                            }
                        }
                    });
                });
                egui::CentralPanel::no_frame().show(ui, |ui| {
                    if let Some(page) = self.pages.get_mut(&pid) {
                        let mut env = canvas::Env {
                            settings: &self.settings,
                            nb: &self.notebooks[ni],
                            textures: &mut self.textures,
                            speller: &self.speller,
                            dark,
                            actions: &mut actions,
                            clip: &mut self.clip,
                            highlights: &highlights,
                            no_copy: false,
                        };
                        changed = canvas::show(ui, page, &mut w.editor, &mut env);
                    }
                });
                if ui.ctx().input(|i| i.viewport().close_requested()) {
                    w.open = false;
                }
            });
            if changed {
                self.mark_dirty(pid);
            }
            for a in actions {
                self.handle_action(ctx, a);
            }
        }
        pinned.retain(|w| w.open);
        self.pinned.extend(pinned);
    }
}

pub fn sorted_pages(s: &Section) -> Vec<&PageMeta> {
    let mut v: Vec<&PageMeta> = s.pages.iter().collect();
    match s.sort {
        PageSort::Manual => {}
        PageSort::Title => v.sort_by_key(|p| p.title.to_lowercase()),
        PageSort::Created => v.sort_by(|a, b| b.created.cmp(&a.created)),
        PageSort::Modified => v.sort_by(|a, b| b.modified.cmp(&a.modified)),
    }
    // Pinned on top; keep subpages after their parents only in manual order.
    if s.sort != PageSort::Manual {
        v.iter_mut().for_each(|_| {});
    }
    v.sort_by_key(|p| !p.pinned);
    v
}

pub fn now_secs() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

fn egui_extras_install(ctx: &Context) {
    ctx.options_mut(|o| {
        o.zoom_with_keyboard = true;
    });
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.textures.poll(&ctx);
        self.poll_background(&ctx);
        self.hotkey_quick_note(&ctx);
        self.process_pending_add(&ctx);
        if self.dialogs.is_empty() {
            self.handle_shortcuts(&ctx);
        }
        self.drive_export(&ctx);
        // Files dropped onto the window.
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        if !dropped.is_empty() {
            let at = ctx.pointer_hover_pos().and_then(|_| None);
            let files = dropped
                .into_iter()
                .map(|f| {
                    let p = f.path().to_path_buf();
                    let name = p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                    (Some(p), None, name)
                })
                .collect();
            self.import_files(files, at);
        }

        crate::ui_main::draw(self, ui);
        crate::ui_dialogs::draw(self, &ctx);
        self.viewports(&ctx);
        self.periodic(&ctx);

        if ctx.input(|i| i.viewport().close_requested()) {
            self.save_all();
            if let (Some(pid), Some(nb)) = (self.cur_page, self.nb()) {
                nb.release_page_lock(pid, &self.settings.author);
            }
            self.settings.open_notebooks = self.notebooks.iter().map(|n| n.path.clone()).collect();
            self.settings.save();
            if self.settings.clear_temp_on_exit {
                let _ = std::fs::remove_dir_all(Settings::temp_dir());
            }
        }
    }

    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        visuals.panel_fill.to_normalized_gamma_f32()
    }
}
