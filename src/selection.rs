//! Floating toolbar for selected ink and containers: colour, width, shape
//! style, align / distribute, flip, order, group, lock, layer, convert, save.

use std::sync::Arc;

use egui::{Color32, Rect, Ui, Vec2, pos2, vec2};

use crate::canvas::{Action, Editor, Env, Selection, apply_xform};
use crate::ink::{self, Xform};
use crate::model::*;

pub const PALETTE24: [[u8; 3]; 24] = [
    [0, 0, 0], [64, 64, 64], [128, 128, 128], [192, 192, 192], [255, 255, 255], [120, 70, 30],
    [210, 30, 30], [240, 90, 40], [245, 160, 0], [250, 215, 0], [170, 210, 40], [40, 160, 60],
    [0, 150, 120], [0, 170, 200], [30, 120, 230], [25, 60, 170], [90, 50, 190], [150, 60, 200],
    [210, 50, 150], [255, 130, 170], [255, 200, 150], [200, 230, 255], [210, 255, 210], [255, 245, 170],
];

pub fn swatches(ui: &mut Ui, mut pick: impl FnMut([u8; 4])) {
    egui::Grid::new(ui.next_auto_id()).spacing(vec2(3.0, 3.0)).show(ui, |ui| {
        for (i, c) in PALETTE24.iter().enumerate() {
            let (r, resp) = ui.allocate_exact_size(vec2(20.0, 20.0), egui::Sense::click());
            ui.painter().rect_filled(r, 4.0, Color32::from_rgb(c[0], c[1], c[2]));
            ui.painter().rect_stroke(r, 4.0, egui::Stroke::new(1.0, Color32::GRAY), egui::StrokeKind::Inside);
            if resp.clicked() {
                pick([c[0], c[1], c[2], 255]);
            }
            if i % 6 == 5 {
                ui.end_row();
            }
        }
    });
}

/// Units for align/distribute: one per group, loose stroke and container.
enum Unit {
    Strokes(Vec<usize>),
    Item(Id),
}

fn units(page: &Page, sel: &Selection) -> Vec<(Unit, Rect)> {
    let mut out = Vec::new();
    let mut seen_groups = std::collections::HashSet::new();
    for &i in &sel.strokes {
        let Some(s) = page.strokes.get(i) else { continue };
        if s.group != 0 {
            if !seen_groups.insert(s.group) {
                continue;
            }
            let members: Vec<usize> = sel.strokes.iter().copied().filter(|&j| page.strokes.get(j).is_some_and(|x| x.group == s.group)).collect();
            let r = members.iter().fold(Rect::NOTHING, |r, &j| r.union(page.strokes[j].bbox()));
            out.push((Unit::Strokes(members), r));
        } else {
            out.push((Unit::Strokes(vec![i]), s.bbox()));
        }
    }
    for id in &sel.items {
        if let Some(it) = page.item(*id) {
            out.push((Unit::Item(*id), it.rect()));
        }
    }
    out
}

fn move_unit(page: &mut Page, u: &Unit, d: Vec2) {
    if d == Vec2::ZERO {
        return;
    }
    let x = Xform { offset: d, ..Default::default() };
    match u {
        Unit::Strokes(v) => {
            for &i in v {
                if !page.strokes[i].locked {
                    page.strokes[i] = x.apply_stroke(&page.strokes[i]);
                }
            }
        }
        Unit::Item(id) => {
            if let Some(it) = page.item_mut(*id) {
                if !it.locked {
                    it.pos[0] += d.x;
                    it.pos[1] += d.y;
                }
            }
        }
    }
}

#[derive(Clone, Copy)]
enum AlignOp {
    Left,
    CenterH,
    Right,
    Top,
    Middle,
    Bottom,
    DistH,
    DistV,
}

fn align(page: &mut Page, ed: &Editor, op: AlignOp) {
    let us = units(page, &ed.selection);
    if us.len() < 2 {
        return;
    }
    let all = us.iter().fold(Rect::NOTHING, |r, u| r.union(u.1));
    match op {
        AlignOp::DistH | AlignOp::DistV => {
            if us.len() < 3 {
                return;
            }
            let mut v: Vec<(usize, Rect)> = us.iter().enumerate().map(|(i, u)| (i, u.1)).collect();
            let horiz = matches!(op, AlignOp::DistH);
            v.sort_by(|a, b| {
                let (x, y) = if horiz { (a.1.center().x, b.1.center().x) } else { (a.1.center().y, b.1.center().y) };
                x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal)
            });
            let total: f32 = v.iter().map(|(_, r)| if horiz { r.width() } else { r.height() }).sum();
            let span = if horiz { all.width() } else { all.height() };
            let gap = (span - total) / (v.len() - 1) as f32;
            let mut cursor = if horiz { all.min.x } else { all.min.y };
            for (i, r) in v {
                let d = if horiz { vec2(cursor - r.min.x, 0.0) } else { vec2(0.0, cursor - r.min.y) };
                move_unit(page, &us[i].0, d);
                cursor += (if horiz { r.width() } else { r.height() }) + gap;
            }
        }
        _ => {
            for (u, r) in &us {
                let d = match op {
                    AlignOp::Left => vec2(all.min.x - r.min.x, 0.0),
                    AlignOp::CenterH => vec2(all.center().x - r.center().x, 0.0),
                    AlignOp::Right => vec2(all.max.x - r.max.x, 0.0),
                    AlignOp::Top => vec2(0.0, all.min.y - r.min.y),
                    AlignOp::Middle => vec2(0.0, all.center().y - r.center().y),
                    AlignOp::Bottom => vec2(0.0, all.max.y - r.max.y),
                    _ => Vec2::ZERO,
                };
                move_unit(page, u, d);
            }
        }
    }
    crate::canvas::update_connectors(page);
}

/// Moves selected strokes to the front / back (keeps the selection valid).
fn reorder(page: &mut Page, ed: &mut Editor, front: bool, one_step: bool) {
    let sel: std::collections::HashSet<usize> = ed.selection.strokes.iter().copied().collect();
    let n = page.strokes.len();
    let mut order: Vec<usize> = (0..n).collect();
    if one_step {
        if front {
            for i in (0..n.saturating_sub(1)).rev() {
                if sel.contains(&order[i]) && !sel.contains(&order[i + 1]) {
                    order.swap(i, i + 1);
                }
            }
        } else {
            for i in 1..n {
                if sel.contains(&order[i]) && !sel.contains(&order[i - 1]) {
                    order.swap(i, i - 1);
                }
            }
        }
    } else {
        let (a, b): (Vec<usize>, Vec<usize>) = order.iter().partition(|i| !sel.contains(i));
        order = if front { a.into_iter().chain(b).collect() } else { b.into_iter().chain(a).collect() };
    }
    let old = std::mem::take(&mut page.strokes);
    let mut slots: Vec<Option<Stroke>> = old.into_iter().map(Some).collect();
    page.strokes = order.iter().map(|&i| slots[i].take().unwrap()).collect();
    ed.selection.strokes = order.iter().enumerate().filter(|(_, o)| sel.contains(o)).map(|(i, _)| i).collect();
    // Items too.
    let ids = ed.selection.items.clone();
    if !ids.is_empty() && !one_step {
        let (mut a, b): (Vec<Item>, Vec<Item>) = page.items.drain(..).partition(|it| !ids.contains(&it.id));
        if front {
            a.extend(b);
            page.items = a;
        } else {
            let mut b = b;
            b.extend(a);
            page.items = b;
        }
    }
}

fn edit_strokes(page: &mut Page, ed: &Editor, mut f: impl FnMut(&mut Stroke)) {
    for &i in &ed.selection.strokes {
        if let Some(s) = page.strokes.get_mut(i) {
            if !s.locked {
                f(s);
            }
        }
    }
}

fn edit_style(page: &mut Page, ed: &Editor, mut f: impl FnMut(&mut StrokeStyle)) {
    edit_strokes(page, ed, |s| {
        let mut st = (*s.style).clone();
        f(&mut st);
        s.style = Arc::new(st);
    });
}

/// Returns true when the page changed.
pub fn bar(ui: &mut Ui, page: &mut Page, ed: &mut Editor, env: &mut Env, body: Rect, clip: Rect) -> bool {
    let mut changed = false;
    let pos = pos2(body.min.x.max(clip.min.x + 4.0), (body.max.y + 8.0).min(clip.max.y - 40.0));
    let area = egui::Area::new(egui::Id::new(("selbar", ed.page_id))).fixed_pos(pos).order(egui::Order::Foreground);
    let has_strokes = !ed.selection.strokes.is_empty();
    let first = ed.selection.strokes.first().and_then(|&i| page.strokes.get(i)).cloned();
    area.show(ui.ctx(), |ui| {
        egui::Frame::popup(ui.style()).inner_margin(4.0).show(ui, |ui| {
            ui.horizontal(|ui| {
                let mut snap = |ed: &mut Editor, page: &Page| ed.snapshot(page);
                if ui.small_button("⧉").on_hover_text("Копировать (Ctrl+C)").clicked() && !env.no_copy {
                    *env.clip = ed.copy_selection(page);
                }
                if ui.small_button("✂").on_hover_text("Вырезать (Ctrl+X)").clicked() && !env.no_copy {
                    *env.clip = ed.copy_selection(page);
                    ed.delete_selection(page);
                    changed = true;
                }
                if ui.small_button("⿻").on_hover_text("Дублировать (Ctrl+D)").clicked() {
                    let c = ed.copy_selection(page);
                    ed.paste(page, &c, None);
                    changed = true;
                }
                if ui.small_button("🗑").on_hover_text("Удалить (Del)").clicked() {
                    ed.delete_selection(page);
                    changed = true;
                }
                if has_strokes {
                    ui.separator();
                    let cur = first.as_ref().map(|s| s.color).unwrap_or([0, 0, 0, 255]);
                    ui.menu_button(egui::RichText::new("●").color(Color32::from_rgb(cur[0], cur[1], cur[2])), |ui| {
                        ui.label("Цвет всех выделенных штрихов");
                        swatches(ui, |c| {
                            snap(ed, page);
                            edit_strokes(page, ed, |s| s.color = [c[0], c[1], c[2], s.color[3]]);
                            changed = true;
                        });
                        let mut col = Color32::from_rgba_unmultiplied(cur[0], cur[1], cur[2], cur[3]);
                        if ui.color_edit_button_srgba(&mut col).changed() {
                            edit_strokes(page, ed, |s| s.color = col.to_srgba_unmultiplied());
                            changed = true;
                        }
                    })
                    .response
                    .on_hover_text("Цвет");
                    ui.menu_button("≡", |ui| {
                        ui.label("Толщина");
                        let mut k = 1.0f32;
                        if ui.add(egui::Slider::new(&mut k, 0.25..=4.0).logarithmic(true).text("× множитель")).drag_stopped() {
                            snap(ed, page);
                            edit_strokes(page, ed, |s| s.width = (s.width * k).clamp(0.2, 200.0));
                            changed = true;
                        }
                        ui.horizontal_wrapped(|ui| {
                            for mm in [0.25, 0.5, 0.75, 1.0, 1.5, 2.0, 3.0, 5.0, 8.0, 12.0] {
                                if ui.small_button(format!("{mm}")).clicked() {
                                    snap(ed, page);
                                    edit_strokes(page, ed, |s| s.width = mm * crate::settings::MM);
                                    changed = true;
                                }
                            }
                            ui.label("мм");
                        });
                    })
                    .response
                    .on_hover_text("Толщина");
                    ui.menu_button("◇", |ui| {
                        ui.label("Стиль линий и фигур");
                        let st = first.as_ref().map(|s| (*s.style).clone()).unwrap_or_default();
                        ui.horizontal(|ui| {
                            for d in Dash::ALL {
                                if ui.selectable_label(st.dash == d, d.label()).clicked() {
                                    snap(ed, page);
                                    edit_style(page, ed, |x| x.dash = d);
                                    changed = true;
                                }
                            }
                        });
                        ui.horizontal(|ui| {
                            if ui.selectable_label(st.arrow_start, "← начало").clicked() {
                                edit_style(page, ed, |x| x.arrow_start = !st.arrow_start);
                                changed = true;
                            }
                            if ui.selectable_label(st.arrow_end, "конец →").clicked() {
                                edit_style(page, ed, |x| x.arrow_end = !st.arrow_end);
                                changed = true;
                            }
                        });
                        ui.horizontal(|ui| {
                            if ui.selectable_label(st.shadow, "Тень").clicked() {
                                edit_style(page, ed, |x| x.shadow = !st.shadow);
                                changed = true;
                            }
                            if ui.selectable_label(st.glow, "Свечение").clicked() {
                                edit_style(page, ed, |x| x.glow = !st.glow);
                                changed = true;
                            }
                        });
                        ui.separator();
                        ui.label("Заливка");
                        ui.horizontal(|ui| {
                            if ui.small_button("Без заливки").clicked() {
                                snap(ed, page);
                                edit_strokes(page, ed, |s| s.fill = None);
                                changed = true;
                            }
                        });
                        swatches(ui, |c| {
                            snap(ed, page);
                            edit_strokes(page, ed, |s| {
                                if s.closed {
                                    s.fill = Some([c[0], c[1], c[2], 200]);
                                }
                            });
                            changed = true;
                        });
                        ui.separator();
                        ui.label("Подпись внутри фигуры");
                        let mut label = st.label.clone();
                        if ui.text_edit_singleline(&mut label).changed() {
                            edit_style(page, ed, |x| x.label = label.clone());
                            changed = true;
                        }
                        ui.separator();
                        ui.label("Эффект");
                        ui.horizontal_wrapped(|ui| {
                            for e in [Effect::None, Effect::Rainbow, Effect::Gold, Effect::Silver] {
                                if ui.selectable_label(st.effect == e, e.label()).clicked() {
                                    edit_style(page, ed, |x| x.effect = e);
                                    changed = true;
                                }
                            }
                        });
                    })
                    .response
                    .on_hover_text("Стиль фигуры");
                }
                ui.separator();
                ui.menu_button("⫷", |ui| {
                    for (label, op) in [
                        ("По левому краю", AlignOp::Left),
                        ("По центру", AlignOp::CenterH),
                        ("По правому краю", AlignOp::Right),
                        ("По верху", AlignOp::Top),
                        ("По середине", AlignOp::Middle),
                        ("По низу", AlignOp::Bottom),
                        ("Распределить по горизонтали", AlignOp::DistH),
                        ("Распределить по вертикали", AlignOp::DistV),
                    ] {
                        if ui.button(label).clicked() {
                            snap(ed, page);
                            align(page, ed, op);
                            changed = true;
                            ui.close();
                        }
                    }
                })
                .response
                .on_hover_text("Выровнять и распределить");
                if ui.small_button("⇋").on_hover_text("Отразить по горизонтали").clicked() {
                    let b = ed.selection_bounds(page);
                    snap(ed, page);
                    apply_xform(page, ed, &Xform { pivot: b.center(), scale: vec2(-1.0, 1.0), ..Default::default() });
                    changed = true;
                }
                if ui.small_button("⇵").on_hover_text("Отразить по вертикали").clicked() {
                    let b = ed.selection_bounds(page);
                    snap(ed, page);
                    apply_xform(page, ed, &Xform { pivot: b.center(), scale: vec2(1.0, -1.0), ..Default::default() });
                    changed = true;
                }
                ui.menu_button("⧉↕", |ui| {
                    for (label, front, step) in [("На передний план", true, false), ("Выше", true, true), ("Ниже", false, true), ("На задний план", false, false)] {
                        if ui.button(label).clicked() {
                            snap(ed, page);
                            reorder(page, ed, front, step);
                            changed = true;
                            ui.close();
                        }
                    }
                })
                .response
                .on_hover_text("Порядок");
                if has_strokes {
                    let grouped = first.as_ref().is_some_and(|s| s.group != 0);
                    if ui.small_button(if grouped { "⊟" } else { "⊞" }).on_hover_text(if grouped { "Разгруппировать (Ctrl+Shift+G)" } else { "Группировать (Ctrl+G)" }).clicked() {
                        snap(ed, page);
                        group(page, ed, !grouped);
                        changed = true;
                    }
                    if ui.small_button("🔒").on_hover_text("Заблокировать (запрет перемещения)").clicked() {
                        snap(ed, page);
                        edit_strokes(page, ed, |s| s.locked = true);
                        for id in ed.selection.items.clone() {
                            if let Some(it) = page.item_mut(id) {
                                it.locked = true;
                            }
                        }
                        ed.selection = Selection::default();
                        changed = true;
                    }
                    ui.menu_button("☰", |ui| {
                        ui.label("Слой");
                        for (i, name) in LAYER_NAMES.iter().enumerate() {
                            if ui.button(*name).clicked() {
                                snap(ed, page);
                                edit_strokes(page, ed, |s| s.layer = i as u8);
                                changed = true;
                                ui.close();
                            }
                        }
                    })
                    .response
                    .on_hover_text("Переместить в слой");
                    ui.menu_button("⟲", |ui| {
                        if ui.button("Рукопись в текст").clicked() {
                            env.actions.push(Action::OcrInk);
                            ui.close();
                        }
                        if ui.button("Рукопись в фигуры").clicked() {
                            snap(ed, page);
                            to_shapes(page, ed);
                            changed = true;
                            ui.close();
                        }
                        if ui.button("Рукопись в формулу").clicked() {
                            env.actions.push(Action::InkToMath);
                            ui.close();
                        }
                    })
                    .response
                    .on_hover_text("Преобразовать");
                    ui.menu_button("💾", |ui| {
                        if ui.button("Сохранить как PNG…").clicked() {
                            env.actions.push(Action::ExportSelection("png"));
                            ui.close();
                        }
                        if ui.button("Сохранить как SVG…").clicked() {
                            env.actions.push(Action::ExportSelection("svg"));
                            ui.close();
                        }
                        if ui.button("Копировать как изображение").clicked() && !env.no_copy {
                            env.actions.push(Action::ExportSelection("copy"));
                            ui.close();
                        }
                    })
                    .response
                    .on_hover_text("Сохранить рисунок");
                }
            });
        });
    });
    changed
}

pub fn group(page: &mut Page, ed: &Editor, on: bool) {
    let g = if on { rand_u64() as u32 | 1 } else { 0 };
    edit_strokes(page, ed, |s| s.group = g);
}

pub fn to_shapes(page: &mut Page, ed: &mut Editor) {
    let mut idx = ed.selection.strokes.clone();
    idx.sort_unstable();
    for &i in &idx {
        let Some(s) = page.strokes.get(i) else { continue };
        if s.kind == InkKind::Shape || s.locked {
            continue;
        }
        let Some(rec) = ink::recognize(&s.points) else { continue };
        let (pts, closed) = match rec {
            ink::Recognized::Line(a, b) => (vec![a, b], false),
            ink::Recognized::Polygon(v) => (v, true),
            ink::Recognized::Ellipse(r) => (ink::ellipse_points(r, 72), true),
        };
        let mut n = ink::make_shape_stroke(pts, closed, s.color, s.width, None);
        n.layer = s.layer;
        n.group = s.group;
        page.strokes[i] = n;
    }
}

/// Shift+click-free helper for the app: unlock everything on the page.
pub fn unlock_all(page: &mut Page) {
    for s in page.strokes.iter_mut() {
        s.locked = false;
    }
    for it in page.items.iter_mut() {
        it.locked = false;
    }
}
