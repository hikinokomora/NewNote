//! Data model: notebooks, section groups, sections, pages and page content.

use chrono::{DateTime, Local, NaiveDateTime};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

pub type Id = Uuid;

pub fn new_id() -> Id {
    Uuid::new_v4()
}

pub fn now() -> NaiveDateTime {
    Local::now().naive_local()
}

/// 16 section colours.
pub const PALETTE: [[u8; 3]; 16] = [
    [0xE5, 0x73, 0x73],
    [0xF0, 0x62, 0x92],
    [0xBA, 0x68, 0xC8],
    [0x95, 0x75, 0xCD],
    [0x79, 0x86, 0xCB],
    [0x64, 0xB5, 0xF6],
    [0x4F, 0xC3, 0xF7],
    [0x4D, 0xD0, 0xE1],
    [0x4D, 0xB6, 0xAC],
    [0x81, 0xC7, 0x84],
    [0xAE, 0xD5, 0x81],
    [0xDC, 0xE7, 0x75],
    [0xFF, 0xD5, 0x4F],
    [0xFF, 0xB7, 0x4D],
    [0xFF, 0x8A, 0x65],
    [0xA1, 0x88, 0x7F],
];

pub fn palette_color(i: u8) -> egui::Color32 {
    let c = PALETTE[(i as usize) % 16];
    egui::Color32::from_rgb(c[0], c[1], c[2])
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PageSort {
    #[default]
    Manual,
    Title,
    Created,
    Modified,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Sensitivity {
    #[default]
    None,
    Public,
    Internal,
    Confidential,
}

impl Sensitivity {
    pub fn label(self) -> &'static str {
        match self {
            Sensitivity::None => "Без метки",
            Sensitivity::Public => "Публично",
            Sensitivity::Internal => "Внутреннее",
            Sensitivity::Confidential => "Конфиденциально",
        }
    }
    pub const ALL: [Sensitivity; 4] = [
        Sensitivity::None,
        Sensitivity::Public,
        Sensitivity::Internal,
        Sensitivity::Confidential,
    ];
}

/// Password protection header: salt for Argon2 and an encrypted verifier.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LockInfo {
    pub salt: String,
    pub verifier: String,
    #[serde(default)]
    pub no_copy: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PageMeta {
    pub id: Id,
    pub title: String,
    pub created: NaiveDateTime,
    pub modified: NaiveDateTime,
    #[serde(default)]
    pub level: u8,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub collapsed: bool,
    #[serde(default)]
    pub author: String,
}

impl PageMeta {
    pub fn new(title: &str, author: &str) -> Self {
        Self {
            id: new_id(),
            title: title.to_owned(),
            created: now(),
            modified: now(),
            level: 0,
            pinned: false,
            collapsed: false,
            author: author.to_owned(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Section {
    pub id: Id,
    pub name: String,
    pub color: u8,
    pub pages: Vec<PageMeta>,
    #[serde(default)]
    pub sort: PageSort,
    #[serde(default)]
    pub lock: Option<LockInfo>,
    #[serde(default)]
    pub default_template: Option<String>,
}

impl Section {
    pub fn new(name: &str, color: u8) -> Self {
        Self {
            id: new_id(),
            name: name.to_owned(),
            color,
            pages: Vec::new(),
            sort: PageSort::Manual,
            lock: None,
            default_template: None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SectionGroup {
    pub id: Id,
    pub name: String,
    pub children: Vec<Node>,
    #[serde(default)]
    pub collapsed: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Node {
    Group(SectionGroup),
    Section(Section),
}

impl Node {
    pub fn id(&self) -> Id {
        match self {
            Node::Group(g) => g.id,
            Node::Section(s) => s.id,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum TrashKind {
    Page { meta: PageMeta, section: Id },
    Section { section: Section, parent: Option<Id> },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrashEntry {
    pub kind: TrashKind,
    pub deleted: NaiveDateTime,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NotebookMeta {
    pub id: Id,
    pub name: String,
    pub color: u8,
    pub created: NaiveDateTime,
    pub tree: Vec<Node>,
    #[serde(default)]
    pub trash: Vec<TrashEntry>,
}

impl NotebookMeta {
    pub fn new(name: &str) -> Self {
        Self {
            id: new_id(),
            name: name.to_owned(),
            color: 5,
            created: now(),
            tree: Vec::new(),
            trash: Vec::new(),
        }
    }

    pub fn for_each_section(&self, f: &mut dyn FnMut(&Section)) {
        fn walk(nodes: &[Node], f: &mut dyn FnMut(&Section)) {
            for n in nodes {
                match n {
                    Node::Group(g) => walk(&g.children, f),
                    Node::Section(s) => f(s),
                }
            }
        }
        walk(&self.tree, f);
    }

    pub fn sections(&self) -> Vec<&Section> {
        fn walk<'a>(nodes: &'a [Node], out: &mut Vec<&'a Section>) {
            for n in nodes {
                match n {
                    Node::Group(g) => walk(&g.children, out),
                    Node::Section(s) => out.push(s),
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.tree, &mut out);
        out
    }

    pub fn section(&self, id: Id) -> Option<&Section> {
        self.sections().into_iter().find(|s| s.id == id)
    }

    pub fn section_mut(&mut self, id: Id) -> Option<&mut Section> {
        fn walk(nodes: &mut [Node], id: Id) -> Option<&mut Section> {
            for n in nodes {
                match n {
                    Node::Group(g) => {
                        if let Some(s) = walk(&mut g.children, id) {
                            return Some(s);
                        }
                    }
                    Node::Section(s) if s.id == id => return Some(s),
                    _ => {}
                }
            }
            None
        }
        walk(&mut self.tree, id)
    }

    pub fn group_mut(&mut self, id: Id) -> Option<&mut SectionGroup> {
        fn walk(nodes: &mut [Node], id: Id) -> Option<&mut SectionGroup> {
            for n in nodes {
                if let Node::Group(g) = n {
                    if g.id == id {
                        return Some(g);
                    }
                    if let Some(r) = walk(&mut g.children, id) {
                        return Some(r);
                    }
                }
            }
            None
        }
        walk(&mut self.tree, id)
    }

    /// Removes a node anywhere in the tree, returning it and its parent group id.
    pub fn take_node(&mut self, id: Id) -> Option<(Node, Option<Id>)> {
        fn walk(nodes: &mut Vec<Node>, id: Id, parent: Option<Id>) -> Option<(Node, Option<Id>)> {
            if let Some(i) = nodes.iter().position(|n| n.id() == id) {
                return Some((nodes.remove(i), parent));
            }
            for n in nodes.iter_mut() {
                if let Node::Group(g) = n {
                    let gid = g.id;
                    if let Some(r) = walk(&mut g.children, id, Some(gid)) {
                        return Some(r);
                    }
                }
            }
            None
        }
        walk(&mut self.tree, id, None)
    }

    pub fn insert_node(&mut self, node: Node, parent: Option<Id>) {
        match parent.and_then(|p| self.group_mut(p)) {
            Some(g) => g.children.push(node),
            None => self.tree.push(node),
        }
    }

    pub fn section_of_page(&self, page: Id) -> Option<Id> {
        self.sections()
            .into_iter()
            .find(|s| s.pages.iter().any(|p| p.id == page))
            .map(|s| s.id)
    }

    pub fn page_meta(&self, page: Id) -> Option<&PageMeta> {
        self.sections()
            .into_iter()
            .flat_map(|s| s.pages.iter())
            .find(|p| p.id == page)
    }

    pub fn page_meta_mut(&mut self, page: Id) -> Option<&mut PageMeta> {
        let sid = self.section_of_page(page)?;
        self.section_mut(sid)?
            .pages
            .iter_mut()
            .find(|p| p.id == page)
    }
}

// ----------------------------------------------------------------------------
// Page content

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Background {
    #[default]
    Plain,
    Lines,
    Grid,
    Dots,
    Millimeter,
}

impl Background {
    pub const ALL: [Background; 5] = [
        Background::Plain,
        Background::Lines,
        Background::Grid,
        Background::Dots,
        Background::Millimeter,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Background::Plain => "Чистый",
            Background::Lines => "Линейка",
            Background::Grid => "Клетка",
            Background::Dots => "Точки",
            Background::Millimeter => "Миллиметровка",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PageSize {
    #[default]
    Infinite,
    A4,
    A5,
    Letter,
    A4Landscape,
}

impl PageSize {
    pub const ALL: [PageSize; 5] = [
        PageSize::Infinite,
        PageSize::A4,
        PageSize::A5,
        PageSize::Letter,
        PageSize::A4Landscape,
    ];
    pub fn label(self) -> &'static str {
        match self {
            PageSize::Infinite => "Безграничный холст",
            PageSize::A4 => "A4",
            PageSize::A5 => "A5",
            PageSize::Letter => "Letter",
            PageSize::A4Landscape => "A4 альбомная",
        }
    }
    /// Size in canvas units (1 unit = 1 px at 96 dpi).
    pub fn size(self) -> Option<egui::Vec2> {
        let mm = 96.0 / 25.4;
        match self {
            PageSize::Infinite => None,
            PageSize::A4 => Some(egui::vec2(210.0 * mm, 297.0 * mm)),
            PageSize::A5 => Some(egui::vec2(148.0 * mm, 210.0 * mm)),
            PageSize::Letter => Some(egui::vec2(8.5 * 96.0, 11.0 * 96.0)),
            PageSize::A4Landscape => Some(egui::vec2(297.0 * mm, 210.0 * mm)),
        }
    }
}

/// One sample of ink: position in canvas units and normalized pressure.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct InkPoint(pub f32, pub f32, pub f32);

impl InkPoint {
    pub fn pos(&self) -> egui::Pos2 {
        egui::pos2(self.0, self.1)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum InkKind {
    #[default]
    Pen,
    Highlighter,
    Shape,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Stroke {
    pub points: Arc<Vec<InkPoint>>,
    pub color: [u8; 4],
    pub width: f32,
    #[serde(default)]
    pub kind: InkKind,
    #[serde(default)]
    pub fill: Option<[u8; 4]>,
    #[serde(default)]
    pub closed: bool,
}

impl Stroke {
    pub fn color32(&self) -> egui::Color32 {
        let [r, g, b, a] = self.color;
        egui::Color32::from_rgba_unmultiplied(r, g, b, a)
    }
    pub fn bbox(&self) -> egui::Rect {
        let mut r = egui::Rect::NOTHING;
        for p in self.points.iter() {
            r.extend_with(p.pos());
        }
        r.expand(self.width)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Task {
    pub done: bool,
    #[serde(default)]
    pub due: Option<NaiveDateTime>,
    #[serde(default)]
    pub priority: u8,
    #[serde(default)]
    pub notified: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
    Justify,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TextBlock {
    /// Lightweight markup, see `richtext.rs`.
    pub text: String,
    #[serde(default)]
    pub font: String,
    pub size: f32,
    pub color: [u8; 4],
    #[serde(default)]
    pub align: Align,
    #[serde(default = "one")]
    pub line_spacing: f32,
    #[serde(default)]
    pub para_before: f32,
    #[serde(default)]
    pub para_after: f32,
    #[serde(default)]
    pub indent: f32,
    #[serde(default)]
    pub rtl: bool,
    #[serde(default)]
    pub lang: String,
}

fn one() -> f32 {
    1.0
}

impl TextBlock {
    pub fn new(text: &str, size: f32, font: &str) -> Self {
        Self {
            text: text.to_owned(),
            font: font.to_owned(),
            size,
            color: [0, 0, 0, 0],
            align: Align::Left,
            line_spacing: 1.0,
            para_before: 0.0,
            para_after: 0.0,
            indent: 0.0,
            rtl: false,
            lang: String::new(),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ImageItem {
    pub asset: String,
    #[serde(default)]
    pub rotation: u8,
    #[serde(default)]
    pub flip_h: bool,
    #[serde(default)]
    pub flip_v: bool,
    /// Normalized crop rect (min x, min y, max x, max y).
    #[serde(default = "full_crop")]
    pub crop: [f32; 4],
    #[serde(default)]
    pub caption: String,
    #[serde(default)]
    pub alt: String,
    #[serde(default)]
    pub ocr_text: Option<String>,
}

pub fn full_crop() -> [f32; 4] {
    [0.0, 0.0, 1.0, 1.0]
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FileItem {
    pub asset: String,
    pub name: String,
    pub size: u64,
    #[serde(default)]
    pub extracted_text: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Merge {
    pub r: usize,
    pub c: usize,
    pub rs: usize,
    pub cs: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Table {
    pub cells: Vec<Vec<String>>,
    pub col_widths: Vec<f32>,
    pub row_heights: Vec<f32>,
    #[serde(default)]
    pub header: bool,
    #[serde(default)]
    pub banded: bool,
    #[serde(default = "default_border")]
    pub border: [u8; 4],
    /// Per-cell fill, key "r,c".
    #[serde(default)]
    pub fills: std::collections::BTreeMap<String, [u8; 4]>,
    /// Per-cell alignment, key "r,c".
    #[serde(default)]
    pub aligns: std::collections::BTreeMap<String, Align>,
    #[serde(default)]
    pub merges: Vec<Merge>,
}

fn default_border() -> [u8; 4] {
    [140, 140, 140, 255]
}

impl Table {
    pub fn new(rows: usize, cols: usize) -> Self {
        Self {
            cells: vec![vec![String::new(); cols]; rows],
            col_widths: vec![110.0; cols],
            row_heights: vec![26.0; rows],
            header: true,
            banded: false,
            border: default_border(),
            fills: Default::default(),
            aligns: Default::default(),
            merges: Vec::new(),
        }
    }
    pub fn rows(&self) -> usize {
        self.cells.len()
    }
    pub fn cols(&self) -> usize {
        self.cells.first().map_or(0, |r| r.len())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CodeItem {
    pub lang: String,
    pub code: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiagramKind {
    #[default]
    MindMap,
    Flow,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Diagram {
    pub kind: DiagramKind,
    /// Mind map: indented list. Flow: lines like `A -> B`.
    pub source: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ItemKind {
    Text(TextBlock),
    Image(ImageItem),
    File(FileItem),
    Table(Table),
    Equation(String),
    Code(CodeItem),
    Diagram(Diagram),
    Divider { page_break: bool },
}

impl ItemKind {
    pub fn type_name(&self) -> &'static str {
        match self {
            ItemKind::Text(_) => "Текст",
            ItemKind::Image(_) => "Изображение",
            ItemKind::File(_) => "Файл",
            ItemKind::Table(_) => "Таблица",
            ItemKind::Equation(_) => "Формула",
            ItemKind::Code(_) => "Код",
            ItemKind::Diagram(_) => "Диаграмма",
            ItemKind::Divider { .. } => "Разделитель",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Item {
    pub id: Id,
    pub pos: [f32; 2],
    pub width: f32,
    #[serde(default)]
    pub height: f32,
    pub kind: ItemKind,
    #[serde(default)]
    pub tags: Vec<u32>,
    #[serde(default)]
    pub task: Option<Task>,
    #[serde(default)]
    pub author: String,
    #[serde(default = "now")]
    pub modified: NaiveDateTime,
}

impl Item {
    pub fn new(pos: egui::Pos2, width: f32, kind: ItemKind, author: &str) -> Self {
        Self {
            id: new_id(),
            pos: [pos.x, pos.y],
            width,
            height: 0.0,
            kind,
            tags: Vec::new(),
            task: None,
            author: author.to_owned(),
            modified: now(),
        }
    }
    pub fn pos2(&self) -> egui::Pos2 {
        egui::pos2(self.pos[0], self.pos[1])
    }
    pub fn rect(&self) -> egui::Rect {
        egui::Rect::from_min_size(self.pos2(), egui::vec2(self.width, self.height.max(20.0)))
    }
    /// Plain text used by search, export and the assistant.
    pub fn plain_text(&self) -> String {
        match &self.kind {
            ItemKind::Text(t) => crate::richtext::strip_markup(&t.text),
            ItemKind::Image(i) => {
                let mut s = format!("{} {}", i.caption, i.alt);
                if let Some(o) = &i.ocr_text {
                    s.push(' ');
                    s.push_str(o);
                }
                s
            }
            ItemKind::File(f) => {
                let mut s = f.name.clone();
                if let Some(t) = &f.extracted_text {
                    s.push(' ');
                    s.push_str(t);
                }
                s
            }
            ItemKind::Table(t) => t
                .cells
                .iter()
                .map(|r| r.join("\t"))
                .collect::<Vec<_>>()
                .join("\n"),
            ItemKind::Equation(e) => e.clone(),
            ItemKind::Code(c) => c.code.clone(),
            ItemKind::Diagram(d) => d.source.clone(),
            ItemKind::Divider { .. } => String::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Comment {
    pub id: Id,
    pub author: String,
    pub time: NaiveDateTime,
    pub text: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Page {
    pub id: Id,
    #[serde(default)]
    pub background: Background,
    #[serde(default)]
    pub bg_color: Option<[u8; 4]>,
    #[serde(default)]
    pub size: PageSize,
    #[serde(default)]
    pub items: Vec<Item>,
    #[serde(default)]
    pub strokes: Vec<Stroke>,
    #[serde(default)]
    pub comments: Vec<Comment>,
    #[serde(default)]
    pub sensitivity: Sensitivity,
    /// Text recognized from ink (for search).
    #[serde(default)]
    pub ink_text: String,
}

impl Page {
    pub fn new(id: Id) -> Self {
        Self {
            id,
            background: Background::Plain,
            bg_color: None,
            size: PageSize::Infinite,
            items: Vec::new(),
            strokes: Vec::new(),
            comments: Vec::new(),
            sensitivity: Sensitivity::None,
            ink_text: String::new(),
        }
    }

    pub fn plain_text(&self) -> String {
        let mut items: Vec<&Item> = self.items.iter().collect();
        items.sort_by(|a, b| {
            a.pos[1]
                .partial_cmp(&b.pos[1])
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.pos[0].partial_cmp(&b.pos[0]).unwrap_or(std::cmp::Ordering::Equal))
        });
        let mut out = String::new();
        for it in items {
            let t = it.plain_text();
            if !t.trim().is_empty() {
                out.push_str(&t);
                out.push_str("\n\n");
            }
        }
        if !self.ink_text.is_empty() {
            out.push_str(&self.ink_text);
        }
        out
    }

    pub fn content_bounds(&self) -> egui::Rect {
        let mut r = egui::Rect::NOTHING;
        for it in &self.items {
            r = r.union(it.rect());
        }
        for s in &self.strokes {
            r = r.union(s.bbox());
        }
        r
    }

    pub fn item(&self, id: Id) -> Option<&Item> {
        self.items.iter().find(|i| i.id == id)
    }
    pub fn item_mut(&mut self, id: Id) -> Option<&mut Item> {
        self.items.iter_mut().find(|i| i.id == id)
    }
}

// ----------------------------------------------------------------------------
// Tags

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TagDef {
    pub id: u32,
    pub name: String,
    pub icon: String,
    pub color: [u8; 3],
    /// Digit for Ctrl+<digit> shortcut.
    #[serde(default)]
    pub shortcut: Option<u8>,
    /// Tag acts as a task (checkbox).
    #[serde(default)]
    pub is_task: bool,
}

pub fn builtin_tags() -> Vec<TagDef> {
    let t = |id, name: &str, icon: &str, color: [u8; 3], shortcut, is_task| TagDef {
        id,
        name: name.into(),
        icon: icon.into(),
        color,
        shortcut,
        is_task,
    };
    vec![
        t(1, "Задача", "☐", [66, 133, 244], Some(1), true),
        t(2, "Важно", "★", [244, 180, 0], Some(2), false),
        t(3, "Вопрос", "?", [156, 39, 176], Some(3), false),
        t(4, "Идея", "💡", [255, 193, 7], Some(4), false),
        t(5, "Критично", "!", [219, 68, 55], Some(5), false),
        t(6, "Контакт", "👤", [0, 150, 136], Some(6), false),
        t(7, "Адрес", "⌂", [121, 85, 72], Some(7), false),
        t(8, "Телефон", "☎", [63, 81, 181], Some(8), false),
        t(9, "Прочитать", "📖", [96, 125, 139], Some(9), false),
        t(10, "Фильм", "🎬", [233, 30, 99], None, false),
        t(11, "Музыка", "♪", [103, 58, 183], None, false),
    ]
}

pub fn fmt_dt(dt: &NaiveDateTime) -> String {
    dt.format("%d.%m.%Y %H:%M").to_string()
}

pub fn local_now() -> DateTime<Local> {
    Local::now()
}
