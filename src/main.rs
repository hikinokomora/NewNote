#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod brush;
mod calc;
mod canvas;
mod crypto;
mod export;
mod fonts;
mod helper;
mod ink;
mod items;
mod model;
mod ops;
mod pen;
mod raster;
mod richtext;
mod search;
mod selection;
mod settings;
mod shapes;
mod smooth;
mod storage;
mod templates;
mod textures;
mod ui_dialogs;
mod ui_draw;
mod ui_main;
mod ui_panels;
mod winapi;

use std::path::PathBuf;

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut add = Vec::new();
    let mut minimized = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--add" if i + 1 < args.len() => {
                add.push(PathBuf::from(&args[i + 1]));
                i += 1;
            }
            "--minimized" => minimized = true,
            other => add.push(PathBuf::from(other)),
        }
        i += 1;
    }
    // Only one instance writes to the notebooks; others hand their files over.
    if !winapi::single_instance() {
        let dir = winapi::inbox_dir();
        let _ = std::fs::create_dir_all(&dir);
        let mut body: String = add.iter().map(|p| format!("{}\n", p.display())).collect();
        body.push_str("__focus__\n");
        let _ = std::fs::write(dir.join(format!("{}.txt", uuid::Uuid::new_v4())), body);
        return Ok(());
    }
    let icon = make_icon();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("NewNote")
            .with_inner_size([1360.0, 860.0])
            .with_min_inner_size([640.0, 420.0])
            .with_drag_and_drop(true)
            .with_icon(icon),
        multisampling: 4,
        ..Default::default()
    };
    eframe::run_native("NewNote", options, Box::new(move |cc| Ok(Box::new(app::App::new(cc, add, minimized)))))
}

/// Simple generated icon: a notebook with a pen stroke.
fn make_icon() -> egui::IconData {
    let n = 64u32;
    let mut rgba = vec![0u8; (n * n * 4) as usize];
    for y in 0..n {
        for x in 0..n {
            let i = ((y * n + x) * 4) as usize;
            let inside = (8..56).contains(&x) && (4..60).contains(&y);
            let spine = (8..16).contains(&x) && (4..60).contains(&y);
            let stroke = {
                let fx = x as f32;
                let fy = y as f32;
                let yy = 34.0 + 8.0 * ((fx - 20.0) / 7.0).sin();
                (20..52).contains(&x) && (fy - yy).abs() < 2.2
            };
            let c: [u8; 4] = if stroke {
                [255, 255, 255, 255]
            } else if spine {
                [40, 70, 160, 255]
            } else if inside {
                [70, 120, 230, 255]
            } else {
                [0, 0, 0, 0]
            };
            rgba[i..i + 4].copy_from_slice(&c);
        }
    }
    egui::IconData { rgba, width: n, height: n }
}
