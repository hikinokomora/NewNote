//! Windows system services, all on-device: OCR (Windows.Media.Ocr), spell
//! checking (ISpellChecker), text to speech (SAPI), toast notifications, PDF
//! rendering (Windows.Data.Pdf), global hotkey, shell integration.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

#[cfg(windows)]
use windows::core::{HSTRING, PCWSTR};

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

// ----------------------------------------------------------------------------
// Shell

pub fn shell_open(target: &str) {
    shell_exec("open", target);
}

pub fn shell_print(path: &Path) {
    shell_exec("print", &path.to_string_lossy());
}

#[cfg(windows)]
fn shell_exec(verb: &str, target: &str) {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let v = wide(verb);
    let t = wide(target);
    unsafe {
        ShellExecuteW(std::ptr::null_mut(), v.as_ptr(), t.as_ptr(), std::ptr::null(), std::ptr::null(), SW_SHOWNORMAL);
    }
}

#[cfg(not(windows))]
fn shell_exec(_verb: &str, _target: &str) {}

fn run_hidden(cmd: &str, args: &[&str]) -> bool {
    let mut c = std::process::Command::new(cmd);
    c.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    c.status().is_ok_and(|s| s.success())
}

const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";

pub fn set_autostart(on: bool) -> bool {
    let exe = std::env::current_exe().unwrap_or_default();
    if on {
        let v = format!("\"{}\" --minimized", exe.display());
        run_hidden("reg", &["add", RUN_KEY, "/v", "NewNote", "/t", "REG_SZ", "/d", &v, "/f"])
    } else {
        run_hidden("reg", &["delete", RUN_KEY, "/v", "NewNote", "/f"])
    }
}

const CTX_KEY: &str = r"HKCU\Software\Classes\*\shell\NewNote";

pub fn set_explorer_menu(on: bool) -> bool {
    let exe = std::env::current_exe().unwrap_or_default();
    if on {
        let cmd = format!("\"{}\" --add \"%1\"", exe.display());
        let icon = format!("\"{}\"", exe.display());
        run_hidden("reg", &["add", CTX_KEY, "/ve", "/d", "Добавить файл в блокнот", "/f"])
            && run_hidden("reg", &["add", CTX_KEY, "/v", "Icon", "/d", &icon, "/f"])
            && run_hidden("reg", &["add", &format!(r"{CTX_KEY}\command"), "/ve", "/d", &cmd, "/f"])
    } else {
        run_hidden("reg", &["delete", CTX_KEY, "/f"])
    }
}

/// Launches the system screen snipping tool; the result lands on the clipboard.
pub fn start_screen_clip() {
    shell_open("ms-screenclip:");
}

/// Opens Windows voice typing (Win+H) for the focused text field.
#[cfg(windows)]
pub fn start_dictation() {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
    let key = |vk: u16, up: bool| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT { wVk: vk, wScan: 0, dwFlags: if up { KEYEVENTF_KEYUP } else { 0 }, time: 0, dwExtraInfo: 0 },
        },
    };
    let inputs = [key(VK_LWIN, false), key(b'H' as u16, false), key(b'H' as u16, true), key(VK_LWIN, true)];
    unsafe {
        SendInput(inputs.len() as u32, inputs.as_ptr(), std::mem::size_of::<INPUT>() as i32);
    }
}

#[cfg(not(windows))]
pub fn start_dictation() {}

/// Returns false if another instance already runs.
#[cfg(windows)]
pub fn single_instance() -> bool {
    use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
    use windows_sys::Win32::System::Threading::CreateMutexW;
    let name = wide("Local\\NewNote.SingleInstance");
    unsafe {
        let h = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
        !h.is_null() && GetLastError() != ERROR_ALREADY_EXISTS
    }
}

#[cfg(not(windows))]
pub fn single_instance() -> bool {
    true
}

// ----------------------------------------------------------------------------
// Global hotkey (Win+Alt+N, fallback Ctrl+Alt+N) on its own thread.

pub fn spawn_hotkey(ctx: egui::Context, tx: mpsc::Sender<()>) {
    #[cfg(windows)]
    std::thread::spawn(move || unsafe {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
        use windows_sys::Win32::UI::WindowsAndMessaging::*;
        let ok = RegisterHotKey(std::ptr::null_mut(), 1, MOD_WIN | MOD_ALT | MOD_NOREPEAT, b'N' as u32) != 0
            || RegisterHotKey(std::ptr::null_mut(), 1, MOD_CONTROL | MOD_ALT | MOD_NOREPEAT, b'N' as u32) != 0;
        if !ok {
            return;
        }
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            if msg.message == WM_HOTKEY {
                let _ = tx.send(());
                ctx.request_repaint();
            }
        }
    });
    #[cfg(not(windows))]
    let _ = (ctx, tx);
}

// ----------------------------------------------------------------------------
// Toast notifications (no server involved).

pub fn notify(title: &str, body: &str) {
    let (t, b) = (title.to_owned(), body.to_owned());
    std::thread::spawn(move || {
        #[cfg(windows)]
        let _ = toast(&t, &b);
        #[cfg(not(windows))]
        let _ = (t, b);
    });
}

#[cfg(windows)]
fn toast(title: &str, body: &str) -> windows::core::Result<()> {
    use windows::Data::Xml::Dom::XmlDocument;
    use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};
    let xml = format!(
        "<toast><visual><binding template=\"ToastGeneric\"><text>{}</text><text>{}</text></binding></visual><audio src=\"ms-winsoundevent:Notification.Reminder\"/></toast>",
        crate::richtext::esc(title),
        crate::richtext::esc(body)
    );
    let doc = XmlDocument::new()?;
    doc.LoadXml(&HSTRING::from(xml))?;
    let n = ToastNotification::CreateToastNotification(&doc)?;
    // Unpackaged apps need a registered AppUserModelID; PowerShell's is always present.
    let aumid = HSTRING::from("{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\\WindowsPowerShell\\v1.0\\powershell.exe");
    ToastNotificationManager::CreateToastNotifierWithId(&aumid)?.Show(&n)
}

// ----------------------------------------------------------------------------
// OCR

/// Recognizes text in an RGBA image. Lines are separated by newlines.
#[cfg(windows)]
pub fn ocr(img: &image::RgbaImage, lang: Option<&str>) -> Result<String, String> {
    use windows::Globalization::Language;
    use windows::Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap};
    use windows::Media::Ocr::OcrEngine;
    use windows::Storage::Streams::DataWriter;
    init_mta();
    let r = (|| -> windows::core::Result<String> {
        let engine = match lang {
            Some(l) if !l.is_empty() => {
                let lang = Language::CreateLanguage(&HSTRING::from(l))?;
                if OcrEngine::IsLanguageSupported(&lang)? {
                    OcrEngine::TryCreateFromLanguage(&lang)?
                } else {
                    OcrEngine::TryCreateFromUserProfileLanguages()?
                }
            }
            _ => OcrEngine::TryCreateFromUserProfileLanguages()?,
        };
        let max = OcrEngine::MaxImageDimension()?.max(256);
        let mut img = img.clone();
        if img.width() > max || img.height() > max {
            let k = max as f32 / img.width().max(img.height()) as f32;
            let (w, h) = (((img.width() as f32 * k) as u32).max(1), ((img.height() as f32 * k) as u32).max(1));
            img = image::imageops::resize(&img, w, h, image::imageops::FilterType::Triangle);
        }
        let (w, h) = (img.width() as i32, img.height() as i32);
        let mut bgra = img.into_raw();
        for px in bgra.chunks_exact_mut(4) {
            px.swap(0, 2);
        }
        let writer = DataWriter::new()?;
        writer.WriteBytes(&bgra)?;
        let buf = writer.DetachBuffer()?;
        let bmp = SoftwareBitmap::CreateCopyFromBuffer(&buf, BitmapPixelFormat::Bgra8, w, h)?;
        let res = engine.RecognizeAsync(&bmp)?.join()?;
        let mut out = String::new();
        for line in res.Lines()? {
            out.push_str(&line.Text()?.to_string_lossy());
            out.push('\n');
        }
        Ok(out.trim_end().to_owned())
    })();
    r.map_err(|e| e.message().to_string())
}

#[cfg(not(windows))]
pub fn ocr(_img: &image::RgbaImage, _lang: Option<&str>) -> Result<String, String> {
    Err("OCR доступен только в Windows".into())
}

#[cfg(windows)]
fn init_mta() {
    use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
}

pub fn ocr_languages() -> Vec<String> {
    #[cfg(windows)]
    {
        init_mta();
        if let Ok(langs) = windows::Media::Ocr::OcrEngine::AvailableRecognizerLanguages() {
            return langs.into_iter().filter_map(|l| l.LanguageTag().ok()).map(|t| t.to_string_lossy()).collect();
        }
    }
    Vec::new()
}

// ----------------------------------------------------------------------------
// PDF → images ("printout")

/// Renders every page of a PDF to PNG bytes.
#[cfg(windows)]
pub fn render_pdf(path: &Path, width: u32) -> Result<Vec<Vec<u8>>, String> {
    use windows::Data::Pdf::{PdfDocument, PdfPageRenderOptions};
    use windows::Storage::StorageFile;
    use windows::Storage::Streams::{DataReader, InMemoryRandomAccessStream};
    init_mta();
    let full = std::fs::canonicalize(path).map_err(|e| e.to_string())?;
    let p = full.to_string_lossy().trim_start_matches(r"\\?\").to_string();
    let r = (|| -> windows::core::Result<Vec<Vec<u8>>> {
        let file = StorageFile::GetFileFromPathAsync(&HSTRING::from(p))?.join()?;
        let doc = PdfDocument::LoadFromFileAsync(&file)?.join()?;
        let mut out = Vec::new();
        for i in 0..doc.PageCount()?.min(300) {
            let page = doc.GetPage(i)?;
            let stream = InMemoryRandomAccessStream::new()?;
            let opts = PdfPageRenderOptions::new()?;
            opts.SetDestinationWidth(width)?;
            page.RenderWithOptionsToStreamAsync(&stream, &opts)?.join()?;
            let size = stream.Size()? as u32;
            let reader = DataReader::CreateDataReader(&stream.GetInputStreamAt(0)?)?;
            reader.LoadAsync(size)?.join()?;
            let mut buf = vec![0u8; size as usize];
            reader.ReadBytes(&mut buf)?;
            out.push(buf);
        }
        Ok(out)
    })();
    r.map_err(|e| e.message().to_string())
}

#[cfg(not(windows))]
pub fn render_pdf(_path: &Path, _width: u32) -> Result<Vec<Vec<u8>>, String> {
    Err("Недоступно".into())
}

// ----------------------------------------------------------------------------
// Spell checking: a worker thread checks each unique word once.

pub struct Speller {
    tx: mpsc::Sender<SpellReq>,
    state: Arc<Mutex<SpellState>>,
}

#[derive(Default)]
struct SpellState {
    lang: String,
    bad: HashSet<String>,
    checked: HashSet<String>,
    pending: HashSet<String>,
    user: HashSet<String>,
}

enum SpellReq {
    Check(String, Vec<String>),
    Suggest(String, String, mpsc::Sender<Vec<String>>),
    Add(String, String),
}

impl Speller {
    pub fn new(ctx: egui::Context) -> Self {
        let (tx, rx) = mpsc::channel::<SpellReq>();
        let state = Arc::new(Mutex::new(SpellState::default()));
        let st = state.clone();
        std::thread::Builder::new()
            .name("speller".into())
            .spawn(move || {
                #[cfg(windows)]
                {
                    use windows::Win32::Globalization::*;
                    use windows::Win32::System::Com::*;
                    init_mta();
                    let factory: Option<ISpellCheckerFactory> =
                        unsafe { CoCreateInstance(&SpellCheckerFactory, None, CLSCTX_INPROC_SERVER).ok() };
                    let mut checkers: HashMap<String, Option<ISpellChecker>> = HashMap::new();
                    let mut get = |lang: &str| -> Option<ISpellChecker> {
                        checkers
                            .entry(lang.to_owned())
                            .or_insert_with(|| {
                                let f = factory.as_ref()?;
                                let l = HSTRING::from(lang);
                                let supported = unsafe { f.IsSupported(&l).ok()?.as_bool() };
                                if !supported {
                                    return None;
                                }
                                unsafe { f.CreateSpellChecker(&l).ok() }
                            })
                            .clone()
                    };
                    while let Ok(req) = rx.recv() {
                        match req {
                            SpellReq::Check(lang, words) => {
                                let Some(ch) = get(&lang) else {
                                    let mut s = st.lock().unwrap();
                                    for w in words {
                                        s.pending.remove(&w);
                                        s.checked.insert(w);
                                    }
                                    continue;
                                };
                                let mut bad = Vec::new();
                                for w in &words {
                                    let hw = HSTRING::from(w.as_str());
                                    if let Ok(errs) = unsafe { ch.Check(&hw) } {
                                        let mut e: Option<ISpellingError> = None;
                                        if unsafe { errs.Next(&mut e) }.is_ok() && e.is_some() {
                                            bad.push(w.clone());
                                        }
                                    }
                                }
                                let mut s = st.lock().unwrap();
                                if s.lang == lang {
                                    for w in words {
                                        s.pending.remove(&w);
                                        s.checked.insert(w);
                                    }
                                    s.bad.extend(bad);
                                }
                                drop(s);
                                ctx.request_repaint();
                            }
                            SpellReq::Suggest(lang, word, reply) => {
                                let mut out = Vec::new();
                                if let Some(ch) = get(&lang) {
                                    let hw = HSTRING::from(word.as_str());
                                    if let Ok(en) = unsafe { ch.Suggest(&hw) } {
                                        loop {
                                            let mut arr = [windows::core::PWSTR::null()];
                                            let mut n = 0u32;
                                            let hr = unsafe { en.Next(&mut arr, Some(&mut n)) };
                                            if hr.is_err() || n == 0 {
                                                break;
                                            }
                                            unsafe {
                                                out.push(arr[0].to_string().unwrap_or_default());
                                                CoTaskMemFree(Some(arr[0].0 as *const _));
                                            }
                                            if out.len() >= 8 {
                                                break;
                                            }
                                        }
                                    }
                                }
                                let _ = reply.send(out);
                            }
                            SpellReq::Add(lang, word) => {
                                if let Some(ch) = get(&lang) {
                                    if let Ok(ch2) = windows::core::Interface::cast::<ISpellChecker2>(&ch) {
                                        let hw = HSTRING::from(word.as_str());
                                        let _ = unsafe { ch2.Add(&hw) };
                                    }
                                }
                            }
                        }
                    }
                }
                #[cfg(not(windows))]
                {
                    let _ = (rx, st, ctx);
                }
            })
            .expect("speller thread");
        Self { tx, state }
    }

    pub fn set_language(&self, lang: &str, user: &[String]) {
        let mut s = self.state.lock().unwrap();
        if s.lang != lang {
            s.lang = lang.to_owned();
            s.bad.clear();
            s.checked.clear();
            s.pending.clear();
        }
        s.user = user.iter().map(|w| w.to_lowercase()).collect();
    }

    /// Non-blocking: unknown words are queued and reported on a later frame.
    pub fn is_bad(&self, word: &str) -> bool {
        // Words in another script than the checker language, and CamelCase names, are not checked.
        let latin = word.chars().all(|c| c.is_ascii());
        let cyr_lang = {
            let l = self.state.lock().unwrap();
            l.lang.starts_with("ru") || l.lang.starts_with("uk") || l.lang.starts_with("be")
        };
        if latin == cyr_lang || word.chars().skip(1).any(|c| c.is_uppercase()) {
            return false;
        }
        let mut s = self.state.lock().unwrap();
        if s.user.contains(&word.to_lowercase()) {
            return false;
        }
        if s.bad.contains(word) {
            return true;
        }
        if !s.checked.contains(word) && !s.pending.contains(word) && !s.lang.is_empty() {
            s.pending.insert(word.to_owned());
            let lang = s.lang.clone();
            let _ = self.tx.send(SpellReq::Check(lang, vec![word.to_owned()]));
        }
        false
    }

    pub fn suggest(&self, word: &str) -> Vec<String> {
        let lang = self.state.lock().unwrap().lang.clone();
        let (tx, rx) = mpsc::channel();
        let _ = self.tx.send(SpellReq::Suggest(lang, word.to_owned(), tx));
        rx.recv_timeout(std::time::Duration::from_millis(800)).unwrap_or_default()
    }

    pub fn add_word(&self, word: &str) {
        let mut s = self.state.lock().unwrap();
        s.bad.remove(word);
        s.user.insert(word.to_lowercase());
        let _ = self.tx.send(SpellReq::Add(s.lang.clone(), word.to_owned()));
    }
}

// ----------------------------------------------------------------------------
// Text to speech with word position tracking.

#[derive(Default, Clone)]
pub struct TtsStatus {
    pub speaking: bool,
    pub paused: bool,
    /// Char offset (UTF-16 units converted to chars) and length of the current word.
    pub word: Option<(usize, usize)>,
    pub voices: Vec<String>,
}

enum TtsCmd {
    Speak(String, i32, u16, String),
    Pause,
    Resume,
    Stop,
}

pub struct Tts {
    tx: mpsc::Sender<TtsCmd>,
    pub status: Arc<Mutex<TtsStatus>>,
}

impl Tts {
    pub fn new(ctx: egui::Context) -> Self {
        let (tx, rx) = mpsc::channel::<TtsCmd>();
        let status = Arc::new(Mutex::new(TtsStatus::default()));
        let st = status.clone();
        std::thread::Builder::new()
            .name("tts".into())
            .spawn(move || {
                #[cfg(windows)]
                unsafe {
                    use windows::Win32::Media::Speech::*;
                    use windows::Win32::System::Com::*;
                    let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
                    let Ok(voice) = CoCreateInstance::<_, ISpVoice>(&SpVoice, None, CLSCTX_ALL) else { return };
                    // Voices.
                    let mut tokens: Vec<(String, ISpObjectToken)> = Vec::new();
                    if let Ok(cat) = CoCreateInstance::<_, ISpObjectTokenCategory>(&SpObjectTokenCategory, None, CLSCTX_ALL) {
                        if cat.SetId(SPCAT_VOICES, false).is_ok() {
                            if let Ok(en) = cat.EnumTokens(PCWSTR::null(), PCWSTR::null()) {
                                loop {
                                    let mut t: Option<ISpObjectToken> = None;
                                    let mut n = 0u32;
                                    if en.Next(1, &mut t, Some(&mut n)).is_err() || n == 0 {
                                        break;
                                    }
                                    if let Some(t) = t {
                                        let name = t.GetStringValue(PCWSTR::null()).map(|p| p.to_string().unwrap_or_default()).unwrap_or_default();
                                        tokens.push((name, t));
                                    }
                                }
                            }
                        }
                    }
                    st.lock().unwrap().voices = tokens.iter().map(|t| t.0.clone()).collect();
                    let mut text16: Vec<u16> = Vec::new();
                    loop {
                        let speaking = st.lock().unwrap().speaking;
                        let cmd = if speaking {
                            match rx.recv_timeout(std::time::Duration::from_millis(40)) {
                                Ok(c) => Some(c),
                                Err(mpsc::RecvTimeoutError::Timeout) => None,
                                Err(_) => break,
                            }
                        } else {
                            match rx.recv() {
                                Ok(c) => Some(c),
                                Err(_) => break,
                            }
                        };
                        match cmd {
                            Some(TtsCmd::Speak(text, rate, vol, vname)) => {
                                if let Some((_, t)) = tokens.iter().find(|(n, _)| *n == vname) {
                                    let _ = voice.SetVoice(t);
                                }
                                let _ = voice.SetRate(rate.clamp(-10, 10));
                                let _ = voice.SetVolume(vol.min(100));
                                text16 = text.encode_utf16().chain(std::iter::once(0)).collect();
                                let flags = (SPF_ASYNC.0 | SPF_PURGEBEFORESPEAK.0 | SPF_IS_NOT_XML.0) as u32;
                                let _ = voice.Speak(PCWSTR(text16.as_ptr()), flags, None);
                                let mut s = st.lock().unwrap();
                                s.speaking = true;
                                s.paused = false;
                            }
                            Some(TtsCmd::Pause) => {
                                let _ = voice.Pause();
                                st.lock().unwrap().paused = true;
                            }
                            Some(TtsCmd::Resume) => {
                                let _ = voice.Resume();
                                st.lock().unwrap().paused = false;
                            }
                            Some(TtsCmd::Stop) => {
                                let empty = [0u16];
                                let _ = voice.Speak(PCWSTR(empty.as_ptr()), (SPF_ASYNC.0 | SPF_PURGEBEFORESPEAK.0) as u32, None);
                                let mut s = st.lock().unwrap();
                                s.speaking = false;
                                s.paused = false;
                                s.word = None;
                            }
                            None => {}
                        }
                        // Poll progress.
                        let mut vs = SPVOICESTATUS::default();
                        if voice.GetStatus(&mut vs, std::ptr::null_mut()).is_ok() {
                            let mut s = st.lock().unwrap();
                            if s.speaking {
                                if vs.dwRunningState == SPRS_DONE.0 as u32 && !s.paused {
                                    s.speaking = false;
                                    s.word = None;
                                } else {
                                    // UTF-16 offset → char offset.
                                    let pos = vs.ulInputWordPos as usize;
                                    let len = vs.ulInputWordLen as usize;
                                    let to_chars = |u: usize| String::from_utf16_lossy(&text16[..u.min(text16.len().saturating_sub(1))]).chars().count();
                                    s.word = Some((to_chars(pos), to_chars(pos + len) - to_chars(pos)));
                                }
                                ctx.request_repaint();
                            }
                        }
                    }
                }
                #[cfg(not(windows))]
                {
                    let _ = (rx, st, ctx);
                }
            })
            .expect("tts thread");
        Self { tx, status }
    }

    pub fn speak(&self, text: &str, rate: i32, volume: u16, voice: &str) {
        let _ = self.tx.send(TtsCmd::Speak(text.to_owned(), rate, volume, voice.to_owned()));
    }
    pub fn pause(&self) {
        let _ = self.tx.send(TtsCmd::Pause);
    }
    pub fn resume(&self) {
        let _ = self.tx.send(TtsCmd::Resume);
    }
    pub fn stop(&self) {
        let _ = self.tx.send(TtsCmd::Stop);
    }
    pub fn status(&self) -> TtsStatus {
        self.status.lock().unwrap().clone()
    }
}

/// Folder used for "send to notes" requests from other processes.
pub fn inbox_dir() -> PathBuf {
    crate::settings::app_data_dir().join("inbox")
}
