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
    /// Set when the notebook file was damaged and had to be recovered.
    pub recovered: Option<String>,
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
            recovered: None,
        };
        nb.save_meta();
        Ok(nb)
    }

    /// Previous good copy of `notebook.json`, refreshed at most every few minutes.
    pub fn meta_backup_file(path: &Path) -> PathBuf {
        path.join("notebook.json.bak")
    }

    /// Opens a notebook; a damaged `notebook.json` is replaced by its last good copy.
    pub fn open(path: &Path) -> Result<Self, String> {
        let main = Self::file(path);
        match Self::open_from(path, &main) {
            Ok(nb) => Ok(nb),
            Err(e) => {
                let bak = Self::meta_backup_file(path);
                let mut nb = Self::open_from(path, &bak).map_err(|_| e.clone())?;
                if main.exists() {
                    let _ = std::fs::copy(&main, path.join(format!("notebook.json.damaged-{}", chrono::Local::now().format("%Y%m%d-%H%M%S"))));
                }
                let _ = std::fs::copy(&bak, &main);
                nb.recovered = Some(format!("Файл записной книжки «{}» был повреждён ({e}). Восстановлен из резервного файла; повреждённый файл сохранён рядом.", nb.meta.name));
                Ok(nb)
            }
        }
    }

    fn open_from(path: &Path, file: &Path) -> Result<Self, String> {
        let data = std::fs::read(file).map_err(|e| e.to_string())?;
        let file: NotebookFile = serde_json::from_slice(&data).map_err(|e| e.to_string())?;
        if file.meta.is_none() && file.enc.is_none() {
            return Err("пустой файл блокнота".into());
        }
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
            recovered: None,
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
            let main = Self::file(&self.path);
            // Keep the previous good state aside (refreshed at most every 5 minutes).
            let bak = Self::meta_backup_file(&self.path);
            let bak_age = std::fs::metadata(&bak).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).map(|d| d.as_secs());
            if bak_age.is_none_or(|a| a > 300) {
                if let Ok(old) = std::fs::read(&main) {
                    if old != b && serde_json::from_slice::<NotebookFile>(&old).is_ok() {
                        atomic_write(&bak, &old);
                    }
                }
            }
            atomic_write(&main, &b);
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

    /// Newest readable copy of a page from its version history.
    pub fn page_from_versions(&self, page: Id) -> Option<(Page, chrono::NaiveDateTime)> {
        self.list_versions(page).into_iter().find_map(|(t, p)| self.load_version(page, &p).map(|pg| (pg, t)))
    }

    /// Moves a damaged page file aside so it is never silently overwritten.
    pub fn keep_damaged_page(&self, page: Id) {
        let path = self.page_path(page);
        if path.exists() {
            let _ = std::fs::copy(&path, path.with_extension(format!("page.damaged-{}", chrono::Local::now().format("%Y%m%d-%H%M%S"))));
        }
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

pub struct BackupOpts {
    pub dir: PathBuf,
    pub mirror: Option<PathBuf>,
    /// Always keep this many newest copies.
    pub recent: u32,
    /// Then one copy per day for this many days...
    pub days: u32,
    /// ...and one per week for this many weeks.
    pub weeks: u32,
    pub key: Option<Key>,
}

pub struct BackupDone {
    pub file: PathBuf,
    pub size: u64,
    pub mirror_error: Option<String>,
}

/// Cheap change detector: names, sizes and write times of the notebook files.
pub fn fingerprint(nb_path: &Path) -> u64 {
    fn walk(base: &Path, dir: &Path, h: &mut u64) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        let mut entries: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
        entries.sort();
        for p in entries {
            let rel = p.strip_prefix(base).unwrap_or(&p).to_string_lossy().replace('\\', "/");
            if rel.starts_with("locks") || rel.starts_with("versions") || rel.ends_with("tmp~") || rel.ends_with(".bak") || rel.contains(".damaged-") {
                continue;
            }
            if p.is_dir() {
                walk(base, &p, h);
            } else if let Ok(m) = std::fs::metadata(&p) {
                let t = m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_nanos() as u64).unwrap_or(0);
                for v in [fnv1a(rel.as_bytes()), m.len(), t] {
                    *h = (*h ^ v).wrapping_mul(0x100000001b3);
                }
            }
        }
    }
    let mut h = 0xcbf29ce484222325;
    walk(nb_path, nb_path, &mut h);
    h
}

/// Checks that an archive opens, has a readable notebook file and every entry decompresses.
pub fn verify_archive(data: &[u8]) -> Result<usize, String> {
    let mut za = zip::ZipArchive::new(std::io::Cursor::new(data)).map_err(|e| format!("архив не читается: {e}"))?;
    let mut has_meta = false;
    for i in 0..za.len() {
        let mut f = za.by_index(i).map_err(|e| e.to_string())?;
        let name = f.name().to_owned();
        let mut buf = Vec::new();
        f.read_to_end(&mut buf).map_err(|e| format!("{name}: {e}"))?;
        if name == "notebook.json" {
            serde_json::from_slice::<NotebookFile>(&buf).map_err(|e| format!("notebook.json: {e}"))?;
            has_meta = true;
        }
    }
    if !has_meta {
        return Err("в архиве нет notebook.json".into());
    }
    Ok(za.len())
}

fn backup_folder(dir: &Path, name: &str) -> PathBuf {
    dir.join(sanitize(name))
}

/// Backs a notebook up unless nothing changed since the last copy (`force` skips that check).
/// Returns `Ok(None)` when skipped.
pub fn backup_notebook(nb_path: &Path, name: &str, o: &BackupOpts, force: bool) -> Result<Option<BackupDone>, String> {
    let folder = backup_folder(&o.dir, name);
    let state = folder.join(".state");
    let fp = fingerprint(nb_path);
    let last = std::fs::read_to_string(&state).ok().and_then(|s| s.trim().parse::<u64>().ok());
    if !force && last == Some(fp) && !list_backups(&o.dir, name).is_empty() {
        return Ok(None);
    }
    let data = zip_dir(nb_path, false).map_err(|e| e.to_string())?;
    verify_archive(&data)?;
    let (data, ext) = match &o.key {
        Some(k) => (crypto::encrypt(k, &data), "nnbak"),
        None => (data, "zip"),
    };
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    let write_to = |folder: &Path| -> Result<PathBuf, String> {
        let mut file = folder.join(format!("{stamp}.{ext}"));
        let mut n = 1;
        while file.exists() {
            n += 1;
            file = folder.join(format!("{stamp}-{n}.{ext}"));
        }
        if !atomic_write(&file, &data) {
            return Err(format!("не удалось записать {}", file.display()));
        }
        // Read back and compare: catches a full disk or a failing drive.
        match std::fs::read(&file) {
            Ok(back) if fnv1a(&back) == fnv1a(&data) && back.len() == data.len() => Ok(file),
            _ => {
                let _ = std::fs::remove_file(&file);
                Err(format!("копия {} не прошла проверку после записи", file.display()))
            }
        }
    };
    let file = write_to(&folder)?;
    prune_backups(&folder, o);
    let mirror_error = o.mirror.as_ref().and_then(|m| {
        let mf = backup_folder(m, name);
        match write_to(&mf) {
            Ok(_) => {
                prune_backups(&mf, o);
                None
            }
            Err(e) => Some(e),
        }
    });
    atomic_write(&state, fp.to_string().as_bytes());
    Ok(Some(BackupDone { file, size: data.len() as u64, mirror_error }))
}

fn backup_time(p: &Path) -> Option<chrono::NaiveDateTime> {
    let stem = p.file_stem()?.to_string_lossy().to_string();
    chrono::NaiveDateTime::parse_from_str(stem.get(..15)?, "%Y%m%d-%H%M%S").ok()
}

/// Tiered retention: the newest `recent` copies, everything from the last day,
/// one per day for `days` days and one per week for `weeks` weeks.
pub fn prune_backups(folder: &Path, o: &BackupOpts) {
    let mut list: Vec<(chrono::NaiveDateTime, PathBuf)> = std::fs::read_dir(folder)
        .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "zip" || x == "nnbak")).filter_map(|p| Some((backup_time(&p)?, p))).collect())
        .unwrap_or_default();
    list.sort_by(|a, b| b.0.cmp(&a.0));
    let now = now();
    let mut days_seen = std::collections::HashSet::new();
    let mut weeks_seen = std::collections::HashSet::new();
    for (i, (t, p)) in list.iter().enumerate() {
        let age = now - *t;
        let day = t.date();
        let week = { use chrono::Datelike; (t.iso_week().year(), t.iso_week().week()) };
        let keep = i < o.recent.max(1) as usize
            || age < chrono::Duration::hours(24)
            || (age < chrono::Duration::days(o.days as i64) && days_seen.insert(day))
            || (age < chrono::Duration::weeks(o.weeks as i64) && weeks_seen.insert(week));
        if !keep {
            let _ = std::fs::remove_file(p);
        }
    }
}

pub fn list_backups(dir: &Path, name: &str) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(backup_folder(dir, name))
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

/// Copies `settings.json` into the backup folder when it changed (keeps 60 copies).
pub fn backup_settings(dir: &Path, settings_file: &Path) {
    let Ok(data) = std::fs::read(settings_file) else { return };
    let folder = dir.join("_Настройки программы");
    let mut list: Vec<PathBuf> = std::fs::read_dir(&folder).map(|rd| rd.flatten().map(|e| e.path()).collect()).unwrap_or_default();
    list.sort();
    if list.last().and_then(|p| std::fs::read(p).ok()).is_some_and(|last| last == data) {
        return;
    }
    atomic_write(&folder.join(format!("{}.json", chrono::Local::now().format("%Y%m%d-%H%M%S"))), &data);
    list.reverse();
    for old in list.iter().skip(59) {
        let _ = std::fs::remove_file(old);
    }
}

/// Reads a whole backup (decrypting it if needed).
pub fn read_backup(backup: &Path, key: Option<&Key>) -> Result<Vec<u8>, String> {
    let data = std::fs::read(backup).map_err(|e| e.to_string())?;
    if crypto::is_encrypted(&data) {
        crypto::decrypt(key.ok_or("Нужен пароль резервных копий")?, &data).ok_or_else(|| "Неверный пароль резервных копий".to_string())
    } else {
        Ok(data)
    }
}

/// One file from a backup archive.
pub fn backup_file(archive: &[u8], rel: &str) -> Option<Vec<u8>> {
    let mut za = zip::ZipArchive::new(std::io::Cursor::new(archive)).ok()?;
    let mut f = za.by_name(rel).ok()?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).ok()?;
    Some(buf)
}

/// Sections and pages stored in a backup (unencrypted notebooks only).
pub fn backup_pages(archive: &[u8]) -> Result<Vec<(String, PageMeta)>, String> {
    let meta = backup_file(archive, "notebook.json").ok_or("в копии нет notebook.json")?;
    let file: NotebookFile = serde_json::from_slice(&meta).map_err(|e| e.to_string())?;
    let meta = file.meta.ok_or("блокнот в копии защищён паролем — восстановите его целиком")?;
    Ok(meta.sections().iter().flat_map(|s| s.pages.iter().map(move |p| (s.name.clone(), p.clone()))).collect())
}

/// Newest readable copy of a page across the backups (primary folder, then mirror).
pub fn page_from_backups(dirs: &[PathBuf], name: &str, page: Id, page_key: Option<&Key>, backup_key: Option<&Key>) -> Option<(Page, PathBuf)> {
    let rel = format!("pages/{page}.page");
    let mut all: Vec<PathBuf> = dirs.iter().flat_map(|d| list_backups(d, name)).collect();
    all.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
    for b in all {
        let Ok(archive) = read_backup(&b, backup_key) else { continue };
        if let Some(data) = backup_file(&archive, &rel) {
            if let Ok(p) = decode_page(&data, page_key) {
                return Some((p, b));
            }
        }
    }
    None
}

/// Restores `notebook.json` from the newest backup that has a readable one.
pub fn meta_from_backups(dirs: &[PathBuf], name: &str, nb_path: &Path, backup_key: Option<&Key>) -> Option<PathBuf> {
    let mut all: Vec<PathBuf> = dirs.iter().flat_map(|d| list_backups(d, name)).collect();
    all.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
    for b in all {
        let Ok(archive) = read_backup(&b, backup_key) else { continue };
        if let Some(meta) = backup_file(&archive, "notebook.json") {
            if serde_json::from_slice::<NotebookFile>(&meta).is_ok() {
                let main = Notebook::file(nb_path);
                if main.exists() {
                    let _ = std::fs::copy(&main, nb_path.join(format!("notebook.json.damaged-{}", chrono::Local::now().format("%Y%m%d-%H%M%S"))));
                }
                if atomic_write(&main, &meta) {
                    return Some(b);
                }
            }
        }
    }
    None
}

pub fn restore_backup(backup: &Path, nb_path: &Path, key: Option<&Key>) -> Result<(), String> {
    let data = read_backup(backup, key)?;
    verify_archive(&data)?;
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

#[cfg(test)]
mod safety_tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("nn-safety-{tag}-{}", new_id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn sample(root: &Path) -> (Notebook, Id) {
        let mut nb = Notebook::create(root, "Тест").unwrap();
        let mut sec = Section::new("Раздел", 1);
        let pm = PageMeta::new("Страница", "a");
        let mut page = Page::new(pm.id);
        page.ink_text = "важный текст".into();
        assert!(write_page(&nb.page_path(pm.id), &page, None));
        let id = pm.id;
        sec.pages.push(pm);
        nb.meta.tree.push(Node::Section(sec));
        nb.save_meta();
        (nb, id)
    }

    fn opts(dir: &Path, mirror: Option<PathBuf>) -> BackupOpts {
        BackupOpts { dir: dir.to_owned(), mirror, recent: 3, days: 30, weeks: 4, key: None }
    }

    #[test]
    fn backup_skip_verify_mirror_and_restore_page() {
        let root = tmp("b");
        let (nb, id) = sample(&root.join("nb"));
        let o = opts(&root.join("bk"), Some(root.join("mirror")));
        let first = backup_notebook(&nb.path, "Тест", &o, false).unwrap().expect("first copy");
        assert!(first.mirror_error.is_none());
        // Nothing changed: no new copy.
        assert!(backup_notebook(&nb.path, "Тест", &o, false).unwrap().is_none());
        assert_eq!(list_backups(&o.dir, "Тест").len(), 1);
        assert_eq!(list_backups(&root.join("mirror"), "Тест").len(), 1);
        // The archive is readable and the page can be taken back out of it.
        let a = read_backup(&first.file, None).unwrap();
        assert!(verify_archive(&a).is_ok());
        assert_eq!(backup_pages(&a).unwrap().len(), 1);
        // Damage the page file: it comes back from the backup.
        std::fs::write(nb.page_path(id), b"{broken").unwrap();
        assert!(nb.load_page(id).is_err());
        let (p, _) = page_from_backups(&[o.dir.clone()], "Тест", id, None, None).unwrap();
        assert_eq!(p.ink_text, "важный текст");
        // A change makes a new copy.
        std::fs::write(nb.page_path(id), encode_page(&p, None)).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        assert!(backup_notebook(&nb.path, "Тест", &o, false).unwrap().is_some());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn damaged_archive_is_rejected() {
        assert!(verify_archive(b"not a zip").is_err());
    }

    #[test]
    fn notebook_file_recovers_from_bak() {
        let root = tmp("m");
        let (mut nb, _) = sample(&root.join("nb"));
        // Force the .bak to be refreshed, then corrupt the main file.
        let _ = std::fs::remove_file(Notebook::meta_backup_file(&nb.path));
        nb.meta.name = "Тест 2".into();
        nb.save_meta();
        assert!(Notebook::meta_backup_file(&nb.path).exists());
        std::fs::write(Notebook::file(&nb.path), b"\0\0garbage").unwrap();
        let back = Notebook::open(&nb.path).unwrap();
        assert!(back.recovered.is_some());
        assert_eq!(back.meta.sections().len(), 1);
        // The damaged file is kept next to it.
        assert!(std::fs::read_dir(&nb.path).unwrap().flatten().any(|e| e.file_name().to_string_lossy().contains("damaged")));
        // And notebook.json was repaired on disk.
        assert!(Notebook::open(&nb.path).unwrap().recovered.is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn notebook_file_recovers_from_backup() {
        let root = tmp("r");
        let (nb, _) = sample(&root.join("nb"));
        let o = opts(&root.join("bk"), None);
        backup_notebook(&nb.path, "nb", &o, true).unwrap();
        std::fs::write(Notebook::file(&nb.path), b"garbage").unwrap();
        let _ = std::fs::remove_file(Notebook::meta_backup_file(&nb.path));
        assert!(Notebook::open(&nb.path).is_err());
        assert!(meta_from_backups(&[o.dir.clone()], "nb", &nb.path, None).is_some());
        assert_eq!(Notebook::open(&nb.path).unwrap().meta.sections().len(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn retention_keeps_recent_daily_weekly() {
        let root = tmp("p");
        let folder = root.join("x");
        std::fs::create_dir_all(&folder).unwrap();
        let now = now();
        // 4 copies a day for 60 days.
        for d in 0..60 {
            for h in [1, 7, 13, 19] {
                let t = now - chrono::Duration::days(d) - chrono::Duration::hours(h);
                std::fs::write(folder.join(format!("{}.zip", t.format("%Y%m%d-%H%M%S"))), b"z").unwrap();
            }
        }
        let o = BackupOpts { dir: root.clone(), mirror: None, recent: 3, days: 30, weeks: 8, key: None };
        prune_backups(&folder, &o);
        let left = std::fs::read_dir(&folder).unwrap().count();
        // Last 24 h (4) + one per day for the rest of 30 days (~29) + a few weekly ones.
        assert!((33..=45).contains(&left), "left {left}");
        let _ = std::fs::remove_dir_all(&root);
    }
}
