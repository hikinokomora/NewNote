//! Image textures decoded off the UI thread and cached per orientation.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc;

use egui::{ColorImage, TextureHandle, TextureOptions};

pub enum Tex {
    Loading,
    Ready(TextureHandle, [usize; 2]),
    Failed,
}

pub struct Textures {
    map: HashMap<String, Tex>,
    tx: mpsc::Sender<(String, Option<ColorImage>)>,
    rx: mpsc::Receiver<(String, Option<ColorImage>)>,
    jobs: mpsc::Sender<(String, PathBuf, u8, bool, bool)>,
}

const MAX_DIM: u32 = 2048;

impl Textures {
    pub fn new(ctx: egui::Context) -> Self {
        let (tx, rx) = mpsc::channel();
        let (jobs, jrx) = mpsc::channel::<(String, PathBuf, u8, bool, bool)>();
        let out = tx.clone();
        // A small pool keeps decoding of many images parallel but bounded.
        let jrx = std::sync::Arc::new(std::sync::Mutex::new(jrx));
        for n in 0..2 {
            let jrx = jrx.clone();
            let out = out.clone();
            let ctx = ctx.clone();
            std::thread::Builder::new()
                .name(format!("imgdec{n}"))
                .spawn(move || loop {
                    let job = { jrx.lock().unwrap().recv() };
                    let Ok((key, path, rot, fh, fv)) = job else { break };
                    let img = decode(&path, rot, fh, fv);
                    let _ = out.send((key, img));
                    ctx.request_repaint();
                })
                .ok();
        }
        Self { map: HashMap::new(), tx, rx, jobs }
    }

    pub fn poll(&mut self, ctx: &egui::Context) {
        while let Ok((key, img)) = self.rx.try_recv() {
            let t = match img {
                Some(img) => {
                    let size = img.size;
                    Tex::Ready(ctx.load_texture(&key, img, TextureOptions::LINEAR), size)
                }
                None => Tex::Failed,
            };
            self.map.insert(key, t);
        }
        let _ = &self.tx;
    }

    pub fn get(&mut self, path: PathBuf, rot: u8, fh: bool, fv: bool) -> &Tex {
        let key = format!("{}|{rot}|{fh}|{fv}", path.display());
        if !self.map.contains_key(&key) {
            self.map.insert(key.clone(), Tex::Loading);
            let _ = self.jobs.send((key.clone(), path, rot, fh, fv));
        }
        &self.map[&key]
    }

    pub fn forget(&mut self, path: &std::path::Path) {
        let p = path.display().to_string();
        self.map.retain(|k, _| !k.starts_with(&p));
    }
}

pub fn load_oriented(path: &std::path::Path, rot: u8, fh: bool, fv: bool) -> Option<image::DynamicImage> {
    let mut img = image::open(path).ok()?;
    if fh {
        img = img.fliph();
    }
    if fv {
        img = img.flipv();
    }
    img = match rot % 4 {
        1 => img.rotate90(),
        2 => img.rotate180(),
        3 => img.rotate270(),
        _ => img,
    };
    Some(img)
}

fn decode(path: &std::path::Path, rot: u8, fh: bool, fv: bool) -> Option<ColorImage> {
    let mut img = load_oriented(path, rot, fh, fv)?;
    if img.width() > MAX_DIM || img.height() > MAX_DIM {
        img = img.resize(MAX_DIM, MAX_DIM, image::imageops::FilterType::Triangle);
    }
    let rgba = img.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    Some(ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()))
}

/// Re-encodes an image to fit `max_dim` (JPEG unless it has transparency).
pub fn compress(bytes: &[u8], max_dim: u32, quality: u8) -> Option<(Vec<u8>, &'static str)> {
    let img = image::load_from_memory(bytes).ok()?;
    let img = if img.width() > max_dim || img.height() > max_dim {
        img.resize(max_dim, max_dim, image::imageops::FilterType::Lanczos3)
    } else {
        img
    };
    let has_alpha = img.color().has_alpha() && img.to_rgba8().pixels().any(|p| p.0[3] < 250);
    let mut out = Vec::new();
    if has_alpha {
        img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).ok()?;
        Some((out, "png"))
    } else {
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality).encode_image(&img.to_rgb8()).ok()?;
        Some((out, "jpg"))
    }
}
