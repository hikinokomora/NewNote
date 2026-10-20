//! Content containers on the canvas: text, tables, images, files,
//! formulas, code, diagrams and dividers, plus tags and task markers.

use egui::text::{CCursor, CCursorRange, LayoutJob, TextFormat};
use egui::{Align2, Color32, CursorIcon, FontId, Pos2, Rect, Sense, Stroke as EStroke, TextEdit, Ui, Vec2, pos2, vec2};

use crate::calc;
use crate::canvas::{Action, Editor, Env, Selection};
use crate::model::*;
use crate::richtext::{self, Click};
use crate::textures::Tex;

pub enum Op {
    Snapshot,
    Delete(Id),
    Duplicate(Id),
    Front(Id),
    Back(Id),
    TextToTable(Id),
    TableToText(Id),
}

pub fn apply_ops(ops: Vec<Op>, page: &mut Page, ed: &mut Editor, _env: &mut Env) -> bool {
    let mut changed = false;
    let mut snap = false;
    for op in ops {
        if !snap && !matches!(op, Op::Snapshot) {
            ed.snapshot(page);
            snap = true;
        }
        match op {
            Op::Snapshot => {
                if !snap {
                    ed.snapshot(page);
                    snap = true;
                }
            }
            Op::Delete(id) => {
                page.items.retain(|i| i.id != id);
                if ed.editing == Some(id) {
                    ed.editing = None;
                }
                ed.selection.items.retain(|i| *i != id);
                changed = true;
            }
            Op::Duplicate(id) => {
                if let Some(it) = page.item(id).cloned() {
                    let mut it = it;
                    it.id = new_id();
                    it.pos[0] += 20.0;
                    it.pos[1] += 20.0;
                    page.items.push(it);
                    changed = true;
                }
            }
            Op::Front(id) => {
                if let Some(i) = page.items.iter().position(|x| x.id == id) {
                    let it = page.items.remove(i);
                    page.items.push(it);
                    changed = true;
                }
            }
            Op::Back(id) => {
                if let Some(i) = page.items.iter().position(|x| x.id == id) {
                    let it = page.items.remove(i);
                    page.items.insert(0, it);
                    changed = true;
                }
            }
            Op::TextToTable(id) => {
                if let Some(it) = page.item_mut(id) {
                    if let ItemKind::Text(t) = &it.kind {
                        let plain = richtext::strip_markup(&t.text);
                        let cells = crate::export::parse_csv(&plain);
                        it.kind = ItemKind::Table(crate::export::table_from_cells(cells));
                        changed = true;
                    }
                }
            }
            Op::TableToText(id) => {
                if let Some(it) = page.item_mut(id) {
                    if let ItemKind::Table(t) = &it.kind {
                        let text = (0..t.rows())
                            .map(|r| (0..t.cols()).map(|c| calc::cell_display(&t.cells, r, c)).collect::<Vec<_>>().join("\t"))
                            .collect::<Vec<_>>()
                            .join("\n");
                        it.kind = ItemKind::Text(TextBlock::new(&text, 15.0, ""));
                        changed = true;
                    }
                }
            }
        }
    }
    changed
}

fn c32(c: [u8; 4]) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3])
}

/// Draws one item. Returns true when content changed.
#[allow(clippy::too_many_arguments)]
pub fn item_ui(ui: &mut Ui, item: &mut Item, sr: Rect, clip: Rect, ed: &mut Editor, env: &mut Env, interactive: bool, selected: bool, ops: &mut Vec<Op>) -> bool {
    let mut changed = false;
    let id = egui::Id::new(("item", item.id));
    let zoom = ed.zoom;
    let editing = ed.editing == Some(item.id);
    let hovered = interactive && ui.rect_contains_pointer(sr.expand2(vec2(14.0, 16.0)).intersect(clip));
    let painter = ui.painter_at(clip);

    // Content.
    let content_h = match &item.kind {
        ItemKind::Text(_) => text_ui(ui, item, sr, clip, ed, env, interactive, &mut changed, ops),
        ItemKind::Table(_) => table_ui(ui, item, sr, clip, ed, env, interactive, &mut changed),
        ItemKind::Image(_) => image_ui(ui, item, sr, clip, ed, env, interactive, &mut changed),
        ItemKind::File(_) => file_ui(ui, item, sr, clip, ed, env, interactive),
        ItemKind::Equation(_) => equation_ui(ui, item, sr, clip, ed, env, interactive, &mut changed),
        ItemKind::Code(_) => code_ui(ui, item, sr, clip, ed, env, interactive, &mut changed),
        ItemKind::Diagram(_) => diagram_ui(ui, item, sr, clip, ed, env, interactive, &mut changed),
        ItemKind::Chart(_) => chart_ui(ui, item, sr, clip, ed, env, interactive, &mut changed),
        ItemKind::Divider { page_break } => {
            let y = sr.min.y + 10.0 * zoom;
            let c = if env.dark { Color32::from_gray(110) } else { Color32::from_gray(150) };
            if *page_break {
                painter.add(egui::Shape::dashed_line(&[pos2(sr.min.x, y), pos2(sr.max.x, y)], EStroke::new(1.0, c), 6.0, 4.0));
                painter.text(pos2(sr.center().x, y - 2.0), Align2::CENTER_BOTTOM, "разрыв страницы", FontId::proportional(10.0 * zoom), c);
            } else {
                painter.line_segment([pos2(sr.min.x, y), pos2(sr.max.x, y)], EStroke::new(1.5 * zoom.max(0.5), c));
            }
            if interactive {
                let r = ui.interact(Rect::from_min_max(pos2(sr.min.x, y - 6.0), pos2(sr.max.x, y + 6.0)).intersect(clip), id.with("div"), Sense::click());
                if r.clicked() {
                    ed.selection = Selection { strokes: vec![], items: vec![item.id] };
                }
            }
            20.0 * zoom
        }
    };
    let new_h = content_h / zoom;
    if (item.height - new_h).abs() > 0.5 {
        item.height = new_h;
    }
    let full = Rect::from_min_size(sr.min, vec2(sr.width(), content_h.max(20.0 * zoom)));

    // Tags and task marker to the left.
    let mut x = full.min.x - 4.0;
    let tag_y = full.min.y + 2.0;
    if let Some(task) = item.task.clone() {
        x -= 18.0;
        let r = Rect::from_min_size(pos2(x, tag_y), vec2(16.0, 16.0));
        let col = match task.priority {
            1 => Color32::from_rgb(220, 50, 50),
            2 => Color32::from_rgb(230, 150, 0),
            _ => Color32::from_rgb(66, 133, 244),
        };
        painter.rect_stroke(r, 3.0, EStroke::new(1.5, col), egui::StrokeKind::Inside);
        if task.done {
            painter.text(r.center(), Align2::CENTER_CENTER, "✔", FontId::proportional(13.0), col);
        }
        if interactive && ui.interact(r, id.with("task"), Sense::click()).clicked() {
            ops.push(Op::Snapshot);
            if let Some(t) = item.task.as_mut() {
                t.done = !t.done;
            }
            changed = true;
        }
        if let Some(due) = task.due {
            let overdue = !task.done && due < now();
            painter.text(
                pos2(full.max.x + 6.0, tag_y),
                Align2::LEFT_TOP,
                format!("⏰ {}", due.format("%d.%m %H:%M")),
                FontId::proportional(11.0),
                if overdue { Color32::from_rgb(220, 50, 50) } else { Color32::from_gray(130) },
            );
        }
    }
    for tid in item.tags.clone() {
        if let Some(t) = env.settings.tag(tid) {
            if t.is_task && item.task.is_some() {
                continue;
            }
            x -= 18.0;
            let r = Rect::from_min_size(pos2(x, tag_y), vec2(16.0, 16.0));
            let col = Color32::from_rgb(t.color[0], t.color[1], t.color[2]);
            painter.text(r.center(), Align2::CENTER_CENTER, &t.icon, FontId::proportional(13.0), col);
            if interactive {
                ui.interact(r, id.with(("tag", tid)), Sense::hover()).on_hover_text(&t.name);
            }
        }
    }

    // Chrome: border, move grip, width handle.
    let show_chrome = interactive && (hovered || editing || selected) && !ed.readonly;
    if show_chrome || selected {
        let col = if selected || editing { Color32::from_rgb(60, 130, 255) } else { Color32::from_gray(170) };
        painter.rect_stroke(full.expand(3.0), 2.0, EStroke::new(1.0, col.gamma_multiply(0.7)), egui::StrokeKind::Outside);
    }
    if show_chrome {
        let grip = Rect::from_min_size(full.min - vec2(3.0, 15.0), vec2(full.width() + 6.0, 11.0));
        let col = if env.dark { Color32::from_gray(70) } else { Color32::from_gray(215) };
        painter.rect_filled(grip, 2.0, col);
        painter.text(grip.center(), Align2::CENTER_CENTER, "⋯", FontId::proportional(11.0), Color32::from_gray(120));
        let g = ui.interact(grip.intersect(clip), id.with("grip"), Sense::click_and_drag());
        if g.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::Move);
        }
        if g.drag_started() {
            ops.push(Op::Snapshot);
        }
        if g.dragged() {
            let d = g.drag_delta() / zoom;
            item.pos[0] += d.x;
            item.pos[1] += d.y;
            changed = true;
        }
        if g.clicked() {
            let add = ui.input(|i| i.modifiers.shift || i.modifiers.ctrl);
            if !add {
                ed.selection = Selection::default();
            }
            ed.selection.items.push(item.id);
            ed.editing = None;
        }
        g.context_menu(|ui| item_menu(ui, item, ed, env, ops, &mut changed));
        let hr = Rect::from_center_size(pos2(full.max.x + 4.0, full.center().y), vec2(8.0, 26.0f32.min(full.height())));
        painter.rect_filled(hr, 3.0, col);
        let h = ui.interact(hr.intersect(clip), id.with("resize"), Sense::drag());
        if h.hovered() || h.dragged() {
            ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
        }
        if h.drag_started() {
            ops.push(Op::Snapshot);
        }
        if h.dragged() {
            item.width = (item.width + h.drag_delta().x / zoom).max(40.0);
            changed = true;
        }
    }
    if changed {
        item.modified = now();
        item.author = env.settings.author.clone();
    }
    changed
}

fn item_menu(ui: &mut Ui, item: &mut Item, ed: &mut Editor, env: &mut Env, ops: &mut Vec<Op>, changed: &mut bool) {
    ui.menu_button("Теги", |ui| {
        for t in env.settings.tags.clone() {
            let mut on = item.tags.contains(&t.id);
            if ui.checkbox(&mut on, format!("{} {}", t.icon, t.name)).changed() {
                ops.push(Op::Snapshot);
                toggle_tag(item, &t);
                *changed = true;
            }
        }
    });
    ui.menu_button("Задача", |ui| {
        let mut is_task = item.task.is_some();
        if ui.checkbox(&mut is_task, "Задача с флажком").changed() {
            ops.push(Op::Snapshot);
            item.task = is_task.then(|| Task { done: false, due: None, priority: 0, notified: false });
            *changed = true;
        }
        if item.task.is_some() {
            ui.separator();
            ui.label("Срок:");
            let today = chrono::Local::now().date_naive();
            let at = |d: chrono::NaiveDate| d.and_hms_opt(18, 0, 0).unwrap();
            for (label, due) in [
                ("Сегодня", Some(at(today))),
                ("Завтра", Some(at(today + chrono::Duration::days(1)))),
                ("Через неделю", Some(at(today + chrono::Duration::days(7)))),
                ("Без срока", None),
            ] {
                if ui.button(label).clicked() {
                    ops.push(Op::Snapshot);
                    if let Some(t) = item.task.as_mut() {
                        t.due = due;
                        t.notified = false;
                    }
                    *changed = true;
                    ui.close();
                }
            }
            if ui.button("Указать дату и время…").clicked() {
                env.actions.push(Action::SetTaskDue(item.id));
                ui.close();
            }
            ui.separator();
            ui.label("Приоритет:");
            for (label, p) in [("Высокий", 1u8), ("Средний", 2), ("Обычный", 0)] {
                if ui.radio(item.task.as_ref().is_some_and(|t| t.priority == p), label).clicked() {
                    if let Some(t) = item.task.as_mut() {
                        t.priority = p;
                    }
                    *changed = true;
                }
            }
        }
    });
    ui.separator();
    if ui.button("Копировать текст").clicked() && !env.no_copy {
        env.actions.push(Action::CopyText(item.plain_text()));
        ui.close();
    }
    if ui.button("Копировать ссылку на абзац").clicked() {
        env.actions.push(Action::ItemLink(item.id));
        ui.close();
    }
    match &item.kind {
        ItemKind::Text(_) => {
            if ui.button("Преобразовать в таблицу").clicked() {
                ops.push(Op::TextToTable(item.id));
                ui.close();
            }
        }
        ItemKind::Table(_) => {
            if ui.button("Преобразовать в текст").clicked() {
                ops.push(Op::TableToText(item.id));
                ui.close();
            }
        }
        _ => {}
    }
    ui.separator();
    if ui.button("Изменить").clicked() {
        ed.editing = Some(item.id);
        ed.focus_item = Some(item.id);
        ui.close();
    }
    if ui.button("Дублировать").clicked() {
        ops.push(Op::Duplicate(item.id));
        ui.close();
    }
    if ui.button("На передний план").clicked() {
        ops.push(Op::Front(item.id));
        ui.close();
    }
    if ui.button("На задний план").clicked() {
        ops.push(Op::Back(item.id));
        ui.close();
    }
    if ui.button("Удалить").clicked() {
        ops.push(Op::Delete(item.id));
        ui.close();
    }
}

pub fn toggle_tag(item: &mut Item, t: &TagDef) {
    if let Some(i) = item.tags.iter().position(|x| *x == t.id) {
        item.tags.remove(i);
        if t.is_task {
            item.task = None;
        }
    } else {
        item.tags.push(t.id);
        if t.is_task && item.task.is_none() {
            item.task = Some(Task { done: false, due: None, priority: 0, notified: false });
        }
    }
}

// ----------------------------------------------------------------------------
// Text

#[allow(clippy::too_many_arguments)]
fn text_ui(ui: &mut Ui, item: &mut Item, sr: Rect, clip: Rect, ed: &mut Editor, env: &mut Env, interactive: bool, changed: &mut bool, ops: &mut Vec<Op>) -> f32 {
    let item_id = item.id;
    let ItemKind::Text(tb) = &mut item.kind else { return 0.0 };
    let zoom = ed.zoom;
    let dark = env.dark;
    let hl: Vec<(usize, usize)> = env_highlights(env, item_id);
    let speller = env.speller;
    let spell_on = env.settings.spellcheck;
    let misspelled = |w: &str| spell_on && speller.is_bad(w);
    let wrap = (sr.width() - 2.0).max(10.0);
    if ed.editing == Some(item_id) && interactive && !ed.readonly {
        let tid = egui::Id::new(("txt", item_id));
        let style = TextBlock { text: String::new(), ..tb.clone() };
        let mut layouter = |ui: &Ui, buf: &dyn egui::TextBuffer, w: f32| {
            let mut t = style.clone();
            t.text = buf.as_str().to_owned();
            let cx = richtext::Ctx { dark, zoom, edit: true, highlights: &hl, misspelled: &misspelled, detect: false };
            let l = richtext::layout(&t, &cx, w);
            ui.fonts_mut(|f| f.layout_job(l.job))
        };
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_min_size(sr.min, vec2(sr.width(), 100_000.0))).layout(egui::Layout::top_down(egui::Align::Min)));
        child.set_clip_rect(clip);
        let before = tb.text.clone();
        let out = TextEdit::multiline(&mut tb.text)
            .id(tid)
            .frame(egui::Frame::NONE)
            .desired_width(wrap)
            .desired_rows(1)
            .lock_focus(true)
            .layouter(&mut layouter)
            .show(&mut child);
        if ed.focus_item == Some(item_id) {
            out.response.request_focus();
            ed.focus_item = None;
        }
        if out.response.has_focus() && child.input(|i| i.key_pressed(egui::Key::Escape)) {
            ed.editing = None;
        }
        if tb.text != before {
            if ed.undo.last().is_none_or(|_| true) && before.is_empty() && false {
                ops.push(Op::Snapshot);
            }
            *changed = true;
            post_edit(ui.ctx(), tid, tb, &before, env);
        }
        // Spelling suggestions on right click.
        let resp = out.response.response.clone();
        let cursor = out.cursor_range;
        let text_clone = tb.text.clone();
        let mut replace: Option<(usize, usize, String)> = None;
        resp.context_menu(|ui| {
            if let Some(cr) = cursor {
                let ci = cr.primary.index.0;
                if let Some((a, b, word)) = word_at(&text_clone, ci) {
                    if env.speller.is_bad(&word) {
                        let sugg = env.speller.suggest(&word);
                        if sugg.is_empty() {
                            ui.label("Нет вариантов");
                        }
                        for s in sugg {
                            if ui.button(&s).clicked() {
                                replace = Some((a, b, s));
                                ui.close();
                            }
                        }
                        if ui.button("Добавить в словарь").clicked() {
                            env.speller.add_word(&word);
                            env.actions.push(Action::Status(format!("«{word}» добавлено в словарь")));
                            ui.close();
                        }
                        ui.separator();
                    }
                }
            }
            if ui.button("Вырезать").clicked() {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::RequestCut);
                ui.close();
            }
            if ui.button("Копировать").clicked() && !env.no_copy {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::RequestCopy);
                ui.close();
            }
            if ui.button("Вставить").clicked() {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::RequestPaste);
                ui.close();
            }
            ui.separator();
            if ui.button("Очистить форматирование").clicked() {
                if let Some(cr) = cursor {
                    let [a, b] = cr.sorted_cursors();
                    let mut t = text_clone.clone();
                    richtext::clear_format(&mut t, a.index.0, b.index.0);
                    replace = Some((0, text_clone.chars().count(), t));
                }
                ui.close();
            }
        });
        if let Some((a, b, s)) = replace {
            ops.push(Op::Snapshot);
            let (ba, bb) = (richtext::char_to_byte(&tb.text, a), richtext::char_to_byte(&tb.text, b));
            tb.text.replace_range(ba..bb, &s);
            *changed = true;
        }
        return out.response.rect.height().max(tb.size * zoom * 1.3);
    }
    // Display mode.
    let cx = richtext::Ctx { dark, zoom, edit: false, highlights: &hl, misspelled: &misspelled, detect: true };
    let l = richtext::layout(tb, &cx, wrap);
    let galley = ui.fonts_mut(|f| f.layout_job(l.job));
    let rect = Rect::from_min_size(sr.min, vec2(sr.width(), galley.size().y.max(tb.size * zoom * 1.3)));
    let painter = ui.painter_at(clip);
    painter.galley(sr.min, galley.clone(), Color32::GRAY);
    if interactive {
        let resp = ui.interact(rect.intersect(clip), egui::Id::new(("txtd", item_id)), Sense::click());
        if resp.hovered() {
            // Links show a hand cursor.
            if let Some(p) = resp.hover_pos() {
                let ci = galley.cursor_from_pos(p - sr.min).index.0;
                if l.clicks.iter().any(|(a, b, _)| ci >= *a && ci < *b) {
                    ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                } else {
                    ui.ctx().set_cursor_icon(CursorIcon::Text);
                }
            }
        }
        if resp.clicked() {
            let mut handled = false;
            if let Some(p) = resp.interact_pointer_pos() {
                let ci = galley.cursor_from_pos(p - sr.min).index.0;
                for (a, b, c) in &l.clicks {
                    if ci >= *a && ci < *b {
                        match c {
                            Click::Toggle(line) => {
                                if !ed.readonly {
                                    ops.push(Op::Snapshot);
                                    richtext::toggle_check_line(&mut tb.text, *line);
                                    *changed = true;
                                }
                            }
                            Click::Link(url) => env.actions.push(Action::OpenLink(url.clone())),
                        }
                        handled = true;
                        break;
                    }
                }
            }
            if !handled && !ed.readonly {
                if let Some(st) = ed.format_painter.take() {
                    // Apply the copied format to the whole block.
                    ops.push(Op::Snapshot);
                    let n = tb.text.chars().count();
                    richtext::apply_style(&mut tb.text, 0, n, &st);
                    *changed = true;
                } else {
                    ed.editing = Some(item_id);
                    ed.focus_item = Some(item_id);
                    ed.selection = Selection::default();
                }
            }
        }
    }
    rect.height()
}

fn env_highlights(env: &Env, id: Id) -> Vec<(usize, usize)> {
    env.highlights.get(&id).cloned().unwrap_or_default()
}

fn word_at(text: &str, ci: usize) -> Option<(usize, usize, String)> {
    let cs: Vec<char> = text.chars().collect();
    if cs.is_empty() {
        return None;
    }
    let mut a = ci.min(cs.len());
    while a > 0 && cs[a - 1].is_alphabetic() {
        a -= 1;
    }
    let mut b = ci.min(cs.len());
    while b < cs.len() && cs[b].is_alphabetic() {
        b += 1;
    }
    (b > a).then(|| (a, b, cs[a..b].iter().collect()))
}

/// Auto lists, autocorrect and list continuation after a text edit.
fn post_edit(ctx: &egui::Context, tid: egui::Id, tb: &mut TextBlock, before: &str, env: &Env) {
    let Some(mut state) = egui::text_edit::TextEditState::load(ctx, tid) else { return };
    let Some(range) = state.cursor.char_range() else { return };
    let ci = range.primary.index.0;
    let n_before = before.chars().count();
    let n_after = tb.text.chars().count();
    if n_after != n_before + 1 || ci == 0 {
        return;
    }
    let typed = tb.text.chars().nth(ci - 1).unwrap_or(' ');
    let mut shift: isize = 0;
    if typed == '\n' && env.settings.auto_lists {
        shift += richtext::continue_list(&mut tb.text, ci) as isize;
        if shift == 0 && tb.text.chars().count() < n_after {
            // List ended: the empty marker was removed.
            let removed = n_after as isize - tb.text.chars().count() as isize;
            shift -= removed;
        }
    } else if (typed == ' ' || ".,!?;:".contains(typed)) && env.settings.autocorrect {
        if typed == ' ' && env.settings.auto_lists {
            shift += richtext::autoformat_line(&mut tb.text, ci);
        }
        shift += richtext::autocorrect(&mut tb.text, ci, &env.settings.autocorrect_map);
    }
    if shift != 0 {
        let nc = (ci as isize + shift).max(0) as usize;
        state.cursor.set_char_range(Some(CCursorRange::one(CCursor::new(nc))));
        state.store(ctx, tid);
    }
}

#[derive(Clone)]
pub enum FormatCmd {
    Wrap(&'static str, &'static str),
    WrapOwned(String, String),
    Clear,
    LinePrefix(String),
    Indent(bool),
    Style(richtext::Inline),
    Insert(String),
    Align(Align),
    Size(f32),
    Font(String),
    Color([u8; 4]),
    Spacing(f32),
    Rtl,
}

/// Applies a formatting command to the edited text container (selection) or
/// to all selected text containers.
pub fn apply_format(ctx: &egui::Context, page: &mut Page, ed: &mut Editor, cmd: FormatCmd) -> bool {
    let targets: Vec<Id> = match ed.editing {
        Some(id) => vec![id],
        None => ed.selection.items.clone(),
    };
    if targets.is_empty() {
        return false;
    }
    ed.snapshot(page);
    let mut changed = false;
    for id in targets {
        let editing = ed.editing == Some(id);
        let Some(item) = page.item_mut(id) else { continue };
        let ItemKind::Text(tb) = &mut item.kind else { continue };
        let tid = egui::Id::new(("txt", id));
        let n = tb.text.chars().count();
        let mut state = if editing { egui::text_edit::TextEditState::load(ctx, tid) } else { None };
        let (a, b) = state
            .as_ref()
            .and_then(|s| s.cursor.char_range())
            .map(|r| {
                let [x, y] = r.sorted_cursors();
                (x.index.0, y.index.0)
            })
            .unwrap_or((0, n));
        // Whole-word when nothing is selected.
        let (a, b) = if a == b && editing && !matches!(cmd, FormatCmd::Insert(_) | FormatCmd::LinePrefix(_) | FormatCmd::Indent(_)) {
            word_at(&tb.text, a).map(|(x, y, _)| (x, y)).unwrap_or((a, b))
        } else {
            (a, b)
        };
        let mut sel = (a, b);
        match &cmd {
            FormatCmd::Wrap(o, c) => sel = richtext::toggle_wrap(&mut tb.text, a, b, o, c),
            FormatCmd::WrapOwned(o, c) => sel = richtext::toggle_wrap(&mut tb.text, a, b, o, c),
            FormatCmd::Clear => sel = richtext::clear_format(&mut tb.text, a, b),
            FormatCmd::Style(st) => sel = richtext::apply_style(&mut tb.text, a, b, st),
            FormatCmd::LinePrefix(p) => {
                let d = richtext::set_line_prefix(&mut tb.text, a, b, p);
                sel = ((a as isize + d).max(0) as usize, (b as isize + d).max(0) as usize);
            }
            FormatCmd::Indent(out) => {
                let d = richtext::indent_lines(&mut tb.text, a, b, *out);
                sel = ((a as isize + d).max(0) as usize, (b as isize + d).max(0) as usize);
            }
            FormatCmd::Insert(s) => {
                let (ba, bb) = (richtext::char_to_byte(&tb.text, a), richtext::char_to_byte(&tb.text, b));
                tb.text.replace_range(ba..bb, s);
                let e = a + s.chars().count();
                sel = (e, e);
            }
            FormatCmd::Align(al) => tb.align = *al,
            FormatCmd::Size(s) => {
                if editing && a != b {
                    sel = richtext::toggle_wrap(&mut tb.text, a, b, &format!("{{s:{s}|"), "}");
                } else {
                    tb.size = *s;
                }
            }
            FormatCmd::Font(f) => {
                if editing && a != b {
                    sel = richtext::toggle_wrap(&mut tb.text, a, b, &format!("{{f:{f}|"), "}");
                } else {
                    tb.font = f.clone();
                }
            }
            FormatCmd::Color(c) => {
                if editing && a != b {
                    sel = richtext::toggle_wrap(&mut tb.text, a, b, &format!("{{c:#{:02x}{:02x}{:02x}|", c[0], c[1], c[2]), "}");
                } else {
                    tb.color = *c;
                }
            }
            FormatCmd::Spacing(s) => tb.line_spacing = *s,
            FormatCmd::Rtl => tb.rtl = !tb.rtl,
        }
        item.modified = now();
        changed = true;
        if let Some(st) = state.as_mut() {
            st.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(sel.0), CCursor::new(sel.1))));
            st.clone().store(ctx, tid);
            ctx.memory_mut(|m| m.request_focus(tid));
        }
    }
    changed
}

/// Inline style under the cursor of the edited text (format painter).
pub fn style_under_cursor(ctx: &egui::Context, page: &Page, ed: &Editor) -> Option<richtext::Inline> {
    let id = ed.editing?;
    let ItemKind::Text(tb) = &page.item(id)?.kind else { return None };
    let st = egui::text_edit::TextEditState::load(ctx, egui::Id::new(("txt", id)))?;
    let ci = st.cursor.char_range()?.primary.index.0;
    Some(richtext::style_at(&tb.text, ci))
}

// ----------------------------------------------------------------------------
// Tables

#[allow(clippy::too_many_arguments)]
fn table_ui(ui: &mut Ui, item: &mut Item, sr: Rect, clip: Rect, ed: &mut Editor, env: &mut Env, interactive: bool, changed: &mut bool) -> f32 {
    let item_id = item.id;
    let ItemKind::Table(t) = &mut item.kind else { return 0.0 };
    let z = ed.zoom;
    let active = ed.editing == Some(item_id) && interactive && !ed.readonly;
    let rows = t.rows();
    let cols = t.cols();
    // Fit column widths to the container width.
    let total_w: f32 = t.col_widths.iter().sum();
    if (total_w - item.width).abs() > 1.0 && total_w > 0.0 {
        let k = item.width / total_w;
        for w in t.col_widths.iter_mut() {
            *w *= k;
        }
    }
    let mut xs = vec![sr.min.x];
    for w in &t.col_widths {
        xs.push(xs.last().unwrap() + w * z);
    }
    let mut ys = vec![sr.min.y];
    for h in &t.row_heights {
        ys.push(ys.last().unwrap() + h * z);
    }
    let painter = ui.painter_at(clip);
    let border = c32(t.border);
    let dark = env.dark;
    let text_col = if dark { Color32::from_gray(225) } else { Color32::from_gray(25) };
    let covered = |r: usize, c: usize, t: &Table| t.merges.iter().any(|m| r >= m.r && r < m.r + m.rs && c >= m.c && c < m.c + m.cs && !(r == m.r && c == m.c));
    let cell_rect = |r: usize, c: usize, t: &Table| -> Rect {
        let (rs, cs) = t.merges.iter().find(|m| m.r == r && m.c == c).map(|m| (m.rs, m.cs)).unwrap_or((1, 1));
        Rect::from_min_max(pos2(xs[c], ys[r]), pos2(xs[(c + cs).min(cols)], ys[(r + rs).min(rows)]))
    };
    let font = FontId::proportional(14.0 * z);
    let mut next_cell: Option<(usize, usize)> = None;
    for r in 0..rows {
        for c in 0..cols {
            if covered(r, c, t) {
                continue;
            }
            let cr = cell_rect(r, c, t);
            if !cr.intersects(clip) {
                continue;
            }
            let fill = t.fills.get(&format!("{r},{c}")).map(|f| c32(*f)).or_else(|| {
                if t.header && r == 0 {
                    Some(if dark { Color32::from_gray(55) } else { Color32::from_gray(230) })
                } else if t.banded && r % 2 == 1 {
                    Some(if dark { Color32::from_gray(38) } else { Color32::from_gray(245) })
                } else {
                    None
                }
            });
            if let Some(f) = fill {
                painter.rect_filled(cr, 0.0, f);
            }
            painter.rect_stroke(cr, 0.0, EStroke::new(1.0, border), egui::StrokeKind::Inside);
            let align = t.aligns.get(&format!("{r},{c}")).copied().unwrap_or(Align::Left);
            let is_cur = active && ed.table_cell == Some((r, c));
            if is_cur {
                let tid = egui::Id::new(("cell", item_id, r, c));
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(cr.shrink(3.0)));
                child.set_clip_rect(clip);
                let before = t.cells[r][c].clone();
                let resp = child.add(TextEdit::singleline(&mut t.cells[r][c]).id(tid).frame(egui::Frame::NONE).font(font.clone()).desired_width(cr.width() - 6.0));
                if ed.focus_item == Some(item_id) {
                    resp.request_focus();
                    ed.focus_item = None;
                }
                if t.cells[r][c] != before {
                    *changed = true;
                }
                let (tab, shift, enter, up, down) = ui.input(|i| {
                    (i.key_pressed(egui::Key::Tab), i.modifiers.shift, i.key_pressed(egui::Key::Enter), i.key_pressed(egui::Key::ArrowUp), i.key_pressed(egui::Key::ArrowDown))
                });
                if resp.has_focus() || resp.lost_focus() {
                    if tab {
                        next_cell = Some(if shift {
                            if c > 0 { (r, c - 1) } else if r > 0 { (r - 1, cols - 1) } else { (0, 0) }
                        } else if c + 1 < cols {
                            (r, c + 1)
                        } else {
                            (r + 1, 0)
                        });
                    } else if enter || down {
                        next_cell = Some((r + 1, c));
                    } else if up && r > 0 {
                        next_cell = Some((r - 1, c));
                    }
                }
            } else {
                let disp = calc::cell_display(&t.cells, r, c);
                let bold = t.header && r == 0;
                let f = if bold { crate::fonts::font("", 14.0 * z, true, false, false) } else { font.clone() };
                let galley = ui.fonts_mut(|fo| fo.layout(disp, f, text_col, cr.width() - 6.0));
                let x = match align {
                    Align::Center => cr.center().x - galley.size().x / 2.0,
                    Align::Right => cr.max.x - 3.0 - galley.size().x,
                    _ => cr.min.x + 3.0,
                };
                painter.with_clip_rect(cr.intersect(clip)).galley(pos2(x, cr.center().y - galley.size().y / 2.0), galley, text_col);
                if interactive && !ed.readonly {
                    let resp = ui.interact(cr.intersect(clip), egui::Id::new(("celld", item_id, r, c)), Sense::click());
                    if resp.clicked() {
                        ed.editing = Some(item_id);
                        ed.table_cell = Some((r, c));
                        ed.focus_item = Some(item_id);
                        ed.selection = Selection::default();
                    }
                }
            }
        }
    }
    if let Some((r, c)) = next_cell {
        if r >= t.rows() {
            t.cells.push(vec![String::new(); cols]);
            t.row_heights.push(26.0);
            *changed = true;
        }
        ed.table_cell = Some((r, c.min(cols - 1)));
        ed.focus_item = Some(item_id);
    }
    // Column resize handles.
    if active {
        for c in 1..cols {
            let x = xs[c];
            let hr = Rect::from_min_max(pos2(x - 3.0, ys[0]), pos2(x + 3.0, *ys.last().unwrap()));
            let h = ui.interact(hr.intersect(clip), egui::Id::new(("colrs", item_id, c)), Sense::drag());
            if h.hovered() || h.dragged() {
                ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
            }
            if h.dragged() {
                let d = h.drag_delta().x / z;
                if t.col_widths[c - 1] + d > 20.0 && t.col_widths[c] - d > 20.0 {
                    t.col_widths[c - 1] += d;
                    t.col_widths[c] -= d;
                    *changed = true;
                }
            }
        }
        for r in 1..=rows {
            let y = ys[r];
            let hr = Rect::from_min_max(pos2(xs[0], y - 3.0), pos2(*xs.last().unwrap(), y + 3.0));
            let h = ui.interact(hr.intersect(clip), egui::Id::new(("rowrs", item_id, r)), Sense::drag());
            if h.hovered() || h.dragged() {
                ui.ctx().set_cursor_icon(CursorIcon::ResizeVertical);
            }
            if h.dragged() {
                t.row_heights[r - 1] = (t.row_heights[r - 1] + h.drag_delta().y / z).max(16.0);
                *changed = true;
            }
        }
        table_toolbar(ui, t, sr, clip, ed, env, changed, item.width);
    }
    *ys.last().unwrap() - sr.min.y
}

#[allow(clippy::too_many_arguments)]
fn table_toolbar(ui: &mut Ui, t: &mut Table, sr: Rect, clip: Rect, ed: &mut Editor, env: &mut Env, changed: &mut bool, width: f32) {
    let bar = Rect::from_min_size(sr.min - vec2(0.0, 52.0), vec2(width.max(560.0) * 1.0, 30.0));
    if !bar.intersects(clip) {
        return;
    }
    let (r, c) = ed.table_cell.unwrap_or((0, 0));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(bar).layout(egui::Layout::left_to_right(egui::Align::Center)));
    egui::Frame::popup(ui.style()).inner_margin(3.0).show(&mut child, |ui| {
        ui.horizontal(|ui| {
            let cols = t.cols();
            if ui.small_button("+ строка").on_hover_text("Вставить строку ниже").clicked() {
                t.cells.insert(r + 1, vec![String::new(); cols]);
                t.row_heights.insert(r + 1, 26.0);
                *changed = true;
            }
            if ui.small_button("+ столбец").on_hover_text("Вставить столбец справа").clicked() {
                for row in t.cells.iter_mut() {
                    row.insert(c + 1, String::new());
                }
                let w = t.col_widths[c];
                t.col_widths.insert(c + 1, w);
                *changed = true;
            }
            if ui.small_button("− строка").clicked() && t.rows() > 1 {
                t.cells.remove(r);
                t.row_heights.remove(r);
                t.merges.retain(|m| !(r >= m.r && r < m.r + m.rs));
                ed.table_cell = Some((r.min(t.rows() - 1), c));
                *changed = true;
            }
            if ui.small_button("− столбец").clicked() && cols > 1 {
                for row in t.cells.iter_mut() {
                    row.remove(c);
                }
                t.col_widths.remove(c);
                t.merges.retain(|m| !(c >= m.c && c < m.c + m.cs));
                ed.table_cell = Some((r, c.min(t.cols() - 1)));
                *changed = true;
            }
            ui.separator();
            ui.menu_button("Ячейки", |ui| {
                if ui.button("Объединить вправо").clicked() && c + 1 < t.cols() {
                    merge(t, r, c, 0, 1);
                    *changed = true;
                }
                if ui.button("Объединить вниз").clicked() && r + 1 < t.rows() {
                    merge(t, r, c, 1, 0);
                    *changed = true;
                }
                if ui.button("Разделить").clicked() {
                    t.merges.retain(|m| !(m.r == r && m.c == c));
                    *changed = true;
                }
                ui.separator();
                for (label, al) in [("По левому краю", Align::Left), ("По центру", Align::Center), ("По правому краю", Align::Right)] {
                    if ui.button(label).clicked() {
                        t.aligns.insert(format!("{r},{c}"), al);
                        *changed = true;
                    }
                }
                ui.separator();
                ui.label("Заливка:");
                ui.horizontal_wrapped(|ui| {
                    for col in [[255, 255, 255, 0], [255, 235, 160, 255], [200, 235, 200, 255], [200, 220, 255, 255], [255, 210, 210, 255], [230, 230, 230, 255]] {
                        let (rect, resp) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::click());
                        ui.painter().rect_filled(rect, 3.0, c32(col));
                        ui.painter().rect_stroke(rect, 3.0, EStroke::new(1.0, Color32::GRAY), egui::StrokeKind::Inside);
                        if resp.clicked() {
                            if col[3] == 0 {
                                t.fills.remove(&format!("{r},{c}"));
                            } else {
                                t.fills.insert(format!("{r},{c}"), col);
                            }
                            *changed = true;
                        }
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Границы:");
                    let mut b = Color32::from_rgba_unmultiplied(t.border[0], t.border[1], t.border[2], t.border[3]);
                    if ui.color_edit_button_srgba(&mut b).changed() {
                        t.border = b.to_srgba_unmultiplied();
                        *changed = true;
                    }
                });
            });
            ui.menu_button("Сортировка", |ui| {
                for (label, desc) in [("По возрастанию", false), ("По убыванию", true)] {
                    if ui.button(label).clicked() {
                        sort_table(t, c, desc);
                        *changed = true;
                        ui.close();
                    }
                }
            });
            if ui.selectable_label(t.header, "Заголовок").clicked() {
                t.header = !t.header;
                *changed = true;
            }
            if ui.selectable_label(t.banded, "Полосы").clicked() {
                t.banded = !t.banded;
                *changed = true;
            }
            if ui.small_button("Автоподбор").clicked() {
                autofit(ui, t);
                *changed = true;
            }
            ui.menu_button("Σ", |ui| {
                let col = crate::calc::col_name(c);
                let last = t.rows();
                let start = if t.header { 2 } else { 1 };
                for (label, f) in [("Сумма", "SUM"), ("Среднее", "AVERAGE"), ("Минимум", "MIN"), ("Максимум", "MAX"), ("Количество", "COUNT")] {
                    if ui.button(label).on_hover_text("Добавляет строку с формулой под столбцом").clicked() {
                        let cols = t.cols();
                        t.cells.push(vec![String::new(); cols]);
                        t.row_heights.push(26.0);
                        let rr = t.rows() - 1;
                        t.cells[rr][c] = format!("={f}({col}{start}:{col}{last})");
                        *changed = true;
                        ui.close();
                    }
                }
            });
            ui.menu_button("Файл", |ui| {
                if ui.button("Экспорт в CSV…").clicked() {
                    if let Some(p) = rfd::FileDialog::new().add_filter("CSV", &["csv"]).set_file_name("таблица.csv").save_file() {
                        let mut data = vec![0xEF, 0xBB, 0xBF];
                        data.extend_from_slice(crate::export::table_to_csv(t).as_bytes());
                        let _ = std::fs::write(p, data);
                    }
                    ui.close();
                }
                if ui.button("Экспорт в XLSX…").clicked() {
                    if let Some(p) = rfd::FileDialog::new().add_filter("Excel", &["xlsx"]).set_file_name("таблица.xlsx").save_file() {
                        if let Err(e) = crate::export::table_to_xlsx(t, &p) {
                            env.actions.push(Action::Status(format!("Ошибка: {e}")));
                        }
                    }
                    ui.close();
                }
                if ui.button("Импорт из CSV/XLSX…").clicked() {
                    if let Some(p) = rfd::FileDialog::new().add_filter("Таблицы", &["csv", "xlsx", "xls", "ods", "txt"]).pick_file() {
                        let cells = if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("csv") || e.eq_ignore_ascii_case("txt")) {
                            std::fs::read(&p).ok().map(|b| crate::export::parse_csv(&String::from_utf8_lossy(&b)))
                        } else {
                            crate::export::table_from_xlsx(&p).ok()
                        };
                        if let Some(cells) = cells {
                            let nt = crate::export::table_from_cells(cells);
                            *t = nt;
                            ed.table_cell = Some((0, 0));
                            *changed = true;
                        }
                    }
                    ui.close();
                }
            });
        });
    });
}

fn merge(t: &mut Table, r: usize, c: usize, dr: usize, dc: usize) {
    let cur = t.merges.iter().position(|m| m.r == r && m.c == c);
    let (rs, cs) = cur.map(|i| (t.merges[i].rs, t.merges[i].cs)).unwrap_or((1, 1));
    let (nrs, ncs) = ((rs + dr).min(t.rows() - r), (cs + dc).min(t.cols() - c));
    if let Some(i) = cur {
        t.merges.remove(i);
    }
    // Merge content into the top-left cell.
    for rr in r..r + nrs {
        for cc in c..c + ncs {
            if (rr, cc) != (r, c) && !t.cells[rr][cc].is_empty() {
                let v = std::mem::take(&mut t.cells[rr][cc]);
                if !t.cells[r][c].is_empty() {
                    t.cells[r][c].push(' ');
                }
                t.cells[r][c].push_str(&v);
            }
        }
    }
    t.merges.retain(|m| !(m.r >= r && m.r < r + nrs && m.c >= c && m.c < c + ncs));
    t.merges.push(Merge { r, c, rs: nrs, cs: ncs });
}

pub fn sort_table(t: &mut Table, col: usize, desc: bool) {
    let start = if t.header { 1 } else { 0 };
    if t.rows() <= start + 1 {
        return;
    }
    let disp: Vec<String> = (0..t.rows()).map(|r| calc::cell_display(&t.cells, r, col)).collect();
    let mut idx: Vec<usize> = (start..t.rows()).collect();
    idx.sort_by(|&a, &b| {
        let (x, y) = (&disp[a], &disp[b]);
        let o = match (calc::parse_number(x), calc::parse_number(y)) {
            (Some(p), Some(q)) => p.partial_cmp(&q).unwrap_or(std::cmp::Ordering::Equal),
            _ => x.to_lowercase().cmp(&y.to_lowercase()),
        };
        if desc { o.reverse() } else { o }
    });
    let mut cells: Vec<Vec<String>> = t.cells[..start].to_vec();
    let mut heights: Vec<f32> = t.row_heights[..start].to_vec();
    for i in idx {
        cells.push(t.cells[i].clone());
        heights.push(t.row_heights[i]);
    }
    t.cells = cells;
    t.row_heights = heights;
    t.merges.clear();
}

fn autofit(ui: &Ui, t: &mut Table) {
    for c in 0..t.cols() {
        let mut w: f32 = 40.0;
        for r in 0..t.rows() {
            let s = calc::cell_display(&t.cells, r, c);
            let g = ui.fonts_mut(|f| f.layout_no_wrap(s, FontId::proportional(14.0), Color32::WHITE));
            w = w.max(g.size().x + 14.0);
        }
        t.col_widths[c] = w.min(500.0);
    }
    for h in t.row_heights.iter_mut() {
        *h = 26.0;
    }
}

// ----------------------------------------------------------------------------
// Images

#[allow(clippy::too_many_arguments)]
fn image_ui(ui: &mut Ui, item: &mut Item, sr: Rect, clip: Rect, ed: &mut Editor, env: &mut Env, interactive: bool, changed: &mut bool) -> f32 {
    let item_id = item.id;
    let width = item.width;
    let ItemKind::Image(img) = &mut item.kind else { return 0.0 };
    let z = ed.zoom;
    let path = env.nb.asset_path(&img.asset);
    let painter = ui.painter_at(clip);
    let crop = Rect::from_min_max(pos2(img.crop[0], img.crop[1]), pos2(img.crop[2], img.crop[3]));
    let (tex, size) = match env.textures.get(path.clone(), img.rotation, img.flip_h, img.flip_v) {
        Tex::Ready(h, s) => (Some(h.id()), *s),
        Tex::Loading => (None, [4, 3]),
        Tex::Failed => (None, [4, 3]),
    };
    let aspect = (crop.height() * size[1] as f32) / (crop.width() * size[0] as f32).max(1e-3) * img.stretch_k();
    let h = width * aspect * z;
    let ir = Rect::from_min_size(sr.min, vec2(sr.width(), h));
    let cropping = ed.crop == Some(item_id) && interactive;
    match tex {
        Some(t) if cropping => {
            // Show the whole image with the crop rectangle.
            let full_h = width * (size[1] as f32 / size[0] as f32) * z;
            let fr = Rect::from_min_size(sr.min, vec2(sr.width(), full_h));
            painter.image(t, fr, Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)), Color32::from_white_alpha(110));
            let cr = Rect::from_min_max(fr.lerp_inside(crop.min.to_vec2()), fr.lerp_inside(crop.max.to_vec2()));
            painter.image(t, cr, crop, Color32::WHITE);
            painter.rect_stroke(cr, 0.0, EStroke::new(2.0, Color32::from_rgb(60, 130, 255)), egui::StrokeKind::Outside);
            let mut c = img.crop;
            for (k, (hx, hy)) in [(0usize, 1usize), (2, 3)].iter().enumerate() {
                let p = pos2(fr.min.x + c[*hx] * fr.width(), fr.min.y + c[*hy] * fr.height());
                let hr = Rect::from_center_size(p, vec2(14.0, 14.0));
                painter.rect_filled(hr, 2.0, Color32::WHITE);
                let resp = ui.interact(hr, egui::Id::new(("crop", item_id, k)), Sense::drag());
                if resp.dragged() {
                    let d = resp.drag_delta();
                    c[*hx] = (c[*hx] + d.x / fr.width()).clamp(0.0, 1.0);
                    c[*hy] = (c[*hy] + d.y / fr.height()).clamp(0.0, 1.0);
                }
            }
            if c[2] - c[0] > 0.02 && c[3] - c[1] > 0.02 && c != img.crop {
                img.crop = c;
                *changed = true;
            }
            let done = Rect::from_min_size(fr.left_bottom() + vec2(0.0, 6.0), vec2(90.0, 22.0));
            if ui.put(done, egui::Button::new("Готово")).clicked() {
                ed.crop = None;
            }
            return full_h + 30.0;
        }
        Some(t) => {
            painter.image(t, ir, crop, Color32::WHITE);
        }
        None => {
            painter.rect_filled(ir, 2.0, Color32::from_gray(200));
            painter.text(ir.center(), Align2::CENTER_CENTER, "Загрузка…", FontId::proportional(12.0), Color32::from_gray(90));
        }
    }
    let mut total = h;
    if !img.caption.is_empty() {
        let g = ui.fonts_mut(|f| f.layout(img.caption.clone(), FontId::proportional(12.0 * z), Color32::from_gray(120), sr.width()));
        painter.galley(pos2(sr.min.x, ir.max.y + 4.0), g.clone(), Color32::GRAY);
        total += g.size().y + 6.0;
    }
    if interactive {
        let resp = ui.interact(ir.intersect(clip), egui::Id::new(("img", item_id)), Sense::click());
        if resp.clicked() {
            ed.selection = Selection { strokes: vec![], items: vec![item_id] };
            ed.editing = None;
        }
        if resp.double_clicked() {
            crate::winapi::shell_open(&path.to_string_lossy());
        }
        let mut alt = img.alt.clone();
        let mut cap = img.caption.clone();
        resp.on_hover_text(if img.alt.is_empty() { "Двойной щелчок — открыть".to_owned() } else { img.alt.clone() }).context_menu(|ui| {
            ui.horizontal(|ui| {
                if ui.button("⟲").on_hover_text("Повернуть влево").clicked() {
                    img.rotation = (img.rotation + 3) % 4;
                    *changed = true;
                }
                if ui.button("⟳").on_hover_text("Повернуть вправо").clicked() {
                    img.rotation = (img.rotation + 1) % 4;
                    *changed = true;
                }
                if ui.button("⇋").on_hover_text("Отразить по горизонтали").clicked() {
                    img.flip_h = !img.flip_h;
                    *changed = true;
                }
                if ui.button("⇵").on_hover_text("Отразить по вертикали").clicked() {
                    img.flip_v = !img.flip_v;
                    *changed = true;
                }
            });
            if ui.button("Обрезать").clicked() {
                ed.crop = Some(item_id);
                ui.close();
            }
            if ui.button("Сбросить обрезку").clicked() {
                img.crop = full_crop();
                *changed = true;
                ui.close();
            }
            ui.horizontal(|ui| {
                ui.label("Подпись:");
                if ui.text_edit_singleline(&mut cap).changed() {
                    img.caption = cap.clone();
                    *changed = true;
                }
            });
            ui.horizontal(|ui| {
                ui.label("Замещающий текст:");
                if ui.text_edit_singleline(&mut alt).changed() {
                    img.alt = alt.clone();
                    *changed = true;
                }
            });
            ui.separator();
            if ui.button("Распознать текст (OCR)").clicked() {
                env.actions.push(Action::OcrImage(item_id));
                ui.close();
            }
            if let Some(t) = &img.ocr_text {
                if ui.button("Копировать распознанный текст").clicked() && !env.no_copy {
                    env.actions.push(Action::CopyText(t.clone()));
                    ui.close();
                }
            }
            if ui.button("Сжать изображение").clicked() {
                if let Ok(bytes) = std::fs::read(&path) {
                    if let Some((data, ext)) = crate::textures::compress(&bytes, env.settings.image_max_dim.min(1600), 75) {
                        if data.len() < bytes.len() {
                            img.asset = env.nb.import_asset(&data, ext);
                            *changed = true;
                            env.actions.push(Action::Status(format!(
                                "Размер: {} → {}",
                                crate::storage::human_size(bytes.len() as u64),
                                crate::storage::human_size(data.len() as u64)
                            )));
                        }
                    }
                }
                ui.close();
            }
            if ui.button("Сохранить как…").clicked() {
                if let Some(p) = rfd::FileDialog::new().set_file_name(&img.asset).save_file() {
                    let _ = std::fs::copy(&path, p);
                }
                ui.close();
            }
            if ui.button("Исходный размер").clicked() {
                *changed = true;
                env.actions.push(Action::Status("Ширина восстановлена".into()));
                ui.close();
            }
        });
    }
    total
}

// ----------------------------------------------------------------------------
// Files

fn file_ui(ui: &mut Ui, item: &mut Item, sr: Rect, clip: Rect, ed: &mut Editor, env: &mut Env, interactive: bool) -> f32 {
    let item_id = item.id;
    let ItemKind::File(f) = &item.kind else { return 0.0 };
    let z = ed.zoom;
    let r = Rect::from_min_size(sr.min, vec2(sr.width(), 52.0 * z));
    let painter = ui.painter_at(clip);
    painter.rect_filled(r, 6.0, if env.dark { Color32::from_gray(45) } else { Color32::from_gray(240) });
    let ext = std::path::Path::new(&f.name).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let icon = match ext.as_str() {
        "pdf" => "📕",
        "doc" | "docx" | "odt" | "rtf" => "📘",
        "xls" | "xlsx" | "csv" | "ods" => "📗",
        "mp3" | "wav" | "ogg" | "m4a" | "flac" => "🎵",
        "mp4" | "avi" | "mkv" | "mov" | "webm" => "🎬",
        "zip" | "rar" | "7z" => "🗜",
        _ => "📄",
    };
    painter.text(r.left_center() + vec2(10.0 * z, 0.0), Align2::LEFT_CENTER, icon, FontId::proportional(26.0 * z), Color32::GRAY);
    let tc = if env.dark { Color32::from_gray(225) } else { Color32::from_gray(30) };
    painter.text(r.left_top() + vec2(50.0 * z, 8.0 * z), Align2::LEFT_TOP, &f.name, FontId::proportional(14.0 * z), tc);
    painter.text(r.left_top() + vec2(50.0 * z, 28.0 * z), Align2::LEFT_TOP, crate::storage::human_size(f.size), FontId::proportional(11.0 * z), Color32::GRAY);
    if interactive {
        let path = env.nb.asset_path(&f.asset);
        let resp = ui.interact(r.intersect(clip), egui::Id::new(("file", item_id)), Sense::click());
        if resp.double_clicked() {
            // Open a temp copy with the original name so the right program is used.
            let tmp = crate::settings::Settings::temp_dir().join(&f.name);
            let _ = std::fs::create_dir_all(tmp.parent().unwrap());
            if std::fs::copy(&path, &tmp).is_ok() {
                crate::winapi::shell_open(&tmp.to_string_lossy());
            }
        } else if resp.clicked() {
            ed.selection = Selection { strokes: vec![], items: vec![item_id] };
        }
        let name = f.name.clone();
        resp.on_hover_text("Двойной щелчок — открыть").context_menu(|ui| {
            if ui.button("Открыть").clicked() {
                let tmp = crate::settings::Settings::temp_dir().join(&name);
                let _ = std::fs::create_dir_all(tmp.parent().unwrap());
                if std::fs::copy(&path, &tmp).is_ok() {
                    crate::winapi::shell_open(&tmp.to_string_lossy());
                }
                ui.close();
            }
            if ui.button("Сохранить как…").clicked() {
                if let Some(p) = rfd::FileDialog::new().set_file_name(&name).save_file() {
                    let _ = std::fs::copy(&path, p);
                }
                ui.close();
            }
            if ext == "pdf" && ui.button("Вставить как печатную копию").clicked() {
                env.actions.push(Action::PdfPrintout(item_id));
                ui.close();
            }
        });
    }
    r.height()
}

// ----------------------------------------------------------------------------
// Formulas

#[allow(clippy::too_many_arguments)]
fn equation_ui(ui: &mut Ui, item: &mut Item, sr: Rect, clip: Rect, ed: &mut Editor, env: &mut Env, interactive: bool, changed: &mut bool) -> f32 {
    let item_id = item.id;
    let ItemKind::Equation(src) = &mut item.kind else { return 0.0 };
    let z = ed.zoom;
    let tc = if env.dark { Color32::from_gray(230) } else { Color32::from_gray(20) };
    let mut y = sr.min.y;
    let painter = ui.painter_at(clip);
    if ed.editing == Some(item_id) && interactive && !ed.readonly {
        let r = Rect::from_min_size(sr.min, vec2(sr.width(), 26.0));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(r));
        child.set_clip_rect(clip);
        let resp = child.add(
            TextEdit::singleline(src)
                .id(egui::Id::new(("eq", item_id)))
                .font(FontId::monospace(15.0 * z))
                .desired_width(sr.width())
                .hint_text("x^2 - 5x + 6 = 0  или  sin(x)*x  или  2+2*2"),
        );
        if ed.focus_item == Some(item_id) {
            resp.request_focus();
            ed.focus_item = None;
        }
        if resp.changed() {
            *changed = true;
        }
        y += 30.0;
        let sym = Rect::from_min_size(pos2(sr.min.x, y), vec2(sr.width(), 24.0));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(sym).layout(egui::Layout::left_to_right(egui::Align::Center)));
        child.set_clip_rect(clip);
        for s in ["√", "π", "²", "³", "^", "∞", "±", "≤", "≥", "≠", "α", "β", "θ", "Δ", "∑", "∫"] {
            if child.small_button(s).clicked() {
                src.push_str(s);
                *changed = true;
            }
        }
        y += 28.0;
    }
    let pretty = calc::prettify(src);
    let g = ui.fonts_mut(|f| f.layout(pretty, crate::fonts::font("Cambria", 20.0 * z, false, true, false), tc, sr.width()));
    painter.galley(pos2(sr.min.x, y), g.clone(), tc);
    y += g.size().y + 4.0;
    // Evaluate, solve or plot.
    let trimmed = src.trim();
    if !trimmed.is_empty() {
        let mut lines: Vec<String> = Vec::new();
        let mut plot = false;
        if trimmed.contains('=') {
            match calc::solve(trimmed) {
                Ok(s) => lines.extend(s.steps),
                Err(e) => lines.push(e),
            }
        } else {
            let vars = std::collections::HashMap::new();
            match calc::eval(trimmed, &calc::Vars(&vars)) {
                Ok(v) => lines.push(format!("= {}", calc::fmt_num(v))),
                Err(_) => {
                    if calc::f_of_x(trimmed, 1.0).is_ok() {
                        plot = true;
                    }
                }
            }
        }
        for l in lines {
            let g = ui.fonts_mut(|f| f.layout(l, FontId::proportional(13.0 * z), Color32::from_rgb(40, 120, 60), sr.width()));
            painter.galley(pos2(sr.min.x + 8.0, y), g.clone(), tc);
            y += g.size().y + 2.0;
        }
        if plot {
            let pr = Rect::from_min_size(pos2(sr.min.x, y + 4.0), vec2(sr.width(), sr.width() * 0.6));
            draw_plot(&painter, pr, trimmed, env.dark);
            y = pr.max.y + 4.0;
        }
    }
    if interactive {
        let r = Rect::from_min_max(sr.min, pos2(sr.max.x, y)).intersect(clip);
        if ed.editing != Some(item_id) && ui.interact(r, egui::Id::new(("eqd", item_id)), Sense::click()).clicked() && !ed.readonly {
            ed.editing = Some(item_id);
            ed.focus_item = Some(item_id);
        }
    }
    (y - sr.min.y).max(30.0)
}

fn draw_plot(painter: &egui::Painter, r: Rect, expr: &str, dark: bool) {
    let bg = if dark { Color32::from_gray(35) } else { Color32::from_gray(250) };
    let axis = Color32::from_gray(140);
    painter.rect_filled(r, 4.0, bg);
    let (x0, x1) = (-10.0f64, 10.0f64);
    let n = 400;
    let mut pts = Vec::with_capacity(n);
    let (mut ymin, mut ymax) = (f64::INFINITY, f64::NEG_INFINITY);
    for i in 0..=n {
        let x = x0 + (x1 - x0) * i as f64 / n as f64;
        let y = calc::f_of_x(expr, x).unwrap_or(f64::NAN);
        if y.is_finite() && y.abs() < 1e6 {
            ymin = ymin.min(y);
            ymax = ymax.max(y);
        }
        pts.push((x, y));
    }
    if !ymin.is_finite() {
        return;
    }
    if (ymax - ymin).abs() < 1e-9 {
        ymin -= 1.0;
        ymax += 1.0;
    }
    let pad = (ymax - ymin) * 0.08;
    let (ymin, ymax) = (ymin - pad, ymax + pad);
    let map = |x: f64, y: f64| pos2(r.min.x + ((x - x0) / (x1 - x0)) as f32 * r.width(), r.max.y - ((y - ymin) / (ymax - ymin)) as f32 * r.height());
    if ymin < 0.0 && ymax > 0.0 {
        painter.line_segment([map(x0, 0.0), map(x1, 0.0)], EStroke::new(1.0, axis));
    }
    painter.line_segment([map(0.0, ymin), map(0.0, ymax)], EStroke::new(1.0, axis));
    painter.text(r.left_top() + vec2(4.0, 2.0), Align2::LEFT_TOP, format!("{:.2}", ymax), FontId::proportional(10.0), axis);
    painter.text(r.left_bottom() + vec2(4.0, -2.0), Align2::LEFT_BOTTOM, format!("{:.2}", ymin), FontId::proportional(10.0), axis);
    let mut seg: Vec<Pos2> = Vec::new();
    let line = Color32::from_rgb(40, 110, 220);
    for (x, y) in pts {
        if y.is_finite() && y >= ymin && y <= ymax {
            seg.push(map(x, y));
        } else if seg.len() > 1 {
            painter.add(egui::Shape::line(std::mem::take(&mut seg), EStroke::new(2.0, line)));
        } else {
            seg.clear();
        }
    }
    if seg.len() > 1 {
        painter.add(egui::Shape::line(seg, EStroke::new(2.0, line)));
    }
}

// ----------------------------------------------------------------------------
// Code with syntax highlighting

const LANGS: &[&str] = &["text", "rust", "python", "javascript", "typescript", "c", "cpp", "csharp", "java", "go", "sql", "html", "css", "json", "bash", "powershell"];

fn keywords(lang: &str) -> &'static [&'static str] {
    match lang {
        "rust" => &["fn", "let", "mut", "pub", "struct", "enum", "impl", "trait", "use", "mod", "match", "if", "else", "for", "while", "loop", "return", "self", "Self", "const", "static", "where", "as", "in", "ref", "move", "async", "await", "dyn", "true", "false", "Some", "None", "Ok", "Err"],
        "python" => &["def", "class", "import", "from", "as", "if", "elif", "else", "for", "while", "return", "yield", "with", "try", "except", "finally", "raise", "lambda", "None", "True", "False", "and", "or", "not", "in", "is", "pass", "break", "continue", "async", "await", "self"],
        "javascript" | "typescript" => &["function", "const", "let", "var", "if", "else", "for", "while", "return", "class", "extends", "new", "this", "import", "export", "from", "default", "async", "await", "try", "catch", "finally", "throw", "null", "undefined", "true", "false", "typeof", "interface", "type"],
        "c" | "cpp" => &["int", "char", "float", "double", "void", "long", "short", "unsigned", "struct", "union", "enum", "typedef", "if", "else", "for", "while", "do", "return", "switch", "case", "break", "continue", "static", "const", "class", "public", "private", "protected", "virtual", "template", "namespace", "using", "new", "delete", "nullptr", "true", "false", "auto", "include", "define"],
        "csharp" | "java" => &["class", "public", "private", "protected", "static", "void", "int", "string", "bool", "var", "new", "return", "if", "else", "for", "foreach", "while", "using", "namespace", "import", "package", "extends", "implements", "interface", "null", "true", "false", "this", "async", "await", "final", "override"],
        "go" => &["func", "package", "import", "var", "const", "type", "struct", "interface", "if", "else", "for", "range", "return", "go", "defer", "chan", "map", "nil", "true", "false", "switch", "case"],
        "sql" => &["SELECT", "FROM", "WHERE", "INSERT", "INTO", "VALUES", "UPDATE", "SET", "DELETE", "CREATE", "TABLE", "JOIN", "LEFT", "RIGHT", "INNER", "ON", "GROUP", "BY", "ORDER", "HAVING", "AS", "AND", "OR", "NOT", "NULL", "LIMIT", "select", "from", "where", "join", "and", "or"],
        "bash" | "powershell" => &["if", "then", "else", "fi", "for", "do", "done", "while", "function", "echo", "return", "export", "local", "param", "foreach", "in"],
        _ => &[],
    }
}

pub fn highlight_code(code: &str, lang: &str, size: f32, dark: bool) -> LayoutJob {
    let mut job = LayoutJob::default();
    let font = FontId::monospace(size);
    let (kw, st, cm, num, plain) = if dark {
        (Color32::from_rgb(200, 120, 220), Color32::from_rgb(220, 170, 110), Color32::from_rgb(110, 150, 110), Color32::from_rgb(160, 200, 140), Color32::from_gray(220))
    } else {
        (Color32::from_rgb(160, 30, 150), Color32::from_rgb(170, 70, 20), Color32::from_rgb(40, 130, 40), Color32::from_rgb(20, 110, 100), Color32::from_gray(25))
    };
    let kws = keywords(lang);
    let line_comment = match lang {
        "python" | "bash" | "powershell" => "#",
        "sql" => "--",
        _ => "//",
    };
    let cs: Vec<char> = code.chars().collect();
    let mut i = 0;
    let fmt = |c: Color32| TextFormat { font_id: font.clone(), color: c, ..Default::default() };
    while i < cs.len() {
        let rest: String = cs[i..cs.len().min(i + 2)].iter().collect();
        if rest.starts_with(line_comment) || (line_comment == "//" && rest == "/*") {
            let block = rest == "/*";
            let mut j = i;
            if block {
                while j + 1 < cs.len() && !(cs[j] == '*' && cs[j + 1] == '/') {
                    j += 1;
                }
                j = (j + 2).min(cs.len());
            } else {
                while j < cs.len() && cs[j] != '\n' {
                    j += 1;
                }
            }
            job.append(&cs[i..j].iter().collect::<String>(), 0.0, fmt(cm));
            i = j;
        } else if cs[i] == '"' || cs[i] == '\'' || cs[i] == '`' {
            let q = cs[i];
            let mut j = i + 1;
            while j < cs.len() && cs[j] != q && cs[j] != '\n' {
                if cs[j] == '\\' {
                    j += 1;
                }
                j += 1;
            }
            j = (j + 1).min(cs.len());
            job.append(&cs[i..j].iter().collect::<String>(), 0.0, fmt(st));
            i = j;
        } else if cs[i].is_ascii_digit() {
            let mut j = i;
            while j < cs.len() && (cs[j].is_ascii_alphanumeric() || cs[j] == '.' || cs[j] == '_') {
                j += 1;
            }
            job.append(&cs[i..j].iter().collect::<String>(), 0.0, fmt(num));
            i = j;
        } else if cs[i].is_alphabetic() || cs[i] == '_' {
            let mut j = i;
            while j < cs.len() && (cs[j].is_alphanumeric() || cs[j] == '_') {
                j += 1;
            }
            let w: String = cs[i..j].iter().collect();
            job.append(&w, 0.0, fmt(if kws.contains(&w.as_str()) { kw } else { plain }));
            i = j;
        } else {
            let mut j = i;
            while j < cs.len() && !(cs[j].is_alphanumeric() || "\"'`_".contains(cs[j]) || cs[j] == '/' || cs[j] == '#' || cs[j] == '-') {
                j += 1;
            }
            if j == i {
                j = i + 1;
            }
            job.append(&cs[i..j].iter().collect::<String>(), 0.0, fmt(plain));
            i = j;
        }
    }
    job
}

#[allow(clippy::too_many_arguments)]
fn code_ui(ui: &mut Ui, item: &mut Item, sr: Rect, clip: Rect, ed: &mut Editor, env: &mut Env, interactive: bool, changed: &mut bool) -> f32 {
    let item_id = item.id;
    let ItemKind::Code(code) = &mut item.kind else { return 0.0 };
    let z = ed.zoom;
    let dark = env.dark;
    let bg = if dark { Color32::from_gray(28) } else { Color32::from_gray(246) };
    let painter = ui.painter_at(clip);
    let editing = ed.editing == Some(item_id) && interactive && !ed.readonly;
    let mut y = sr.min.y;
    if editing {
        let r = Rect::from_min_size(sr.min, vec2(sr.width(), 24.0));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(r).layout(egui::Layout::left_to_right(egui::Align::Center)));
        child.set_clip_rect(clip);
        egui::ComboBox::from_id_salt(("lang", item_id)).selected_text(code.lang.clone()).show_ui(&mut child, |ui| {
            for l in LANGS {
                if ui.selectable_label(code.lang == *l, *l).clicked() {
                    code.lang = l.to_string();
                    *changed = true;
                }
            }
        });
        y += 28.0;
    }
    let body = Rect::from_min_size(pos2(sr.min.x, y), vec2(sr.width(), 100_000.0));
    let lang = code.lang.clone();
    let h;
    if editing {
        let mut layouter = |ui: &Ui, buf: &dyn egui::TextBuffer, w: f32| {
            let mut job = highlight_code(buf.as_str(), &lang, 13.0 * z, dark);
            job.wrap.max_width = w;
            ui.fonts_mut(|f| f.layout_job(job))
        };
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(body.shrink(4.0)));
        child.set_clip_rect(clip);
        let pre_rect = Rect::from_min_size(body.min, vec2(sr.width(), 30.0));
        let bgi = painter.add(egui::Shape::Noop);
        let out = TextEdit::multiline(&mut code.code)
            .id(egui::Id::new(("code", item_id)))
            .code_editor()
            .frame(egui::Frame::NONE)
            .desired_width(sr.width() - 8.0)
            .layouter(&mut layouter)
            .show(&mut child);
        if ed.focus_item == Some(item_id) {
            out.response.request_focus();
            ed.focus_item = None;
        }
        if out.response.changed() {
            *changed = true;
        }
        h = out.response.rect.height() + 8.0;
        painter.set(bgi, egui::Shape::rect_filled(Rect::from_min_size(pre_rect.min, vec2(sr.width(), h)), 4.0, bg));
    } else {
        let mut job = highlight_code(&code.code, &lang, 13.0 * z, dark);
        job.wrap.max_width = sr.width() - 8.0;
        let g = ui.fonts_mut(|f| f.layout_job(job));
        h = g.size().y + 8.0;
        painter.rect_filled(Rect::from_min_size(body.min, vec2(sr.width(), h)), 4.0, bg);
        painter.galley(body.min + vec2(4.0, 4.0), g, Color32::GRAY);
        painter.text(pos2(sr.max.x - 6.0, body.min.y + 3.0), Align2::RIGHT_TOP, &lang, FontId::proportional(10.0), Color32::GRAY);
        if interactive && !ed.readonly {
            let r = Rect::from_min_size(body.min, vec2(sr.width(), h)).intersect(clip);
            if ui.interact(r, egui::Id::new(("coded", item_id)), Sense::click()).clicked() {
                ed.editing = Some(item_id);
                ed.focus_item = Some(item_id);
            }
        }
    }
    y + h - sr.min.y
}

// ----------------------------------------------------------------------------
// Diagrams (mind map from an indented list, flow chart from "A -> B" lines)

#[allow(clippy::too_many_arguments)]
fn diagram_ui(ui: &mut Ui, item: &mut Item, sr: Rect, clip: Rect, ed: &mut Editor, env: &mut Env, interactive: bool, changed: &mut bool) -> f32 {
    let item_id = item.id;
    let ItemKind::Diagram(d) = &mut item.kind else { return 0.0 };
    let z = ed.zoom;
    let dark = env.dark;
    let painter = ui.painter_at(clip);
    let editing = ed.editing == Some(item_id) && interactive && !ed.readonly;
    let mut y = sr.min.y;
    if editing {
        let r = Rect::from_min_size(sr.min, vec2(sr.width(), 24.0));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(r).layout(egui::Layout::left_to_right(egui::Align::Center)));
        child.set_clip_rect(clip);
        if child.selectable_label(d.kind == DiagramKind::MindMap, "Ментальная карта").clicked() {
            d.kind = DiagramKind::MindMap;
            *changed = true;
        }
        if child.selectable_label(d.kind == DiagramKind::Flow, "Блок-схема").clicked() {
            d.kind = DiagramKind::Flow;
            *changed = true;
        }
        y += 28.0;
        let r = Rect::from_min_size(pos2(sr.min.x, y), vec2(sr.width(), 400.0));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(r));
        child.set_clip_rect(clip);
        let out = TextEdit::multiline(&mut d.source)
            .id(egui::Id::new(("dia", item_id)))
            .code_editor()
            .desired_width(sr.width())
            .desired_rows(4)
            .hint_text(if d.kind == DiagramKind::MindMap { "Тема\n  Ветка 1\n    Подветка\n  Ветка 2" } else { "Начало -> Шаг 1\nШаг 1 -> Решение?\nРешение? -> Конец" })
            .show(&mut child);
        if ed.focus_item == Some(item_id) {
            out.response.request_focus();
            ed.focus_item = None;
        }
        if out.response.changed() {
            *changed = true;
        }
        y += out.response.rect.height() + 8.0;
    }
    let fg = if dark { Color32::from_gray(225) } else { Color32::from_gray(30) };
    let box_fill = if dark { Color32::from_rgb(45, 60, 85) } else { Color32::from_rgb(225, 236, 255) };
    let line = Color32::from_rgb(90, 130, 200);
    let font = FontId::proportional(13.0 * z);
    let start_y = y;
    match d.kind {
        DiagramKind::MindMap => {
            // Tree layout: depth → column, leaves → rows.
            let mut nodes: Vec<(usize, String)> = Vec::new();
            for l in d.source.lines() {
                if l.trim().is_empty() {
                    continue;
                }
                let depth = (l.len() - l.trim_start().len()) / 2;
                nodes.push((depth, l.trim().trim_start_matches("- ").to_owned()));
            }
            let col_w = (sr.width() / (nodes.iter().map(|n| n.0).max().unwrap_or(0) + 1) as f32).max(60.0);
            let row_h = 30.0 * z;
            let mut pos: Vec<Pos2> = Vec::new();
            let mut parent_stack: Vec<usize> = Vec::new();
            for (i, (depth, text)) in nodes.iter().enumerate() {
                let p = pos2(sr.min.x + *depth as f32 * col_w, y + i as f32 * row_h);
                pos.push(p);
                while parent_stack.len() > *depth {
                    parent_stack.pop();
                }
                if let Some(&par) = parent_stack.last() {
                    let a = pos[par] + vec2((col_w - 14.0).min(140.0 * z), row_h * 0.4);
                    let b = p + vec2(0.0, row_h * 0.4);
                    let mid = pos2(a.x + (b.x - a.x) * 0.5, a.y);
                    painter.add(egui::Shape::line(vec![a, mid, pos2(mid.x, b.y), b], EStroke::new(1.5, line)));
                }
                let g = ui.fonts_mut(|f| f.layout(text.clone(), font.clone(), fg, col_w - 20.0));
                let br = Rect::from_min_size(p, vec2((g.size().x + 12.0).min(col_w - 14.0), row_h * 0.8));
                painter.rect_filled(br, 6.0, box_fill);
                painter.rect_stroke(br, 6.0, EStroke::new(1.0, line), egui::StrokeKind::Inside);
                painter.with_clip_rect(br.intersect(clip)).galley(pos2(br.min.x + 6.0, br.center().y - g.size().y / 2.0), g, fg);
                parent_stack.push(i);
            }
            y += nodes.len() as f32 * row_h + 6.0;
        }
        DiagramKind::Flow => {
            let mut order: Vec<String> = Vec::new();
            let mut edges: Vec<(usize, usize)> = Vec::new();
            let mut idx = |s: &str, order: &mut Vec<String>| -> usize {
                let s = s.trim().to_owned();
                match order.iter().position(|x| *x == s) {
                    Some(i) => i,
                    None => {
                        order.push(s);
                        order.len() - 1
                    }
                }
            };
            for l in d.source.lines() {
                let parts: Vec<&str> = l.split("->").collect();
                if parts.len() >= 2 {
                    for w in parts.windows(2) {
                        let a = idx(w[0], &mut order);
                        let b = idx(w[1], &mut order);
                        edges.push((a, b));
                    }
                } else if !l.trim().is_empty() {
                    idx(l, &mut order);
                }
            }
            // Longest-path layering.
            let n = order.len();
            let mut layer = vec![0usize; n];
            for _ in 0..n {
                for &(a, b) in &edges {
                    if layer[b] < layer[a] + 1 && layer[a] + 1 < n {
                        layer[b] = layer[a] + 1;
                    }
                }
            }
            let row_h = 60.0 * z;
            let mut per_layer: std::collections::BTreeMap<usize, Vec<usize>> = Default::default();
            for i in 0..n {
                per_layer.entry(layer[i]).or_default().push(i);
            }
            let mut rects = vec![Rect::NOTHING; n];
            for (l, list) in &per_layer {
                let w = sr.width() / list.len() as f32;
                for (k, &i) in list.iter().enumerate() {
                    let c = pos2(sr.min.x + w * (k as f32 + 0.5), y + *l as f32 * row_h + row_h * 0.35);
                    rects[i] = Rect::from_center_size(c, vec2((w - 16.0).min(170.0 * z), 34.0 * z));
                }
            }
            for &(a, b) in &edges {
                let (ra, rb) = (rects[a], rects[b]);
                let (p, q) = if rb.center().y > ra.center().y { (ra.center_bottom(), rb.center_top()) } else { (ra.center_top(), rb.center_bottom()) };
                painter.arrow(p, q - p, EStroke::new(1.5, line));
            }
            for (i, r) in rects.iter().enumerate() {
                let text = &order[i];
                if text.ends_with('?') {
                    let pts = vec![r.center_top(), r.right_center(), r.center_bottom(), r.left_center()];
                    painter.add(egui::Shape::convex_polygon(pts, box_fill, EStroke::new(1.0, line)));
                } else {
                    painter.rect_filled(*r, 8.0, box_fill);
                    painter.rect_stroke(*r, 8.0, EStroke::new(1.0, line), egui::StrokeKind::Inside);
                }
                let g = ui.fonts_mut(|f| f.layout(text.clone(), font.clone(), fg, r.width() - 8.0));
                painter.with_clip_rect(r.intersect(clip)).galley(r.center() - g.size() / 2.0, g, fg);
            }
            y += (per_layer.len().max(1)) as f32 * row_h;
        }
    }
    if interactive && !editing && !ed.readonly {
        let r = Rect::from_min_max(pos2(sr.min.x, start_y), pos2(sr.max.x, y)).intersect(clip);
        if ui.interact(r, egui::Id::new(("diad", item_id)), Sense::click()).clicked() {
            ed.editing = Some(item_id);
            ed.focus_item = Some(item_id);
        }
    }
    (y - sr.min.y).max(30.0)
}

pub fn _unused(_: Vec2) {}

#[allow(clippy::too_many_arguments)]
fn chart_ui(ui: &mut Ui, item: &mut Item, sr: Rect, clip: Rect, ed: &mut Editor, env: &mut Env, interactive: bool, changed: &mut bool) -> f32 {
    let item_id = item.id;
    let ItemKind::Chart(c) = &mut item.kind else { return 0.0 };
    let editing = ed.editing == Some(item_id) && interactive && !ed.readonly;
    let mut y = sr.min.y;
    if editing {
        let r = Rect::from_min_size(sr.min, vec2(sr.width(), 160.0));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(r));
        child.set_clip_rect(clip);
        child.horizontal(|ui| {
            for (k, l) in [(ChartKind::Bar, "Столбцы"), (ChartKind::Line, "Линия"), (ChartKind::Pie, "Круговая")] {
                if ui.selectable_label(c.kind == k, l).clicked() {
                    c.kind = k;
                    *changed = true;
                }
            }
        });
        *changed |= child.text_edit_singleline(&mut c.title).changed();
        *changed |= child.add(TextEdit::multiline(&mut c.data).desired_rows(4).hint_text("Январь: 10")).changed();
        y += 160.0;
    }
    let rows = c.rows();
    let painter = ui.painter_at(clip);
    let fg = if env.dark { Color32::from_gray(220) } else { Color32::from_gray(40) };
    painter.text(pos2(sr.min.x, y), Align2::LEFT_TOP, &c.title, FontId::proportional(14.0 * ed.zoom), fg);
    let area = Rect::from_min_size(pos2(sr.min.x, y + 22.0), vec2(sr.width(), sr.width() * 0.5));
    let maxv = rows.iter().map(|r| r.1).fold(0.0f64, f64::max).max(1e-9);
    let pal = [Color32::from_rgb(66, 133, 244), Color32::from_rgb(219, 68, 55), Color32::from_rgb(244, 180, 0), Color32::from_rgb(15, 157, 88), Color32::from_rgb(171, 71, 188), Color32::from_rgb(0, 172, 193)];
    if !rows.is_empty() {
        match c.kind {
            ChartKind::Bar => {
                let w = area.width() / rows.len() as f32;
                for (i, (l, v)) in rows.iter().enumerate() {
                    let h = (*v / maxv) as f32 * (area.height() - 18.0);
                    let r = Rect::from_min_max(pos2(area.min.x + i as f32 * w + 4.0, area.max.y - 18.0 - h), pos2(area.min.x + (i + 1) as f32 * w - 4.0, area.max.y - 18.0));
                    painter.rect_filled(r, 2.0, pal[i % pal.len()]);
                    painter.text(pos2(r.center().x, area.max.y - 16.0), Align2::CENTER_TOP, l, FontId::proportional(11.0), fg);
                }
            }
            ChartKind::Line => {
                let n = rows.len().max(2) - 1;
                let pts: Vec<Pos2> = rows.iter().enumerate().map(|(i, (_, v))| pos2(area.min.x + area.width() * i as f32 / n as f32, area.max.y - 18.0 - (*v / maxv) as f32 * (area.height() - 18.0))).collect();
                painter.add(egui::Shape::line(pts.clone(), EStroke::new(2.0, pal[0])));
                for p in pts {
                    painter.circle_filled(p, 3.0, pal[0]);
                }
            }
            ChartKind::Pie => {
                let total: f64 = rows.iter().map(|r| r.1.max(0.0)).sum::<f64>().max(1e-9);
                let center = pos2(area.min.x + area.height() * 0.5, area.center().y);
                let rad = area.height() * 0.45;
                let mut a0 = -std::f32::consts::FRAC_PI_2;
                for (i, (l, v)) in rows.iter().enumerate() {
                    let sweep = (v.max(0.0) / total) as f32 * std::f32::consts::TAU;
                    let n = ((sweep * 20.0) as usize).max(2);
                    let mut pts = vec![center];
                    for k in 0..=n {
                        let a = a0 + sweep * k as f32 / n as f32;
                        pts.push(center + vec2(a.cos(), a.sin()) * rad);
                    }
                    painter.add(egui::Shape::convex_polygon(pts, pal[i % pal.len()], EStroke::NONE));
                    painter.text(pos2(center.x + rad + 14.0, area.min.y + i as f32 * 16.0), Align2::LEFT_TOP, format!("■ {l}: {}", crate::calc::fmt_num(*v)), FontId::proportional(11.0), pal[i % pal.len()]);
                    a0 += sweep;
                }
            }
        }
    }
    let total = area.max.y - sr.min.y;
    if interactive && !editing && ui.interact(Rect::from_min_max(sr.min, area.max).intersect(clip), egui::Id::new(("chartd", item_id)), Sense::click()).double_clicked() {
        ed.editing = Some(item_id);
    }
    total
}
