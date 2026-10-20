//! Window chrome in the style of OneNote: custom title bar with page history
//! and window buttons, tabbed ribbon with flat icon buttons, and the left rail.

use egui::{Align2, Color32, CursorIcon, FontId, Rect, Response, Sense, Stroke, Ui, vec2};

use crate::app::*;
use crate::icons::{self, Icon};

#[derive(Clone, Copy)]
pub struct Pal {
    pub title_bg: Color32,
    pub title_fg: Color32,
    pub ribbon_bg: Color32,
    pub accent: Color32,
    pub accent_text: Color32,
    pub rail_bg: Color32,
    pub nav_bg: Color32,
    pub pages_bg: Color32,
    pub sel: Color32,
    pub hover: Color32,
    pub text: Color32,
    pub weak: Color32,
    pub border: Color32,
    pub search_bg: Color32,
}

pub fn pal(dark: bool) -> Pal {
    if dark {
        Pal {
            title_bg: Color32::from_rgb(41, 41, 41),
            title_fg: Color32::from_gray(235),
            ribbon_bg: Color32::from_rgb(41, 41, 41),
            accent: Color32::from_rgb(184, 134, 224),
            accent_text: Color32::WHITE,
            rail_bg: Color32::from_rgb(31, 31, 31),
            nav_bg: Color32::from_rgb(38, 38, 38),
            pages_bg: Color32::from_rgb(44, 44, 44),
            sel: Color32::from_rgb(64, 64, 64),
            hover: Color32::from_rgb(55, 55, 55),
            text: Color32::from_gray(236),
            weak: Color32::from_gray(160),
            border: Color32::from_gray(58),
            search_bg: Color32::from_rgb(52, 52, 52),
        }
    } else {
        Pal {
            title_bg: Color32::from_rgb(119, 25, 170),
            title_fg: Color32::WHITE,
            ribbon_bg: Color32::from_rgb(243, 242, 241),
            accent: Color32::from_rgb(119, 25, 170),
            accent_text: Color32::from_rgb(119, 25, 170),
            rail_bg: Color32::from_rgb(237, 235, 233),
            nav_bg: Color32::from_rgb(250, 249, 248),
            pages_bg: Color32::from_rgb(243, 242, 241),
            sel: Color32::from_rgb(225, 223, 221),
            hover: Color32::from_rgb(237, 235, 233),
            text: Color32::from_rgb(37, 36, 35),
            weak: Color32::from_rgb(110, 108, 106),
            border: Color32::from_rgb(225, 223, 221),
            search_bg: Color32::from_rgb(150, 70, 195),
        }
    }
}

pub fn cur(ui: &Ui) -> Pal {
    pal(ui.visuals().dark_mode)
}

/// Flat ribbon button: icon (and optional text), background only on hover
/// or when selected.
pub fn flat(ui: &mut Ui, icon: Option<Icon>, label: &str, tip: &str, selected: bool) -> Response {
    let p = cur(ui);
    let font = FontId::proportional(13.0);
    let galley = (!label.is_empty()).then(|| ui.painter().layout_no_wrap(label.to_owned(), font.clone(), p.text));
    let icon_w = if icon.is_some() { 20.0 } else { 0.0 };
    let text_w = galley.as_ref().map(|g| g.size().x).unwrap_or(0.0);
    let gap = if icon.is_some() && galley.is_some() { 6.0 } else { 0.0 };
    let size = vec2((icon_w + gap + text_w + 14.0).max(32.0), 32.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect, 4.0, p.sel);
    } else if resp.hovered() {
        painter.rect_filled(rect, 4.0, p.hover);
    }
    let mut x = rect.min.x + 7.0;
    if let Some(i) = icon {
        let ir = Rect::from_center_size(egui::pos2(x + 10.0, rect.center().y), vec2(20.0, 20.0));
        let col = if selected { p.accent_text } else { p.text };
        icons::paint(painter, ir, i, col);
        x += 20.0 + gap;
    }
    if let Some(g) = galley {
        painter.galley(egui::pos2(x, rect.center().y - g.size().y / 2.0), g, p.text);
    }
    if !tip.is_empty() {
        resp.on_hover_text(tip)
    } else {
        resp
    }
}

/// Icon-only flat button with a dropdown chevron.
pub fn flat_menu(ui: &mut Ui, icon: Icon, label: &str, tip: &str, add: impl FnOnce(&mut Ui)) {
    let text = if label.is_empty() { "⌄".to_string() } else { format!("{label} ⌄") };
    let r = flat(ui, Some(icon), &text, tip, false);
    egui::Popup::menu(&r).show(add);
}

/// Text glyph button ("Ж", "К", "Ч"): same look as `flat`.
pub fn glyph(ui: &mut Ui, text: &str, bold: bool, italic: bool, underline: bool, tip: &str) -> Response {
    let p = cur(ui);
    let (rect, resp) = ui.allocate_exact_size(vec2(30.0, 32.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, 4.0, p.hover);
    }
    let g = ui.painter().layout_no_wrap(text.to_owned(), crate::fonts::font("", 15.0, bold, italic, false), p.text);
    let pos = egui::pos2(rect.center().x - g.size().x / 2.0, rect.center().y - g.size().y / 2.0);
    let w = g.size().x;
    let h = g.size().y;
    ui.painter().galley(pos, g, p.text);
    if underline {
        ui.painter().hline(pos.x..=pos.x + w, pos.y + h - 2.0, Stroke::new(1.2, p.text));
    }
    resp.on_hover_text(tip)
}

/// Narrow dropdown arrow next to a split button.
pub fn chevron(ui: &mut Ui, tip: &str) -> Response {
    let p = cur(ui);
    let (rect, resp) = ui.allocate_exact_size(vec2(16.0, 32.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, 4.0, p.hover);
    }
    icons::paint(ui.painter(), Rect::from_center_size(rect.center(), vec2(11.0, 11.0)), Icon::Chevron, p.text);
    if tip.is_empty() { resp } else { resp.on_hover_text(tip) }
}

/// Icon with a colour bar under it (highlighter, font colour).
pub fn color_icon(ui: &mut Ui, icon: Icon, bar: Color32, tip: &str) -> Response {
    let p = cur(ui);
    let (rect, resp) = ui.allocate_exact_size(vec2(30.0, 32.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, 4.0, p.hover);
    }
    icons::paint(ui.painter(), Rect::from_center_size(rect.center() - vec2(0.0, 2.0), vec2(17.0, 17.0)), icon, p.text);
    ui.painter().rect_filled(Rect::from_center_size(egui::pos2(rect.center().x, rect.center().y + 9.0), vec2(16.0, 3.5)), 0.5, bar);
    resp.on_hover_text(tip)
}

/// Combo-like box with a caption (style gallery).
pub fn boxed(ui: &mut Ui, text: &str, width: f32, font: FontId, tip: &str) -> (Response, Response) {
    let p = cur(ui);
    let (rect, _) = ui.allocate_exact_size(vec2(width, 32.0), Sense::hover());
    let r = Rect::from_min_max(egui::pos2(rect.min.x, rect.min.y + 3.0), egui::pos2(rect.max.x, rect.max.y - 3.0));
    let main = Rect::from_min_max(r.min, egui::pos2(r.max.x - 20.0, r.max.y));
    let arrow = Rect::from_min_max(egui::pos2(r.max.x - 20.0, r.min.y), r.max);
    let mr = ui.interact(main, ui.id().with(("boxed", text)), Sense::click());
    let ar = ui.interact(arrow, ui.id().with(("boxed_arrow", text)), Sense::click());
    if mr.hovered() {
        ui.painter().rect_filled(main, 3.0, p.hover);
    }
    if ar.hovered() {
        ui.painter().rect_filled(arrow, 3.0, p.hover);
    }
    ui.painter().rect_stroke(r, 3.0, Stroke::new(1.0, p.border), egui::StrokeKind::Inside);
    let g = ui.painter().layout_no_wrap(text.to_owned(), font, p.text);
    ui.painter().with_clip_rect(main.shrink(2.0)).galley(egui::pos2(main.min.x + 8.0, main.center().y - g.size().y / 2.0), g, p.text);
    icons::paint(ui.painter(), Rect::from_center_size(arrow.center(), vec2(11.0, 11.0)), Icon::Chevron, p.text);
    (mr.on_hover_text(tip), ar)
}

/// Thin vertical divider between ribbon groups.
pub fn divider(ui: &mut Ui) {
    let p = cur(ui);
    let (r, _) = ui.allocate_exact_size(vec2(9.0, 28.0), Sense::hover());
    ui.painter().vline(r.center().x, r.y_range(), Stroke::new(1.0, p.border));
}

/// Ribbon group: in the full ribbon a caption is shown under the commands.
pub fn group(ui: &mut Ui, full: bool, caption: &str, add: impl FnOnce(&mut Ui)) {
    if full {
        ui.vertical(|ui| {
            ui.horizontal(add);
            let p = cur(ui);
            ui.vertical_centered(|ui| ui.label(egui::RichText::new(caption).size(11.0).color(p.weak)));
        });
    } else {
        ui.horizontal(add);
    }
    divider(ui);
}

// ----------------------------------------------------------------------------
// Title bar

pub fn title_bar(app: &mut App, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    let p = cur(ui);
    let rect = Rect::from_min_size(ui.max_rect().min, vec2(ui.max_rect().width(), 34.0));
    let custom = !app.settings.native_frame;
    // Drag / double click on the empty background.
    let bg = ui.interact(rect, ui.id().with("titlebar_bg"), Sense::click_and_drag());
    if custom {
        if bg.drag_started_by(egui::PointerButton::Primary) {
            ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        if bg.double_clicked() {
            let max = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!max));
        }
    }
    // Page and notebook name, centred over the whole bar.
    let title = app
        .cur_page
        .and_then(|pid| app.nb().and_then(|nb| nb.meta.page_meta(pid)))
        .map(|pm| if pm.title.trim().is_empty() { "Без названия".to_string() } else { pm.title.clone() })
        .unwrap_or_default();
    let nb = app.nb().map(|n| n.meta.name.clone()).unwrap_or_default();
    let caption = match (title.is_empty(), nb.is_empty()) {
        (true, true) => "NewNote".to_string(),
        (true, false) => nb,
        (false, true) => title,
        _ => format!("{title} - {nb}"),
    };
    let author = app.settings.author.clone();
    let author_g = ui.painter().layout_no_wrap(author, FontId::proportional(12.0), p.title_fg);
    let right_w = author_g.size().x + 20.0 + if custom { 46.0 * 3.0 } else { 0.0 };
    let g = ui.painter().layout_no_wrap(caption, FontId::proportional(12.0), p.title_fg);
    let free = Rect::from_min_max(egui::pos2(rect.min.x + 90.0, rect.min.y), egui::pos2(rect.max.x - right_w - 10.0, rect.max.y));
    if free.width() > 60.0 {
        let x = (rect.center().x - g.size().x / 2.0).clamp(free.min.x, (free.max.x - g.size().x).max(free.min.x));
        ui.painter().with_clip_rect(free).galley(egui::pos2(x, rect.center().y - g.size().y / 2.0), g, p.title_fg);
    }
    ui.allocate_ui_with_layout(vec2(rect.width(), 34.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
        ui.set_height(34.0);
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.add_space(4.0);
        let can_back = !app.hist_back.is_empty();
        if title_btn(ui, Icon::Back, "Назад (Alt+←)", p, can_back).clicked() && can_back {
            app.go_back();
        }
        let can_fwd = !app.hist_fwd.is_empty();
        if title_btn(ui, Icon::Forward, "Вперёд (Alt+→)", p, can_fwd).clicked() && can_fwd {
            app.go_forward();
        }
        let rest = ui.available_width() - right_w;
        if rest > 0.0 {
            ui.add_space(rest);
        }
        let (ar, aresp) = ui.allocate_exact_size(vec2(author_g.size().x + 20.0, 30.0), Sense::click());
        if aresp.hovered() {
            ui.painter().rect_filled(ar, 4.0, Color32::from_white_alpha(24));
        }
        ui.painter().galley(egui::pos2(ar.min.x + 10.0, ar.center().y - author_g.size().y / 2.0), author_g, p.title_fg);
        if aresp.on_hover_text("Имя автора заметок").clicked() {
            app.dialogs.push(Dialog::Settings(0));
        }
        if custom {
            window_buttons(ui, &ctx, p);
        }
    });
}

fn title_btn(ui: &mut Ui, icon: Icon, tip: &str, p: Pal, enabled: bool) -> Response {
    let (r, resp) = ui.allocate_exact_size(vec2(40.0, 34.0), Sense::click());
    if resp.hovered() && enabled {
        ui.painter().rect_filled(r, 0.0, Color32::from_white_alpha(24));
    }
    let col = if enabled { p.title_fg } else { p.title_fg.gamma_multiply(0.4) };
    icons::paint(ui.painter(), Rect::from_center_size(r.center(), vec2(16.0, 16.0)), icon, col);
    resp.on_hover_text(tip)
}

fn window_buttons(ui: &mut Ui, ctx: &egui::Context, p: Pal) {
    let max = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
    for (icon, tip, cmd) in [
        (Icon::Minimize, "Свернуть", 0),
        (if max { Icon::Restore } else { Icon::Maximize }, if max { "Восстановить" } else { "Развернуть" }, 1),
        (Icon::Close, "Закрыть", 2),
    ] {
        let (r, resp) = ui.allocate_exact_size(vec2(46.0, 32.0), Sense::click());
        if resp.hovered() {
            ui.painter().rect_filled(r, 0.0, if cmd == 2 { Color32::from_rgb(196, 43, 28) } else { Color32::from_white_alpha(28) });
        }
        icons::paint(ui.painter(), Rect::from_center_size(r.center(), vec2(14.0, 14.0)), icon, if resp.hovered() && cmd == 2 { Color32::WHITE } else { p.title_fg });
        if resp.on_hover_text(tip).clicked() {
            match cmd {
                0 => ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true)),
                1 => ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!max)),
                _ => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            }
        }
    }
}

pub fn settings_menu(app: &mut App, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    if ui.button("Параметры…").clicked() {
        app.dialogs.push(Dialog::Settings(0));
        ui.close();
    }
    if ui.button("Горячие клавиши").clicked() {
        app.dialogs.push(Dialog::Hotkeys);
        ui.close();
    }
    ui.menu_button("Тема", |ui| {
        for (t, l) in [(crate::settings::Theme::Light, "Светлая"), (crate::settings::Theme::Dark, "Тёмная"), (crate::settings::Theme::System, "Системная")] {
            if ui.radio(app.settings.theme == t, l).clicked() {
                app.settings.theme = t;
                app.apply_theme(&ctx);
                app.settings.save();
            }
        }
    });
    ui.menu_button("Лента", |ui| {
        if ui.radio(!app.settings.ribbon_full, "Упрощённая (одна строка)").clicked() {
            app.settings.ribbon_full = false;
            app.settings.save();
        }
        if ui.radio(app.settings.ribbon_full, "Классическая (с подписями групп)").clicked() {
            app.settings.ribbon_full = true;
            app.settings.save();
        }
        if ui.checkbox(&mut app.ribbon_collapsed, "Свернуть ленту").changed() {}
    });
    if ui.checkbox(&mut app.settings.status_bar, "Строка состояния").changed() {
        app.settings.save();
    }
    ui.menu_button("Разделы и страницы", |ui| {
        if ui.checkbox(&mut app.settings.sections_on_top, "Разделы вкладками над страницей").changed() {
            app.settings.save();
        }
        if ui.checkbox(&mut app.settings.pages_right, "Список страниц справа").changed() {
            app.settings.save();
        }
        if ui.checkbox(&mut app.settings.page_list_details, "Подробный список страниц").changed() {
            app.settings.save();
        }
    });
    if ui.checkbox(&mut app.settings.native_frame, "Системная рамка окна (после перезапуска)").changed() {
        app.settings.save();
    }
    ui.separator();
    if ui.button("Резервные копии…").clicked() {
        app.dialogs.push(Dialog::Backups);
        ui.close();
    }
    if ui.checkbox(&mut app.settings.offline_only, "Только офлайн").changed() {
        app.settings.save();
    }
    ui.separator();
    if ui.button("О программе").clicked() {
        app.dialogs.push(Dialog::About);
        ui.close();
    }
}

/// Resize handles along the edges of a frameless window.
pub fn resize_edges(ctx: &egui::Context) {
    if ctx.input(|i| i.viewport().maximized.unwrap_or(false) || i.viewport().fullscreen.unwrap_or(false)) {
        return;
    }
    let Some(pos) = ctx.pointer_hover_pos() else { return };
    let r = ctx.content_rect();
    let m = 5.0;
    let (l, rr, t, b) = (pos.x - r.min.x < m, r.max.x - pos.x < m, pos.y - r.min.y < m, r.max.y - pos.y < m);
    use egui::ResizeDirection as D;
    let dir = match (l, rr, t, b) {
        (true, _, true, _) => Some((D::NorthWest, CursorIcon::ResizeNwSe)),
        (_, true, true, _) => Some((D::NorthEast, CursorIcon::ResizeNeSw)),
        (true, _, _, true) => Some((D::SouthWest, CursorIcon::ResizeNeSw)),
        (_, true, _, true) => Some((D::SouthEast, CursorIcon::ResizeNwSe)),
        (true, ..) => Some((D::West, CursorIcon::ResizeHorizontal)),
        (_, true, ..) => Some((D::East, CursorIcon::ResizeHorizontal)),
        (_, _, true, _) => Some((D::North, CursorIcon::ResizeVertical)),
        (_, _, _, true) => Some((D::South, CursorIcon::ResizeVertical)),
        _ => None,
    };
    if let Some((d, c)) = dir {
        ctx.set_cursor_icon(c);
        if ctx.input(|i| i.pointer.primary_pressed()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(d));
        }
    }
}

// ----------------------------------------------------------------------------
// Ribbon

pub fn ribbon(app: &mut App, ui: &mut Ui) {
    let p = cur(ui);
    let ctx = ui.ctx().clone();
    // Tabs row.
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.add_space(6.0);
        let file = tab(ui, "Файл", false, p);
        egui::Popup::menu(&file).show(|ui| crate::ui_main::file_menu(app, ui));
        for (r, l) in [(Ribbon::Home, "Главная"), (Ribbon::Insert, "Вставка"), (Ribbon::Draw, "Рисование"), (Ribbon::View, "Вид"), (Ribbon::Help, "Справка")] {
            let resp = tab(ui, l, app.ribbon == r, p);
            if resp.clicked() {
                if app.ribbon == r {
                    app.ribbon_collapsed = !app.ribbon_collapsed;
                } else {
                    app.ribbon = r;
                    app.ribbon_collapsed = false;
                    app.settings.last_ribbon = r as u8;
                    app.settings.save();
                    if r == Ribbon::Draw {
                        app.with_page(|_, ed| {
                            if !ed.tool.is_ink() {
                                let pen = ed.pen.clone();
                                ed.set_pen(&pen, ed.pen_slot);
                            }
                        });
                    }
                }
            }
            if resp.double_clicked() {
                app.ribbon_collapsed = !app.ribbon_collapsed;
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.add_space(6.0);
            if tab_icon(ui, if app.ribbon_collapsed { Icon::Chevron } else { Icon::ChevronUp }, if app.ribbon_collapsed { "Закрепить ленту" } else { "Свернуть ленту" }, false, p).clicked() {
                app.ribbon_collapsed = !app.ribbon_collapsed;
            }
            let more = tab_icon(ui, Icon::More, "Параметры и прочее", false, p);
            egui::Popup::menu(&more).show(|ui| settings_menu(app, ui));
            let full_page = app.nav_hidden;
            if tab_icon(ui, Icon::Fullscreen, "Полностраничное представление (Ctrl+Shift+F)", full_page, p).clicked() {
                app.nav_hidden = !app.nav_hidden;
            }
            let share = tab_text(ui, Icon::Share, "Общий доступ", p);
            egui::Popup::menu(&share).show(|ui| {
                ui.set_min_width(220.0);
                crate::ui_main::export_menu(app, ui, false);
            });
            let tasks = app.right == Some(RightPanel::Tasks);
            if tab_icon(ui, Icon::Bell, "Мои задачи и напоминания", tasks, p).clicked() {
                app.right = if tasks { None } else { Some(RightPanel::Tasks) };
                app.collect_summary(&ctx);
            }
            let helper = app.right == Some(RightPanel::Helper);
            if tab_icon(ui, Icon::Bulb, "Помощник", helper, p).clicked() {
                app.right = if helper { None } else { Some(RightPanel::Helper) };
            }
            let saving = !app.dirty.is_empty();
            let sync = tab_icon(ui, Icon::Sync, if saving { "Сохранение…" } else { "Все изменения сохранены. Нажмите, чтобы сохранить сейчас" }, false, p);
            if saving {
                ui.painter().circle_filled(sync.rect.right_top() + vec2(-8.0, 8.0), 3.0, p.accent);
            }
            if sync.clicked() {
                app.save_all();
                app.set_status("Сохранено");
            }
        });
    });
    if app.ribbon_collapsed {
        return;
    }
    egui::ScrollArea::horizontal().id_salt("ribbon_scroll").scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 1.0;
            ui.add_space(6.0);
            undo_redo(app, ui);
            match app.ribbon {
                Ribbon::Home => crate::ui_main::home_ribbon(app, ui, &ctx),
                Ribbon::Insert => crate::ui_main::insert_ribbon(app, ui, &ctx),
                Ribbon::Draw => crate::ui_draw::draw_ribbon(app, ui),
                Ribbon::View => crate::ui_main::view_ribbon(app, ui, &ctx),
                Ribbon::Help => help_ribbon(app, ui),
            }
        });
    });
    ui.add_space(2.0);
}

fn undo_redo(app: &mut App, ui: &mut Ui) {
    let can_undo = app.cur_page.and_then(|p| app.editors.get(&p)).is_some_and(|e| e.can_undo());
    let can_redo = app.cur_page.and_then(|p| app.editors.get(&p)).is_some_and(|e| e.can_redo());
    for (icon, tip, redo, on) in [(Icon::Undo, "Отменить (Ctrl+Z)", false, can_undo), (Icon::Redo, "Повторить (Ctrl+Y)", true, can_redo)] {
        ui.add_enabled_ui(on, |ui| {
            if flat(ui, Some(icon), "", tip, false).clicked() {
                if let Some(pid) = app.cur_page {
                    if app.with_page(|pg, ed| if redo { ed.redo(pg) } else { ed.undo(pg) }) == Some(true) {
                        app.mark_dirty(pid);
                    }
                }
            }
        });
    }
    divider(ui);
}

fn tab_icon(ui: &mut Ui, icon: Icon, tip: &str, active: bool, p: Pal) -> Response {
    let (r, resp) = ui.allocate_exact_size(vec2(32.0, 28.0), Sense::click());
    if active {
        ui.painter().rect_filled(r, 4.0, p.sel);
    } else if resp.hovered() {
        ui.painter().rect_filled(r, 4.0, p.hover);
    }
    icons::paint(ui.painter(), Rect::from_center_size(r.center(), vec2(16.0, 16.0)), icon, if active { p.accent_text } else { p.text });
    resp.on_hover_text(tip)
}

fn tab_text(ui: &mut Ui, icon: Icon, text: &str, p: Pal) -> Response {
    let g = ui.painter().layout_no_wrap(text.to_owned(), FontId::proportional(13.0), p.text);
    let (r, resp) = ui.allocate_exact_size(vec2(g.size().x + 36.0, 28.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(r, 4.0, p.hover);
    }
    icons::paint(ui.painter(), Rect::from_center_size(egui::pos2(r.min.x + 15.0, r.center().y), vec2(16.0, 16.0)), icon, p.text);
    ui.painter().galley(egui::pos2(r.min.x + 28.0, r.center().y - g.size().y / 2.0), g, p.text);
    resp
}

fn tab(ui: &mut Ui, text: &str, active: bool, p: Pal) -> Response {
    let font = if active { crate::fonts::font("", 13.5, true, false, false) } else { FontId::proportional(13.5) };
    let col = if active { p.text } else { p.weak };
    let g = ui.painter().layout_no_wrap(text.to_owned(), font, col);
    let size = vec2(g.size().x + 20.0, 30.0);
    let (r, resp) = ui.allocate_exact_size(size, Sense::click());
    if resp.hovered() && !active {
        ui.painter().rect_filled(r.shrink2(vec2(0.0, 3.0)), 4.0, p.hover);
    }
    ui.painter().galley(egui::pos2(r.center().x - g.size().x / 2.0, r.center().y - g.size().y / 2.0 - 1.0), g.clone(), col);
    if active {
        let w = g.size().x;
        let y = r.max.y - 2.0;
        ui.painter().line_segment([egui::pos2(r.center().x - w / 2.0, y), egui::pos2(r.center().x + w / 2.0, y)], Stroke::new(3.0, p.accent));
    }
    resp
}

fn help_ribbon(app: &mut App, ui: &mut Ui) {
    let full = app.settings.ribbon_full;
    group(ui, full, "Справка", |ui| {
        if flat(ui, Some(Icon::Help), "Справка", "Краткое руководство по программе", false).clicked() {
            app.dialogs.push(Dialog::Report(
                "Руководство".into(),
                vec![
                    "Щёлкните на странице, чтобы писать текст; перо планшета рисует сразу.".into(),
                    "Боковая кнопка пера — лассо, обратный конец — ластик, двойное нажатие кнопки переключает перо и ластик.".into(),
                    "Shift при рисовании — прямая линия. Задержите перо в конце линии — получится ровная фигура.".into(),
                    "Зачеркните рукопись зигзагом, чтобы удалить её.".into(),
                    "Ctrl+колесо или щипок — масштаб, пробел + перетаскивание — прокрутка.".into(),
                    "Alt+← и Alt+→ — назад и вперёд по просмотренным страницам.".into(),
                    "Быстрая заметка из любого окна: Win+Alt+N.".into(),
                ],
            ));
        }
        if flat(ui, Some(Icon::Text), "Сочетания клавиш", "", false).clicked() {
            app.dialogs.push(Dialog::Hotkeys);
        }
        if flat(ui, Some(Icon::Bulb), "Что нового", "", false).clicked() {
            app.dialogs.push(Dialog::Report(
                "Что нового".into(),
                vec![
                    "Импорт OneNote: разделы .one, записные книжки .onetoc2 и пакеты .onepkg — текст, списки, таблицы, рисунки, вложения и рукописный ввод.".into(),
                    "Импорт веб-архивов MHT, Evernote с вложениями, Joplin, Google Keep, Simplenote, Standard Notes, Obsidian и Notion.".into(),
                    "Рисунки Xournal++ и SVG превращаются в рукописные штрихи, которые можно править.".into(),
                    "Кнопки «Назад» и «Вперёд» в заголовке окна.".into(),
                ],
            ));
        }
        if flat(ui, Some(Icon::Feedback), "Отзыв", "Как сообщить о проблеме", false).clicked() {
            app.dialogs.push(Dialog::Report(
                "Отзыв".into(),
                vec!["Программа работает без сети и ничего не отправляет сама.".into(), "Опишите проблему и приложите файл журнала из папки настроек — так её проще всего воспроизвести.".into()],
            ));
        }
        if flat(ui, None, "О программе", "Сведения о версии", false).clicked() {
            app.dialogs.push(Dialog::About);
        }
    });
    group(ui, full, "Блокнот", |ui| {
        if flat(ui, Some(Icon::Lock), "Проверить целостность", "", false).clicked() {
            if let Some(nb) = app.nb_mut() {
                let r = nb.check_integrity();
                let name = nb.meta.name.clone();
                app.dialogs.push(Dialog::Report(format!("Проверка «{name}»"), r));
            }
        }
    });
}

// ----------------------------------------------------------------------------
// Left rail

pub fn rail(app: &mut App, ui: &mut Ui) {
    let p = cur(ui);
    ui.add_space(6.0);
    for (mode, icon, tip) in [(RailMode::Notebooks, Icon::Notebooks, "Записные книжки"), (RailMode::Search, Icon::Search, "Поиск (Ctrl+E)"), (RailMode::Recent, Icon::Recent, "Последние заметки")] {
        let active = app.rail == mode && !app.nav_hidden;
        let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::click());
        if resp.hovered() {
            ui.painter().rect_filled(r.shrink(3.0), 4.0, p.hover);
        }
        if active {
            ui.painter().rect_filled(Rect::from_min_size(r.min + vec2(0.0, 10.0), vec2(3.0, 24.0)), 1.5, p.accent);
        }
        icons::paint(ui.painter(), Rect::from_center_size(r.center(), vec2(20.0, 20.0)), icon, if active { p.accent_text } else { p.text });
        if resp.on_hover_text(tip).clicked() {
            if app.rail == mode {
                app.nav_hidden = !app.nav_hidden;
            } else {
                app.rail = mode;
                app.nav_hidden = false;
            }
            if mode == RailMode::Search {
                app.search_focus = true;
            }
        }
    }
}

/// Russian long date: "3 июня 2026 г."
pub fn long_date(d: &chrono::NaiveDateTime) -> String {
    use chrono::Datelike;
    const M: [&str; 12] = ["января", "февраля", "марта", "апреля", "мая", "июня", "июля", "августа", "сентября", "октября", "ноября", "декабря"];
    format!("{} {} {} г.", d.day(), M[d.month0() as usize], d.year())
}

pub fn text_center(ui: &Ui, rect: Rect, text: &str, color: Color32) {
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, text, FontId::proportional(13.0), color);
}
