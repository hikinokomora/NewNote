//! Local storage: a notebook is a folder with `notebook.json`, `pages/`, `assets/`,
//! `versions/` and `locks/`. Encrypted content is stored as AES-GCM blobs.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc;

use serde::{Deserialize, Serialize};

use crate::crypto::{self, Key};
use crate::model::*;

pub fn atomic_write(path: &Path, data: &[u8]) -> bool {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = path.with_extension("tmp~");
    let ok = (|| -> std::io::Result<()> {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
        Ok(())
    })()
    .is_ok();
    ok && std::fs::rename(&tmp, path).is_ok()
}

#[derive(Serialize, Deserialize)]
struct NotebookFile {
    format: u32,
    #[serde(default)]
    lock: Option<LockInfo>,
    #[serde(default)]
    meta: Option<NotebookMeta>,
    #[serde(default)]
    enc: Option<String>,
}

pub struct Notebook {
    pub path: PathBuf,
    pub meta: NotebookMeta,
    /// Notebook-level password protection.
    pub lock: Option<LockInfo>,
    /// Keys of unlocked sections (by section id) and the notebook (by notebook id).
    pub keys: HashMap<Id, Key>,
    /// True while a password-protected notebook has not been unlocked yet.
    pub sealed: bool,
    pub dirty: bool,
}

impl Notebook {
    pub fn file(path: &Path) -> PathBuf {
        path.join("notebook.json")
    }

    pub fn create(dir: &Path, name: &str) -> std::io::Result<Self> {
        let mut path = dir.join(sanitize(name));
        let mut n = 1;
        while path.exists() {
            n += 1;
            path = dir.join(format!("{} ({n})", sanitize(name)));
        }
        std::fs::create_dir_all(path.join("pages"))?;
        std::fs::create_dir_all(path.join("assets"))?;
        let nb = Self {
            path,
            meta: NotebookMeta::new(name),
            lock: None,
            keys: HashMap::new(),
            sealed: false,
            dirty: true,
        };
        nb.save_meta();
        Ok(nb)
    }

    pub fn open(path: &Path) -> Result<Self, String> {
        let data = std::fs::read(Self::file(path)).map_err(|e| e.to_string())?;
        let file: NotebookFile = serde_json::from_slice(&data).map_err(|e| e.to_string())?;
        let sealed = file.meta.is_none();
        let meta = file.meta.unwrap_or_else(|| {
            let mut m = NotebookMeta::new(
                &path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(),
            );
            m.id = Id::nil();
            m
        });
        Ok(Self {
            path: path.to_owned(),
            meta,
            lock: file.lock,
            keys: HashMap::new(),
            sealed,
            dirty: false,
        })
    }

    pub fn unseal(&mut self, password: &str) -> bool {
        let Some(lock) = &self.lock else { return true };
        let Some(key) = crypto::unlock(lock, password) else { return false };
        let Ok(data) = std::fs::read(Self::file(&self.path)) else { return false };
        let Ok(file) = serde_json::from_slice::<NotebookFile>(&data) else { return false };
        if let Some(enc) = file.enc.and_then(|e| crypto::unhex(&e)) {
            if let Some(plain) = crypto::decrypt(&key, &enc) {
                if let Ok(meta) = serde_json::from_slice::<NotebookMeta>(&plain) {
                    self.meta = meta;
                    self.sealed = false;
                    self.keys.insert(self.meta.id, key);
                    return true;
                }
            }
        }
        false
    }

    pub fn seal(&mut self) {
        if self.lock.is_some() {
            self.save_meta();
            self.keys.clear();
            self.sealed = true;
        }
    }

    pub fn save_meta(&self) {
        if self.sealed {
            return;
        }
        let file = match (&self.lock, self.keys.get(&self.meta.id)) {
            (Some(lock), Some(key)) => {
                let plain = serde_json::to_vec(&self.meta).unwrap_or_default();
                NotebookFile {
                    format: 1,
                    lock: Some(lock.clone()),
                    meta: None,
                    enc: Some(crypto::hex(&crypto::encrypt(key, &plain))),
                }
            }
            _ => NotebookFile { format: 1, lock: None, meta: Some(self.meta.clone()), enc: None },
        };
        if let Ok(b) = serde_json::to_vec_pretty(&file) {
            atomic_write(&Self::file(&self.path), &b);
        }
    }

    pub fn set_password(&mut self, password: Option<&str>) {
        // Re-encrypt every page with the new key.
        let pages: Vec<(Id, Option<Page>)> = self
            .meta
            .sections()
            .iter()
            .flat_map(|s| s.pages.iter().map(|p| p.id))
            .map(|id| (id, self.load_page(id).ok()))
            .collect();
        match password {
            Some(pw) => {
                let (lock, key) = crypto::new_lock(pw);
                self.lock = Some(lock);
                self.keys.insert(self.meta.id, key);
            }
            None => {
                self.lock = None;
                self.keys.remove(&self.meta.id);
            }
        }
        for (_, p) in pages {
            if let Some(p) = p {
                let key = self.key_for_page(p.id).cloned();
                write_page(&self.page_path(p.id), &p, key.as_ref());
            }
        }
        self.save_meta();
    }

    pub fn page_path(&self, id: Id) -> PathBuf {
        self.path.join("pages").join(format!("{id}.page"))
    }

    pub fn asset_path(&self, name: &str) -> PathBuf {
        self.path.join("assets").join(name)
    }

    /// Key needed for a page: the section key wins over the notebook key.
    pub fn key_for_page(&self, page: Id) -> Option<&Key> {
        let sid = self.meta.section_of_page(page)?;
        if self.meta.section(sid).is_some_and(|s| s.lock.is_some()) {
            return self.keys.get(&sid);
        }
        if self.lock.is_some() {
            return self.keys.get(&self.meta.id);
        }
        None
    }

    pub fn page_accessible(&self, page: Id) -> bool {
        let Some(sid) = self.meta.section_of_page(page) else { return false };
        self.section_accessible(sid)
    }

    pub fn section_accessible(&self, sid: Id) -> bool {
        match self.meta.section(sid) {
            Some(s) if s.lock.is_some() => self.keys.contains_key(&sid),
            Some(_) => !self.sealed,
            None => false,
        }
    }

    pub fn load_page(&self, id: Id) -> Result<Page, String> {
        let path = self.page_path(id);
        let data = match std::fs::read(&path) {
            Ok(d) => d,
            Err(_) => return Ok(Page::new(id)),
        };
        decode_page(&data, self.key_for_page(id))
    }

    pub fn lock_section(&mut self, sid: Id, password: &str) {
        let pages: Vec<Page> = self
            .meta
            .section(sid)
            .map(|s| s.pages.iter().filter_map(|p| self.load_page(p.id).ok()).collect())
            .unwrap_or_default();
        let (lock, key) = crypto::new_lock(password);
        if let Some(s) = self.meta.section_mut(sid) {
            s.lock = Some(lock);
        }
        self.keys.insert(sid, key);
        for p in pages {
            let key = self.key_for_page(p.id).cloned();
            write_page(&self.page_path(p.id), &p, key.as_ref());
        }
        self.save_meta();
    }

    pub fn remove_section_password(&mut self, sid: Id) {
        let pages: Vec<Page> = self
            .meta
            .section(sid)
            .map(|s| s.pages.iter().filter_map(|p| self.load_page(p.id).ok()).collect())
            .unwrap_or_default();
        if let Some(s) = self.meta.section_mut(sid) {
            s.lock = None;
        }
        self.keys.remove(&sid);
        for p in pages {
            let key = self.key_for_page(p.id).cloned();
            write_page(&self.page_path(p.id), &p, key.as_ref());
        }
        self.save_meta();
    }

    pub fn import_asset(&self, bytes: &[u8], ext: &str) -> String {
        let name = format!("{:016x}.{}", fnv1a(bytes), ext.to_lowercase());
        let path = self.asset_path(&name);
        if !path.exists() {
            atomic_write(&path, bytes);
        }
        name
    }

    pub fn purge_trash(&mut self, days: u32) {
        let limit = now() - chrono::Duration::days(days as i64);
        let (old, keep): (Vec<_>, Vec<_>) =
            self.meta.trash.drain(..).partition(|t| t.deleted < limit);
        self.meta.trash = keep;
        for t in old {
            self.delete_trash_files(&t);
            self.dirty = true;
        }
    }

    pub fn delete_trash_files(&self, t: &TrashEntry) {
        match &t.kind {
            TrashKind::Page { meta, .. } => {
                let _ = std::fs::remove_file(self.page_path(meta.id));
            }
            TrashKind::Section { section, .. } => {
                for p in &section.pages {
                    let _ = std::fs::remove_file(self.page_path(p.id));
                }
            }
        }
    }

    // ---- versions --------------------------------------------------------

    pub fn versions_dir(&self, page: Id) -> PathBuf {
        self.path.join("versions").join(page.to_string())
    }

    pub fn save_version(&self, page: &Page, keep: u32, days: u32) {
        let dir = self.versions_dir(page.id);
        let name = format!("{}.page", chrono::Local::now().format("%Y%m%d-%H%M%S"));
        write_page(&dir.join(name), page, self.key_for_page(page.id));
        // Retention.
        let mut list = self.list_versions(page.id);
        let limit = now() - chrono::Duration::days(days as i64);
        list.sort_by(|a, b| b.0.cmp(&a.0));
        for (i, (t, p)) in list.iter().enumerate() {
            if i >= keep as usize || *t < limit {
                let _ = std::fs::remove_file(p);
            }
        }
    }

    pub fn list_versions(&self, page: Id) -> Vec<(chrono::NaiveDateTime, PathBuf)> {
        let mut out = Vec::new();
        if let Ok(rd) = std::fs::read_dir(self.versions_dir(page)) {
            for e in rd.flatten() {
                let p = e.path();
                let stem = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                if let Ok(t) = chrono::NaiveDateTime::parse_from_str(&stem, "%Y%m%d-%H%M%S") {
                    out.push((t, p));
                }
            }
        }
        out.sort_by(|a, b| b.0.cmp(&a.0));
        out
    }

    pub fn load_version(&self, page: Id, path: &Path) -> Option<Page> {
        let data = std::fs::read(path).ok()?;
        decode_page(&data, self.key_for_page(page)).ok()
    }

    // ---- shared folder locks ----------------------------------------------

    fn lock_path(&self, page: Id) -> PathBuf {
        self.path.join("locks").join(format!("{page}.lock"))
    }

    /// Returns the name of another user editing the page right now.
    pub fn page_locked_by(&self, page: Id, me: &str) -> Option<String> {
        let path = self.lock_path(page);
        let meta = std::fs::metadata(&path).ok()?;
        let age = meta.modified().ok()?.elapsed().ok()?;
        if age.as_secs() > 120 {
            return None;
        }
        let who = std::fs::read_to_string(&path).ok()?;
        let who = who.trim().to_owned();
        (who != me && !who.is_empty()).then_some(who)
    }

    pub fn acquire_page_lock(&self, page: Id, me: &str) {
        atomic_write(&self.lock_path(page), me.as_bytes());
    }

    pub fn release_page_lock(&self, page: Id, me: &str) {
        let path = self.lock_path(page);
        if std::fs::read_to_string(&path).is_ok_and(|w| w.trim() == me) {
            let _ = std::fs::remove_file(path);
        }
    }

    // ---- integrity ---------------------------------------------------------

    /// Checks every page and asset; repairs what can be repaired.
    pub fn check_integrity(&mut self) -> Vec<String> {
        let mut report = Vec::new();
        let known: std::collections::HashSet<Id> = self
            .meta
            .sections()
            .iter()
            .flat_map(|s| s.pages.iter().map(|p| p.id))
            .chain(self.meta.trash.iter().flat_map(|t| match &t.kind {
                TrashKind::Page { meta, .. } => vec![meta.id],
                TrashKind::Section { section, .. } => section.pages.iter().map(|p| p.id).collect(),
            }))
            .collect();
        let mut assets_used = std::collections::HashSet::new();
        for id in known.iter().copied() {
            if !self.page_path(id).exists() {
                continue;
            }
            if !self.page_accessible(id) && self.meta.section_of_page(id).is_some() {
                continue;
            }
            match self.load_page(id) {
                Ok(p) => {
                    for it in &p.items {
                        let a = match &it.kind {
                            ItemKind::Image(i) => Some(i.asset.clone()),
                            ItemKind::File(f) => Some(f.asset.clone()),
                            _ => None,
                        };
                        if let Some(a) = a {
                            if !self.asset_path(&a).exists() {
                                report.push(format!("Отсутствует вложение {a} на странице {id}"));
                            }
                            assets_used.insert(a);
                        }
                    }
                }
                Err(e) => {
                    let bad = self.page_path(id);
                    let _ = std::fs::copy(&bad, bad.with_extension("corrupt"));
                    report.push(format!("Повреждена страница {id}: {e}. Копия сохранена как .corrupt"));
                }
            }
        }
        // Orphan pages: put them into a recovery section.
        let mut orphans = Vec::new();
        if let Ok(rd) = std::fs::read_dir(self.path.join("pages")) {
            for e in rd.flatten() {
                let p = e.path();
                if p.extension().is_some_and(|x| x == "page") {
                    if let Some(id) = p.file_stem().and_then(|s| Id::parse_str(&s.to_string_lossy()).ok()) {
                        if !known.contains(&id) {
                            orphans.push(id);
                        }
                    }
                }
            }
        }
        if !orphans.is_empty() {
            let mut sec = Section::new("Восстановленные страницы", 0);
            for id in &orphans {
                let mut pm = PageMeta::new("Восстановленная страница", "");
                pm.id = *id;
                if let Ok(p) = self.load_page(*id) {
                    let t = p.plain_text();
                    if let Some(first) = t.lines().find(|l| !l.trim().is_empty()) {
                        pm.title = first.chars().take(60).collect();
                    }
                }
                sec.pages.push(pm);
            }
            report.push(format!("Найдено потерянных страниц: {}. Добавлены в раздел «Восстановленные страницы»", orphans.len()));
            self.meta.tree.push(Node::Section(sec));
            self.dirty = true;
        }
        // Remove leftover temp files.
        for sub in ["pages", "assets"] {
            if let Ok(rd) = std::fs::read_dir(self.path.join(sub)) {
                for e in rd.flatten() {
                    if e.path().extension().is_some_and(|x| x == "tmp~") {
                        let _ = std::fs::remove_file(e.path());
                        report.push(format!("Удалён незавершённый файл {}", e.path().display()));
                    }
                }
            }
        }
        if report.is_empty() {
            report.push("Ошибок не найдено.".into());
        }
        report
    }

    /// (total bytes, attachments bytes, pages count)
    pub fn size_info(&self) -> (u64, u64, usize) {
        let total = dir_size(&self.path);
        let assets = dir_size(&self.path.join("assets"));
        let pages = self.meta.sections().iter().map(|s| s.pages.len()).sum();
        (total, assets, pages)
    }
}

pub fn decode_page(data: &[u8], key: Option<&Key>) -> Result<Page, String> {
    let plain;
    let bytes = if crypto::is_encrypted(data) {
        let key = key.ok_or("Раздел заблокирован")?;
        plain = crypto::decrypt(key, data).ok_or("Неверный ключ или повреждённые данные")?;
        &plain[..]
    } else {
        data
    };
    serde_json::from_slice(bytes).map_err(|e| e.to_string())
}

pub fn encode_page(page: &Page, key: Option<&Key>) -> Vec<u8> {
    let json = serde_json::to_vec(page).unwrap_or_default();
    match key {
        Some(k) => crypto::encrypt(k, &json),
        None => json,
    }
}

pub fn write_page(path: &Path, page: &Page, key: Option<&Key>) -> bool {
    atomic_write(path, &encode_page(page, key))
}

pub fn dir_size(p: &Path) -> u64 {
    let mut total = 0;
    if let Ok(rd) = std::fs::read_dir(p) {
        for e in rd.flatten() {
            let path = e.path();
            if path.is_dir() {
                total += dir_size(&path);
            } else if let Ok(m) = e.metadata() {
                total += m.len();
            }
        }
    }
    total
}

pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

pub fn sanitize(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| if "<>:\"/\\|?*".contains(c) || c.is_control() { '_' } else { c })
        .collect();
    let s = s.trim().trim_end_matches('.').to_owned();
    if s.is_empty() { "Без имени".into() } else { s }
}

pub fn human_size(b: u64) -> String {
    let b = b as f64;
    if b < 1024.0 {
        format!("{b:.0} Б")
    } else if b < 1024.0 * 1024.0 {
        format!("{:.1} КБ", b / 1024.0)
    } else if b < 1024.0 * 1024.0 * 1024.0 {
        format!("{:.1} МБ", b / 1024.0 / 1024.0)
    } else {
        format!("{:.2} ГБ", b / 1024.0 / 1024.0 / 1024.0)
    }
}

// ----------------------------------------------------------------------------
// Background writer: pages are serialized and written off the UI thread.

pub enum SaveJob {
    Page { path: PathBuf, page: Box<Page>, key: Option<Key> },
    Bytes { path: PathBuf, data: Vec<u8> },
    Flush(mpsc::Sender<()>),
}

pub struct Saver {
    tx: mpsc::Sender<SaveJob>,
}

impl Saver {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel::<SaveJob>();
        std::thread::Builder::new()
            .name("saver".into())
            .spawn(move || {
                while let Ok(job) = rx.recv() {
                    match job {
                        SaveJob::Page { path, page, key } => {
                            write_page(&path, &page, key.as_ref());
                        }
                        SaveJob::Bytes { path, data } => {
                            atomic_write(&path, &data);
                        }
                        SaveJob::Flush(done) => {
                            let _ = done.send(());
                        }
                    }
                }
            })
            .expect("saver thread");
        Self { tx }
    }

    pub fn send(&self, job: SaveJob) {
        let _ = self.tx.send(job);
    }

    /// Blocks until every queued job is written.
    pub fn flush(&self) {
        let (tx, rx) = mpsc::channel();
        let _ = self.tx.send(SaveJob::Flush(tx));
        let _ = rx.recv_timeout(std::time::Duration::from_secs(20));
    }
}

// ----------------------------------------------------------------------------
// Archives and backups

pub fn zip_dir(src: &Path, skip_versions: bool) -> std::io::Result<Vec<u8>> {
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut zw = zip::ZipWriter::new(&mut buf);
        let opts: zip::write::SimpleFileOptions =
            zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        fn walk(
            zw: &mut zip::ZipWriter<&mut std::io::Cursor<Vec<u8>>>,
            base: &Path,
            dir: &Path,
            opts: zip::write::SimpleFileOptions,
            skip_versions: bool,
        ) -> std::io::Result<()> {
            for e in std::fs::read_dir(dir)?.flatten() {
                let p = e.path();
                let rel = p.strip_prefix(base).unwrap_or(&p).to_string_lossy().replace('\\', "/");
                if rel.starts_with("locks") || (skip_versions && rel.starts_with("versions")) {
                    continue;
                }
                if p.is_dir() {
                    walk(zw, base, &p, opts, skip_versions)?;
                } else {
                    zw.start_file(rel, opts).map_err(std::io::Error::other)?;
                    zw.write_all(&std::fs::read(&p)?)?;
                }
            }
            Ok(())
        }
        walk(&mut zw, src, src, opts, skip_versions)?;
        zw.finish().map_err(std::io::Error::other)?;
    }
    Ok(buf.into_inner())
}

pub fn unzip_to(data: &[u8], dst: &Path) -> Result<(), String> {
    let mut za = zip::ZipArchive::new(std::io::Cursor::new(data)).map_err(|e| e.to_string())?;
    for i in 0..za.len() {
        let mut f = za.by_index(i).map_err(|e| e.to_string())?;
        let Some(rel) = f.enclosed_name() else { continue };
        let out = dst.join(rel);
        if f.is_dir() {
            let _ = std::fs::create_dir_all(&out);
            continue;
        }
        let mut buf = Vec::new();
        f.read_to_end(&mut buf).map_err(|e| e.to_string())?;
        if !atomic_write(&out, &buf) {
            return Err(format!("Не удалось записать {}", out.display()));
        }
    }
    Ok(())
}

pub fn backup_notebook(nb_path: &Path, name: &str, dir: &Path, keep: u32, key: Option<&Key>) -> Result<PathBuf, String> {
    let data = zip_dir(nb_path, false).map_err(|e| e.to_string())?;
    let (data, ext) = match key {
        Some(k) => (crypto::encrypt(k, &data), "nnbak"),
        None => (data, "zip"),
    };
    let folder = dir.join(sanitize(name));
    let file = folder.join(format!("{}.{ext}", chrono::Local::now().format("%Y%m%d-%H%M%S")));
    if !atomic_write(&file, &data) {
        return Err("Не удалось записать резервную копию".into());
    }
    let mut list = list_backups(dir, name);
    list.sort_by(|a, b| b.cmp(a));
    for old in list.iter().skip(keep.max(1) as usize) {
        let _ = std::fs::remove_file(old);
    }
    Ok(file)
}

pub fn list_backups(dir: &Path, name: &str) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir.join(sanitize(name)))
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "zip" || x == "nnbak"))
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v.reverse();
    v
}

pub fn restore_backup(backup: &Path, nb_path: &Path, key: Option<&Key>) -> Result<(), String> {
    let data = std::fs::read(backup).map_err(|e| e.to_string())?;
    let data = if crypto::is_encrypted(&data) {
        crypto::decrypt(key.ok_or("Нужен пароль резервных копий")?, &data).ok_or("Неверный пароль")?
    } else {
        data
    };
    // Keep the current state aside so a failed restore loses nothing.
    let aside = nb_path.with_extension(format!("before-restore-{}", chrono::Local::now().format("%Y%m%d%H%M%S")));
    std::fs::rename(nb_path, &aside).map_err(|e| e.to_string())?;
    match unzip_to(&data, nb_path) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = std::fs::remove_dir_all(nb_path);
            let _ = std::fs::rename(&aside, nb_path);
            Err(e)
        }
    }
}
