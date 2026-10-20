//! Local full-text index over page text, titles, tags, recognized ink and
//! OCR text. Built in the background and updated on every save.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use chrono::NaiveDateTime;

use crate::crypto::Key;
use crate::model::*;

pub const K_TEXT: u32 = 1;
pub const K_IMAGE: u32 = 2;
pub const K_INK: u32 = 4;
pub const K_FILE: u32 = 8;
pub const K_TABLE: u32 = 16;

#[derive(Clone)]
pub struct Entry {
    pub nb: PathBuf,
    pub nb_name: String,
    pub section: Id,
    pub section_name: String,
    pub page: Id,
    pub title: String,
    pub author: String,
    pub modified: NaiveDateTime,
    pub text: String,
    pub lower: String,
    pub tags: Vec<u32>,
    pub authors: Vec<String>,
    pub kinds: u32,
}

#[derive(Default)]
pub struct Index {
    pub entries: Arc<RwLock<Vec<Entry>>>,
    pub building: Arc<AtomicBool>,
    /// Bumped on every change, so views can cache what they derive from the entries.
    pub generation: Arc<AtomicU64>,
}

pub struct PageSrc {
    pub nb: PathBuf,
    pub nb_name: String,
    pub section: Id,
    pub section_name: String,
    pub meta: PageMeta,
    pub path: PathBuf,
    pub key: Option<Key>,
}

pub fn entry_for(src: &PageSrc, page: &Page) -> Entry {
    let mut kinds = 0;
    let mut tags = Vec::new();
    let mut authors = vec![src.meta.author.clone()];
    for it in &page.items {
        kinds |= match it.kind {
            ItemKind::Image(_) => K_IMAGE,
            ItemKind::File(_) => K_FILE,
            ItemKind::Table(_) => K_TABLE,
            _ => K_TEXT,
        };
        tags.extend(it.tags.iter().copied());
        if !authors.contains(&it.author) {
            authors.push(it.author.clone());
        }
    }
    if !page.strokes.is_empty() {
        kinds |= K_INK;
    }
    tags.sort_unstable();
    tags.dedup();
    let text = format!("{}\n{}", src.meta.title, page.plain_text());
    Entry {
        nb: src.nb.clone(),
        nb_name: src.nb_name.clone(),
        section: src.section,
        section_name: src.section_name.clone(),
        page: src.meta.id,
        title: src.meta.title.clone(),
        author: src.meta.author.clone(),
        modified: src.meta.modified,
        lower: text.to_lowercase(),
        text,
        tags,
        authors,
        kinds,
    }
}

impl Index {
    pub fn rebuild(&self, sources: Vec<PageSrc>, ctx: egui::Context) {
        let entries = self.entries.clone();
        let building = self.building.clone();
        let generation = self.generation.clone();
        building.store(true, Ordering::SeqCst);
        std::thread::Builder::new()
            .name("indexer".into())
            .spawn(move || {
                let mut out = Vec::with_capacity(sources.len());
                for src in &sources {
                    let page = std::fs::read(&src.path)
                        .ok()
                        .and_then(|d| crate::storage::decode_page(&d, src.key.as_ref()).ok())
                        .unwrap_or_else(|| Page::new(src.meta.id));
                    out.push(entry_for(src, &page));
                }
                *entries.write().unwrap() = out;
                generation.fetch_add(1, Ordering::SeqCst);
                building.store(false, Ordering::SeqCst);
                ctx.request_repaint();
            })
            .ok();
    }

    pub fn update(&self, e: Entry) {
        let mut v = self.entries.write().unwrap();
        match v.iter_mut().find(|x| x.page == e.page) {
            Some(x) => *x = e,
            None => v.push(e),
        }
        self.bump();
    }

    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }

    fn bump(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
    }

    pub fn remove(&self, page: Id) {
        self.entries.write().unwrap().retain(|e| e.page != page);
        self.bump();
    }

    pub fn remove_notebook(&self, nb: &std::path::Path) {
        self.entries.write().unwrap().retain(|e| e.nb != nb);
        self.bump();
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Scope {
    Page,
    Section,
    Notebook,
    #[default]
    All,
}

impl Scope {
    pub const ALL: [Scope; 4] = [Scope::Page, Scope::Section, Scope::Notebook, Scope::All];
    pub fn label(self) -> &'static str {
        match self {
            Scope::Page => "Страница",
            Scope::Section => "Раздел",
            Scope::Notebook => "Блокнот",
            Scope::All => "Все блокноты",
        }
    }
}

#[derive(Clone, Default)]
pub struct Filters {
    pub scope: Scope,
    pub tag: Option<u32>,
    pub author: String,
    pub kinds: u32,
    pub from: Option<chrono::NaiveDate>,
    pub to: Option<chrono::NaiveDate>,
}

pub struct Hit {
    pub nb: PathBuf,
    pub page: Id,
    pub title: String,
    pub location: String,
    pub snippet: String,
    pub score: f32,
}

pub fn query(
    idx: &Index,
    q: &str,
    f: &Filters,
    cur_nb: Option<&std::path::Path>,
    cur_section: Option<Id>,
    cur_page: Option<Id>,
) -> Vec<Hit> {
    let terms: Vec<String> = q.to_lowercase().split_whitespace().map(|s| s.to_owned()).collect();
    let entries = idx.entries.read().unwrap();
    let mut hits = Vec::new();
    for e in entries.iter() {
        match f.scope {
            Scope::Page if Some(e.page) != cur_page => continue,
            Scope::Section if Some(e.section) != cur_section => continue,
            Scope::Notebook if Some(e.nb.as_path()) != cur_nb => continue,
            _ => {}
        }
        if let Some(t) = f.tag {
            if !e.tags.contains(&t) {
                continue;
            }
        }
        if !f.author.is_empty() && !e.authors.iter().any(|a| a.to_lowercase().contains(&f.author.to_lowercase())) {
            continue;
        }
        if f.kinds != 0 && e.kinds & f.kinds == 0 {
            continue;
        }
        if let Some(d) = f.from {
            if e.modified.date() < d {
                continue;
            }
        }
        if let Some(d) = f.to {
            if e.modified.date() > d {
                continue;
            }
        }
        let mut score = 0.0;
        let mut first = None;
        let mut all = true;
        for t in &terms {
            let c = e.lower.matches(t.as_str()).count();
            if c == 0 {
                all = false;
                break;
            }
            score += c as f32;
            if e.title.to_lowercase().contains(t.as_str()) {
                score += 10.0;
            }
            if first.is_none() {
                first = e.lower.find(t.as_str());
            }
        }
        if !all || (terms.is_empty() && f.tag.is_none()) {
            continue;
        }
        let snippet = match first {
            Some(b) => snippet_at(&e.text, &e.lower, b),
            None => e.text.chars().take(120).collect(),
        };
        hits.push(Hit {
            nb: e.nb.clone(),
            page: e.page,
            title: e.title.clone(),
            location: format!("{} › {}", e.nb_name, e.section_name),
            snippet,
            score,
        });
    }
    hits.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    hits.truncate(500);
    hits
}

fn snippet_at(text: &str, lower: &str, byte: usize) -> String {
    // Lowercasing may change byte lengths; map via char count.
    let ci = lower[..byte.min(lower.len())].chars().count();
    let chars: Vec<char> = text.chars().collect();
    let a = ci.saturating_sub(40);
    let b = (ci + 80).min(chars.len());
    let mut s: String = chars[a..b].iter().collect();
    s = s.replace('\n', " ");
    if a > 0 {
        s.insert(0, '…');
    }
    if b < chars.len() {
        s.push('…');
    }
    s
}
