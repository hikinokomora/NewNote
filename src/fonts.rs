//! System font loading: each font gets regular / bold / italic / bold italic families.

use std::collections::HashSet;
use std::sync::{Arc, OnceLock, RwLock};

use egui::{FontData, FontDefinitions, FontFamily, FontId};

/// (display name, regular, bold, italic, bold italic) file names in C:\Windows\Fonts.
const FONTS: &[(&str, &str, &str, &str, &str)] = &[
    ("Segoe UI", "segoeui.ttf", "segoeuib.ttf", "segoeuii.ttf", "segoeuiz.ttf"),
    ("Arial", "arial.ttf", "arialbd.ttf", "ariali.ttf", "arialbi.ttf"),
    ("Calibri", "calibri.ttf", "calibrib.ttf", "calibrii.ttf", "calibriz.ttf"),
    ("Times New Roman", "times.ttf", "timesbd.ttf", "timesi.ttf", "timesbi.ttf"),
    ("Georgia", "georgia.ttf", "georgiab.ttf", "georgiai.ttf", "georgiaz.ttf"),
    ("Verdana", "verdana.ttf", "verdanab.ttf", "verdanai.ttf", "verdanaz.ttf"),
    ("Cambria", "cambria.ttc", "cambriab.ttf", "cambriai.ttf", "cambriaz.ttf"),
    ("Comic Sans MS", "comic.ttf", "comicbd.ttf", "comici.ttf", "comicz.ttf"),
    ("Consolas", "consola.ttf", "consolab.ttf", "consolai.ttf", "consolaz.ttf"),
];

static LOADED: OnceLock<RwLock<HashSet<String>>> = OnceLock::new();

fn loaded() -> &'static RwLock<HashSet<String>> {
    LOADED.get_or_init(Default::default)
}

pub fn font_names() -> Vec<String> {
    let l = loaded().read().unwrap();
    FONTS.iter().map(|f| f.0.to_string()).filter(|n| l.contains(n)).collect()
}

fn fonts_dir() -> std::path::PathBuf {
    std::env::var_os("WINDIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "C:\\Windows".into())
        .join("Fonts")
}

pub fn install(ctx: &egui::Context) {
    let mut defs = FontDefinitions::default();
    let dir = fonts_dir();
    let mut fallbacks: Vec<String> = Vec::new();
    for (key, file) in [("sym", "seguisym.ttf"), ("emoji", "seguiemj.ttf")] {
        if let Ok(b) = std::fs::read(dir.join(file)) {
            defs.font_data.insert(key.into(), Arc::new(FontData::from_owned(b)));
            fallbacks.push(key.into());
        }
    }
    let default_prop = defs.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let default_mono = defs.families.get(&FontFamily::Monospace).cloned().unwrap_or_default();
    let mut names = HashSet::new();
    for (name, reg, bold, ital, bi) in FONTS {
        let Ok(rb) = std::fs::read(dir.join(reg)) else { continue };
        let variants = [(*name).to_string(), format!("{name} Bold"), format!("{name} Italic"), format!("{name} Bold Italic")];
        let data = [Some(rb), std::fs::read(dir.join(bold)).ok(), std::fs::read(dir.join(ital)).ok(), std::fs::read(dir.join(bi)).ok()];
        let mut reg_key = None;
        for (fam, d) in variants.iter().zip(data) {
            let key = match d {
                Some(d) => {
                    defs.font_data.insert(fam.clone(), Arc::new(FontData::from_owned(d)));
                    fam.clone()
                }
                None => reg_key.clone().unwrap_or_else(|| name.to_string()),
            };
            if reg_key.is_none() {
                reg_key = Some(key.clone());
            }
            let mut list = vec![key];
            list.extend(fallbacks.iter().cloned());
            list.extend(default_prop.iter().cloned());
            defs.families.insert(FontFamily::Name(fam.as_str().into()), list);
            names.insert(fam.clone());
        }
        names.insert(name.to_string());
    }
    // UI fonts.
    if names.contains("Segoe UI") {
        let p = defs.families.entry(FontFamily::Proportional).or_default();
        p.insert(0, "Segoe UI".into());
        for (i, f) in fallbacks.iter().enumerate() {
            p.insert(1 + i, f.clone());
        }
    }
    if names.contains("Consolas") {
        let m = defs.families.entry(FontFamily::Monospace).or_default();
        m.insert(0, "Consolas".into());
        m.extend(fallbacks.iter().cloned());
    } else {
        let _ = default_mono;
    }
    *loaded().write().unwrap() = names;
    ctx.set_fonts(defs);
}

/// Font id for a named font with style, falling back to the UI font.
pub fn font(name: &str, size: f32, bold: bool, italic: bool, mono: bool) -> FontId {
    let l = loaded().read().unwrap();
    let base = if mono {
        if l.contains("Consolas") { "Consolas" } else { return FontId::monospace(size) }
    } else if name.is_empty() {
        "Segoe UI"
    } else {
        name
    };
    let fam = match (bold, italic) {
        (false, false) => base.to_string(),
        (true, false) => format!("{base} Bold"),
        (false, true) => format!("{base} Italic"),
        (true, true) => format!("{base} Bold Italic"),
    };
    if l.contains(&fam) {
        FontId::new(size, FontFamily::Name(fam.into()))
    } else if mono {
        FontId::monospace(size)
    } else {
        FontId::proportional(size)
    }
}

/// Whether the italic variant is a real font (so no synthetic slant is needed).
pub fn has_italic(name: &str) -> bool {
    let base = if name.is_empty() { "Segoe UI" } else { name };
    loaded().read().unwrap().contains(&format!("{base} Italic"))
}
