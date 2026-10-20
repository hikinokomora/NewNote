//! Import from other note formats: OneNote (.one, .onetoc2, .onepkg), MHT,
//! HTML, Markdown / Obsidian vaults, Notion exports, Joplin (.jex), Google
//! Keep, Simplenote, Standard Notes, Evernote (.enex), Xournal++ (.xopp,
//! .xoj), SVG drawings, OpenDocument text (.odt) and RTF.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::export::{html_to_markup, markdown_to_markup, parse_csv, table_from_cells};
use crate::model::*;
use crate::storage::Notebook;

pub struct ImpPage {
    pub title: String,
    pub created: Option<chrono::NaiveDateTime>,
    pub level: u8,
    pub page: Page,
}

pub struct ImpSection {
    pub name: String,
    pub color: u8,
    pub pages: Vec<ImpPage>,
}

pub enum ImpNode {
    Group(String, Vec<ImpNode>),
    Section(ImpSection),
}

pub struct Imported {
    /// Suggested notebook name (for whole-notebook sources).
    pub name: String,
    pub nodes: Vec<ImpNode>,
    /// Loose pages for "into the current section" imports.
    pub pages: Vec<ImpPage>,
    pub warnings: Vec<String>,
}

pub struct Opts {
    pub author: String,
    pub font: String,
    pub size: f32,
}

/// Sources that become a whole new notebook.
pub fn is_notebook_source(path: &Path) -> bool {
    if path.is_dir() {
        return true;
    }
    matches!(ext_of(path).as_str(), "onetoc2" | "onepkg" | "jex" | "zip")
}

pub fn ext_of(path: &Path) -> String {
    path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

fn stem_of(path: &Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "Импорт".into())
}

pub const SUPPORTED: &[&str] = &[
    "one", "onetoc2", "onepkg", "mht", "mhtml", "html", "htm", "md", "markdown", "txt", "enex", "docx", "odt", "rtf", "xopp", "xoj", "svg", "jex", "json", "zip", "pdf", "csv", "xlsx", "xls", "ods",
];

/// Converts any supported source. Assets are stored in `nb`.
pub fn import(path: &Path, nb: &Notebook, o: &Opts) -> Result<Imported, String> {
    let mut out = Imported { name: stem_of(path), nodes: Vec::new(), pages: Vec::new(), warnings: Vec::new() };
    if path.is_dir() {
        import_dir(path, nb, o, &mut out)?;
        return Ok(out);
    }
    match ext_of(path).as_str() {
        "onetoc2" => onenote_notebook(path, nb, o, &mut out)?,
        "onepkg" => onenote_package(path, nb, o, &mut out)?,
        "one" => {
            let sec = onenote_section_file(path, nb, o)?;
            out.nodes.push(ImpNode::Section(sec));
        }
        "mht" | "mhtml" => {
            let data = std::fs::read(path).map_err(|e| e.to_string())?;
            out.pages.push(mht_page(&data, &stem_of(path), nb, o));
        }
        "html" | "htm" => {
            let data = std::fs::read(path).map_err(|e| e.to_string())?;
            let html = decode_text(&data);
            let dir = path.parent().map(Path::to_path_buf);
            let mut b = Builder::new(nb, o);
            html_into(&mut b, &html, &|src| load_src(src, dir.as_deref(), &HashMap::new()));
            out.pages.push(b.finish(title_from_html(&html).unwrap_or_else(|| stem_of(path))));
        }
        "md" | "markdown" | "txt" => {
            let text = decode_text(&std::fs::read(path).map_err(|e| e.to_string())?);
            let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
            out.pages.push(markdown_page(&text, &stem_of(path), &dir, &HashMap::new(), nb, o));
        }
        "enex" => out.pages.extend(enex(&std::fs::read_to_string(path).map_err(|e| e.to_string())?, nb, o)),
        "docx" => {
            let md = crate::export::docx_to_markup(path)?;
            let mut b = Builder::new(nb, o);
            b.text(&md);
            docx_images(path, &mut b);
            out.pages.push(b.finish(stem_of(path)));
        }
        "odt" => out.pages.push(odt(path, nb, o)?),
        "rtf" => {
            let data = std::fs::read(path).map_err(|e| e.to_string())?;
            let mut b = Builder::new(nb, o);
            b.text(&rtf_to_text(&data));
            out.pages.push(b.finish(stem_of(path)));
        }
        "xopp" | "xoj" => out.pages.extend(xournal(path, nb, o)?),
        "svg" => {
            let svg = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
            let mut b = Builder::new(nb, o);
            b.page.strokes = svg_strokes(&svg);
            out.pages.push(b.finish(stem_of(path)));
        }
        "jex" => joplin(path, nb, o, &mut out)?,
        "json" => json_notes(path, nb, o, &mut out)?,
        "zip" => {
            let data = std::fs::read(path).map_err(|e| e.to_string())?;
            let tmp = crate::settings::Settings::temp_dir().join(format!("unzip-{}", new_id()));
            crate::storage::unzip_to(&data, &tmp)?;
            import_dir(&tmp, nb, o, &mut out)?;
            let _ = std::fs::remove_dir_all(&tmp);
            out.name = stem_of(path);
        }
        e => return Err(format!("Формат .{e} не поддерживается")),
    }
    Ok(out)
}

// ----------------------------------------------------------------------------
// Page builder

pub struct Builder<'a> {
    nb: &'a Notebook,
    o: &'a Opts,
    pub page: Page,
    pub y: f32,
    pub x: f32,
}

impl<'a> Builder<'a> {
    pub fn new(nb: &'a Notebook, o: &'a Opts) -> Self {
        Self { nb, o, page: Page::new(new_id()), y: 0.0, x: 0.0 }
    }

    fn est_height(&self, markup: &str, width: f32) -> f32 {
        let chars_per_line = (width / (self.o.size * 0.5)).max(10.0);
        let lines: f32 = markup.lines().map(|l| (l.chars().count() as f32 / chars_per_line).ceil().max(1.0)).sum();
        lines * self.o.size * 1.45 + 8.0
    }

    pub fn text_at(&mut self, markup: &str, x: f32, y: f32, width: f32) -> Id {
        let tb = TextBlock::new(markup.trim_end(), self.o.size, &self.o.font);
        let mut it = Item::new(egui::pos2(x, y), width.max(120.0), ItemKind::Text(tb), &self.o.author);
        it.height = self.est_height(markup, width);
        let id = it.id;
        self.page.items.push(it);
        id
    }

    /// Text in the running flow.
    pub fn text(&mut self, markup: &str) {
        if markup.trim().is_empty() {
            return;
        }
        let w = 680.0;
        self.text_at(markup, self.x, self.y, w);
        self.y += self.est_height(markup, w) + 12.0;
    }

    pub fn image_at(&mut self, bytes: &[u8], ext: &str, x: f32, y: f32, width: Option<f32>, alt: &str) -> f32 {
        let (w, h) = image::load_from_memory(bytes).map(|i| (i.width() as f32, i.height() as f32)).unwrap_or((400.0, 300.0));
        let ext = if ext.is_empty() { guess_ext(bytes) } else { ext.trim_start_matches('.').to_lowercase() };
        let asset = self.nb.import_asset(bytes, &ext);
        let width = width.unwrap_or(w).clamp(16.0, 900.0);
        let mut it = Item::new(egui::pos2(x, y), width, ItemKind::Image(ImageItem { asset, crop: full_crop(), alt: alt.to_owned(), ..Default::default() }), &self.o.author);
        it.height = width * h / w.max(1.0);
        let hh = it.height;
        self.page.items.push(it);
        hh
    }

    pub fn image(&mut self, bytes: &[u8], ext: &str, width: Option<f32>, alt: &str) {
        let h = self.image_at(bytes, ext, self.x, self.y, width, alt);
        self.y += h + 14.0;
    }

    pub fn file_at(&mut self, bytes: &[u8], name: &str, x: f32, y: f32) {
        let ext = Path::new(name).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_else(|| "bin".into());
        let asset = self.nb.import_asset(bytes, &ext);
        let it = Item::new(egui::pos2(x, y), 320.0, ItemKind::File(FileItem { asset, name: name.to_owned(), size: bytes.len() as u64, extracted_text: None }), &self.o.author);
        self.page.items.push(it);
    }

    pub fn file(&mut self, bytes: &[u8], name: &str) {
        self.file_at(bytes, name, self.x, self.y);
        self.y += 66.0;
    }

    pub fn table_at(&mut self, cells: Vec<Vec<String>>, x: f32, y: f32) -> f32 {
        if cells.is_empty() {
            return 0.0;
        }
        let mut t = table_from_cells(cells);
        t.header = false;
        let w: f32 = t.col_widths.iter().sum();
        let h: f32 = t.row_heights.iter().sum();
        self.page.items.push(Item::new(egui::pos2(x, y), w, ItemKind::Table(t), &self.o.author));
        h
    }

    pub fn table(&mut self, cells: Vec<Vec<String>>) {
        let h = self.table_at(cells, self.x, self.y);
        self.y += h + 14.0;
    }

    /// Moves content so it starts near the top-left corner.
    fn normalize(&mut self) {
        let b = self.page.content_bounds();
        if !b.is_positive() {
            return;
        }
        let d = egui::vec2(-b.min.x.min(0.0), 6.0 - b.min.y);
        if d == egui::Vec2::ZERO {
            return;
        }
        for it in self.page.items.iter_mut() {
            it.pos[0] += d.x;
            it.pos[1] += d.y;
        }
        let x = crate::ink::Xform { offset: d, ..Default::default() };
        for s in self.page.strokes.iter_mut() {
            *s = x.apply_stroke(s);
        }
    }

    pub fn finish(mut self, title: String) -> ImpPage {
        self.normalize();
        ImpPage { title, created: None, level: 0, page: self.page }
    }
}

fn guess_ext(b: &[u8]) -> String {
    if b.starts_with(&[0x89, b'P', b'N', b'G']) {
        "png"
    } else if b.starts_with(&[0xFF, 0xD8]) {
        "jpg"
    } else if b.starts_with(b"GIF8") {
        "gif"
    } else if b.starts_with(b"BM") {
        "bmp"
    } else if b.len() > 12 && &b[8..12] == b"WEBP" {
        "webp"
    } else {
        "bin"
    }
    .into()
}

pub fn decode_text(data: &[u8]) -> String {
    if let Some(rest) = data.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(rest).to_string();
    }
    if data.starts_with(&[0xFF, 0xFE]) {
        let u: Vec<u16> = data[2..].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        return String::from_utf16_lossy(&u);
    }
    match std::str::from_utf8(data) {
        Ok(s) => s.to_owned(),
        Err(_) => {
            // Windows-1251 is the usual alternative for Russian documents.
            let head = String::from_utf8_lossy(&data[..data.len().min(2048)]).to_lowercase();
            if head.contains("1252") || head.contains("iso-8859-1") { latin1(data) } else { cp1251(data) }
        }
    }
}

fn latin1(data: &[u8]) -> String {
    data.iter().map(|b| *b as char).collect()
}

pub fn cp1251(data: &[u8]) -> String {
    const HI: &str = "ЂЃ‚ѓ„…†‡€‰Љ‹ЊЌЋЏђ‘’“”•–—\u{98}™љ›њќћџ\u{a0}ЎўЈ¤Ґ¦§Ё©Є«¬\u{ad}®Ї°±Ііґµ¶·ё№є»јЅѕїАБВГДЕЖЗИЙКЛМНОПРСТУФХЦЧШЩЪЫЬЭЮЯабвгдежзийклмнопрстуфхцчшщъыьэюя";
    let hi: Vec<char> = HI.chars().collect();
    data.iter().map(|&b| if b < 0x80 { b as char } else { hi.get((b - 0x80) as usize).copied().unwrap_or('?') }).collect()
}

pub fn b64decode(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut buf = 0u32;
    let mut bits = 0;
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => continue,
        } as u32;
        buf = (buf << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
        }
    }
    out
}

fn palette_index(r: u8, g: u8, b: u8) -> u8 {
    PALETTE
        .iter()
        .enumerate()
        .min_by_key(|(_, c)| {
            let d = |a: u8, b: u8| (a as i32 - b as i32).pow(2);
            d(c[0], r) + d(c[1], g) + d(c[2], b)
        })
        .map(|(i, _)| i as u8)
        .unwrap_or(5)
}

fn ts(secs: i64) -> Option<chrono::NaiveDateTime> {
    chrono::DateTime::from_timestamp(secs, 0).map(|d| d.with_timezone(&chrono::Local).naive_local())
}

// ----------------------------------------------------------------------------
// OneNote

use onenote_parser::contents::{Content, EmbeddedFile, Image as OnImage, Ink as OnInk, OutlineElement, OutlineItem, RichText, Table as OnTable};
use onenote_parser::page::PageContent;
use onenote_parser::section::SectionEntry;

const HALF_INCH: f32 = 48.0;
const INK_UNIT: f32 = 96.0 / 2540.0;

fn tp(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

fn onenote_notebook(path: &Path, nb: &Notebook, o: &Opts, out: &mut Imported) -> Result<(), String> {
    out.name = path.parent().and_then(|p| p.file_name()).map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| stem_of(path));
    let parser = onenote_parser::Parser::new();
    let s = tp(path);
    match parser.parse_notebook(typed_path::TypedPath::derive(&s)) {
        Ok(book) => {
            out.nodes = book.entries().iter().map(|e| entry_node(e, nb, o)).collect();
            Ok(())
        }
        Err(e) => {
            // Fall back to the section files one by one (e.g. a locked section breaks the whole parse).
            out.warnings.push(format!("Оглавление не прочитано ({e}), импортированы отдельные разделы"));
            let dir = path.parent().ok_or("Нет папки")?;
            onenote_folder(dir, nb, o, out);
            Ok(())
        }
    }
}

fn onenote_folder(dir: &Path, nb: &Notebook, o: &Opts, out: &mut Imported) {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir).map(|rd| rd.flatten().map(|e| e.path()).collect()).unwrap_or_default();
    files.sort();
    for f in files {
        if f.is_dir() && !f.file_name().is_some_and(|n| n.to_string_lossy().starts_with("OneNote_RecycleBin")) {
            let mut sub = Imported { name: String::new(), nodes: Vec::new(), pages: Vec::new(), warnings: Vec::new() };
            onenote_folder(&f, nb, o, &mut sub);
            if !sub.nodes.is_empty() {
                out.nodes.push(ImpNode::Group(stem_of(&f), sub.nodes));
            }
            out.warnings.extend(sub.warnings);
        } else if ext_of(&f) == "one" {
            match onenote_section_file(&f, nb, o) {
                Ok(s) => out.nodes.push(ImpNode::Section(s)),
                Err(e) => out.warnings.push(format!("{}: {e}", f.display())),
            }
        }
    }
}

fn onenote_package(path: &Path, nb: &Notebook, o: &Opts, out: &mut Imported) -> Result<(), String> {
    let parser = onenote_parser::Parser::new();
    let s = tp(path);
    let book = parser.parse_package(typed_path::TypedPath::derive(&s)).map_err(|e| format!("Пакет .onepkg не прочитан: {e}"))?;
    out.name = stem_of(path);
    out.nodes = book.entries().iter().map(|e| entry_node(e, nb, o)).collect();
    Ok(())
}

fn onenote_section_file(path: &Path, nb: &Notebook, o: &Opts) -> Result<ImpSection, String> {
    let parser = onenote_parser::Parser::new();
    let s = tp(path);
    let sec = parser.parse_section(typed_path::TypedPath::derive(&s)).map_err(|e| format!("Раздел не прочитан: {e}"))?;
    Ok(section_of(&sec, nb, o))
}

fn entry_node(e: &SectionEntry, nb: &Notebook, o: &Opts) -> ImpNode {
    match e {
        SectionEntry::Section(s) => ImpNode::Section(section_of(s, nb, o)),
        SectionEntry::SectionGroup(g) => ImpNode::Group(g.display_name().to_owned(), g.entries().iter().map(|x| entry_node(x, nb, o)).collect()),
    }
}

fn section_of(sec: &onenote_parser::section::Section, nb: &Notebook, o: &Opts) -> ImpSection {
    let color = sec.color().map(|c| palette_index(c.r(), c.g(), c.b())).unwrap_or(5);
    let mut pages = Vec::new();
    for series in sec.page_series() {
        for p in series.pages() {
            pages.push(onenote_page(p, nb, o));
        }
    }
    ImpSection { name: sec.display_name().trim_end_matches(".one").to_owned(), color, pages }
}

fn onenote_page(p: &onenote_parser::page::Page, nb: &Notebook, o: &Opts) -> ImpPage {
    let mut b = Builder::new(nb, o);
    for c in p.contents() {
        match c {
            PageContent::Outline(ol) => {
                let x = ol.offset_horizontal().unwrap_or(0.0) * HALF_INCH;
                let y = ol.offset_vertical().unwrap_or(0.0) * HALF_INCH;
                let w = ol.layout_max_width().map(|w| w * HALF_INCH).unwrap_or(620.0).clamp(160.0, 1400.0);
                b.x = x;
                b.y = y;
                let mut lines: Vec<String> = Vec::new();
                let mut tags: Vec<u32> = Vec::new();
                outline_items(ol.items(), 0, &mut lines, &mut tags, &mut b, w);
                flush_lines(&mut b, &mut lines, &mut tags, w);
            }
            PageContent::Image(img) => {
                let x = img.offset_horizontal().unwrap_or(0.0) * HALF_INCH;
                let y = img.offset_vertical().unwrap_or(0.0) * HALF_INCH;
                image_content(&mut b, img, Some((x, y)));
            }
            PageContent::EmbeddedFile(f) => {
                let x = f.offset_horizontal().unwrap_or(0.0) * HALF_INCH;
                let y = f.offset_vertical().unwrap_or(0.0) * HALF_INCH;
                file_content(&mut b, f, Some((x, y)));
            }
            PageContent::Ink(ink) => {
                let ox = ink.offset_horizontal().unwrap_or(0.0) * HALF_INCH;
                let oy = ink.offset_vertical().unwrap_or(0.0) * HALF_INCH;
                ink_strokes(ink, ox, oy, &mut b.page.strokes);
            }
            #[allow(unreachable_patterns)]
            _ => {}
        }
    }
    if let Some(r) = p.ink_recognition() {
        b.page.ink_text = r.text();
    }
    let title = p.title_text().map(|t| t.trim().to_owned()).filter(|t| !t.is_empty()).unwrap_or_else(|| "Без заголовка".into());
    let mut ip = b.finish(title);
    ip.created = ts(p.created_time().unix_timestamp());
    ip.level = (p.level() - 1).clamp(0, 2) as u8;
    ip
}

fn flush_lines(b: &mut Builder, lines: &mut Vec<String>, tags: &mut Vec<u32>, w: f32) {
    if lines.iter().all(|l| l.trim().is_empty()) {
        lines.clear();
        return;
    }
    let text = lines.join("\n");
    let id = b.text_at(&text, b.x, b.y, w);
    if !tags.is_empty() {
        if let Some(it) = b.page.item_mut(id) {
            tags.sort_unstable();
            tags.dedup();
            it.tags = tags.clone();
            if tags.contains(&1) {
                it.task = Some(Task { done: false, due: None, priority: 0, notified: false });
            }
        }
    }
    b.y += b.est_height(&text, w) + 6.0;
    lines.clear();
    tags.clear();
}

fn outline_items(items: &[OutlineItem], depth: usize, lines: &mut Vec<String>, tags: &mut Vec<u32>, b: &mut Builder, w: f32) {
    for it in items {
        match it {
            OutlineItem::Group(g) => outline_items(g.outlines(), depth + 1, lines, tags, b, w),
            OutlineItem::Element(e) => {
                outline_element(e, depth, lines, tags, b, w);
                outline_items(e.children(), depth + 1, lines, tags, b, w);
            }
        }
    }
}

fn list_prefix(e: &OutlineElement) -> Option<String> {
    let l = e.list_contents().first()?;
    let fmt: String = l.list_format().iter().collect();
    if fmt.contains('.') || fmt.contains(')') || fmt.chars().any(|c| c.is_ascii_digit()) {
        Some("1. ".into())
    } else {
        Some("- ".into())
    }
}

fn outline_element(e: &OutlineElement, depth: usize, lines: &mut Vec<String>, tags: &mut Vec<u32>, b: &mut Builder, w: f32) {
    let indent = "  ".repeat(depth.min(6));
    let list = list_prefix(e);
    for c in e.contents() {
        match c {
            Content::RichText(rt) => {
                let (markup, check, tag_ids) = rich_text(rt);
                tags.extend(tag_ids);
                let style = rt.paragraph_style().style_id().unwrap_or("");
                let heading = style.strip_prefix('h').and_then(|n| n.parse::<usize>().ok()).filter(|n| (1..=6).contains(n));
                let mut line_prefix = match (check, heading, &list) {
                    (Some(done), _, _) => format!("{indent}{} ", if done { "[x]" } else { "[ ]" }),
                    (_, Some(n), _) => format!("{} ", "#".repeat(n)),
                    (_, _, Some(p)) => format!("{indent}{p}"),
                    _ if style == "blockquote" || style == "cite" => "> ".into(),
                    _ => indent.clone(),
                };
                if style == "code" {
                    lines.push("```".into());
                    lines.extend(markup.lines().map(str::to_owned));
                    lines.push("```".into());
                    continue;
                }
                for (i, l) in markup.split('\n').enumerate() {
                    lines.push(format!("{line_prefix}{l}"));
                    if i == 0 && list.is_some() {
                        line_prefix = format!("{indent}  ");
                    }
                }
            }
            Content::Table(t) => {
                flush_lines(b, lines, tags, w);
                let h = b.table_at(table_cells(t), b.x, b.y);
                b.y += h + 10.0;
            }
            Content::Image(img) => {
                flush_lines(b, lines, tags, w);
                image_content(b, img, None);
            }
            Content::EmbeddedFile(f) => {
                flush_lines(b, lines, tags, w);
                file_content(b, f, None);
            }
            #[allow(unreachable_patterns)]
            _ => {}
        }
    }
}

fn table_cells(t: &OnTable) -> Vec<Vec<String>> {
    t.contents()
        .iter()
        .map(|row| {
            row.contents()
                .iter()
                .map(|cell| {
                    cell.contents()
                        .iter()
                        .flat_map(|el| el.contents().iter())
                        .filter_map(|c| match c {
                            Content::RichText(rt) => Some(rt.text().replace(['\u{b}', '\r'], " ")),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .collect()
        })
        .collect()
}

fn image_content(b: &mut Builder, img: &OnImage, abs: Option<(f32, f32)>) {
    let Some(mut r) = img.read() else { return };
    let mut bytes = Vec::new();
    if r.read_to_end(&mut bytes).is_err() || bytes.is_empty() {
        return;
    }
    let width = img.picture_width().or(img.layout_max_width()).map(|w| w * HALF_INCH);
    let ext = img.extension().unwrap_or("").to_owned();
    let alt = img.alt_text().unwrap_or("").to_owned();
    let (x, y) = abs.unwrap_or((b.x, b.y));
    let h = b.image_at(&bytes, &ext, x, y, width, &alt);
    if let Some(t) = img.text().filter(|t| !t.trim().is_empty()) {
        if let Some(ItemKind::Image(im)) = b.page.items.last_mut().map(|i| &mut i.kind) {
            im.ocr_text = Some(t.to_owned());
        }
    }
    if abs.is_none() {
        b.y += h + 12.0;
    }
}

fn file_content(b: &mut Builder, f: &EmbeddedFile, abs: Option<(f32, f32)>) {
    let mut bytes = Vec::new();
    if f.read().read_to_end(&mut bytes).is_err() {
        return;
    }
    let (x, y) = abs.unwrap_or((b.x, b.y));
    b.file_at(&bytes, f.filename(), x, y);
    if abs.is_none() {
        b.y += 66.0;
    }
}

fn ink_strokes(ink: &OnInk, ox: f32, oy: f32, out: &mut Vec<Stroke>) {
    // Ink coordinates are absolute in ink space; anchor the drawing's top-left at the offset.
    let mut minx = f32::INFINITY;
    let mut miny = f32::INFINITY;
    collect_min(ink, &mut minx, &mut miny);
    if !minx.is_finite() {
        return;
    }
    add_ink(ink, ox, oy, minx, miny, out);
}

fn collect_min(ink: &OnInk, minx: &mut f32, miny: &mut f32) {
    for s in ink.ink_strokes() {
        for p in s.absolute_path() {
            *minx = minx.min(p.x());
            *miny = miny.min(p.y());
        }
    }
    for g in ink.child_groups() {
        collect_min(g, minx, miny);
    }
}

fn add_ink(ink: &OnInk, ox: f32, oy: f32, minx: f32, miny: f32, out: &mut Vec<Stroke>) {
    for s in ink.ink_strokes() {
        let pts: Vec<InkPoint> = s.absolute_path().iter().map(|p| InkPoint(ox + (p.x() - minx) * INK_UNIT, oy + (p.y() - miny) * INK_UNIT, 1.0)).collect();
        if pts.is_empty() {
            continue;
        }
        let c = s.color().unwrap_or(0);
        let (r, g, bl) = ((c & 0xFF) as u8, ((c >> 8) & 0xFF) as u8, ((c >> 16) & 0xFF) as u8);
        let t = s.transparency().unwrap_or(0);
        let alpha = 255u8.saturating_sub(t);
        let highlighter = t > 40;
        let width = (s.width() * INK_UNIT).clamp(0.5, 40.0);
        let mut st = Stroke::new(pts, [r, g, bl, alpha.max(40)], width, if highlighter { InkKind::Highlighter } else { InkKind::Pen });
        st.style = Arc::new(StrokeStyle { no_pressure: true, ..Default::default() });
        if highlighter {
            st.layer = 0;
        }
        out.push(st);
    }
    for g in ink.child_groups() {
        add_ink(g, ox, oy, minx, miny, out);
    }
}

fn esc_md(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let cs: Vec<char> = s.chars().collect();
    for (i, c) in cs.iter().enumerate() {
        let needs = match c {
            '*' | '_' | '~' | '^' | '=' | '`' | '[' | ']' | '{' | '}' | '\\' => true,
            ',' => cs.get(i + 1) == Some(&','),
            _ => false,
        };
        if needs {
            out.push('\\');
        }
        out.push(*c);
    }
    out
}

/// Rich text → markup, checkbox state and our tag ids.
fn rich_text(rt: &RichText) -> (String, Option<bool>, Vec<u32>) {
    let text: Vec<u16> = rt.text().replace('\u{b}', "\n").replace('\r', "").encode_utf16().collect();
    let idx: Vec<usize> = rt.text_run_indices().iter().map(|i| *i as usize).collect();
    let fmts = rt.text_run_formatting();
    let mut bounds = vec![0usize];
    bounds.extend(idx.iter().copied().filter(|i| *i <= text.len()));
    bounds.push(text.len());
    let mut out = String::new();
    for (k, w) in bounds.windows(2).enumerate() {
        let (a, bnd) = (w[0].min(text.len()), w[1].min(text.len()));
        if bnd <= a {
            continue;
        }
        let seg = String::from_utf16_lossy(&text[a..bnd]);
        let Some(f) = fmts.get(k) else {
            out.push_str(&esc_md(&seg));
            continue;
        };
        // Keep spaces and line breaks outside the markers.
        let lead: String = seg.chars().take_while(|c| c.is_whitespace()).collect();
        let trail: String = seg.chars().rev().take_while(|c| c.is_whitespace()).collect::<Vec<_>>().into_iter().rev().collect();
        let core = seg.trim();
        if core.is_empty() {
            out.push_str(&seg);
            continue;
        }
        let mut s = esc_md(core);
        let mut wrap = |o: &str, c: &str| s = format!("{o}{s}{c}");
        if f.bold() {
            wrap("**", "**");
        }
        if f.italic() {
            wrap("*", "*");
        }
        if f.underline() {
            wrap("__", "__");
        }
        if f.strikethrough() {
            wrap("~~", "~~");
        }
        if f.superscript() {
            wrap("^", "^");
        }
        if f.subscript() {
            wrap(",,", ",,");
        }
        if let Some(onenote_parser::property::common::ColorRef::Manual { r, g, b }) = f.font_color() {
            if !(r < 20 && g < 20 && b < 20) {
                wrap(&format!("{{c:#{r:02x}{g:02x}{b:02x}|"), "}");
            }
        }
        if let Some(onenote_parser::property::common::ColorRef::Manual { r, g, b }) = f.highlight() {
            if r == 255 && g == 255 && b == 0 {
                wrap("==", "==");
            } else {
                wrap(&format!("{{h:#{r:02x}{g:02x}{b:02x}|"), "}");
            }
        }
        if let Some(sz) = f.font_size() {
            let pt = sz as f32 / 2.0;
            if (pt - 11.0).abs() > 0.6 && !core.contains('\n') {
                wrap(&format!("{{s:{:.0}|", pt * 96.0 / 72.0), "}");
            }
        }
        out.push_str(&lead);
        out.push_str(&s);
        out.push_str(&trail);
    }
    // Hyperlinks.
    let plain: Vec<u16> = text.clone();
    for h in rt.hyperlinks() {
        let (a, bnd) = (h.start() as usize, h.end() as usize);
        if bnd <= a || bnd > plain.len() {
            continue;
        }
        let label = esc_md(&String::from_utf16_lossy(&plain[a..bnd]));
        if let Some(pos) = out.find(&label) {
            out.replace_range(pos..pos + label.len(), &format!("[{label}]({})", h.target().replace(' ', "%20")));
        }
    }
    // Note tags.
    let mut check = None;
    let mut tag_ids = Vec::new();
    for t in rt.note_tags() {
        let Some(def) = t.definition() else { continue };
        let label = def.label().to_lowercase();
        let is_todo = format!("{:?}", def.shape()).to_lowercase().contains("check") || label.contains("to do") || label.contains("задач");
        if is_todo {
            check = Some(t.completed_at().is_some() || format!("{:?}", t.item_status()).to_lowercase().contains("complet"));
            tag_ids.push(1);
            continue;
        }
        let map = [("важн", 2), ("important", 2), ("вопрос", 3), ("question", 3), ("иде", 4), ("idea", 4), ("критич", 5), ("critical", 5), ("контакт", 6), ("contact", 6), ("адрес", 7), ("address", 7), ("телефон", 8), ("phone", 8), ("прочит", 9), ("read", 9), ("фильм", 10), ("movie", 10), ("музык", 11), ("music", 11)];
        if let Some((_, id)) = map.iter().find(|(k, _)| label.contains(k)) {
            tag_ids.push(*id);
        }
    }
    (out, check, tag_ids)
}

// ----------------------------------------------------------------------------
// MHT / HTML

/// Splits a MIME multipart document into its HTML and the resources by location.
fn parse_mime(data: &[u8]) -> (String, HashMap<String, (Vec<u8>, String)>) {
    let text = latin1(data);
    let mut res = HashMap::new();
    let (head, body) = split_headers(&text);
    let ctype = header(&head, "content-type").unwrap_or_default();
    let Some(boundary) = param(&ctype, "boundary") else {
        // Single part.
        let enc = header(&head, "content-transfer-encoding").unwrap_or_default();
        let bytes = decode_body(body, &enc);
        return (charset_decode(&bytes, &ctype), res);
    };
    let mut html = String::new();
    for part in body.split(&format!("--{boundary}")) {
        let part = part.trim_start_matches(['\r', '\n']);
        if part.starts_with("--") || part.trim().is_empty() {
            continue;
        }
        let (ph, pb) = split_headers(part);
        let pct = header(&ph, "content-type").unwrap_or_default();
        let enc = header(&ph, "content-transfer-encoding").unwrap_or_default();
        let bytes = decode_body(pb, &enc);
        if pct.to_lowercase().starts_with("text/html") && html.is_empty() {
            html = charset_decode(&bytes, &pct);
        } else {
            let loc = header(&ph, "content-location").unwrap_or_default();
            let cid = header(&ph, "content-id").map(|c| format!("cid:{}", c.trim_matches(['<', '>'])));
            let mime = pct.split(';').next().unwrap_or("").trim().to_owned();
            if !loc.is_empty() {
                res.insert(loc.clone(), (bytes.clone(), mime.clone()));
                if let Some(name) = loc.rsplit(['/', '\\']).next() {
                    res.insert(name.to_owned(), (bytes.clone(), mime.clone()));
                }
            }
            if let Some(c) = cid {
                res.insert(c, (bytes, mime));
            }
        }
    }
    (html, res)
}

fn split_headers(s: &str) -> (String, &str) {
    let i = s.find("\r\n\r\n").map(|i| (i, 4)).or_else(|| s.find("\n\n").map(|i| (i, 2)));
    match i {
        Some((i, n)) => (s[..i].replace("\r\n\t", " ").replace("\r\n ", " ").replace("\n\t", " ").replace("\n ", " "), &s[i + n..]),
        None => (String::new(), s),
    }
}

fn header(h: &str, name: &str) -> Option<String> {
    h.lines().find_map(|l| {
        let (k, v) = l.split_once(':')?;
        (k.trim().eq_ignore_ascii_case(name)).then(|| v.trim().to_owned())
    })
}

fn param(h: &str, name: &str) -> Option<String> {
    let lower = h.to_lowercase();
    let i = lower.find(&format!("{name}="))? + name.len() + 1;
    let rest = &h[i..];
    let v = if let Some(r) = rest.strip_prefix('"') { r.split('"').next()? } else { rest.split([';', ' ']).next()? };
    Some(v.to_owned())
}

fn decode_body(body: &str, enc: &str) -> Vec<u8> {
    let raw: Vec<u8> = body.chars().map(|c| c as u32 as u8).collect();
    match enc.to_lowercase().as_str() {
        "base64" => b64decode(body),
        "quoted-printable" => {
            let mut out = Vec::with_capacity(raw.len());
            let mut i = 0;
            while i < raw.len() {
                if raw[i] == b'=' {
                    if raw.get(i + 1) == Some(&b'\r') && raw.get(i + 2) == Some(&b'\n') {
                        i += 3;
                        continue;
                    }
                    if raw.get(i + 1) == Some(&b'\n') {
                        i += 2;
                        continue;
                    }
                    if let Some(h) = raw.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(std::str::from_utf8(h).ok()?, 16).ok()) {
                        out.push(h);
                        i += 3;
                        continue;
                    }
                }
                out.push(raw[i]);
                i += 1;
            }
            out
        }
        _ => raw,
    }
}

fn charset_decode(bytes: &[u8], ctype: &str) -> String {
    match param(ctype, "charset").map(|c| c.to_lowercase()) {
        Some(c) if c.contains("1251") => cp1251(bytes),
        Some(c) if c.contains("1252") || c.contains("8859-1") => latin1(bytes),
        _ => decode_text(bytes),
    }
}

fn mht_page(data: &[u8], name: &str, nb: &Notebook, o: &Opts) -> ImpPage {
    let (html, res) = parse_mime(data);
    let mut b = Builder::new(nb, o);
    html_into(&mut b, &html, &|src| load_src(src, None, &res));
    let title = title_from_html(&html).unwrap_or_else(|| name.to_owned());
    b.finish(title)
}

fn title_from_html(html: &str) -> Option<String> {
    let l = html.to_lowercase();
    let a = l.find("<title>")? + 7;
    let e = l[a..].find("</title>")? + a;
    let t = html_to_markup(&html[a..e]);
    (!t.trim().is_empty()).then(|| t.trim().to_owned())
}

/// Resolves an image source: data URI, MHT resource or a file next to the document.
fn load_src(src: &str, dir: Option<&Path>, res: &HashMap<String, (Vec<u8>, String)>) -> Option<(Vec<u8>, String)> {
    let src = src.trim();
    if let Some(rest) = src.strip_prefix("data:") {
        let (meta, data) = rest.split_once(',')?;
        let ext = meta.split(['/', ';']).nth(1).unwrap_or("png").to_owned();
        return Some((b64decode(data), ext));
    }
    let clean = url_decode(src.trim_start_matches("file:///"));
    if let Some((b, mime)) = res.get(src).or_else(|| res.get(&clean)).or_else(|| clean.rsplit(['/', '\\']).next().and_then(|n| res.get(n))) {
        let ext = mime.split('/').nth(1).unwrap_or("").to_owned();
        return Some((b.clone(), ext));
    }
    let p = Path::new(&clean);
    let full = if p.is_absolute() { p.to_path_buf() } else { dir?.join(p) };
    let bytes = std::fs::read(&full).ok()?;
    Some((bytes, ext_of(&full)))
}

fn url_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_lowercase();
    let mut from = 0;
    while let Some(i) = lower[from..].find(name) {
        let i = from + i;
        let before_ok = i == 0 || !lower.as_bytes()[i - 1].is_ascii_alphanumeric();
        let rest = lower[i + name.len()..].trim_start();
        if before_ok && rest.starts_with('=') {
            let start = tag.len() - rest.len() + 1;
            let v = tag[start..].trim_start();
            let val = if let Some(r) = v.strip_prefix('"') {
                r.split('"').next()
            } else if let Some(r) = v.strip_prefix('\'') {
                r.split('\'').next()
            } else {
                v.split([' ', '>', '/']).next()
            };
            return val.map(|s| s.replace("&amp;", "&"));
        }
        from = i + name.len();
    }
    None
}

/// HTML with images and tables → page blocks.
fn html_into(b: &mut Builder, html: &str, load: &dyn Fn(&str) -> Option<(Vec<u8>, String)>) {
    let lower = html.to_lowercase();
    let body_start = lower.find("<body").and_then(|i| lower[i..].find('>').map(|j| i + j + 1)).unwrap_or(0);
    let mut i = body_start;
    let mut text_start = i;
    while i < html.len() {
        let Some(rel) = lower[i..].find('<') else { break };
        let j = i + rel;
        let is_img = lower[j..].starts_with("<img");
        let is_table = lower[j..].starts_with("<table");
        if !is_img && !is_table {
            i = j + 1;
            continue;
        }
        b.text(&html_to_markup(&html[text_start..j]));
        if is_img {
            let end = lower[j..].find('>').map(|e| j + e + 1).unwrap_or(html.len());
            let tag = &html[j..end];
            if let Some(src) = attr(tag, "src") {
                if let Some((bytes, ext)) = load(&src) {
                    let width = attr(tag, "width").and_then(|w| w.trim_end_matches("px").parse::<f32>().ok());
                    b.image(&bytes, &ext, width, &attr(tag, "alt").unwrap_or_default());
                }
            }
            i = end;
        } else {
            let end = lower[j..].find("</table>").map(|e| j + e + 8).unwrap_or(html.len());
            b.table(html_table(&html[j..end]));
            i = end;
        }
        text_start = i;
    }
    let end = lower.rfind("</body>").unwrap_or(html.len()).max(text_start);
    b.text(&html_to_markup(&html[text_start..end]));
}

fn html_table(t: &str) -> Vec<Vec<String>> {
    let lower = t.to_lowercase();
    let mut rows = Vec::new();
    let mut i = 0;
    while let Some(r) = lower[i..].find("<tr") {
        let rs = i + r;
        let re = lower[rs..].find("</tr>").map(|e| rs + e).unwrap_or(t.len());
        let row = &t[rs..re];
        let rl = &lower[rs..re];
        let mut cells = Vec::new();
        let mut k = 0;
        while let Some(c) = rl[k..].find("<t").map(|c| k + c) {
            if !(rl[c..].starts_with("<td") || rl[c..].starts_with("<th")) {
                k = c + 2;
                continue;
            }
            let open_end = rl[c..].find('>').map(|e| c + e + 1).unwrap_or(row.len());
            let close = rl[open_end..].find("</t").map(|e| open_end + e).unwrap_or(row.len());
            cells.push(crate::richtext::strip_markup(&html_to_markup(&row[open_end..close])).replace('\n', " ").trim().to_owned());
            k = close + 3;
        }
        if !cells.is_empty() {
            rows.push(cells);
        }
        i = re + 5;
        if i >= lower.len() {
            break;
        }
    }
    rows
}

// ----------------------------------------------------------------------------
// Markdown, Obsidian, Notion

fn markdown_page(text: &str, name: &str, dir: &Path, files: &HashMap<String, PathBuf>, nb: &Notebook, o: &Opts) -> ImpPage {
    let mut b = Builder::new(nb, o);
    // YAML front matter.
    let text = if let Some(r) = text.strip_prefix("---\n") { r.split_once("\n---").map(|(_, rest)| rest).unwrap_or(text) } else { text };
    let mut buf = String::new();
    let mut title = None;
    for line in text.lines() {
        if title.is_none() {
            if let Some(t) = line.strip_prefix("# ") {
                title = Some(t.trim().to_owned());
                continue;
            }
        }
        // Images: ![alt](path) and ![[file.png]]
        let img = if let Some(inner) = line.trim().strip_prefix("![[").and_then(|r| r.strip_suffix("]]")) {
            let name = inner.split('|').next().unwrap_or(inner).trim();
            files.get(&name.to_lowercase()).cloned().or_else(|| Some(dir.join(name)))
        } else if let (Some(a), Some(e)) = (line.find("]("), line.rfind(')')) {
            (line.trim_start().starts_with("![") && e > a).then(|| dir.join(url_decode(&line[a + 2..e])))
        } else {
            None
        };
        match img.filter(|p| p.is_file() && matches!(ext_of(p).as_str(), "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp")) {
            Some(p) => {
                b.text(&markdown_to_markup(&std::mem::take(&mut buf)));
                if let Ok(bytes) = std::fs::read(&p) {
                    b.image(&bytes, &ext_of(&p), None, "");
                }
            }
            None => {
                // Markdown tables.
                buf.push_str(&line.replace("[[", "").replace("]]", ""));
                buf.push('\n');
            }
        }
    }
    let rest = std::mem::take(&mut buf);
    md_with_tables(&mut b, &rest);
    b.finish(title.unwrap_or_else(|| clean_name(name)))
}

/// Pipe tables become table containers, the rest stays text.
fn md_with_tables(b: &mut Builder, md: &str) {
    let mut text = String::new();
    let mut table: Vec<Vec<String>> = Vec::new();
    for line in md.lines() {
        let t = line.trim();
        if t.starts_with('|') && t.ends_with('|') && t.len() > 1 {
            if t.chars().all(|c| matches!(c, '|' | '-' | ':' | ' ')) {
                continue;
            }
            if table.is_empty() {
                b.text(&markdown_to_markup(&std::mem::take(&mut text)));
            }
            table.push(t.trim_matches('|').split('|').map(|c| c.trim().to_owned()).collect());
        } else {
            if !table.is_empty() {
                b.table(std::mem::take(&mut table));
            }
            text.push_str(line);
            text.push('\n');
        }
    }
    if !table.is_empty() {
        b.table(table);
    }
    b.text(&markdown_to_markup(&text));
}

/// Notion adds a 32-character id to file names.
fn clean_name(n: &str) -> String {
    let n = n.trim();
    if let Some((a, b)) = n.rsplit_once(' ') {
        if b.len() == 32 && b.chars().all(|c| c.is_ascii_hexdigit()) {
            return a.to_owned();
        }
    }
    n.to_owned()
}

fn import_dir(dir: &Path, nb: &Notebook, o: &Opts, out: &mut Imported) -> Result<(), String> {
    out.name = clean_name(&stem_of(dir));
    let entries: Vec<PathBuf> = walk(dir);
    // OneNote notebook folder.
    if let Some(toc) = entries.iter().find(|p| ext_of(p) == "onetoc2") {
        return onenote_notebook(toc, nb, o, out);
    }
    if entries.iter().any(|p| ext_of(p) == "one") {
        onenote_folder(dir, nb, o, out);
        return Ok(());
    }
    // Google Keep takeout: JSON notes with textContent.
    let keep: Vec<&PathBuf> = entries.iter().filter(|p| ext_of(p) == "json" && std::fs::read_to_string(p).map(|s| s.contains("\"textContent\"") || s.contains("\"listContent\"")).unwrap_or(false)).collect();
    if !keep.is_empty() {
        let mut sec = ImpSection { name: "Google Keep".into(), color: 12, pages: Vec::new() };
        for p in keep {
            if let Some(pg) = keep_note(p, nb, o) {
                sec.pages.push(pg);
            }
        }
        out.nodes.push(ImpNode::Section(sec));
        return Ok(());
    }
    // Markdown / Obsidian / Notion folder.
    let files: HashMap<String, PathBuf> = entries.iter().filter_map(|p| Some((p.file_name()?.to_string_lossy().to_lowercase(), p.clone()))).collect();
    let nodes = md_folder(dir, dir, &files, nb, o);
    out.nodes.extend(nodes);
    if out.nodes.is_empty() {
        return Err("В папке нет поддерживаемых заметок".into());
    }
    Ok(())
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.')) {
                continue;
            }
            if p.is_dir() {
                out.extend(walk(&p));
            } else {
                out.push(p);
            }
        }
    }
    out
}

fn md_folder(root: &Path, dir: &Path, files: &HashMap<String, PathBuf>, nb: &Notebook, o: &Opts) -> Vec<ImpNode> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir).map(|rd| rd.flatten().map(|e| e.path()).filter(|p| !p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.'))).collect()).unwrap_or_default();
    entries.sort();
    let mut pages = Vec::new();
    let mut nodes = Vec::new();
    for p in &entries {
        if p.is_file() {
            let e = ext_of(p);
            match e.as_str() {
                "md" | "markdown" | "txt" => {
                    let text = decode_text(&std::fs::read(p).unwrap_or_default());
                    pages.push(markdown_page(&text, &stem_of(p), p.parent().unwrap_or(root), files, nb, o));
                    // Notion: a folder with the same name holds the subpages.
                    let sub = p.with_extension("");
                    if sub.is_dir() {
                        for mut sp in md_pages_flat(&sub, files, nb, o) {
                            sp.level = 1;
                            pages.push(sp);
                        }
                    }
                }
                "html" | "htm" => {
                    let html = decode_text(&std::fs::read(p).unwrap_or_default());
                    let d = p.parent().map(Path::to_path_buf);
                    let mut b = Builder::new(nb, o);
                    html_into(&mut b, &html, &|src| load_src(src, d.as_deref(), &HashMap::new()));
                    pages.push(b.finish(title_from_html(&html).unwrap_or_else(|| clean_name(&stem_of(p)))));
                }
                "csv" => {
                    let mut b = Builder::new(nb, o);
                    b.table(parse_csv(&decode_text(&std::fs::read(p).unwrap_or_default())));
                    pages.push(b.finish(clean_name(&stem_of(p))));
                }
                _ => {}
            }
        }
    }
    for p in &entries {
        if p.is_dir() {
            // Skip Notion subpage folders already consumed above.
            let is_sub = ["md", "html"].iter().any(|e| p.with_extension(e).is_file());
            if is_sub {
                continue;
            }
            let children = md_folder(root, p, files, nb, o);
            if !children.is_empty() {
                let only_one = children.len() == 1 && matches!(children[0], ImpNode::Section(_));
                if only_one {
                    nodes.extend(children);
                } else {
                    nodes.push(ImpNode::Group(clean_name(&stem_of(p)), children));
                }
            }
        }
    }
    if !pages.is_empty() {
        let name = if dir == root { clean_name(&stem_of(dir)) } else { clean_name(&stem_of(dir)) };
        let n = nodes.len() as u8;
        nodes.insert(0, ImpNode::Section(ImpSection { name, color: (3 + n) % 16, pages }));
    }
    nodes
}

fn md_pages_flat(dir: &Path, files: &HashMap<String, PathBuf>, nb: &Notebook, o: &Opts) -> Vec<ImpPage> {
    let mut out = Vec::new();
    for p in walk(dir) {
        if matches!(ext_of(&p).as_str(), "md" | "markdown") {
            let text = decode_text(&std::fs::read(&p).unwrap_or_default());
            out.push(markdown_page(&text, &stem_of(&p), p.parent().unwrap_or(dir), files, nb, o));
        }
    }
    out
}

// ----------------------------------------------------------------------------
// Google Keep, Simplenote, Standard Notes

fn json_str(v: &serde_json::Value, k: &str) -> String {
    v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_owned()
}

fn keep_note(path: &Path, nb: &Notebook, o: &Opts) -> Option<ImpPage> {
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    if v.get("isTrashed").and_then(|x| x.as_bool()) == Some(true) {
        return None;
    }
    let mut b = Builder::new(nb, o);
    let mut text = json_str(&v, "textContent");
    if let Some(list) = v.get("listContent").and_then(|x| x.as_array()) {
        for it in list {
            let done = it.get("isChecked").and_then(|x| x.as_bool()).unwrap_or(false);
            text.push_str(&format!("{} {}\n", if done { "[x]" } else { "[ ]" }, json_str(it, "text")));
        }
    }
    b.text(&esc_md(&text).replace("\\[x\\]", "[x]").replace("\\[ \\]", "[ ]"));
    if let Some(att) = v.get("attachments").and_then(|x| x.as_array()) {
        for a in att {
            let p = path.parent()?.join(json_str(a, "filePath"));
            if let Ok(bytes) = std::fs::read(&p) {
                if json_str(a, "mimetype").starts_with("image") {
                    b.image(&bytes, &ext_of(&p), None, "");
                } else {
                    b.file(&bytes, &p.file_name()?.to_string_lossy());
                }
            }
        }
    }
    let title = json_str(&v, "title");
    let mut pg = b.finish(if title.is_empty() { text.lines().next().unwrap_or("Заметка Keep").chars().take(60).collect() } else { title });
    pg.created = v.get("createdTimestampUsec").and_then(|x| x.as_i64()).and_then(|us| ts(us / 1_000_000));
    Some(pg)
}

fn json_notes(path: &Path, nb: &Notebook, o: &Opts, out: &mut Imported) -> Result<(), String> {
    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    let v: serde_json::Value = serde_json::from_slice(&data).map_err(|e| e.to_string())?;
    if v.get("textContent").is_some() || v.get("listContent").is_some() {
        if let Some(p) = keep_note(path, nb, o) {
            out.pages.push(p);
        }
        return Ok(());
    }
    // Simplenote.
    if let Some(notes) = v.get("activeNotes").and_then(|x| x.as_array()) {
        let mut sec = ImpSection { name: "Simplenote".into(), color: 6, pages: Vec::new() };
        for n in notes {
            let content = json_str(n, "content");
            let mut b = Builder::new(nb, o);
            let (title, body) = content.split_once('\n').unwrap_or((&content, ""));
            md_with_tables(&mut b, body);
            sec.pages.push(b.finish(title.trim().chars().take(80).collect()));
        }
        out.nodes.push(ImpNode::Section(sec));
        return Ok(());
    }
    // Standard Notes.
    if let Some(items) = v.get("items").and_then(|x| x.as_array()) {
        let mut sec = ImpSection { name: "Standard Notes".into(), color: 9, pages: Vec::new() };
        for it in items.iter().filter(|i| json_str(i, "content_type") == "Note") {
            let c = it.get("content").cloned().unwrap_or_default();
            let mut b = Builder::new(nb, o);
            md_with_tables(&mut b, &json_str(&c, "text"));
            let t = json_str(&c, "title");
            sec.pages.push(b.finish(if t.is_empty() { "Без заголовка".into() } else { t }));
        }
        out.nodes.push(ImpNode::Section(sec));
        return Ok(());
    }
    Err("Неизвестный формат JSON".into())
}

// ----------------------------------------------------------------------------
// Joplin (.jex = tar)

fn joplin(path: &Path, nb: &Notebook, o: &Opts, out: &mut Imported) -> Result<(), String> {
    let f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut ar = tar::Archive::new(f);
    let mut mds: Vec<String> = Vec::new();
    let mut resources: HashMap<String, (Vec<u8>, String)> = HashMap::new();
    for e in ar.entries().map_err(|e| e.to_string())? {
        let mut e = e.map_err(|e| e.to_string())?;
        let p = e.path().map_err(|e| e.to_string())?.to_path_buf();
        let mut buf = Vec::new();
        let _ = e.read_to_end(&mut buf);
        if p.starts_with("resources") {
            let id = stem_of(&p);
            resources.insert(id, (buf, ext_of(&p)));
        } else if ext_of(&p) == "md" {
            mds.push(String::from_utf8_lossy(&buf).to_string());
        }
    }
    struct Note {
        title: String,
        body: String,
        parent: String,
        created: Option<chrono::NaiveDateTime>,
    }
    let mut folders: HashMap<String, (String, String)> = HashMap::new();
    let mut notes = Vec::new();
    for md in mds {
        // Metadata block at the end: "key: value" lines after the last blank line.
        let (content, meta) = match md.rfind("\n\nid: ") {
            Some(i) => (&md[..i], &md[i + 2..]),
            None => (md.as_str(), ""),
        };
        let m: HashMap<&str, &str> = meta.lines().filter_map(|l| l.split_once(": ")).collect();
        let (title, body) = content.split_once("\n\n").unwrap_or((content, ""));
        match m.get("type_").copied() {
            Some("2") => {
                folders.insert(m.get("id").unwrap_or(&"").to_string(), (title.trim().to_owned(), m.get("parent_id").unwrap_or(&"").to_string()));
            }
            Some("1") => notes.push(Note {
                title: title.trim().to_owned(),
                body: body.to_owned(),
                parent: m.get("parent_id").unwrap_or(&"").to_string(),
                created: m.get("created_time").and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok()).map(|d| d.with_timezone(&chrono::Local).naive_local()),
            }),
            _ => {}
        }
    }
    let full_name = |id: &str| {
        let mut parts = Vec::new();
        let mut cur = id.to_owned();
        while let Some((n, p)) = folders.get(&cur) {
            parts.push(n.clone());
            cur = p.clone();
            if parts.len() > 10 {
                break;
            }
        }
        parts.reverse();
        if parts.is_empty() { "Joplin".into() } else { parts.join(" › ") }
    };
    let mut sections: Vec<ImpSection> = Vec::new();
    for n in notes {
        let name = full_name(&n.parent);
        let mut b = Builder::new(nb, o);
        // Resource images: ![alt](:/id)
        let mut text = String::new();
        for line in n.body.lines() {
            if let Some(a) = line.find("](:/") {
                let id: String = line[a + 4..].chars().take_while(|c| c.is_ascii_hexdigit()).collect();
                if let Some((bytes, ext)) = resources.get(&id) {
                    md_with_tables(&mut b, &std::mem::take(&mut text));
                    if matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp") {
                        b.image(bytes, ext, None, "");
                    } else {
                        b.file(bytes, &format!("{id}.{ext}"));
                    }
                    continue;
                }
            }
            text.push_str(line);
            text.push('\n');
        }
        md_with_tables(&mut b, &text);
        let mut pg = b.finish(n.title);
        pg.created = n.created;
        match sections.iter_mut().find(|s| s.name == name) {
            Some(s) => s.pages.push(pg),
            None => {
                let c = (sections.len() % 16) as u8;
                sections.push(ImpSection { name, color: c, pages: vec![pg] });
            }
        }
    }
    out.name = stem_of(path);
    out.nodes.extend(sections.into_iter().map(ImpNode::Section));
    Ok(())
}

// ----------------------------------------------------------------------------
// Evernote ENEX with resources

fn between<'a>(s: &'a str, a: &str, b: &str) -> Option<&'a str> {
    let i = s.find(a)? + a.len();
    let j = s[i..].find(b)? + i;
    Some(&s[i..j])
}

fn enex(xml: &str, nb: &Notebook, o: &Opts) -> Vec<ImpPage> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(a) = rest.find("<note>") {
        let Some(e) = rest[a..].find("</note>") else { break };
        let note = &rest[a..a + e];
        rest = &rest[a + e + 7..];
        let title = between(note, "<title>", "</title>").map(|t| html_to_markup(t)).unwrap_or_else(|| "Без названия".into());
        let mut content = between(note, "<content>", "</content>").unwrap_or("").to_owned();
        if let Some(c) = content.strip_prefix("<![CDATA[") {
            content = c.trim_end_matches("]]>").to_owned();
        }
        // Resources by MD5 hash of their data.
        let mut res: HashMap<String, (Vec<u8>, String)> = HashMap::new();
        let mut rr = note;
        while let Some(i) = rr.find("<resource>") {
            let Some(j) = rr[i..].find("</resource>") else { break };
            let r = &rr[i..i + j];
            rr = &rr[i + j + 11..];
            let data = between(r, ">", "</data>").and_then(|_| r.find("<data").map(|k| &r[k..])).and_then(|d| between(d, ">", "</data>")).map(b64decode).unwrap_or_default();
            let mime = between(r, "<mime>", "</mime>").unwrap_or("").to_owned();
            res.insert(md5_hex(&data), (data, mime.split('/').nth(1).unwrap_or("").to_owned()));
        }
        // <en-media hash="..."/> → <img src="hash">.
        let mut html = String::new();
        let mut c = content.as_str();
        while let Some(i) = c.find("<en-media") {
            html.push_str(&c[..i]);
            let end = c[i..].find('>').map(|e| i + e + 1).unwrap_or(c.len());
            let tag = &c[i..end];
            if let Some(h) = attr(tag, "hash") {
                html.push_str(&format!("<img src=\"enres:{h}\">"));
            }
            c = &c[end..];
            if c.starts_with("</en-media>") {
                c = &c[11..];
            }
        }
        html.push_str(c);
        let mut b = Builder::new(nb, o);
        let files: HashMap<String, (Vec<u8>, String)> = res.into_iter().map(|(k, v)| (format!("enres:{k}"), v)).collect();
        html_into(&mut b, &html, &|src| files.get(src).cloned());
        out.push(b.finish(title));
    }
    out
}

/// MD5 (RFC 1321) for matching Evernote resources.
pub fn md5_hex(data: &[u8]) -> String {
    let s: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10, 15, 21, 6,
        10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
    let k: Vec<u32> = (0..64).map(|i| ((i as f64 + 1.0).sin().abs() * 4294967296.0) as u32).collect();
    let (mut a0, mut b0, mut c0, mut d0) = (0x67452301u32, 0xefcdab89u32, 0x98badcfeu32, 0x10325476u32);
    let mut msg = data.to_vec();
    let bit_len = (data.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_le_bytes());
    for chunk in msg.chunks(64) {
        let m: Vec<u32> = chunk.chunks(4).map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]])).collect();
        let (mut a, mut b, mut c, mut d) = (a0, b0, c0, d0);
        for i in 0..64 {
            let (f, g) = match i / 16 {
                0 => ((b & c) | (!b & d), i),
                1 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                2 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            let f2 = f.wrapping_add(a).wrapping_add(k[i]).wrapping_add(m[g]);
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(f2.rotate_left(s[i]));
        }
        a0 = a0.wrapping_add(a);
        b0 = b0.wrapping_add(b);
        c0 = c0.wrapping_add(c);
        d0 = d0.wrapping_add(d);
    }
    [a0, b0, c0, d0].iter().flat_map(|v| v.to_le_bytes()).map(|b| format!("{b:02x}")).collect()
}

// ----------------------------------------------------------------------------
// Word images, ODT, RTF

fn docx_images(path: &Path, b: &mut Builder) {
    let Ok(f) = std::fs::File::open(path) else { return };
    let Ok(mut z) = zip::ZipArchive::new(f) else { return };
    let names: Vec<String> = (0..z.len()).filter_map(|i| z.by_index(i).ok().map(|f| f.name().to_owned())).filter(|n| n.starts_with("word/media/")).collect();
    for n in names {
        if let Ok(mut f) = z.by_name(&n) {
            let mut buf = Vec::new();
            if f.read_to_end(&mut buf).is_ok() && image::load_from_memory(&buf).is_ok() {
                let ext = ext_of(Path::new(&n));
                b.image(&buf, &ext, None, "");
            }
        }
    }
}

fn odt(path: &Path, nb: &Notebook, o: &Opts) -> Result<ImpPage, String> {
    let f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut z = zip::ZipArchive::new(f).map_err(|e| e.to_string())?;
    let mut xml = String::new();
    z.by_name("content.xml").map_err(|e| e.to_string())?.read_to_string(&mut xml).map_err(|e| e.to_string())?;
    let mut text = String::new();
    let mut i = 0;
    let bytes = xml.as_bytes();
    let mut heading: Option<usize> = None;
    let mut in_list = 0;
    while i < xml.len() {
        if bytes[i] == b'<' {
            let end = xml[i..].find('>').map(|e| i + e).unwrap_or(xml.len() - 1);
            let tag = &xml[i + 1..end];
            if tag.starts_with("text:h") {
                heading = attr(tag, "text:outline-level").and_then(|l| l.parse().ok()).or(Some(1));
                text.push_str(&format!("{} ", "#".repeat(heading.unwrap_or(1).min(6))));
            } else if tag.starts_with("text:list-item") {
                text.push_str(&format!("{}- ", "  ".repeat(in_list.max(1) - 1)));
            } else if tag.starts_with("text:list") && !tag.starts_with("text:list-") {
                in_list += 1;
            } else if tag.starts_with("/text:list") && !tag.starts_with("/text:list-") {
                in_list = in_list.saturating_sub(1);
            } else if tag.starts_with("/text:p") || tag.starts_with("/text:h") {
                text.push('\n');
                heading = None;
            } else if tag.starts_with("text:line-break") {
                text.push('\n');
            } else if tag.starts_with("text:tab") {
                text.push('\t');
            } else if tag.starts_with("text:s") && tag.ends_with('/') {
                let n = attr(tag, "text:c").and_then(|c| c.parse().ok()).unwrap_or(1);
                text.push_str(&" ".repeat(n));
            }
            i = end + 1;
            continue;
        }
        let next = xml[i..].find('<').map(|n| i + n).unwrap_or(xml.len());
        text.push_str(&esc_md(&crate::export::html_to_markup(&xml[i..next])));
        i = next;
    }
    let _ = heading;
    let mut b = Builder::new(nb, o);
    b.text(&text);
    Ok(b.finish(stem_of(path)))
}

pub fn rtf_to_text(data: &[u8]) -> String {
    let s = latin1(data);
    let cs: Vec<char> = s.chars().collect();
    let mut out: Vec<u8> = Vec::new();
    let mut uni: Vec<u16> = Vec::new();
    let mut result = String::new();
    let mut codepage = 1252;
    let mut skip_depth: Option<usize> = None;
    let mut depth = 0usize;
    let mut uc_skip = 1usize;
    let mut i = 0;
    let flush = |out: &mut Vec<u8>, uni: &mut Vec<u16>, result: &mut String, cp: u32| {
        if !out.is_empty() {
            result.push_str(&if cp == 1251 { cp1251(out) } else { latin1(out) });
            out.clear();
        }
        if !uni.is_empty() {
            result.push_str(&String::from_utf16_lossy(uni));
            uni.clear();
        }
    };
    while i < cs.len() {
        let c = cs[i];
        match c {
            '{' => {
                depth += 1;
                // Destinations to skip.
                let ahead: String = cs[i + 1..cs.len().min(i + 16)].iter().collect();
                if skip_depth.is_none() && ["\\*", "\\fonttbl", "\\colortbl", "\\stylesheet", "\\info", "\\pict", "\\object", "\\header", "\\footer"].iter().any(|d| ahead.starts_with(d)) {
                    skip_depth = Some(depth);
                }
                i += 1;
            }
            '}' => {
                if skip_depth == Some(depth) {
                    skip_depth = None;
                }
                depth = depth.saturating_sub(1);
                i += 1;
            }
            '\\' => {
                let n = cs.get(i + 1).copied().unwrap_or(' ');
                if n == '\'' {
                    if skip_depth.is_none() {
                        if let Ok(v) = u8::from_str_radix(&cs[i + 2..(i + 4).min(cs.len())].iter().collect::<String>(), 16) {
                            if !uni.is_empty() {
                                flush(&mut out, &mut uni, &mut result, codepage);
                            }
                            out.push(v);
                        }
                    }
                    i += 4;
                    continue;
                }
                if !n.is_ascii_alphabetic() {
                    if skip_depth.is_none() && matches!(n, '\\' | '{' | '}') {
                        out.push(n as u8);
                    }
                    i += 2;
                    continue;
                }
                let mut j = i + 1;
                while j < cs.len() && cs[j].is_ascii_alphabetic() {
                    j += 1;
                }
                let word: String = cs[i + 1..j].iter().collect();
                let mut k = j;
                if k < cs.len() && (cs[k] == '-' || cs[k].is_ascii_digit()) {
                    k += 1;
                    while k < cs.len() && cs[k].is_ascii_digit() {
                        k += 1;
                    }
                }
                let num: Option<i32> = cs[j..k].iter().collect::<String>().parse().ok();
                if k < cs.len() && cs[k] == ' ' {
                    k += 1;
                }
                i = k;
                match word.as_str() {
                    "ansicpg" => codepage = num.unwrap_or(1252) as u32,
                    "uc" => uc_skip = num.unwrap_or(1).max(0) as usize,
                    _ if skip_depth.is_some() => {}
                    "par" | "line" | "row" => {
                        flush(&mut out, &mut uni, &mut result, codepage);
                        result.push('\n');
                    }
                    "tab" | "cell" => {
                        flush(&mut out, &mut uni, &mut result, codepage);
                        result.push('\t');
                    }
                    "u" => {
                        if let Some(v) = num {
                            flush(&mut out, &mut Vec::new(), &mut result, codepage);
                            uni.push(v as i16 as u16);
                            // Skip the replacement character(s).
                            let mut skipped = 0;
                            while skipped < uc_skip && i < cs.len() {
                                if cs[i] == '\\' && cs.get(i + 1) == Some(&'\'') {
                                    i += 4;
                                } else {
                                    i += 1;
                                }
                                skipped += 1;
                            }
                        }
                    }
                    _ => {}
                }
            }
            '\r' | '\n' => i += 1,
            _ => {
                if skip_depth.is_none() {
                    if !uni.is_empty() {
                        flush(&mut out, &mut uni, &mut result, codepage);
                    }
                    out.push(c as u32 as u8);
                }
                i += 1;
            }
        }
    }
    flush(&mut out, &mut uni, &mut result, codepage);
    esc_md(result.trim())
}

// ----------------------------------------------------------------------------
// Xournal++

fn xournal(path: &Path, nb: &Notebook, o: &Opts) -> Result<Vec<ImpPage>, String> {
    let raw = std::fs::read(path).map_err(|e| e.to_string())?;
    let xml = if raw.starts_with(&[0x1f, 0x8b]) {
        let mut s = String::new();
        flate2::read::GzDecoder::new(&raw[..]).read_to_string(&mut s).map_err(|e| e.to_string())?;
        s
    } else {
        String::from_utf8_lossy(&raw).to_string()
    };
    let pt = 96.0 / 72.0;
    let mut pages = Vec::new();
    let mut rest = xml.as_str();
    let mut n = 0;
    while let Some(a) = rest.find("<page") {
        let Some(e) = rest[a..].find("</page>") else { break };
        let page = &rest[a..a + e];
        rest = &rest[a + e + 7..];
        n += 1;
        let mut b = Builder::new(nb, o);
        if let Some(bg) = page.find("<background").map(|i| &page[i..]) {
            let tag = &bg[..bg.find('>').unwrap_or(bg.len())];
            b.page.pattern = Some(match attr(tag, "style").as_deref() {
                Some("lined") => Pattern::LinesMedium,
                Some("ruled") => Pattern::LinesNarrow,
                Some("graph") => Pattern::GridSmall,
                Some("dotted") => Pattern::DotsSmall,
                _ => Pattern::Plain,
            });
        }
        let mut s = page;
        while let Some(i) = s.find('<') {
            s = &s[i..];
            let tag_end = s.find('>').unwrap_or(s.len() - 1);
            let tag = &s[..tag_end + 1];
            let name: String = tag[1..].chars().take_while(|c| c.is_ascii_alphabetic()).collect();
            let close = format!("</{name}>");
            let (body, adv) = match s.find(&close) {
                Some(c) if matches!(name.as_str(), "stroke" | "text" | "image" | "teximage") => (&s[tag_end + 1..c], c + close.len()),
                _ => ("", tag_end + 1),
            };
            match name.as_str() {
                "stroke" => {
                    let nums: Vec<f32> = body.split_whitespace().filter_map(|v| v.parse().ok()).collect();
                    let widths: Vec<f32> = attr(tag, "width").unwrap_or_default().split_whitespace().filter_map(|v| v.parse().ok()).collect();
                    let base = widths.first().copied().unwrap_or(1.0);
                    let maxw = widths.iter().copied().fold(base, f32::max);
                    let pts: Vec<InkPoint> = nums
                        .chunks_exact(2)
                        .enumerate()
                        .map(|(k, c)| {
                            let w = widths.get(k + 1).copied().unwrap_or(base);
                            InkPoint(c[0] * pt, c[1] * pt, (w / maxw.max(0.01)).clamp(0.05, 1.0))
                        })
                        .collect();
                    if pts.is_empty() || attr(tag, "tool").as_deref() == Some("eraser") {
                        s = &s[adv.min(s.len())..];
                        continue;
                    }
                    let col = attr(tag, "color").unwrap_or_else(|| "#000000ff".into());
                    let c = parse_hex_color(&col).unwrap_or([0, 0, 0, 255]);
                    let hl = attr(tag, "tool").as_deref() == Some("highlighter");
                    let mut st = Stroke::new(pts, c, maxw * pt, if hl { InkKind::Highlighter } else { InkKind::Pen });
                    if widths.len() <= 1 {
                        st.style = Arc::new(StrokeStyle { no_pressure: true, ..Default::default() });
                    }
                    if hl {
                        st.layer = 0;
                        if st.color[3] == 255 {
                            st.color[3] = 110;
                        }
                    }
                    b.page.strokes.push(st);
                }
                "text" => {
                    let x = attr(tag, "x").and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0) * pt;
                    let y = attr(tag, "y").and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0) * pt;
                    let txt = crate::export::html_to_markup(body);
                    let id = b.text_at(&esc_md(&txt), x, y, 400.0);
                    if let (Some(col), Some(ItemKind::Text(t))) = (attr(tag, "color").and_then(|c| parse_hex_color(&c)), b.page.item_mut(id).map(|i| &mut i.kind)) {
                        t.color = col;
                        if let Some(sz) = attr(tag, "size").and_then(|v| v.parse::<f32>().ok()) {
                            t.size = sz * pt;
                        }
                    }
                }
                "image" => {
                    let l = attr(tag, "left").and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0) * pt;
                    let t = attr(tag, "top").and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0) * pt;
                    let r = attr(tag, "right").and_then(|v| v.parse::<f32>().ok()).unwrap_or(100.0) * pt;
                    let bytes = b64decode(body.trim());
                    if !bytes.is_empty() {
                        b.image_at(&bytes, "png", l, t, Some((r - l).max(10.0)), "");
                    }
                }
                _ => {}
            }
            s = &s[adv.min(s.len())..];
        }
        pages.push(b.finish(format!("{} — стр. {n}", stem_of(path))));
    }
    if pages.len() == 1 {
        pages[0].title = stem_of(path);
    }
    Ok(pages)
}

fn parse_hex_color(s: &str) -> Option<[u8; 4]> {
    let h = s.trim().trim_start_matches('#');
    let v = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
    match h.len() {
        6 => Some([v(0)?, v(2)?, v(4)?, 255]),
        8 => Some([v(0)?, v(2)?, v(4)?, v(6)?]),
        3 => {
            let e = |i: usize| u8::from_str_radix(&h[i..i + 1].repeat(2), 16).ok();
            Some([e(0)?, e(1)?, e(2)?, 255])
        }
        _ => None,
    }
}

// ----------------------------------------------------------------------------
// SVG drawings as ink

fn named_color(s: &str) -> Option<[u8; 4]> {
    Some(match s.trim().to_lowercase().as_str() {
        "black" => [0, 0, 0, 255],
        "white" => [255, 255, 255, 255],
        "red" => [255, 0, 0, 255],
        "green" => [0, 128, 0, 255],
        "blue" => [0, 0, 255, 255],
        "yellow" => [255, 255, 0, 255],
        "orange" => [255, 165, 0, 255],
        "gray" | "grey" => [128, 128, 128, 255],
        "purple" => [128, 0, 128, 255],
        _ => return None,
    })
}

fn svg_color(s: &str) -> Option<[u8; 4]> {
    let s = s.trim();
    if s == "none" || s.is_empty() {
        return None;
    }
    if let Some(r) = s.strip_prefix("rgb(") {
        let v: Vec<u8> = r.trim_end_matches(')').split(',').filter_map(|x| x.trim().parse().ok()).collect();
        return (v.len() == 3).then(|| [v[0], v[1], v[2], 255]);
    }
    parse_hex_color(s).or_else(|| named_color(s))
}

fn style_prop(tag: &str, name: &str) -> Option<String> {
    if let Some(st) = attr(tag, "style") {
        for kv in st.split(';') {
            if let Some((k, v)) = kv.split_once(':') {
                if k.trim() == name {
                    return Some(v.trim().to_owned());
                }
            }
        }
    }
    attr(tag, name)
}

fn svg_strokes(svg: &str) -> Vec<Stroke> {
    let mut out = Vec::new();
    let mut s = svg;
    while let Some(i) = s.find('<') {
        s = &s[i..];
        let end = s.find('>').unwrap_or(s.len() - 1);
        let tag = &s[..=end];
        let name: String = tag[1..].chars().take_while(|c| c.is_ascii_alphabetic()).collect();
        let stroke_col = style_prop(tag, "stroke").and_then(|c| svg_color(&c));
        let fill_col = style_prop(tag, "fill").and_then(|c| svg_color(&c));
        let width = style_prop(tag, "stroke-width").and_then(|w| w.trim_end_matches("px").parse::<f32>().ok()).unwrap_or(1.0);
        let num = |k: &str| attr(tag, k).and_then(|v| v.trim_end_matches("px").parse::<f32>().ok()).unwrap_or(0.0);
        let polys: Vec<(Vec<egui::Pos2>, bool)> = match name.as_str() {
            "path" => attr(tag, "d").map(|d| svg_path(&d)).unwrap_or_default(),
            "polyline" | "polygon" => {
                let v: Vec<f32> = attr(tag, "points").unwrap_or_default().split([' ', ',']).filter_map(|x| x.parse().ok()).collect();
                vec![(v.chunks_exact(2).map(|c| egui::pos2(c[0], c[1])).collect(), name == "polygon")]
            }
            "line" => vec![(vec![egui::pos2(num("x1"), num("y1")), egui::pos2(num("x2"), num("y2"))], false)],
            "rect" => {
                let (x, y, w, h) = (num("x"), num("y"), num("width"), num("height"));
                vec![(vec![egui::pos2(x, y), egui::pos2(x + w, y), egui::pos2(x + w, y + h), egui::pos2(x, y + h)], true)]
            }
            "circle" | "ellipse" => {
                let (cx, cy) = (num("cx"), num("cy"));
                let (rx, ry) = if name == "circle" { (num("r"), num("r")) } else { (num("rx"), num("ry")) };
                vec![(crate::ink::ellipse_points(egui::Rect::from_center_size(egui::pos2(cx, cy), egui::vec2(rx * 2.0, ry * 2.0)), 64), true)]
            }
            _ => Vec::new(),
        };
        for (pts, closed) in polys {
            if pts.len() < 2 {
                continue;
            }
            let color = stroke_col.or(if fill_col.is_none() { Some([0, 0, 0, 255]) } else { None });
            let mut st = crate::ink::make_shape_stroke(pts, closed, color.unwrap_or(fill_col.unwrap_or([0, 0, 0, 255])), if stroke_col.is_some() { width } else { 0.3 }, fill_col);
            if !closed {
                st.fill = None;
            }
            out.push(st);
        }
        s = &s[end + 1..];
    }
    out
}

/// SVG path data → polylines (curves flattened).
fn svg_path(d: &str) -> Vec<(Vec<egui::Pos2>, bool)> {
    let mut toks: Vec<String> = Vec::new();
    let mut cur = String::new();
    for c in d.chars() {
        if c.is_ascii_alphabetic() && c != 'e' && c != 'E' {
            if !cur.is_empty() {
                toks.push(std::mem::take(&mut cur));
            }
            toks.push(c.to_string());
        } else if c == ',' || c.is_whitespace() || (c == '-' && !cur.is_empty() && !cur.ends_with(['e', 'E'])) {
            if !cur.is_empty() {
                toks.push(std::mem::take(&mut cur));
            }
            if c == '-' {
                cur.push(c);
            }
        } else if c == '.' && cur.contains('.') {
            toks.push(std::mem::take(&mut cur));
            cur.push(c);
        } else {
            cur.push(c);
        }
    }
    if !cur.is_empty() {
        toks.push(cur);
    }
    let mut out = Vec::new();
    let mut pts: Vec<egui::Pos2> = Vec::new();
    let mut p = egui::pos2(0.0, 0.0);
    let mut start = p;
    let mut last_ctrl: Option<egui::Pos2> = None;
    let mut cmd = 'M';
    let mut i = 0;
    let num = |i: &mut usize| -> Option<f32> {
        let v = toks.get(*i)?.parse().ok()?;
        *i += 1;
        Some(v)
    };
    while i < toks.len() {
        if toks[i].chars().next().is_some_and(|c| c.is_ascii_alphabetic()) {
            cmd = toks[i].chars().next().unwrap();
            i += 1;
            if cmd == 'Z' || cmd == 'z' {
                if pts.len() > 1 {
                    out.push((std::mem::take(&mut pts), true));
                }
                p = start;
                continue;
            }
        }
        let rel = cmd.is_ascii_lowercase();
        let base = if rel { p.to_vec2() } else { egui::Vec2::ZERO };
        match cmd.to_ascii_uppercase() {
            'M' => {
                let (Some(x), Some(y)) = (num(&mut i), num(&mut i)) else { break };
                if pts.len() > 1 {
                    out.push((std::mem::take(&mut pts), false));
                }
                pts.clear();
                p = egui::pos2(x, y) + base;
                start = p;
                pts.push(p);
                cmd = if rel { 'l' } else { 'L' };
            }
            'L' => {
                let (Some(x), Some(y)) = (num(&mut i), num(&mut i)) else { break };
                p = egui::pos2(x, y) + base;
                pts.push(p);
            }
            'H' => {
                let Some(x) = num(&mut i) else { break };
                p = egui::pos2(if rel { p.x + x } else { x }, p.y);
                pts.push(p);
            }
            'V' => {
                let Some(y) = num(&mut i) else { break };
                p = egui::pos2(p.x, if rel { p.y + y } else { y });
                pts.push(p);
            }
            'C' | 'S' | 'Q' | 'T' => {
                let up = cmd.to_ascii_uppercase();
                let (c1, c2, e);
                match up {
                    'C' => {
                        let v: Option<Vec<f32>> = (0..6).map(|_| num(&mut i)).collect();
                        let Some(v) = v else { break };
                        c1 = egui::pos2(v[0], v[1]) + base;
                        c2 = egui::pos2(v[2], v[3]) + base;
                        e = egui::pos2(v[4], v[5]) + base;
                    }
                    'S' => {
                        let v: Option<Vec<f32>> = (0..4).map(|_| num(&mut i)).collect();
                        let Some(v) = v else { break };
                        c1 = last_ctrl.map(|c| p + (p - c)).unwrap_or(p);
                        c2 = egui::pos2(v[0], v[1]) + base;
                        e = egui::pos2(v[2], v[3]) + base;
                    }
                    'Q' => {
                        let v: Option<Vec<f32>> = (0..4).map(|_| num(&mut i)).collect();
                        let Some(v) = v else { break };
                        let q = egui::pos2(v[0], v[1]) + base;
                        e = egui::pos2(v[2], v[3]) + base;
                        c1 = p + (q - p) * (2.0 / 3.0);
                        c2 = e + (q - e) * (2.0 / 3.0);
                    }
                    _ => {
                        let v: Option<Vec<f32>> = (0..2).map(|_| num(&mut i)).collect();
                        let Some(v) = v else { break };
                        let q = last_ctrl.map(|c| p + (p - c)).unwrap_or(p);
                        e = egui::pos2(v[0], v[1]) + base;
                        c1 = p + (q - p) * (2.0 / 3.0);
                        c2 = e + (q - e) * (2.0 / 3.0);
                    }
                }
                let s0 = p;
                for k in 1..=16 {
                    let t = k as f32 / 16.0;
                    let u = 1.0 - t;
                    let q = s0.to_vec2() * u * u * u + c1.to_vec2() * 3.0 * u * u * t + c2.to_vec2() * 3.0 * u * t * t + e.to_vec2() * t * t * t;
                    pts.push(q.to_pos2());
                }
                last_ctrl = Some(c2);
                p = e;
                continue;
            }
            'A' => {
                let v: Option<Vec<f32>> = (0..7).map(|_| num(&mut i)).collect();
                let Some(v) = v else { break };
                p = egui::pos2(v[5], v[6]) + base;
                pts.push(p);
            }
            _ => {
                i += 1;
            }
        }
        last_ctrl = None;
    }
    if pts.len() > 1 {
        out.push((pts, false));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn helpers() {
        assert_eq!(md5_hex(b""), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(md5_hex(b"abc"), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(b64decode("TWFu"), b"Man");
        assert_eq!(cp1251(&[0xCF, 0xF0, 0xE8, 0xE2, 0xE5, 0xF2]), "Привет");
        assert_eq!(svg_path("M0 0 L10 10 Z").len(), 1);
        assert!(rtf_to_text(br"{\rtf1\ansi\ansicpg1251 {\fonttbl{\f0 Arial;}}\'cf\'f0\'e8\'e2\'e5\'f2\par Hi}").contains("Привет"));
        assert_eq!(clean_name("Заметка 0123456789abcdef0123456789abcdef"), "Заметка");
        let (html, res) = parse_mime(b"MIME-Version: 1.0\r\nContent-Type: multipart/related; boundary=\"XX\"\r\n\r\n--XX\r\nContent-Type: text/html; charset=utf-8\r\nContent-Transfer-Encoding: quoted-printable\r\n\r\n<p>Hi=20there</p>\r\n--XX\r\nContent-Type: image/png\r\nContent-Transfer-Encoding: base64\r\nContent-Location: a.png\r\n\r\nTWFu\r\n--XX--\r\n");
        assert!(html.contains("Hi there"));
        assert_eq!(res.get("a.png").map(|r| r.0.clone()), Some(b"Man".to_vec()));
    }
}

#[cfg(test)]
mod files {
    use super::*;

    #[test]
    fn formats() {
        let root = std::env::temp_dir().join(format!("nn-import-{}", new_id()));
        let src = root.join("src");
        std::fs::create_dir_all(src.join("Vault/Sub")).unwrap();
        let nb = Notebook::create(&root, "t").unwrap();
        let o = Opts { author: "a".into(), font: String::new(), size: 15.0 };
        let png = {
            let mut v = Vec::new();
            image::RgbaImage::new(4, 3).write_to(&mut std::io::Cursor::new(&mut v), image::ImageFormat::Png).unwrap();
            v
        };
        std::fs::write(src.join("Vault/pic.png"), &png).unwrap();
        std::fs::write(src.join("Vault/Note.md"), "# Title\nHello **world**\n![[pic.png]]\n| a | b |\n|---|---|\n| 1 | 2 |\n").unwrap();
        std::fs::write(src.join("Vault/Sub/Other.md"), "text").unwrap();
        let r = import(&src.join("Vault"), &nb, &o).unwrap();
        let ImpNode::Section(s) = &r.nodes[0] else { panic!() };
        assert_eq!(s.pages[0].title, "Title");
        let kinds: Vec<&str> = s.pages[0].page.items.iter().map(|i| match i.kind { ItemKind::Text(_) => "t", ItemKind::Image(_) => "i", ItemKind::Table(_) => "tb", _ => "?" }).collect();
        assert_eq!(kinds, ["t", "i", "tb"]);
        assert_eq!(r.nodes.len(), 2);

        let mht = format!("MIME-Version: 1.0\r\nContent-Type: multipart/related; boundary=\"B\"\r\n\r\n--B\r\nContent-Type: text/html; charset=utf-8\r\n\r\n<html><title>Web</title><body><p>Hi</p><img src=\"x.png\"><p>Bye</p></body></html>\r\n--B\r\nContent-Type: image/png\r\nContent-Transfer-Encoding: base64\r\nContent-Location: file:///C:/x.png\r\n\r\n{}\r\n--B--\r\n", b64(&png));
        std::fs::write(src.join("a.mht"), mht).unwrap();
        let r = import(&src.join("a.mht"), &nb, &o).unwrap();
        assert_eq!(r.pages[0].title, "Web");
        assert_eq!(r.pages[0].page.items.len(), 3);

        std::fs::write(src.join("d.svg"), r#"<svg><path d="M0 0 C 10 10 20 10 30 0" stroke="red" fill="none"/><circle cx="5" cy="5" r="4" fill="blue"/></svg>"#).unwrap();
        let r = import(&src.join("d.svg"), &nb, &o).unwrap();
        assert_eq!(r.pages[0].page.strokes.len(), 2);

        let xopp = r##"<xournal><page width="100" height="100"><background type="solid" style="graph"/><layer><stroke tool="pen" color="#ff0000ff" width="2 1 2">0 0 10 10 20 0</stroke><text x="5" y="5" size="12" color="#000000ff">Hey</text></layer></page></xournal>"##;
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        std::io::Write::write_all(&mut gz, xopp.as_bytes()).unwrap();
        std::fs::write(src.join("x.xopp"), gz.finish().unwrap()).unwrap();
        let r = import(&src.join("x.xopp"), &nb, &o).unwrap();
        assert_eq!(r.pages[0].page.strokes.len(), 1);
        assert_eq!(r.pages[0].page.items.len(), 1);

        std::fs::write(src.join("k.json"), r#"{"title":"K","textContent":"","listContent":[{"text":"milk","isChecked":true}]}"#).unwrap();
        let r = import(&src.join("k.json"), &nb, &o).unwrap();
        let ItemKind::Text(t) = &r.pages[0].page.items[0].kind else { panic!() };
        assert!(t.text.contains("[x] milk"), "{}", t.text);

        let enex = format!("<en-export><note><title>E</title><content><![CDATA[<en-note>A<en-media hash=\"{}\" type=\"image/png\"/></en-note>]]></content><resource><data encoding=\"base64\">{}</data><mime>image/png</mime></resource></note></en-export>", md5_hex(&png), b64(&png));
        std::fs::write(src.join("e.enex"), enex).unwrap();
        let r = import(&src.join("e.enex"), &nb, &o).unwrap();
        assert!(r.pages[0].page.items.iter().any(|i| matches!(i.kind, ItemKind::Image(_))));
        let _ = std::fs::remove_dir_all(&root);
    }

    fn b64(d: &[u8]) -> String {
        const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut s = String::new();
        for c in d.chunks(3) {
            let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
            for k in 0..4 {
                s.push(if k <= c.len() { T[(n >> (18 - 6 * k) & 63) as usize] as char } else { '=' });
            }
        }
        s
    }
}

#[cfg(test)]
mod stress {
    use super::*;

    /// Builds a large notebook for performance checks: NN_BIG_DIR=<folder> cargo test big_notebook -- --ignored
    #[test]
    #[ignore]
    fn big_notebook() {
        let Some(dir) = std::env::var_os("NN_BIG_DIR") else { return };
        let mut nb = Notebook::create(Path::new(&dir), "Большой блокнот").unwrap();
        let o = Opts { author: "test".into(), font: String::new(), size: 15.0 };
        let mut tree = Vec::new();
        for s in 0..30 {
            let mut sec = Section::new(&format!("Раздел {s}"), (s % 16) as u8);
            for i in 0..200 {
                let mut b = Builder::new(&nb, &o);
                b.text(&format!("# Страница {i}\nТекст заметки номер {i} в разделе {s}. **Важно** и *курсив*.\n- пункт один\n- пункт два"));
                for k in 0..20 {
                    let pts: Vec<InkPoint> = (0..60).map(|j| InkPoint(100.0 + j as f32 * 4.0, 300.0 + k as f32 * 12.0 + (j as f32 * 0.3).sin() * 6.0, 0.6)).collect();
                    b.page.strokes.push(Stroke::new(pts, [30, 30, 30, 255], 2.0, InkKind::Pen));
                }
                let mut pm = PageMeta::new(&format!("Страница {i}"), "test");
                let mut page = b.finish(String::new()).page;
                page.id = pm.id;
                pm.level = (i % 7 == 3) as u8;
                crate::storage::write_page(&nb.page_path(pm.id), &page, None);
                sec.pages.push(pm);
            }
            tree.push(Node::Section(sec));
        }
        nb.meta.tree = tree;
        nb.save_meta();
        println!("{}", nb.path.display());
    }
}
