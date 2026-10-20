//! Application settings stored in %APPDATA%\NewNote\settings.json.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::model::{TagDef, builtin_tags};

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
    pub width: f32,
    pub highlighter: bool,
    #[serde(default)]
    pub favorite: bool,
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
            pen_presets: vec![
                PenPreset { name: "Чёрная ручка".into(), color: [20, 20, 20, 255], width: 2.0, highlighter: false, favorite: true },
                PenPreset { name: "Синяя ручка".into(), color: [25, 80, 200, 255], width: 2.0, highlighter: false, favorite: true },
                PenPreset { name: "Красная ручка".into(), color: [210, 30, 30, 255], width: 2.0, highlighter: false, favorite: true },
                PenPreset { name: "Карандаш".into(), color: [90, 90, 90, 200], width: 1.2, highlighter: false, favorite: false },
                PenPreset { name: "Жёлтый маркер".into(), color: [255, 230, 0, 110], width: 14.0, highlighter: true, favorite: true },
                PenPreset { name: "Зелёный маркер".into(), color: [80, 230, 80, 110], width: 14.0, highlighter: true, favorite: false },
            ],
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
        std::fs::read(Self::path())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
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
