//! Application settings stored in %APPDATA%\NewNote\settings.json.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::model::{Effect, InkKind, TagDef, builtin_tags};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Theme {
    Light,
    Dark,
    #[default]
    System,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PasteMode {
    #[default]
    KeepFormatting,
    TextOnly,
    Merge,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PenPreset {
    pub name: String,
    pub color: [u8; 4],
    /// Width in canvas units (96 per inch).
    pub width: f32,
    #[serde(default)]
    pub highlighter: bool,
    #[serde(default)]
    pub favorite: bool,
    #[serde(default)]
    pub kind: InkKind,
    #[serde(default = "yes")]
    pub pressure: bool,
    #[serde(default = "one")]
    pub curve: f32,
    #[serde(default = "yes")]
    pub tilt: bool,
    /// 0..1, light by default.
    #[serde(default = "light")]
    pub smoothing: f32,
    /// 0..1, off by default.
    #[serde(default)]
    pub stabilizer: f32,
    #[serde(default)]
    pub effect: Effect,
    /// Highlighter goes under text (layer "Фон") instead of over it.
    #[serde(default = "yes")]
    pub under_text: bool,
}

fn yes() -> bool {
    true
}
fn one() -> f32 {
    1.0
}
fn light() -> f32 {
    0.35
}

pub const MM: f32 = 96.0 / 25.4;

impl PenPreset {
    pub fn new(name: &str, kind: InkKind, color: [u8; 4], width_mm: f32) -> Self {
        Self {
            name: name.into(),
            color,
            width: width_mm * MM,
            highlighter: kind == InkKind::Highlighter,
            favorite: true,
            kind,
            pressure: !matches!(kind, InkKind::Highlighter | InkKind::Ballpoint),
            curve: 1.0,
            tilt: true,
            smoothing: 0.35,
            stabilizer: 0.0,
            effect: Effect::None,
            under_text: true,
        }
    }
    /// Old presets only had the highlighter flag.
    pub fn brush(&self) -> InkKind {
        if self.highlighter && self.kind == InkKind::Pen { InkKind::Highlighter } else { self.kind }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PenSet {
    pub name: String,
    pub pens: Vec<PenPreset>,
}

pub fn builtin_pen_sets() -> Vec<PenSet> {
    use InkKind::*;
    let p = PenPreset::new;
    let hl = |name: &str, c: [u8; 3]| p(name, Highlighter, [c[0], c[1], c[2], 105], 4.5);
    vec![
        PenSet {
            name: "Рабочий".into(),
            pens: vec![
                p("Чёрное перо тонкое", Pen, [20, 20, 20, 255], 0.4),
                p("Чёрное перо", Pen, [20, 20, 20, 255], 0.7),
                p("Синее перо", Pen, [25, 80, 200, 255], 0.6),
                p("Красное перо", Pen, [210, 30, 30, 255], 0.6),
                p("Зелёное перо", Pen, [30, 140, 60, 255], 0.6),
                p("Фиолетовое перо", Pen, [120, 50, 190, 255], 0.6),
                p("Карандаш", Pencil, [60, 60, 60, 230], 0.5),
                hl("Жёлтый маркер", [255, 230, 0]),
                hl("Розовый маркер", [255, 110, 190]),
                hl("Зелёный маркер", [90, 230, 90]),
            ],
        },
        PenSet {
            name: "Учебный".into(),
            pens: vec![
                p("Синяя ручка", Ballpoint, [30, 60, 170, 255], 0.35),
                p("Чёрная ручка", Ballpoint, [25, 25, 25, 255], 0.35),
                p("Красная ручка", Ballpoint, [200, 30, 30, 255], 0.35),
                p("Зелёная ручка", Ballpoint, [30, 130, 60, 255], 0.35),
                p("Простой карандаш", Pencil, [70, 70, 70, 220], 0.45),
                p("Линер", Pen, [20, 20, 20, 255], 0.5),
                hl("Жёлтый маркер", [255, 230, 0]),
                hl("Голубой маркер", [80, 200, 255]),
                hl("Оранжевый маркер", [255, 160, 40]),
                hl("Зелёный маркер", [90, 230, 90]),
            ],
        },
        PenSet {
            name: "Эскиз".into(),
            pens: vec![
                p("Карандаш H", Pencil, [110, 110, 110, 200], 0.35),
                p("Карандаш HB", Pencil, [70, 70, 70, 230], 0.5),
                p("Карандаш 2B", Pencil, [35, 35, 35, 245], 0.8),
                p("Уголь", Oil, [30, 30, 30, 235], 2.2),
                p("Каллиграфия", Calligraphy, [15, 15, 15, 255], 1.6),
                p("Тонкий линер", Ballpoint, [15, 15, 15, 255], 0.25),
                p("Сепия", Pen, [112, 66, 20, 255], 0.8),
                p("Сангина", Pencil, [170, 70, 40, 235], 0.7),
                p("Баллончик", Spray, [40, 40, 40, 220], 3.0),
                p("Мягкая тень", Watercolor, [90, 90, 100, 160], 4.0),
            ],
        },
        PenSet {
            name: "Акварель".into(),
            pens: vec![
                p("Ультрамарин", Watercolor, [40, 70, 190, 200], 4.0),
                p("Кармин", Watercolor, [190, 30, 60, 200], 4.0),
                p("Охра", Watercolor, [205, 150, 50, 200], 4.0),
                p("Изумруд", Watercolor, [20, 140, 100, 200], 4.0),
                p("Сиена", Watercolor, [140, 80, 40, 200], 4.0),
                p("Сажа", Watercolor, [40, 40, 45, 200], 4.0),
                p("Масляная кисть", Oil, [180, 60, 40, 255], 3.0),
                p("Баллончик", Spray, [60, 120, 220, 220], 4.0),
                p("Тушь", Calligraphy, [20, 20, 20, 255], 1.2),
                p("Белила", Oil, [245, 245, 240, 255], 3.0),
            ],
        },
    ]
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SavedSearch {
    pub name: String,
    pub query: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub theme: Theme,
    pub ui_scale: f32,
    pub default_font: String,
    pub default_size: f32,
    pub notebooks_dir: PathBuf,
    pub open_notebooks: Vec<PathBuf>,
    pub autostart: bool,
    pub touch_mode: bool,
    pub author: String,

    // Input
    pub pen_draws: bool,
    pub touch_draws: bool,
    pub mouse_draws: bool,
    pub smoothing: f32,
    pub pressure_curve: f32,
    pub auto_shapes: bool,
    pub pen_presets: Vec<PenPreset>,
    pub pen_sets: Vec<PenSet>,
    pub active_set: usize,
    pub fav_colors: Vec<[u8; 4]>,
    pub scratch_out: bool,
    pub quick_shape: bool,
    pub palm_rejection: bool,
    pub touch_gestures: bool,
    pub snap_grid: bool,
    pub minimap: bool,
    pub auto_ink_to_text: bool,
    pub native_frame: bool,
    pub sections_on_top: bool,
    pub ribbon_full: bool,
    pub page_list_details: bool,
    pub pages_right: bool,
    pub last_ribbon: u8,
    pub status_bar: bool,

    // Editing
    pub autocorrect: bool,
    pub autocorrect_map: BTreeMap<String, String>,
    pub spellcheck: bool,
    pub spell_lang: String,
    pub user_dictionary: Vec<String>,
    pub paste_mode: PasteMode,
    pub auto_lists: bool,

    // Data
    pub autosave_secs: u32,
    pub backup_enabled: bool,
    pub backup_hours: u32,
    pub backup_keep: u32,
    pub backup_dir: PathBuf,
    pub backup_encrypt: bool,
    pub trash_days: u32,
    pub version_minutes: u32,
    pub version_keep: u32,
    pub version_days: u32,
    pub compress_images: bool,
    pub image_max_dim: u32,
    pub jpeg_quality: u8,

    // Privacy
    pub offline_only: bool,
    pub idle_lock_minutes: u32,
    pub clear_temp_on_exit: bool,

    pub tags: Vec<TagDef>,
    pub recent_searches: Vec<String>,
    pub saved_searches: Vec<SavedSearch>,

    // Reading
    pub tts_rate: i32,
    pub tts_volume: u16,
    pub tts_voice: String,
}

pub fn app_data_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("NewNote")
}

fn user_dir() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

impl Default for Settings {
    fn default() -> Self {
        let mut ac = BTreeMap::new();
        for (a, b) in [
            ("(c)", "©"),
            ("(r)", "®"),
            ("(tm)", "™"),
            ("--", "—"),
            ("->", "→"),
            ("<-", "←"),
            ("=>", "⇒"),
            ("...", "…"),
            ("+-", "±"),
            ("!=", "≠"),
        ] {
            ac.insert(a.to_owned(), b.to_owned());
        }
        Self {
            theme: Theme::System,
            ui_scale: 1.0,
            default_font: "Segoe UI".into(),
            default_size: 15.0,
            notebooks_dir: user_dir().join("Documents").join("NewNote"),
            open_notebooks: Vec::new(),
            autostart: false,
            touch_mode: false,
            author: std::env::var("USERNAME").unwrap_or_else(|_| "Автор".into()),
            pen_draws: true,
            touch_draws: false,
            mouse_draws: true,
            smoothing: 0.45,
            pressure_curve: 1.0,
            auto_shapes: false,
            pen_sets: builtin_pen_sets(),
            active_set: 0,
            fav_colors: vec![[20, 20, 20, 255], [25, 80, 200, 255], [210, 30, 30, 255], [30, 140, 60, 255]],
            scratch_out: true,
            quick_shape: true,
            palm_rejection: true,
            touch_gestures: true,
            snap_grid: false,
            minimap: true,
            auto_ink_to_text: false,
            native_frame: false,
            sections_on_top: false,
            ribbon_full: false,
            page_list_details: false,
            pages_right: false,
            last_ribbon: 0,
            status_bar: false,
            pen_presets: Vec::new(),
            autocorrect: true,
            autocorrect_map: ac,
            spellcheck: true,
            spell_lang: "ru-RU".into(),
            user_dictionary: Vec::new(),
            paste_mode: PasteMode::KeepFormatting,
            auto_lists: true,
            autosave_secs: 3,
            backup_enabled: true,
            backup_hours: 24,
            backup_keep: 7,
            backup_dir: app_data_dir().join("Backups"),
            backup_encrypt: false,
            trash_days: 60,
            version_minutes: 10,
            version_keep: 50,
            version_days: 90,
            compress_images: true,
            image_max_dim: 2560,
            jpeg_quality: 85,
            offline_only: true,
            idle_lock_minutes: 10,
            clear_temp_on_exit: true,
            tags: builtin_tags(),
            recent_searches: Vec::new(),
            saved_searches: Vec::new(),
            tts_rate: 0,
            tts_volume: 100,
            tts_voice: String::new(),
        }
    }
}

impl Settings {
    pub fn path() -> PathBuf {
        app_data_dir().join("settings.json")
    }

    pub fn load() -> Self {
        let mut s: Settings = std::fs::read(Self::path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        if s.pen_sets.is_empty() {
            s.pen_sets = builtin_pen_sets();
        }
        s.active_set = s.active_set.min(s.pen_sets.len() - 1);
        s
    }

    pub fn pens(&self) -> &[PenPreset] {
        &self.pen_sets[self.active_set.min(self.pen_sets.len().saturating_sub(1))].pens
    }

    pub fn pens_mut(&mut self) -> &mut Vec<PenPreset> {
        let i = self.active_set.min(self.pen_sets.len().saturating_sub(1));
        &mut self.pen_sets[i].pens
    }

    pub fn save(&self) {
        let _ = std::fs::create_dir_all(app_data_dir());
        if let Ok(b) = serde_json::to_vec_pretty(self) {
            crate::storage::atomic_write(&Self::path(), &b);
        }
    }

    pub fn tag(&self, id: u32) -> Option<&TagDef> {
        self.tags.iter().find(|t| t.id == id)
    }

    pub fn templates_dir() -> PathBuf {
        app_data_dir().join("templates")
    }

    pub fn temp_dir() -> PathBuf {
        std::env::temp_dir().join("NewNote")
    }
}
