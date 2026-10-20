//! Structure operations (pages, sections, groups, notebooks, trash),
//! import and file-based export.

use std::path::{Path, PathBuf};

use crate::app::{App, Dialog, sorted_pages};
use crate::model::*;
use crate::storage::{self, Notebook};

impl App {
    pub fn new_page(&mut self, sub: bool) {
        let Some(ni) = self.cur_nb else { return };
        let Some(sid) = self.cur_section else {
            self.set_status("Сначала создайте раздел");
            return;
        };
        if !self.notebooks[ni].section_accessible(sid) {
            return;
        }
        let author = self.settings.author.clone();
        let mut pm = PageMeta::new("Без названия", &author);
        let pid = pm.id;
        let template = self.notebooks[ni].meta.section(sid).and_then(|s| s.default_template.clone());
        let cur = self.cur_page;
        let nb = &mut self.notebooks[ni];
        let Some(sec) = nb.meta.section_mut(sid) else { return };
        let pos = cur.and_then(|c| sec.pages.iter().position(|p| p.id == c));
        if sub {
            if let Some(i) = pos {
                pm.level = (sec.pages[i].level + 1).min(2);
            }
        }
        // Insert after the current page and its subpages.
        let at = match pos {
            Some(i) => {
                let lvl = sec.pages[i].level;
                let mut j = i + 1;
                while j < sec.pages.len() && sec.pages[j].level > lvl {
                    j += 1;
                }
                j
            }
            None => sec.pages.len(),
        };
        sec.pages.insert(at, pm);
        nb.dirty = true;
        let page = match template {
            Some(t) => crate::templates::instantiate(&t, pid, &self.settings),
            None => Page::new(pid),
        };
        self.pages.insert(pid, page);
        self.editors.insert(pid, crate::canvas::Editor::new(pid));
        self.open_page(Some(pid));
        self.mark_dirty(pid);
        self.flush_page(pid, false);
        self.dialogs.push(Dialog::Rename { target: crate::app::RenameTarget::Page(pid), text: String::new() });
    }

    pub fn new_page_from_template(&mut self, name: &str) {
        let Some(ni) = self.cur_nb else { return };
        let Some(sid) = self.cur_section else { return };
        let pm = PageMeta::new(name, &self.settings.author);
        let pid = pm.id;
        let page = crate::templates::instantiate(name, pid, &self.settings);
        if let Some(sec) = self.notebooks[ni].meta.section_mut(sid) {
            sec.pages.push(pm);
        }
        self.notebooks[ni].dirty = true;
        self.pages.insert(pid, page);
        self.editors.insert(pid, crate::canvas::Editor::new(pid));
        self.open_page(Some(pid));
        self.mark_dirty(pid);
    }

    pub fn new_section(&mut self) {
        let Some(ni) = self.cur_nb else { return };
        if self.notebooks[ni].sealed {
            return;
        }
        let n = self.notebooks[ni].meta.sections().len();
        let mut sec = Section::new("Новый раздел", (n % 16) as u8);
        let pm = PageMeta::new("Без названия", &self.settings.author);
        let pid = pm.id;
        sec.pages.push(pm);
        let sid = sec.id;
        let tmpl = self.settings.tags.is_empty().then_some(()).map(|_| ());
        let _ = tmpl;
        // Sections are added next to the current one (same group).
        let parent = self.cur_section.and_then(|s| parent_of(&self.notebooks[ni].meta.tree, s, None));
        self.notebooks[ni].meta.insert_node(Node::Section(sec), parent.flatten());
        self.notebooks[ni].dirty = true;
        self.cur_section = Some(sid);
        self.pages.insert(pid, Page::new(pid));
        self.editors.insert(pid, crate::canvas::Editor::new(pid));
        self.open_page(Some(pid));
        self.mark_dirty(pid);
        self.dialogs.push(Dialog::Rename { target: crate::app::RenameTarget::Section(sid), text: "Новый раздел".into() });
    }

    pub fn new_group(&mut self) {
        let Some(ni) = self.cur_nb else { return };
        let g = SectionGroup { id: new_id(), name: "Новая группа".into(), children: Vec::new(), collapsed: false };
        let gid = g.id;
        self.notebooks[ni].meta.tree.push(Node::Group(g));
        self.notebooks[ni].dirty = true;
        self.dialogs.push(Dialog::Rename { target: crate::app::RenameTarget::Group(gid), text: "Новая группа".into() });
    }

    pub fn delete_page(&mut self, pid: Id) {
        let Some(ni) = self.nb_index_of_page(pid) else { return };
        self.flush_page(pid, true);
        let nb = &mut self.notebooks[ni];
        let Some(sid) = nb.meta.section_of_page(pid) else { return };
        let Some(sec) = nb.meta.section_mut(sid) else { return };
        let Some(i) = sec.pages.iter().position(|p| p.id == pid) else { return };
        let lvl = sec.pages[i].level;
        // Subpages go to the trash together with the parent.
        let mut j = i + 1;
        while j < sec.pages.len() && sec.pages[j].level > lvl {
            j += 1;
        }
        let removed: Vec<PageMeta> = sec.pages.drain(i..j).collect();
        for meta in removed {
            self.index.remove(meta.id);
            self.pages.remove(&meta.id);
            nb.meta.trash.push(TrashEntry { kind: TrashKind::Page { meta, section: sid }, deleted: now() });
        }
        nb.dirty = true;
        if self.cur_page == Some(pid) {
            self.cur_page = None;
            self.select_first_page();
        }
        self.set_status("Страница перемещена в корзину");
    }

    pub fn delete_section(&mut self, sid: Id) {
        let Some(ni) = self.notebooks.iter().position(|nb| nb.meta.section(sid).is_some()) else { return };
        self.save_all();
        let nb = &mut self.notebooks[ni];
        if let Some((Node::Section(s), parent)) = nb.meta.take_node(sid) {
            for p in &s.pages {
                self.index.remove(p.id);
                self.pages.remove(&p.id);
            }
            nb.meta.trash.push(TrashEntry { kind: TrashKind::Section { section: s, parent }, deleted: now() });
            nb.dirty = true;
        }
        if self.cur_section == Some(sid) {
            self.cur_section = None;
            self.cur_page = None;
            self.select_first_page();
        }
        self.set_status("Раздел перемещён в корзину");
    }

    pub fn delete_group(&mut self, gid: Id) {
        let Some(ni) = self.cur_nb else { return };
        let nb = &mut self.notebooks[ni];
        if let Some((Node::Group(g), parent)) = nb.meta.take_node(gid) {
            // Sections inside go to the trash one by one.
            fn collect(nodes: Vec<Node>, out: &mut Vec<Section>) {
                for n in nodes {
                    match n {
                        Node::Group(g) => collect(g.children, out),
                        Node::Section(s) => out.push(s),
                    }
                }
            }
            let mut secs = Vec::new();
            collect(g.children, &mut secs);
            for s in secs {
                nb.meta.trash.push(TrashEntry { kind: TrashKind::Section { section: s, parent }, deleted: now() });
            }
            nb.dirty = true;
        }
        self.select_first_page();
    }

    pub fn restore_trash(&mut self, ni: usize, idx: usize) {
        let nb = &mut self.notebooks[ni];
        if idx >= nb.meta.trash.len() {
            return;
        }
        let t = nb.meta.trash.remove(idx);
        match t.kind {
            TrashKind::Page { meta, section } => {
                let target = if nb.meta.section(section).is_some() {
                    section
                } else {
                    match nb.meta.sections().first() {
                        Some(s) => s.id,
                        None => {
                            let s = Section::new("Восстановленные", 0);
                            let id = s.id;
                            nb.meta.tree.push(Node::Section(s));
                            id
                        }
                    }
                };
                if let Some(s) = nb.meta.section_mut(target) {
                    s.pages.push(meta);
                }
            }
            TrashKind::Section { section, parent } => {
                nb.meta.insert_node(Node::Section(section), parent);
            }
        }
        nb.dirty = true;
        nb.save_meta();
        let ctx_needed = true;
        let _ = ctx_needed;
        self.set_status("Восстановлено");
    }

    pub fn duplicate_page(&mut self, pid: Id) {
        let Some(ni) = self.nb_index_of_page(pid) else { return };
        self.flush_page(pid, false);
        let Ok(mut page) = self.notebooks[ni].load_page(pid) else { return };
        let nb = &mut self.notebooks[ni];
        let Some(sid) = nb.meta.section_of_page(pid) else { return };
        let Some(meta) = nb.meta.page_meta(pid).cloned() else { return };
        let mut nm = meta.clone();
        nm.id = new_id();
        nm.title = format!("{} (копия)", meta.title);
        nm.created = now();
        nm.modified = now();
        nm.pinned = false;
        page.id = nm.id;
        for it in page.items.iter_mut() {
            it.id = new_id();
        }
        let nid = nm.id;
        if let Some(sec) = nb.meta.section_mut(sid) {
            let i = sec.pages.iter().position(|p| p.id == pid).map(|i| i + 1).unwrap_or(sec.pages.len());
            sec.pages.insert(i, nm);
        }
        let key = nb.key_for_page(nid).cloned();
        storage::write_page(&nb.page_path(nid), &page, key.as_ref());
        nb.dirty = true;
        self.open_page(Some(nid));
    }

    pub fn duplicate_section(&mut self, sid: Id) {
        let Some(ni) = self.cur_nb else { return };
        self.save_all();
        let Some(sec) = self.notebooks[ni].meta.section(sid).cloned() else { return };
        let mut ns = sec.clone();
        ns.id = new_id();
        ns.name = format!("{} (копия)", sec.name);
        ns.lock = None;
        ns.pages.clear();
        let mut pages = Vec::new();
        for pm in &sec.pages {
            if let Ok(mut p) = self.notebooks[ni].load_page(pm.id) {
                let mut m = pm.clone();
                m.id = new_id();
                p.id = m.id;
                pages.push(p);
                ns.pages.push(m);
            }
        }
        let parent = parent_of(&self.notebooks[ni].meta.tree, sid, None).flatten();
        let nb = &mut self.notebooks[ni];
        nb.meta.insert_node(Node::Section(ns), parent);
        for p in pages {
            let key = nb.key_for_page(p.id).cloned();
            storage::write_page(&nb.page_path(p.id), &p, key.as_ref());
        }
        nb.dirty = true;
    }

    /// Moves a page (with subpages) into another section, possibly in another notebook.
    pub fn move_page(&mut self, pid: Id, to_nb: usize, to_sec: Id) {
        let Some(from_nb) = self.nb_index_of_page(pid) else { return };
        if !self.notebooks[to_nb].section_accessible(to_sec) {
            self.set_status("Целевой раздел заблокирован");
            return;
        }
        self.flush_page(pid, false);
        self.saver.flush();
        let from_sid = self.notebooks[from_nb].meta.section_of_page(pid).unwrap();
        let metas: Vec<PageMeta> = {
            let sec = self.notebooks[from_nb].meta.section_mut(from_sid).unwrap();
            let i = sec.pages.iter().position(|p| p.id == pid).unwrap();
            let lvl = sec.pages[i].level;
            let mut j = i + 1;
            while j < sec.pages.len() && sec.pages[j].level > lvl {
                j += 1;
            }
            sec.pages.drain(i..j).collect()
        };
        // Load with the old key before the meta changes.
        let mut loaded = Vec::new();
        for m in &metas {
            let p = {
                // The page is no longer in a section; read with the section key directly.
                let nb = &self.notebooks[from_nb];
                let key = if nb.meta.section(from_sid).is_some_and(|s| s.lock.is_some()) { nb.keys.get(&from_sid) } else if nb.lock.is_some() { nb.keys.get(&nb.meta.id) } else { None };
                std::fs::read(nb.page_path(m.id)).ok().and_then(|d| storage::decode_page(&d, key).ok()).unwrap_or_else(|| Page::new(m.id))
            };
            loaded.push(p);
        }
        let base_level = metas.first().map(|m| m.level).unwrap_or(0);
        for (m, p) in metas.into_iter().zip(loaded) {
            let mut m = m;
            m.level -= base_level;
            // Copy attachments between notebooks.
            if from_nb != to_nb {
                for it in &p.items {
                    let asset = match &it.kind {
                        ItemKind::Image(i) => Some(&i.asset),
                        ItemKind::File(f) => Some(&f.asset),
                        _ => None,
                    };
                    if let Some(a) = asset {
                        let src = self.notebooks[from_nb].asset_path(a);
                        let dst = self.notebooks[to_nb].asset_path(a);
                        if !dst.exists() {
                            let _ = std::fs::create_dir_all(dst.parent().unwrap());
                            let _ = std::fs::copy(src, dst);
                        }
                    }
                }
                let _ = std::fs::remove_file(self.notebooks[from_nb].page_path(m.id));
                self.pages.remove(&m.id);
            }
            if let Some(s) = self.notebooks[to_nb].meta.section_mut(to_sec) {
                s.pages.push(m.clone());
            }
            let nb = &self.notebooks[to_nb];
            let key = nb.key_for_page(m.id).cloned();
            storage::write_page(&nb.page_path(m.id), &p, key.as_ref());
            if let Some(cached) = self.pages.get_mut(&m.id) {
                *cached = p;
            }
        }
        self.notebooks[from_nb].dirty = true;
        self.notebooks[to_nb].dirty = true;
        self.notebooks[from_nb].save_meta();
        self.notebooks[to_nb].save_meta();
        self.set_status("Страница перемещена");
    }

    pub fn move_section(&mut self, sid: Id, to_nb: usize, to_group: Option<Id>) {
        let Some(from_nb) = self.notebooks.iter().position(|nb| nb.meta.section(sid).is_some()) else { return };
        self.save_all();
        if from_nb == to_nb {
            let nb = &mut self.notebooks[from_nb];
            if let Some((node, _)) = nb.meta.take_node(sid) {
                nb.meta.insert_node(node, to_group);
                nb.dirty = true;
            }
            return;
        }
        if self.notebooks[from_nb].meta.section(sid).is_some_and(|s| s.lock.is_some()) && !self.notebooks[from_nb].keys.contains_key(&sid) {
            self.set_status("Сначала разблокируйте раздел");
            return;
        }
        let pages: Vec<Page> = self.notebooks[from_nb]
            .meta
            .section(sid)
            .map(|s| s.pages.iter().filter_map(|p| self.notebooks[from_nb].load_page(p.id).ok()).collect())
            .unwrap_or_default();
        let key = self.notebooks[from_nb].keys.get(&sid).cloned();
        let Some((node, _)) = self.notebooks[from_nb].meta.take_node(sid) else { return };
        for p in &pages {
            for it in &p.items {
                let asset = match &it.kind {
                    ItemKind::Image(i) => Some(&i.asset),
                    ItemKind::File(f) => Some(&f.asset),
                    _ => None,
                };
                if let Some(a) = asset {
                    let dst = self.notebooks[to_nb].asset_path(a);
                    if !dst.exists() {
                        let _ = std::fs::create_dir_all(dst.parent().unwrap());
                        let _ = std::fs::copy(self.notebooks[from_nb].asset_path(a), dst);
                    }
                }
            }
            let _ = std::fs::remove_file(self.notebooks[from_nb].page_path(p.id));
        }
        self.notebooks[to_nb].meta.insert_node(node, to_group);
        if let Some(k) = key {
            self.notebooks[to_nb].keys.insert(sid, k);
        }
        for p in pages {
            let nb = &self.notebooks[to_nb];
            let k = nb.key_for_page(p.id).cloned();
            storage::write_page(&nb.page_path(p.id), &p, k.as_ref());
        }
        self.notebooks[from_nb].save_meta();
        self.notebooks[to_nb].save_meta();
        self.set_status("Раздел перемещён");
    }

    pub fn merge_sections(&mut self, from: Id, into: Id) {
        let Some(ni) = self.cur_nb else { return };
        let ids: Vec<Id> = self.notebooks[ni].meta.section(from).map(|s| s.pages.iter().map(|p| p.id).collect()).unwrap_or_default();
        // Move only top-level pages: subpages travel with their parent.
        let tops: Vec<Id> = {
            let s = self.notebooks[ni].meta.section(from).unwrap();
            s.pages.iter().filter(|p| p.level == 0).map(|p| p.id).collect()
        };
        let _ = ids;
        for pid in tops {
            self.move_page(pid, ni, into);
        }
        let nb = &mut self.notebooks[ni];
        nb.meta.take_node(from);
        nb.dirty = true;
        self.cur_section = Some(into);
        self.select_first_page();
        self.set_status("Разделы объединены");
    }

    pub fn new_notebook(&mut self, name: &str) {
        let dir = self.settings.notebooks_dir.clone();
        match Notebook::create(&dir, name) {
            Ok(mut nb) => {
                let mut sec = Section::new("Новый раздел", 5);
                sec.pages.push(PageMeta::new("Без названия", &self.settings.author));
                nb.meta.tree.push(Node::Section(sec));
                nb.save_meta();
                self.notebooks.push(nb);
                self.cur_nb = Some(self.notebooks.len() - 1);
                self.cur_section = None;
                self.cur_page = None;
                self.select_first_page();
                self.remember_notebooks();
            }
            Err(e) => self.set_status(format!("Не удалось создать блокнот: {e}")),
        }
    }

    pub fn open_notebook_folder(&mut self, ctx: &egui::Context, path: &Path) {
        if self.notebooks.iter().any(|n| n.path == path) {
            self.set_status("Этот блокнот уже открыт");
            return;
        }
        match Notebook::open(path) {
            Ok(mut nb) => {
                nb.purge_trash(self.settings.trash_days);
                self.notebooks.push(nb);
                self.cur_nb = Some(self.notebooks.len() - 1);
                self.cur_section = None;
                self.open_page(None);
                self.select_first_page();
                self.remember_notebooks();
                self.rebuild_index(ctx);
            }
            Err(e) => self.set_status(format!("Это не папка блокнота: {e}")),
        }
    }

    pub fn close_notebook(&mut self, i: usize) {
        self.save_all();
        if i >= self.notebooks.len() {
            return;
        }
        let nb = self.notebooks.remove(i);
        self.index.remove_notebook(&nb.path);
        let ids: Vec<Id> = nb.meta.sections().iter().flat_map(|s| s.pages.iter().map(|p| p.id)).collect();
        for id in ids {
            self.pages.remove(&id);
            self.editors.remove(&id);
        }
        self.cur_nb = if self.notebooks.is_empty() { None } else { Some(0) };
        self.cur_section = None;
        self.cur_page = None;
        self.select_first_page();
        self.remember_notebooks();
    }

    pub fn remember_notebooks(&mut self) {
        self.settings.open_notebooks = self.notebooks.iter().map(|n| n.path.clone()).collect();
        self.settings.save();
    }

    // ---- import -------------------------------------------------------------------

    /// Imports a document as new page(s) of the current section, or a whole
    /// notebook (OneNote notebooks and packages, Joplin, folders, zip exports).
    pub fn import_document(&mut self, ctx: &egui::Context, path: &Path) {
        let ext = crate::import::ext_of(path);
        let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        match ext.as_str() {
            "pdf" => {
                // New page with a printout of the PDF.
                let Some(ni) = self.cur_nb else { return };
                let Some(sid) = self.cur_section else { return };
                let pm = PageMeta::new(&stem, &self.settings.author);
                let pid = pm.id;
                if let Some(s) = self.notebooks[ni].meta.section_mut(sid) {
                    s.pages.push(pm);
                }
                self.notebooks[ni].dirty = true;
                self.pages.insert(pid, Page::new(pid));
                self.editors.insert(pid, crate::canvas::Editor::new(pid));
                self.open_page(Some(pid));
                self.mark_dirty(pid);
                self.pdf_printout_from(path.to_owned(), [0.0, 0.0]);
                return;
            }
            "nnbz" => {
                self.import_archive(ctx, path, None);
                return;
            }
            "zip" if zip_has_notebook(path) => {
                self.import_archive(ctx, path, None);
                return;
            }
            "csv" | "xlsx" | "xls" | "ods" => {
                let cells = if ext == "csv" {
                    std::fs::read(path).ok().map(|b| crate::export::parse_csv(&crate::import::decode_text(&b)))
                } else {
                    crate::export::table_from_xlsx(path).ok()
                };
                if let Some(c) = cells {
                    let t = crate::export::table_from_cells(c);
                    let w = t.col_widths.iter().sum();
                    self.insert_item(ItemKind::Table(t), w);
                }
                return;
            }
            _ => {}
        }
        let opts = crate::import::Opts { author: self.settings.author.clone(), font: self.settings.default_font.clone(), size: self.settings.default_size };
        if crate::import::is_notebook_source(path) {
            // Whole notebook: build it in a fresh folder.
            let _ = std::fs::create_dir_all(&self.settings.notebooks_dir);
            let mut nb = match Notebook::create(&self.settings.notebooks_dir, &stem) {
                Ok(nb) => nb,
                Err(e) => {
                    self.set_status(format!("Ошибка: {e}"));
                    return;
                }
            };
            let dir = nb.path.clone();
            match crate::import::import(path, &nb, &opts) {
                Ok(imp) => {
                    if !imp.name.is_empty() {
                        nb.meta.name = imp.name.clone();
                    }
                    let mut count = 0;
                    let mut tree = store_nodes(&nb, imp.nodes, &self.settings.author, &mut count);
                    if !imp.pages.is_empty() {
                        let mut s = Section::new("Импорт", 5);
                        s.pages = store_pages(&nb, imp.pages, &self.settings.author, &mut count);
                        tree.push(Node::Section(s));
                    }
                    if tree.is_empty() {
                        drop(nb);
                        let _ = std::fs::remove_dir_all(dir);
                        self.set_status("Нечего импортировать");
                        return;
                    }
                    nb.meta.tree = tree;
                    nb.save_meta();
                    drop(nb);
                    self.open_notebook_folder(ctx, &dir);
                    self.report_import(count, &imp.warnings);
                }
                Err(e) => {
                    drop(nb);
                    let _ = std::fs::remove_dir_all(dir);
                    self.set_status(format!("Импорт не удался: {e}"));
                }
            }
            return;
        }
        let Some(ni) = self.cur_nb else {
            self.set_status("Сначала откройте блокнот");
            return;
        };
        let imp = match crate::import::import(path, &self.notebooks[ni], &opts) {
            Ok(i) => i,
            Err(e) => {
                self.set_status(format!("Импорт не удался: {e}"));
                return;
            }
        };
        let mut count = 0;
        let nb = &self.notebooks[ni];
        let nodes = store_nodes(nb, imp.nodes, &self.settings.author, &mut count);
        let first_new_section = nodes.iter().find_map(|n| if let Node::Section(s) = n { Some(s.id) } else { None });
        let pages = store_pages(nb, imp.pages, &self.settings.author, &mut count);
        let mut last = pages.last().map(|p| p.id);
        if !pages.is_empty() {
            match self.cur_section.and_then(|sid| self.notebooks[ni].meta.section_mut(sid)) {
                Some(s) => s.pages.extend(pages),
                None => {
                    let mut s = Section::new("Импорт", 5);
                    s.pages = pages;
                    self.notebooks[ni].meta.tree.push(Node::Section(s));
                }
            }
        }
        if !nodes.is_empty() {
            self.notebooks[ni].meta.tree.extend(nodes);
            if last.is_none() {
                if let Some(sid) = first_new_section {
                    self.cur_section = Some(sid);
                    last = self.notebooks[ni].meta.section_mut(sid).and_then(|s| s.pages.first().map(|p| p.id));
                }
            }
        }
        self.notebooks[ni].dirty = true;
        self.notebooks[ni].save_meta();
        self.open_page(last);
        self.rebuild_index(ctx);
        self.report_import(count, &imp.warnings);
    }

    fn report_import(&mut self, count: usize, warnings: &[String]) {
        if warnings.is_empty() {
            self.set_status(format!("Импортировано страниц: {count}"));
        } else {
            self.set_status(format!("Импортировано страниц: {count}. Предупреждения: {}", warnings.join("; ")));
        }
    }

    pub fn export_archive(&mut self, ni: usize, path: &Path) {
        self.save_all();
        match storage::zip_dir(&self.notebooks[ni].path, true) {
            Ok(data) => {
                if storage::atomic_write(path, &data) {
                    self.set_status(format!("Блокнот сохранён в {}", path.display()));
                }
            }
            Err(e) => self.set_status(format!("Ошибка: {e}")),
        }
    }

    /// Opens an archive as a new notebook, or merges it into `merge_into`.
    /// Merge policy: 0 = newer wins, 1 = keep mine, 2 = take theirs, 3 = keep both.
    pub fn import_archive(&mut self, ctx: &egui::Context, path: &Path, merge_into: Option<(usize, usize)>) {
        let Ok(data) = std::fs::read(path) else { return };
        let tmp = crate::settings::Settings::temp_dir().join(format!("import-{}", new_id()));
        if let Err(e) = storage::unzip_to(&data, &tmp) {
            self.set_status(format!("Архив повреждён: {e}"));
            return;
        }
        match merge_into {
            None => {
                let name = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "Импорт".into());
                let mut dst = self.settings.notebooks_dir.join(storage::sanitize(&name));
                let mut n = 1;
                while dst.exists() {
                    n += 1;
                    dst = self.settings.notebooks_dir.join(format!("{} ({n})", storage::sanitize(&name)));
                }
                let _ = std::fs::create_dir_all(&self.settings.notebooks_dir);
                if std::fs::rename(&tmp, &dst).is_err() {
                    copy_dir(&tmp, &dst);
                    let _ = std::fs::remove_dir_all(&tmp);
                }
                self.open_notebook_folder(ctx, &dst);
            }
            Some((ni, policy)) => {
                let Ok(src) = Notebook::open(&tmp) else {
                    self.set_status("В архиве нет блокнота");
                    return;
                };
                if src.sealed {
                    self.set_status("Объединение защищённых паролем блокнотов не поддерживается — откройте как новый");
                    return;
                }
                self.save_all();
                let mut added = 0;
                let mut conflicts = 0;
                for s in src.meta.sections() {
                    // Find or create the matching section by name.
                    let target = self.notebooks[ni].meta.sections().iter().find(|x| x.name == s.name).map(|x| x.id);
                    let sid = match target {
                        Some(t) => t,
                        None => {
                            let mut ns = s.clone();
                            ns.pages.clear();
                            ns.lock = None;
                            let id = ns.id;
                            self.notebooks[ni].meta.tree.push(Node::Section(ns));
                            id
                        }
                    };
                    for pm in &s.pages {
                        let Ok(page) = src.load_page(pm.id) else { continue };
                        for it in &page.items {
                            if let ItemKind::Image(ImageItem { asset, .. }) | ItemKind::File(FileItem { asset, .. }) = &it.kind {
                                let dst = self.notebooks[ni].asset_path(asset);
                                if !dst.exists() {
                                    let _ = std::fs::copy(src.asset_path(asset), dst);
                                }
                            }
                        }
                        let exists = self.notebooks[ni].meta.page_meta(pm.id).cloned();
                        let (meta, page) = match exists {
                            None => (pm.clone(), page),
                            Some(mine) => {
                                conflicts += 1;
                                let take_theirs = match policy {
                                    0 => pm.modified > mine.modified,
                                    1 => false,
                                    2 => true,
                                    _ => {
                                        let mut m = pm.clone();
                                        m.id = new_id();
                                        m.title = format!("{} (импорт)", pm.title);
                                        let mut p = page;
                                        p.id = m.id;
                                        if let Some(sec) = self.notebooks[ni].meta.section_mut(sid) {
                                            sec.pages.push(m.clone());
                                        }
                                        let nb = &self.notebooks[ni];
                                        storage::write_page(&nb.page_path(m.id), &p, nb.key_for_page(m.id));
                                        added += 1;
                                        continue;
                                    }
                                };
                                if !take_theirs {
                                    continue;
                                }
                                if let Some(m) = self.notebooks[ni].meta.page_meta_mut(pm.id) {
                                    *m = pm.clone();
                                }
                                self.pages.remove(&pm.id);
                                let nb = &self.notebooks[ni];
                                storage::write_page(&nb.page_path(pm.id), &page, nb.key_for_page(pm.id));
                                added += 1;
                                continue;
                            }
                        };
                        if let Some(sec) = self.notebooks[ni].meta.section_mut(sid) {
                            sec.pages.push(meta.clone());
                        }
                        let nb = &self.notebooks[ni];
                        storage::write_page(&nb.page_path(meta.id), &page, nb.key_for_page(meta.id));
                        added += 1;
                    }
                }
                self.notebooks[ni].save_meta();
                let _ = std::fs::remove_dir_all(&tmp);
                self.rebuild_index(ctx);
                self.set_status(format!("Объединено: страниц {added}, конфликтов {conflicts}"));
            }
        }
    }

    // ---- file exports that need no rendering ------------------------------------------

    pub fn export_text_formats(&mut self, fmt: &str, whole_section: bool) {
        let Some(pid) = self.cur_page else { return };
        if self.current_no_copy() {
            self.set_status("Экспорт запрещён для этого раздела");
            return;
        }
        self.save_all();
        let Some(ni) = self.nb_index_of_page(pid) else { return };
        let nb = &self.notebooks[ni];
        let ids: Vec<Id> = if whole_section {
            nb.meta.section_of_page(pid).and_then(|s| nb.meta.section(s)).map(|s| sorted_pages(s).iter().map(|p| p.id).collect()).unwrap_or_default()
        } else {
            vec![pid]
        };
        let pages: Vec<(String, Page)> = ids
            .iter()
            .filter_map(|id| {
                let title = nb.meta.page_meta(*id)?.title.clone();
                let page = self.pages.get(id).cloned().or_else(|| nb.load_page(*id).ok())?;
                Some((title, page))
            })
            .collect();
        let default_name = if whole_section {
            nb.meta.section_of_page(pid).and_then(|s| nb.meta.section(s)).map(|s| s.name.clone()).unwrap_or_default()
        } else {
            pages.first().map(|p| p.0.clone()).unwrap_or_default()
        };
        let Some(path) = rfd::FileDialog::new().add_filter(fmt.to_uppercase(), &[fmt]).set_file_name(format!("{}.{fmt}", storage::sanitize(&default_name))).save_file() else { return };
        let res = match fmt {
            "html" => {
                let body: String = pages.iter().map(|(t, p)| crate::export::page_html_body(nb, p, t)).collect::<Vec<_>>().join("<hr>\n");
                std::fs::write(&path, crate::export::html_document(&default_name, &body))
            }
            "md" => {
                let assets = path.with_file_name(format!("{}_files", path.file_stem().unwrap_or_default().to_string_lossy()));
                let md: String = pages.iter().map(|(t, p)| crate::export::page_markdown(nb, p, t, Some(&assets))).collect::<Vec<_>>().join("\n---\n\n");
                std::fs::write(&path, md)
            }
            "txt" => {
                let s: String = pages.iter().map(|(t, p)| crate::export::page_text(p, t)).collect::<Vec<_>>().join("\n\n");
                std::fs::write(&path, s)
            }
            "docx" => std::fs::write(&path, crate::export::docx(nb, &pages)),
            _ => Ok(()),
        };
        match res {
            Ok(()) => self.set_status(format!("Экспортировано: {}", path.display())),
            Err(e) => self.set_status(format!("Ошибка: {e}")),
        }
    }
}

/// Parent group of a node: Some(None) = top level, None = not found.
pub fn parent_of(nodes: &[Node], id: Id, parent: Option<Id>) -> Option<Option<Id>> {
    for n in nodes {
        if n.id() == id {
            return Some(parent);
        }
        if let Node::Group(g) = n {
            if let Some(r) = parent_of(&g.children, id, Some(g.id)) {
                return Some(r);
            }
        }
    }
    None
}

pub fn copy_dir(src: &Path, dst: &Path) {
    let _ = std::fs::create_dir_all(dst);
    if let Ok(rd) = std::fs::read_dir(src) {
        for e in rd.flatten() {
            let p = e.path();
            let d: PathBuf = dst.join(e.file_name());
            if p.is_dir() {
                copy_dir(&p, &d);
            } else {
                let _ = std::fs::copy(&p, &d);
            }
        }
    }
}

fn zip_has_notebook(path: &Path) -> bool {
    std::fs::File::open(path).ok().and_then(|f| zip::ZipArchive::new(f).ok()).is_some_and(|mut z| z.by_name("notebook.json").is_ok())
}

/// Writes imported pages to disk and returns their metadata.
fn store_pages(nb: &Notebook, pages: Vec<crate::import::ImpPage>, author: &str, count: &mut usize) -> Vec<PageMeta> {
    pages
        .into_iter()
        .map(|ip| {
            let mut pm = PageMeta::new(&ip.title, author);
            if let Some(c) = ip.created {
                pm.created = c;
                pm.modified = c;
            }
            pm.level = ip.level;
            let mut page = ip.page;
            page.id = pm.id;
            storage::write_page(&nb.page_path(pm.id), &page, nb.key_for_page(pm.id));
            *count += 1;
            pm
        })
        .collect()
}

fn store_nodes(nb: &Notebook, nodes: Vec<crate::import::ImpNode>, author: &str, count: &mut usize) -> Vec<Node> {
    nodes
        .into_iter()
        .filter_map(|n| match n {
            crate::import::ImpNode::Section(s) => {
                let mut sec = Section::new(&s.name, s.color);
                sec.pages = store_pages(nb, s.pages, author, count);
                Some(Node::Section(sec))
            }
            crate::import::ImpNode::Group(name, children) => {
                let children = store_nodes(nb, children, author, count);
                (!children.is_empty()).then(|| Node::Group(SectionGroup { id: new_id(), name, children, collapsed: false }))
            }
        })
        .collect()
}
