//! Main window layout: menus, ribbon, notebook tree, page list, page header,
//! find bar, canvas and status bar.

use egui::{Color32, RichText, Ui, vec2};

use crate::app::*;
use crate::canvas::{self, Tool};
use crate::items::FormatCmd;
use crate::model::*;
use crate::richtext;

pub fn draw(app: &mut App, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    let p = crate::chrome::cur(ui);
    let hide = app.focus_mode || app.presentation;
    let custom = !app.settings.native_frame;
    if !app.presentation && (!hide || custom) {
        egui::Panel::top("titlebar")
            .exact_size(34.0)
            .show_separator_line(false)
            .frame(egui::Frame::new().fill(p.title_bg))
            .show(ui, |ui| crate::app::timed("title", || crate::chrome::title_bar(app, ui)));
    }
    if !hide {
        egui::Panel::top("ribbon")
            .frame(egui::Frame::new().fill(p.ribbon_bg).inner_margin(egui::Margin { left: 0, right: 0, top: 2, bottom: 2 }).stroke(egui::Stroke::new(1.0, p.border)))
            .show_separator_line(false)
            .show(ui, |ui| crate::app::timed("ribbon", || crate::chrome::ribbon(app, ui)));
    }
    if !app.presentation && app.settings.status_bar {
        egui::Panel::bottom("status")
            .frame(egui::Frame::new().fill(p.ribbon_bg).inner_margin(egui::Margin::symmetric(8, 2)))
            .show_separator_line(false)
            .show(ui, |ui| status_bar(app, ui));
    }
    if !hide {
        egui::Panel::left("rail")
            .exact_size(48.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(egui::Frame::new().fill(p.rail_bg))
            .show(ui, |ui| crate::chrome::rail(app, ui));
        if !app.nav_hidden {
            let nav_frame = egui::Frame::new().fill(p.nav_bg).inner_margin(egui::Margin::symmetric(0, 0));
            match app.rail {
                RailMode::Notebooks => {
                    egui::Panel::left("nav").resizable(true).default_size(240.0).show_separator_line(false).frame(nav_frame).show(ui, |ui| crate::app::timed("nav", || nav_tree(app, ui)));
                    let pages_frame = egui::Frame::new().fill(p.pages_bg);
                    if app.settings.pages_right {
                        egui::Panel::right("pages").resizable(true).default_size(230.0).show_separator_line(false).frame(pages_frame).show(ui, |ui| crate::app::timed("pages", || page_list(app, ui)));
                    } else {
                        egui::Panel::left("pages").resizable(true).default_size(230.0).show_separator_line(false).frame(pages_frame).show(ui, |ui| crate::app::timed("pages", || page_list(app, ui)));
                    }
                }
                RailMode::Search => {
                    egui::Panel::left("nav_search").resizable(true).default_size(300.0).frame(nav_frame.inner_margin(8)).show(ui, |ui| {
                        ui.heading("Поиск");
                        crate::ui_panels::search_panel(app, ui, &ctx);
                    });
                }
                RailMode::Recent => {
                    egui::Panel::left("nav_recent").resizable(true).default_size(280.0).frame(nav_frame.inner_margin(egui::Margin::symmetric(8, 0))).show(ui, |ui| recent_list(app, ui));
                }
            }
        }
    }
    if let (Some(_), false) = (app.right, app.presentation) {
        egui::Panel::right("right").resizable(true).default_size(330.0).show(ui, |ui| crate::ui_panels::draw(app, ui));
    }
    egui::CentralPanel::no_frame().show(ui, |ui| {
        if app.settings.sections_on_top && !hide {
            egui::Frame::new().fill(p.nav_bg).inner_margin(egui::Margin { left: 0, right: 0, top: 4, bottom: 0 }).show(ui, |ui| section_tabs(app, ui));
        }
        crate::app::timed("central", || central(app, ui, &ctx))
    });
    crate::ui_draw::pen_settings_window(app, &ctx);
    if !app.settings.status_bar {
        status_toast(app, &ctx);
    }
    if custom && !app.presentation {
        crate::chrome::resize_edges(&ctx);
    }
}

pub(crate) fn file_menu(app: &mut App, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    if ui.button("Новый блокнот…").clicked() {
        app.dialogs.push(Dialog::NewNotebook("Новый блокнот".into()));
        ui.close();
    }
    if ui.button("Открыть блокнот…").clicked() {
        if let Some(p) = rfd::FileDialog::new().set_title("Папка блокнота").pick_folder() {
            app.open_notebook_folder(&ctx, &p);
        }
        ui.close();
    }
    if let Some(ni) = app.cur_nb {
        if ui.button("Закрыть блокнот").clicked() {
            app.dialogs.push(Dialog::Confirm { text: "Закрыть блокнот? Файлы останутся на диске.".into(), action: ConfirmAction::CloseNotebook(ni) });
            ui.close();
        }
    }
    ui.separator();
    if ui.button("Импорт файлов…").on_hover_text("OneNote (.one, .onetoc2, .onepkg), MHT, HTML, Markdown, TXT, Evernote, Word, ODT, RTF, Xournal++, SVG, Joplin, Google Keep, Simplenote, Standard Notes, PDF, CSV, Excel").clicked() {
        if let Some(files) = rfd::FileDialog::new()
            .add_filter("Все поддерживаемые", crate::import::SUPPORTED)
            .add_filter("OneNote", &["one", "onetoc2", "onepkg"])
            .add_filter("Веб-страницы", &["mht", "mhtml", "html", "htm"])
            .add_filter("Текст и Markdown", &["md", "markdown", "txt", "rtf", "odt", "docx"])
            .add_filter("Рисунки", &["xopp", "xoj", "svg"])
            .add_filter("Другие заметки", &["enex", "jex", "json", "zip"])
            .pick_files()
        {
            for f in files {
                app.import_document(&ctx, &f);
            }
        }
        ui.close();
    }
    if ui.button("Импорт папки (OneNote, Obsidian, Notion, Keep)…").clicked() {
        if let Some(dir) = rfd::FileDialog::new().set_title("Папка с заметками").pick_folder() {
            app.import_document(&ctx, &dir);
        }
        ui.close();
    }
    ui.menu_button("Импорт архива блокнота", |ui| {
        if ui.button("Открыть как новый блокнот…").clicked() {
            if let Some(p) = rfd::FileDialog::new().add_filter("Архив блокнота", &["nnbz", "zip"]).pick_file() {
                app.import_archive(&ctx, &p, None);
            }
            ui.close();
        }
        if ui.button("Объединить с текущим…").clicked() {
            if let Some(p) = rfd::FileDialog::new().add_filter("Архив блокнота", &["nnbz", "zip"]).pick_file() {
                app.dialogs.push(Dialog::Merge { src: p, policy: 0 });
            }
            ui.close();
        }
    });
    ui.menu_button("Экспорт страницы", |ui| export_menu(app, ui, false));
    ui.menu_button("Экспорт раздела", |ui| export_menu(app, ui, true));
    if let Some(ni) = app.cur_nb {
        if ui.button("Экспорт блокнота (архив)…").clicked() {
            let name = app.notebooks[ni].meta.name.clone();
            if let Some(p) = rfd::FileDialog::new().add_filter("Архив блокнота", &["nnbz"]).set_file_name(format!("{name}.nnbz")).save_file() {
                app.export_archive(ni, &p);
            }
            ui.close();
        }
    }
    ui.separator();
    if ui.button("Печать…   Ctrl+P").clicked() {
        app.dialogs.push(Dialog::Print(PrintOpts::default()));
        ui.close();
    }
    ui.separator();
    if ui.button("Резервные копии…").clicked() {
        app.dialogs.push(Dialog::Backups);
        ui.close();
    }
    if ui.button("Корзина…").clicked() {
        app.dialogs.push(Dialog::Trash);
        ui.close();
    }
    if ui.button("Проверить целостность блокнота").clicked() {
        if let Some(nb) = app.nb_mut() {
            let r = nb.check_integrity();
            let name = nb.meta.name.clone();
            app.dialogs.push(Dialog::Report(format!("Проверка «{name}»"), r));
        }
        ui.close();
    }
    if ui.button("Свойства блокнота").clicked() {
        if let Some(nb) = app.nb() {
            let (total, assets, pages) = nb.size_info();
            let lines = vec![
                format!("Папка: {}", nb.path.display()),
                format!("Страниц: {pages}"),
                format!("Общий размер: {}", crate::storage::human_size(total)),
                format!("Вложения: {}", crate::storage::human_size(assets)),
                format!("История версий: {}", crate::storage::human_size(crate::storage::dir_size(&nb.path.join("versions")))),
                format!("В корзине: {}", nb.meta.trash.len()),
                format!("Создан: {}", fmt_dt(&nb.meta.created)),
            ];
            app.dialogs.push(Dialog::Report(nb.meta.name.clone(), lines));
        }
        ui.close();
    }
    ui.separator();
    if ui.button("Параметры…").clicked() {
        app.dialogs.push(Dialog::Settings(0));
        ui.close();
    }
    if ui.button("Выход").clicked() {
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
    ui.separator();
    ui.menu_button("Страница", |ui| page_menu(app, ui));
    ui.menu_button("Вставка", |ui| insert_menu(app, ui));
    ui.menu_button("Инструменты", |ui| tools_menu(app, ui));
    ui.menu_button("Вид", |ui| view_menu(app, ui));
}

pub(crate) fn tools_menu(app: &mut App, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    for (label, p) in [
        ("Мои задачи", RightPanel::Tasks),
        ("Сводка тегов", RightPanel::Tags),
        ("Умный помощник", RightPanel::Helper),
        ("Чтение вслух", RightPanel::Reading),
        ("Комментарии", RightPanel::Comments),
        ("История версий", RightPanel::Versions),
    ] {
        if ui.button(label).clicked() {
            app.right = Some(p);
            if p == RightPanel::Tags || p == RightPanel::Tasks {
                app.collect_summary(&ctx);
            }
            ui.close();
        }
    }
    ui.separator();
    if ui.button("Голосовой ввод (Win+H)").clicked() {
        crate::winapi::start_dictation();
        ui.close();
    }
    if ui.button("Быстрая заметка   Win+Alt+N").clicked() {
        app.quick_note = Some(String::new());
        ui.close();
    }
    if ui.button("Заблокировать защищённые разделы   Ctrl+Alt+L").clicked() {
        app.lock_all();
        ui.close();
    }
    if ui.button("Создать резервную копию сейчас").clicked() {
        app.run_backups_now();
        ui.close();
    }
    if ui.button("Перестроить индекс поиска").clicked() {
        app.rebuild_index(&ctx);
        ui.close();
    }
    ui.separator();
    if ui.button("Теги…").clicked() {
        app.dialogs.push(Dialog::Tags);
        ui.close();
    }
    if ui.button("Горячие клавиши").clicked() {
        app.dialogs.push(Dialog::Hotkeys);
        ui.close();
    }
    if ui.button("О программе").clicked() {
        app.dialogs.push(Dialog::About);
        ui.close();
    }
}

pub(crate) fn export_menu(app: &mut App, ui: &mut Ui, section: bool) {
    let name = app.cur_page.and_then(|p| app.nb().and_then(|nb| nb.meta.page_meta(p))).map(|p| p.title.clone()).unwrap_or_default();
    if ui.button("PDF…").clicked() {
        let mut o = PrintOpts { whole_section: section, print: false, ..Default::default() };
        o.path = None;
        app.dialogs.push(Dialog::Print(o));
        ui.close();
    }
    for (label, fmt) in [("Word (DOCX)…", "docx"), ("HTML…", "html"), ("Markdown…", "md")] {
        if ui.button(label).clicked() {
            app.export_text_formats(fmt, section);
            ui.close();
        }
    }
    if !section {
        if ui.button("Текст (TXT)…").clicked() {
            app.export_text_formats("txt", false);
            ui.close();
        }
        for (label, ext, kind) in [("Изображение PNG…", "png", ExportKind::Png), ("Изображение JPG…", "jpg", ExportKind::Jpg)] {
            if ui.button(label).clicked() {
                if app.current_no_copy() {
                    app.set_status("Экспорт запрещён для этого раздела");
                } else if let Some(p) = rfd::FileDialog::new().add_filter(ext.to_uppercase(), &[ext]).set_file_name(format!("{}.{ext}", crate::storage::sanitize(&name))).save_file() {
                    app.start_export(kind, p, PrintOpts { header: false, ..Default::default() });
                }
                ui.close();
            }
        }
    }
}

fn insert_menu(app: &mut App, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    if ui.button("Таблица…").clicked() {
        app.dialogs.push(Dialog::Table { rows: 3, cols: 3 });
        ui.close();
    }
    if ui.button("Изображение…").clicked() {
        pick_images(app);
        ui.close();
    }
    if ui.button("Файл…").clicked() {
        pick_files(app);
        ui.close();
    }
    if ui.button("Печатная копия PDF…").clicked() {
        if let Some(p) = rfd::FileDialog::new().add_filter("PDF", &["pdf"]).pick_file() {
            app.pdf_printout_from(p, [0.0, 0.0]);
        }
        ui.close();
    }
    if ui.button("Аудио или видео файл…").clicked() {
        pick_files(app);
        ui.close();
    }
    if ui.button("Снимок экрана").clicked() {
        app.start_screenshot(&ctx);
        ui.close();
    }
    ui.separator();
    if ui.button("Ссылка…   Ctrl+K").clicked() {
        app.dialogs.push(Dialog::Link { text: String::new(), url: String::new() });
        ui.close();
    }
    if ui.button("Формула").clicked() {
        app.insert_item(ItemKind::Equation(String::new()), 420.0);
        ui.close();
    }
    if ui.button("Блок кода").clicked() {
        app.insert_item(ItemKind::Code(CodeItem { lang: "rust".into(), code: String::new() }), 560.0);
        ui.close();
    }
    if ui.button("Диаграмма").clicked() {
        app.insert_item(ItemKind::Diagram(Diagram { kind: DiagramKind::MindMap, source: "Тема\n  Идея 1\n  Идея 2\n    Деталь".into() }), 560.0);
        ui.close();
    }
    ui.separator();
    if ui.button("Дата   Alt+Shift+D").clicked() {
        app.insert_text_at_cursor(&ctx, chrono::Local::now().format("%d.%m.%Y").to_string());
        ui.close();
    }
    if ui.button("Время   Alt+Shift+T").clicked() {
        app.insert_text_at_cursor(&ctx, chrono::Local::now().format("%H:%M").to_string());
        ui.close();
    }
    if ui.button("Дата и время").clicked() {
        app.insert_text_at_cursor(&ctx, chrono::Local::now().format("%d.%m.%Y %H:%M").to_string());
        ui.close();
    }
    if ui.button("Символ…").clicked() {
        app.dialogs.push(Dialog::Symbols);
        ui.close();
    }
    ui.separator();
    if ui.button("Горизонтальная линия").clicked() {
        app.insert_item(ItemKind::Divider { page_break: false }, 600.0);
        app.with_page(|_, ed| ed.editing = None);
        ui.close();
    }
    if ui.button("Разрыв страницы").clicked() {
        app.insert_item(ItemKind::Divider { page_break: true }, 600.0);
        app.with_page(|_, ed| ed.editing = None);
        ui.close();
    }
    if ui.button("Сводка тегов на новую страницу").clicked() {
        crate::ui_panels::insert_summary_page(app, &ctx);
        ui.close();
    }
}

fn page_menu(app: &mut App, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    if ui.button("Новая страница   Ctrl+N").clicked() {
        app.new_page(false);
        ui.close();
    }
    if ui.button("Новая подстраница   Ctrl+Alt+Shift+N").clicked() {
        app.new_page(true);
        ui.close();
    }
    if ui.button("Из шаблона…").clicked() {
        app.dialogs.push(Dialog::Templates);
        ui.close();
    }
    if ui.button("Сохранить как шаблон…").clicked() {
        let title = app.cur_page.and_then(|p| app.nb().and_then(|nb| nb.meta.page_meta(p))).map(|p| p.title.clone()).unwrap_or_default();
        if let Some(page) = app.cur_page.and_then(|p| app.pages.get(&p)) {
            if crate::templates::save_custom(&title, page) {
                app.set_status(format!("Шаблон «{title}» сохранён"));
            }
        }
        ui.close();
    }
    ui.separator();
    page_look(app, ui);
    ui.separator();
    if ui.button("Распознать рукописный текст").clicked() {
        app.handle_action(&ctx, crate::canvas::Action::OcrInk);
        ui.close();
    }
    if ui.button("История версий").clicked() {
        app.right = Some(RightPanel::Versions);
        ui.close();
    }
    if ui.button("Комментарии").clicked() {
        app.right = Some(RightPanel::Comments);
        ui.close();
    }
    if ui.button("Копировать ссылку на страницу").clicked() {
        if let Some(p) = app.cur_page {
            ctx.copy_text(format!("note://page/{p}"));
            app.set_status("Ссылка скопирована");
        }
        ui.close();
    }
}

fn page_look(app: &mut App, ui: &mut Ui) {
    let Some(pid) = app.cur_page else { return };
    let mut changed = false;
    if let Some(page) = app.pages.get_mut(&pid) {
        ui.menu_button("Фон страницы", |ui| {
            let cur = page.pattern();
            for b in Pattern::ALL {
                if ui.radio(cur == b, b.label()).clicked() {
                    page.pattern = Some(b);
                    changed = true;
                }
            }
        });
        ui.menu_button("Размер страницы", |ui| {
            for s in PageSize::ALL {
                if ui.radio(page.size == s, s.label()).clicked() {
                    page.size = s;
                    changed = true;
                }
            }
        });
        ui.menu_button("Цвет страницы", |ui| {
            if ui.button("По умолчанию").clicked() {
                page.bg_color = None;
                changed = true;
            }
            ui.horizontal_wrapped(|ui| {
                for c in [[255, 255, 255], [255, 250, 230], [235, 245, 255], [235, 255, 240], [255, 238, 245], [245, 240, 255], [242, 242, 242], [40, 44, 52]] {
                    let (r, resp) = ui.allocate_exact_size(vec2(22.0, 22.0), egui::Sense::click());
                    ui.painter().rect_filled(r, 4.0, Color32::from_rgb(c[0], c[1], c[2]));
                    ui.painter().rect_stroke(r, 4.0, egui::Stroke::new(1.0, Color32::GRAY), egui::StrokeKind::Inside);
                    if resp.clicked() {
                        page.bg_color = Some([c[0], c[1], c[2], 255]);
                        changed = true;
                    }
                }
            });
        });
        ui.menu_button("Метка конфиденциальности", |ui| {
            for s in Sensitivity::ALL {
                if ui.radio(page.sensitivity == s, s.label()).clicked() {
                    page.sensitivity = s;
                    changed = true;
                }
            }
        });
    }
    if changed {
        app.mark_dirty(pid);
    }
}

fn view_menu(app: &mut App, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    ui.menu_button("Тема", |ui| {
        for (t, l) in [(crate::settings::Theme::Light, "Светлая"), (crate::settings::Theme::Dark, "Тёмная"), (crate::settings::Theme::System, "Системная")] {
            if ui.radio(app.settings.theme == t, l).clicked() {
                app.settings.theme = t;
                app.apply_theme(&ctx);
                app.settings.save();
            }
        }
    });
    if ui.checkbox(&mut app.settings.touch_mode, "Режим для сенсорного экрана").changed() {
        app.apply_theme(&ctx);
        app.settings.save();
    }
    if ui.button("Режим фокусировки   F11").clicked() {
        app.focus_mode = !app.focus_mode;
        ui.close();
    }
    if ui.button("Презентация   F5").clicked() {
        app.presentation = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
        ui.close();
    }
    ui.separator();
    if ui.button("Новое окно со страницей   Ctrl+M").clicked() {
        app.pin_current(false);
        ui.close();
    }
    if ui.button("Закрепить страницу поверх окон").clicked() {
        app.pin_current(true);
        ui.close();
    }
    let mut on_top = app.reading.main_on_top;
    if ui.checkbox(&mut on_top, "Главное окно поверх других").changed() {
        app.reading.main_on_top = on_top;
        ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(if on_top { egui::WindowLevel::AlwaysOnTop } else { egui::WindowLevel::Normal }));
    }
    ui.separator();
    for (label, p) in [("Поиск", RightPanel::Search), ("Теги", RightPanel::Tags), ("Задачи", RightPanel::Tasks), ("Помощник", RightPanel::Helper), ("Чтение", RightPanel::Reading)] {
        if ui.selectable_label(app.right == Some(p), label).clicked() {
            app.right = if app.right == Some(p) { None } else { Some(p) };
            ui.close();
        }
    }
}

fn pick_images(app: &mut App) {
    if let Some(files) = rfd::FileDialog::new().add_filter("Изображения", &["png", "jpg", "jpeg", "gif", "bmp", "webp"]).pick_files() {
        let list = files
            .into_iter()
            .map(|p| {
                let n = p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                (Some(p), None, n)
            })
            .collect();
        app.import_files(list, None);
    }
}

fn pick_files(app: &mut App) {
    if let Some(files) = rfd::FileDialog::new().pick_files() {
        let list = files
            .into_iter()
            .map(|p| {
                let n = p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                (Some(p), None, n)
            })
            .collect();
        app.import_files(list, None);
    }
}

// ----------------------------------------------------------------------------
// Ribbon contents

pub(crate) fn home_ribbon(app: &mut App, ui: &mut Ui, ctx: &egui::Context) {
    use crate::chrome::{boxed, chevron, color_icon, flat, glyph, group};
    use crate::icons::Icon;
    let full = app.settings.ribbon_full;
    // Current text block properties for the combos.
    let (font, size) = app
        .cur_page
        .and_then(|p| {
            let ed = app.editors.get(&p)?;
            let id = ed.editing.or_else(|| ed.selection.items.first().copied())?;
            match &app.pages.get(&p)?.item(id)?.kind {
                ItemKind::Text(t) => Some((t.font.clone(), t.size)),
                _ => None,
            }
        })
        .unwrap_or((app.settings.default_font.clone(), app.settings.default_size));
    group(ui, full, "Буфер обмена", |ui| {
        if flat(ui, Some(Icon::Paste), "", "Вставить (Ctrl+V)", false).clicked() {
            app.paste_from_clipboard(ctx);
        }
        let r = chevron(ui, "Параметры вставки");
        egui::Popup::menu(&r).show(|ui| {
            if ui.button("Вырезать   Ctrl+X").clicked() {
                ctx.send_viewport_cmd(egui::ViewportCommand::RequestCut);
                ui.close();
            }
            if ui.button("Копировать   Ctrl+C").clicked() {
                ctx.send_viewport_cmd(egui::ViewportCommand::RequestCopy);
                ui.close();
            }
            if ui.button("Вставить   Ctrl+V").clicked() {
                app.paste_from_clipboard(ctx);
                ui.close();
            }
            ui.separator();
            ui.label(RichText::new("При вставке").small().weak());
            for (l, m) in [("Сохранить форматирование", crate::settings::PasteMode::KeepFormatting), ("Объединить форматирование", crate::settings::PasteMode::Merge), ("Только текст", crate::settings::PasteMode::TextOnly)] {
                if ui.radio(app.settings.paste_mode == m, l).clicked() {
                    app.settings.paste_mode = m;
                    app.settings.save();
                }
            }
        });
    });
    group(ui, full, "Текст", |ui| {
        ui.add_space(4.0);
        let shown = if font.is_empty() { "Segoe UI".to_string() } else { font.clone() };
        egui::ComboBox::from_id_salt("font").width(118.0).height(400.0).selected_text(shown).show_ui(ui, |ui| {
            for f in crate::fonts::font_names() {
                if ui.selectable_label(f == font, RichText::new(&f).font(crate::fonts::font(&f, 14.0, false, false, false))).clicked() {
                    app.format(ctx, FormatCmd::Font(f));
                }
            }
        });
        ui.add_space(4.0);
        egui::ComboBox::from_id_salt("size").width(46.0).selected_text(format!("{size:.0}")).show_ui(ui, |ui| {
            for s in [8.0, 9.0, 10.0, 11.0, 12.0, 14.0, 15.0, 16.0, 18.0, 20.0, 24.0, 28.0, 32.0, 36.0, 48.0, 72.0] {
                if ui.selectable_label(s == size, format!("{s:.0}")).clicked() {
                    app.format(ctx, FormatCmd::Size(s));
                }
            }
        });
        ui.add_space(4.0);
        if glyph(ui, "Ж", true, false, false, "Полужирный (Ctrl+B)").clicked() {
            app.format(ctx, FormatCmd::Wrap("**", "**"));
        }
        if glyph(ui, "К", false, true, false, "Курсив (Ctrl+I)").clicked() {
            app.format(ctx, FormatCmd::Wrap("*", "*"));
        }
        if glyph(ui, "Ч", false, false, true, "Подчёркнутый (Ctrl+U)").clicked() {
            app.format(ctx, FormatCmd::Wrap("__", "__"));
        }
        if color_icon(ui, Icon::Marker, Color32::from_rgb(255, 230, 0), "Цвет выделения текста").clicked() {
            app.format(ctx, FormatCmd::Wrap("==", "=="));
        }
        let r = chevron(ui, "Цвет выделения");
        egui::Popup::menu(&r).show(|ui| {
            ui.label("Цвет выделения текста");
            color_grid(ui, |c| app.format(ctx, FormatCmd::WrapOwned(format!("{{h:#{:02x}{:02x}{:02x}|", c[0], c[1], c[2]), "}".into())));
        });
        if color_icon(ui, Icon::FontColor, Color32::from_rgb(210, 40, 40), "Цвет шрифта").clicked() {
            app.format(ctx, FormatCmd::Color([210, 40, 40, 255]));
        }
        let r = chevron(ui, "Цвет шрифта");
        egui::Popup::menu(&r).show(|ui| {
            ui.label("Цвет текста");
            color_grid(ui, |c| app.format(ctx, FormatCmd::Color(c)));
        });
        let painter_on = app.cur_page.and_then(|p| app.editors.get(&p)).is_some_and(|e| e.format_painter.is_some());
        if flat(ui, Some(Icon::Painter), "", "Формат по образцу: поставьте курсор в образец, нажмите, затем щёлкните по блоку", painter_on).clicked() {
            let st = app.cur_page.and_then(|p| Some(crate::items::style_under_cursor(ctx, app.pages.get(&p)?, app.editors.get(&p)?))).flatten();
            app.with_page(|_, ed| {
                ed.format_painter = if painter_on { None } else { st };
                ed.editing = None;
            });
        }
        if flat(ui, Some(Icon::ClearFormat), "", "Очистить форматирование (Ctrl+Пробел)", false).clicked() {
            app.format(ctx, FormatCmd::Clear);
        }
        let r = chevron(ui, "Другие параметры текста");
        egui::Popup::menu(&r).show(|ui| {
            for (l, cmd) in [
                ("Зачёркнутый   Ctrl+-", FormatCmd::Wrap("~~", "~~")),
                ("Верхний индекс   Ctrl+Shift+=", FormatCmd::Wrap("^", "^")),
                ("Нижний индекс   Ctrl+=", FormatCmd::Wrap(",,", ",,")),
            ] {
                if ui.button(l).clicked() {
                    app.format(ctx, cmd);
                    ui.close();
                }
            }
        });
    });
    group(ui, full, "Абзац", |ui| {
        for (icon, tip, cmd) in [
            (Icon::Bullets, "Маркированный список (Ctrl+.)", FormatCmd::LinePrefix("- ".into())),
            (Icon::Numbers, "Нумерованный список (Ctrl+/)", FormatCmd::LinePrefix("1. ".into())),
            (Icon::Outdent, "Уменьшить отступ", FormatCmd::Indent(true)),
            (Icon::Indent, "Увеличить отступ", FormatCmd::Indent(false)),
        ] {
            if flat(ui, Some(icon), "", tip, false).clicked() {
                app.format(ctx, cmd);
            }
        }
        let r = chevron(ui, "Выравнивание и интервалы");
        egui::Popup::menu(&r).show(|ui| {
            ui.horizontal(|ui| {
                for (icon, tip, a) in [(Icon::AlignLeft, "По левому краю", Align::Left), (Icon::AlignCenter, "По центру", Align::Center), (Icon::AlignRight, "По правому краю", Align::Right), (Icon::Justify, "По ширине", Align::Justify)] {
                    if flat(ui, Some(icon), "", tip, false).clicked() {
                        app.format(ctx, FormatCmd::Align(a));
                    }
                }
            });
            ui.separator();
            ui.label("Межстрочный интервал");
            ui.horizontal(|ui| {
                for s in [1.0, 1.15, 1.5, 2.0, 2.5] {
                    if ui.button(format!("{s}")).clicked() {
                        app.format(ctx, FormatCmd::Spacing(s));
                    }
                }
            });
            ui.separator();
            if ui.button("Справа налево / слева направо").clicked() {
                app.format(ctx, FormatCmd::Rtl);
                ui.close();
            }
            if let Some(pid) = app.cur_page {
                let mut changed = false;
                if let (Some(page), Some(ed)) = (app.pages.get_mut(&pid), app.editors.get(&pid)) {
                    if let Some(id) = ed.editing.or_else(|| ed.selection.items.first().copied()) {
                        if let Some(ItemKind::Text(t)) = page.item_mut(id).map(|i| &mut i.kind) {
                            ui.separator();
                            changed |= ui.add(egui::Slider::new(&mut t.para_before, 0.0..=40.0).text("Интервал до")).changed();
                            changed |= ui.add(egui::Slider::new(&mut t.para_after, 0.0..=40.0).text("Интервал после")).changed();
                            ui.horizontal(|ui| {
                                ui.label("Язык проверки:");
                                for l in ["", "ru-RU", "en-US", "de-DE", "fr-FR", "uk-UA"] {
                                    if ui.selectable_label(t.lang == l, if l.is_empty() { "по умолч." } else { l }).clicked() {
                                        t.lang = l.into();
                                        changed = true;
                                    }
                                }
                            });
                        }
                    }
                }
                if changed {
                    app.mark_dirty(pid);
                }
            }
        });
    });
    group(ui, full, "Теги", |ui| {
        let tags = app.settings.tags.clone();
        let todo = tags.iter().find(|t| t.is_task).cloned();
        if flat(ui, Some(Icon::Checkbox), "", "Тег «Задача» (Ctrl+1)", false).clicked() {
            match &todo {
                Some(t) => app.toggle_tag_current(t),
                None => app.format(ctx, FormatCmd::LinePrefix("[ ] ".into())),
            }
        }
        let r = chevron(ui, "Все теги");
        egui::Popup::menu(&r).show(|ui| {
            ui.set_min_width(230.0);
            for t in &tags {
                let col = Color32::from_rgb(t.color[0], t.color[1], t.color[2]);
                let mut job = egui::text::LayoutJob::default();
                job.append(&format!("{} ", t.icon), 0.0, egui::TextFormat { color: col, font_id: egui::FontId::proportional(14.0), ..Default::default() });
                job.append(&t.name, 0.0, egui::TextFormat { color: ui.visuals().text_color(), font_id: egui::FontId::proportional(14.0), ..Default::default() });
                if let Some(d) = t.shortcut {
                    job.append(&format!("   Ctrl+{d}"), 0.0, egui::TextFormat { color: ui.visuals().weak_text_color(), font_id: egui::FontId::proportional(12.0), ..Default::default() });
                }
                if ui.button(job).clicked() {
                    app.toggle_tag_current(t);
                    ui.close();
                }
            }
            ui.separator();
            if ui.button("Найти теги").clicked() {
                app.right = Some(RightPanel::Tags);
                app.collect_summary(ctx);
                ui.close();
            }
            if ui.button("Мои задачи").clicked() {
                app.right = Some(RightPanel::Tasks);
                app.collect_summary(ctx);
                ui.close();
            }
            if ui.button("Настроить теги…").clicked() {
                app.dialogs.push(Dialog::Tags);
                ui.close();
            }
        });
    });
    group(ui, full, "Стили", |ui| {
        let styles: [(&str, &str, f32, bool); 10] = [
            ("Заголовок 1", "# ", 15.0, true),
            ("Заголовок 2", "## ", 14.0, true),
            ("Заголовок 3", "### ", 13.0, true),
            ("Заголовок 4", "#### ", 12.5, true),
            ("Заголовок 5", "##### ", 12.0, false),
            ("Заголовок 6", "###### ", 12.0, false),
            ("Обычный текст", "", 12.0, false),
            ("Цитата", "> ", 12.0, false),
            ("Примечание", "!> ", 12.0, false),
            ("Код", "```", 12.0, false),
        ];
        ui.add_space(4.0);
        let (main, arrow) = boxed(ui, "Заголовок 1", 132.0, crate::fonts::font("", 13.5, false, false, false), "Заголовок 1 (Ctrl+Alt+1)");
        if main.clicked() {
            app.format(ctx, FormatCmd::LinePrefix("# ".into()));
        }
        egui::Popup::menu(&arrow).show(|ui| {
            ui.set_min_width(200.0);
            for (l, pfx, sz, bold) in styles {
                let mut t = RichText::new(l).size(sz + 1.0);
                if bold {
                    t = t.strong();
                }
                if ui.button(t).clicked() {
                    if pfx == "```" {
                        app.format(ctx, FormatCmd::WrapOwned("```\n".into(), "\n```".into()));
                    } else {
                        app.format(ctx, FormatCmd::LinePrefix(pfx.into()));
                    }
                    ui.close();
                }
            }
        });
        ui.add_space(4.0);
    });
    group(ui, full, "Инструменты", |ui| {
        let on = app.settings.spellcheck;
        if flat(ui, None, "ABC✓", "Проверка орфографии", on).clicked() {
            app.settings.spellcheck = !on;
            app.settings.save();
        }
        if flat(ui, Some(Icon::Find), "", "Найти на странице (Ctrl+F)", false).clicked() {
            app.with_page(|_, ed| ed.find.open = true);
        }
        if flat(ui, Some(Icon::Dictate), "", "Диктовка (Win+H)", false).clicked() {
            crate::winapi::start_dictation();
        }
        if flat(ui, Some(Icon::Printer), "", "Печать (Ctrl+P)", false).clicked() {
            app.dialogs.push(Dialog::Print(PrintOpts::default()));
        }
    });
}

fn color_grid(ui: &mut Ui, mut f: impl FnMut([u8; 4])) {
    let colors: [[u8; 3]; 16] = [
        [0, 0, 0], [90, 90, 90], [160, 160, 160], [255, 255, 255], [210, 30, 30], [235, 120, 0], [230, 190, 0], [40, 160, 60],
        [0, 150, 160], [30, 90, 210], [110, 60, 200], [200, 40, 150], [120, 70, 30], [0, 90, 50], [20, 40, 120], [255, 160, 180],
    ];
    egui::Grid::new(ui.id().with("cg")).spacing(vec2(3.0, 3.0)).show(ui, |ui| {
        for (i, c) in colors.iter().enumerate() {
            let (r, resp) = ui.allocate_exact_size(vec2(20.0, 20.0), egui::Sense::click());
            ui.painter().rect_filled(r, 3.0, Color32::from_rgb(c[0], c[1], c[2]));
            ui.painter().rect_stroke(r, 3.0, egui::Stroke::new(1.0, Color32::GRAY), egui::StrokeKind::Inside);
            if resp.clicked() {
                f([c[0], c[1], c[2], 255]);
                ui.close();
            }
            if i % 8 == 7 {
                ui.end_row();
            }
        }
    });
}

pub(crate) fn insert_ribbon(app: &mut App, ui: &mut Ui, ctx: &egui::Context) {
    use crate::chrome::{flat, group};
    use crate::icons::Icon;
    let full = app.settings.ribbon_full;
    group(ui, full, "Страницы", |ui| {
        if flat(ui, Some(Icon::Page), "Страница", "Новая страница (Ctrl+N)", false).clicked() {
            app.new_page(false);
        }
        if flat(ui, Some(Icon::Pages), "Подстраница", "Ctrl+Alt+Shift+N", false).clicked() {
            app.new_page(true);
        }
        if flat(ui, Some(Icon::Section), "Раздел", "Новый раздел (Ctrl+T)", false).clicked() {
            app.new_section();
        }
        if flat(ui, Some(Icon::Template), "Шаблоны", "Страница из шаблона", false).clicked() {
            app.dialogs.push(Dialog::Templates);
        }
    });
    group(ui, full, "Таблицы", |ui| {
        let r = flat(ui, Some(Icon::Table), "Таблица", "Вставить таблицу", false);
        egui::Popup::menu(&r).show(|ui| {
            ui.label("Размер таблицы");
            // Grid picker.
            let n = 8;
            let cell = 16.0;
            let (rect, resp) = ui.allocate_exact_size(vec2(n as f32 * cell, n as f32 * cell), egui::Sense::click());
            let hover = resp.hover_pos().map(|p| {
                let c = ((p.x - rect.min.x) / cell).floor() as usize;
                let r = ((p.y - rect.min.y) / cell).floor() as usize;
                (r.min(n - 1) + 1, c.min(n - 1) + 1)
            });
            for r in 0..n {
                for c in 0..n {
                    let cr = egui::Rect::from_min_size(rect.min + vec2(c as f32 * cell, r as f32 * cell), vec2(cell - 2.0, cell - 2.0));
                    let on = hover.is_some_and(|(hr, hc)| r < hr && c < hc);
                    ui.painter().rect(cr, 2.0, if on { Color32::from_rgb(184, 134, 224) } else { Color32::TRANSPARENT }, egui::Stroke::new(1.0, Color32::GRAY), egui::StrokeKind::Inside);
                }
            }
            if let Some((r, c)) = hover {
                ui.label(format!("{c} × {r}"));
                if resp.clicked() {
                    let t = Table::new(r, c);
                    let w = t.col_widths.iter().sum();
                    app.insert_item(ItemKind::Table(t), w);
                    ui.close();
                }
            }
            if ui.button("Указать размер…").clicked() {
                app.dialogs.push(Dialog::Table { rows: 3, cols: 3 });
                ui.close();
            }
            if ui.button("Из CSV или Excel…").clicked() {
                if let Some(f) = rfd::FileDialog::new().add_filter("Таблицы", &["csv", "xlsx", "xls", "ods"]).pick_file() {
                    app.import_document(ctx, &f);
                }
                ui.close();
            }
        });
    });
    group(ui, full, "Файлы", |ui| {
        if flat(ui, Some(Icon::Attach), "Файл", "Вложить файл", false).clicked() {
            pick_files(app);
        }
        if flat(ui, Some(Icon::Pdf), "Распечатка", "Вставить печатную копию PDF", false).clicked() {
            if let Some(p) = rfd::FileDialog::new().add_filter("PDF", &["pdf"]).pick_file() {
                app.pdf_printout_from(p, [0.0, 0.0]);
            }
        }
    });
    group(ui, full, "Изображения", |ui| {
        if flat(ui, Some(Icon::Image), "Рисунки", "Изображение из файла", false).clicked() {
            pick_images(app);
        }
        if flat(ui, Some(Icon::Paste), "", "Из буфера обмена", false).clicked() {
            app.paste_from_clipboard(ctx);
        }
        if flat(ui, Some(Icon::Screenshot), "Снимок", "Снимок области экрана (Win+Shift+S)", false).clicked() {
            app.start_screenshot(ctx);
        }
    });
    group(ui, full, "Ссылки", |ui| {
        if flat(ui, Some(Icon::Link), "Ссылка", "Ctrl+K", false).clicked() {
            app.dialogs.push(Dialog::Link { text: String::new(), url: String::new() });
        }
    });
    group(ui, full, "Метки времени", |ui| {
        let r = flat(ui, Some(Icon::Calendar), "Дата и время ⌄", "", false);
        egui::Popup::menu(&r).show(|ui| {
            if ui.button("Дата   Alt+Shift+D").clicked() {
                app.insert_text_at_cursor(ctx, chrono::Local::now().format("%d.%m.%Y").to_string());
                ui.close();
            }
            if ui.button("Время   Alt+Shift+T").clicked() {
                app.insert_text_at_cursor(ctx, chrono::Local::now().format("%H:%M").to_string());
                ui.close();
            }
            if ui.button("Дата и время   Alt+Shift+F").clicked() {
                app.insert_text_at_cursor(ctx, chrono::Local::now().format("%d.%m.%Y %H:%M").to_string());
                ui.close();
            }
        });
    });
    group(ui, full, "Символы", |ui| {
        if flat(ui, Some(Icon::Equation), "Формула", "Редактор формул и решение уравнений", false).clicked() {
            app.insert_item(ItemKind::Equation(String::new()), 420.0);
        }
        if flat(ui, Some(Icon::Symbol), "Символ", "Специальные знаки и эмодзи", false).clicked() {
            app.dialogs.push(Dialog::Symbols);
        }
        let r = flat(ui, None, "― Линия ⌄", "", false);
        egui::Popup::menu(&r).show(|ui| {
            if ui.button("Горизонтальная линия").clicked() {
                app.insert_item(ItemKind::Divider { page_break: false }, 600.0);
                app.with_page(|_, ed| ed.editing = None);
                ui.close();
            }
            if ui.button("Разрыв страницы").clicked() {
                app.insert_item(ItemKind::Divider { page_break: true }, 600.0);
                app.with_page(|_, ed| ed.editing = None);
                ui.close();
            }
        });
    });
    group(ui, full, "Блоки", |ui| {
        if flat(ui, Some(Icon::Code), "Код", "Блок кода с подсветкой", false).clicked() {
            app.insert_item(ItemKind::Code(CodeItem { lang: "rust".into(), code: String::new() }), 560.0);
        }
        if flat(ui, Some(Icon::Diagram), "Диаграмма", "Ментальная карта или блок-схема", false).clicked() {
            app.insert_item(ItemKind::Diagram(Diagram { kind: DiagramKind::MindMap, source: "Тема\n  Идея 1\n  Идея 2".into() }), 560.0);
        }
        if flat(ui, Some(Icon::Chart), "График", "Диаграмма по данным", false).clicked() {
            app.insert_item(ItemKind::Chart(Chart { kind: ChartKind::Bar, title: "Диаграмма".into(), data: "Январь: 10\nФевраль: 14\nМарт: 9".into() }), 480.0);
        }
        if flat(ui, Some(Icon::Sticky), "Стикер", "Цветная заметка-стикер", false).clicked() {
            let mut tb = TextBlock::new("", app.settings.default_size, &app.settings.default_font);
            tb.bg = Some([255, 241, 150, 255]);
            tb.color = [40, 40, 40, 255];
            app.insert_item(ItemKind::Text(tb), 220.0);
        }
    });
}

pub(crate) fn view_ribbon(app: &mut App, ui: &mut Ui, ctx: &egui::Context) {
    use crate::chrome::{flat, group};
    use crate::icons::Icon;
    let full = app.settings.ribbon_full;
    group(ui, full, "Режимы", |ui| {
        if flat(ui, Some(Icon::Fullscreen), "Во весь экран", "Полностраничное представление: скрыть разделы и страницы", app.nav_hidden).clicked() {
            app.nav_hidden = !app.nav_hidden;
        }
        if flat(ui, Some(Icon::Focus), "Фокус", "Режим фокусировки (F11)", app.focus_mode).clicked() {
            app.focus_mode = !app.focus_mode;
        }
        if flat(ui, Some(Icon::Present), "Презентация", "Страница на весь экран (F5)", false).clicked() {
            app.presentation = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
        }
        if flat(ui, Some(Icon::Speaker), "Читать вслух", "", app.right == Some(RightPanel::Reading)).clicked() {
            app.right = if app.right == Some(RightPanel::Reading) { None } else { Some(RightPanel::Reading) };
        }
        if flat(ui, Some(Icon::Dictate), "Диктовка", "Голосовой ввод Windows (Win+H)", false).clicked() {
            crate::winapi::start_dictation();
        }
    });
    if let Some(pid) = app.cur_page {
        group(ui, full, "Масштаб", |ui| {
            let bounds = app.pages.get(&pid).map(canvas::export_region);
            if let Some(ed) = app.editors.get_mut(&pid) {
                if flat(ui, Some(Icon::ZoomOut), "", "Уменьшить", false).clicked() {
                    let r = ed.last_rect;
                    ed.zoom_at(r, r.center(), 1.0 / 1.2);
                }
                let mut pct = ed.zoom * 100.0;
                if ui.add(egui::Slider::new(&mut pct, 10.0..=800.0).logarithmic(true).show_value(false)).changed() {
                    let r = ed.last_rect;
                    ed.zoom_at(r, r.center(), pct / 100.0 / ed.zoom);
                }
                ui.label(format!("{:.0}%", ed.zoom * 100.0));
                if flat(ui, Some(Icon::ZoomIn), "", "Увеличить", false).clicked() {
                    let r = ed.last_rect;
                    ed.zoom_at(r, r.center(), 1.2);
                }
                if flat(ui, None, "100%", "Исходный масштаб", false).clicked() {
                    let r = ed.last_rect;
                    ed.zoom_at(r, r.center(), 1.0 / ed.zoom);
                }
                if flat(ui, Some(Icon::Fit), "По ширине", "", false).clicked() {
                    if let Some(b) = bounds {
                        let w = ed.last_rect.width();
                        ed.zoom = (w / b.width().max(100.0)).clamp(0.1, 4.0);
                        ed.offset = b.min.to_vec2() - vec2(20.0, 20.0) / ed.zoom;
                    }
                }
            }
        });
        group(ui, full, "Страница", |ui| page_look_inline(app, ui));
    }
    group(ui, full, "Панели", |ui| {
        for (icon, label, p) in [(Icon::Recent, "Версии", RightPanel::Versions), (Icon::Sticky, "Комментарии", RightPanel::Comments), (Icon::Help, "Помощник", RightPanel::Helper)] {
            if flat(ui, Some(icon), label, "", app.right == Some(p)).clicked() {
                app.right = if app.right == Some(p) { None } else { Some(p) };
            }
        }
        if flat(ui, Some(Icon::Notebooks), "Навигация", "Показать или скрыть панели блокнотов и страниц", !app.nav_hidden).clicked() {
            app.nav_hidden = !app.nav_hidden;
        }
        if flat(ui, None, "Строка состояния", "Показать строку состояния внизу окна", app.settings.status_bar).clicked() {
            app.settings.status_bar = !app.settings.status_bar;
            app.settings.save();
        }
    });
    group(ui, full, "Окно", |ui| {
        if flat(ui, Some(Icon::Pages), "Новое окно", "Страница в отдельном окне (Ctrl+M)", false).clicked() {
            app.pin_current(false);
        }
        if flat(ui, Some(Icon::Pin), "Поверх окон", "Закрепить страницу поверх других окон", false).clicked() {
            app.pin_current(true);
        }
    });
    group(ui, full, "Тема", |ui| {
        let dark = ui.visuals().dark_mode;
        if flat(ui, None, if dark { "☀ Светлая" } else { "🌙 Тёмная" }, "Переключить тему", false).clicked() {
            app.settings.theme = if dark { crate::settings::Theme::Light } else { crate::settings::Theme::Dark };
            app.apply_theme(ctx);
            app.settings.save();
        }
    });
}

pub(crate) fn page_look_inline(app: &mut App, ui: &mut Ui) {
    let Some(pid) = app.cur_page else { return };
    let mut changed = false;
    if let Some(page) = app.pages.get_mut(&pid) {
        let cur = page.pattern();
        egui::ComboBox::from_id_salt("bg").selected_text(cur.label()).show_ui(ui, |ui| {
            for b in Pattern::ALL {
                if ui.selectable_label(cur == b, b.label()).clicked() {
                    page.pattern = Some(b);
                    changed = true;
                }
            }
        });
        egui::ComboBox::from_id_salt("psize").selected_text(page.size.label()).show_ui(ui, |ui| {
            for s in PageSize::ALL {
                if ui.selectable_label(page.size == s, s.label()).clicked() {
                    page.size = s;
                    changed = true;
                }
            }
        });
    }
    if changed {
        app.mark_dirty(pid);
    }
}

// ----------------------------------------------------------------------------
// Navigation tree

#[derive(Clone)]
struct PageDrag {
    page: Id,
}

fn nav_tree(app: &mut App, ui: &mut Ui) {
    use crate::icons::{self, Icon};
    let p = crate::chrome::cur(ui);
    let mut actions: Vec<NavAction> = Vec::new();
    ui.add_space(6.0);
    // Notebook picker.
    ui.horizontal(|ui| {
        let name = app.nb().map(|n| if n.sealed { format!("🔒 {}", n.meta.name) } else { n.meta.name.clone() }).unwrap_or_else(|| "Нет записной книжки".into());
        // Exactly the free width: anything wider makes the resizable panel grow every frame.
        let w = (ui.available_width() - 28.0 - ui.spacing().item_spacing.x).max(40.0);
        let (r, resp) = ui.allocate_exact_size(vec2(w, 34.0), egui::Sense::click());
        if resp.hovered() {
            ui.painter().rect_filled(r, 4.0, p.hover);
        }
        icons::paint(ui.painter(), egui::Rect::from_center_size(egui::pos2(r.min.x + 16.0, r.center().y), vec2(18.0, 18.0)), Icon::Notebooks, p.accent_text);
        let g = ui.painter().layout(name, crate::fonts::font("", 13.5, true, false, false), p.text, w - 60.0);
        let clip = ui.painter().with_clip_rect(egui::Rect::from_min_max(r.min, egui::pos2(r.max.x - 22.0, r.max.y)));
        clip.galley(egui::pos2(r.min.x + 32.0, r.center().y - g.size().y / 2.0), g, p.text);
        icons::paint(ui.painter(), egui::Rect::from_center_size(egui::pos2(r.max.x - 12.0, r.center().y), vec2(14.0, 14.0)), Icon::Chevron, p.weak);
        egui::Popup::menu(&resp).show(|ui| {
            ui.set_min_width(240.0);
            for (ni, nb) in app.notebooks.iter().enumerate() {
                let label = if nb.sealed { format!("🔒 {}", nb.meta.name) } else { nb.meta.name.clone() };
                if ui.selectable_label(app.cur_nb == Some(ni), format!("📓 {label}")).clicked() {
                    actions.push(NavAction::SelectNotebook(ni));
                    ui.close();
                }
            }
            ui.separator();
            if ui.button("+ Создать записную книжку…").clicked() {
                app.dialogs.push(Dialog::NewNotebook("Новый блокнот".into()));
                ui.close();
            }
            if ui.button("Открыть…").clicked() {
                actions.push(NavAction::OpenNotebook);
                ui.close();
            }
            if let Some(ni) = app.cur_nb {
                ui.separator();
                notebook_menu(ui, ni, &mut actions);
                if ui.button("Свойства…").clicked() {
                    actions.push(NavAction::Properties(ni));
                    ui.close();
                }
                if ui.button("Показать в проводнике").clicked() {
                    crate::winapi::shell_open(&app.notebooks[ni].path.to_string_lossy());
                    ui.close();
                }
            }
        });
        resp.context_menu(|ui| {
            if let Some(ni) = app.cur_nb {
                notebook_menu(ui, ni, &mut actions);
            }
        });
        let (r2, add) = ui.allocate_exact_size(vec2(28.0, 34.0), egui::Sense::click());
        if add.hovered() {
            ui.painter().rect_filled(r2, 4.0, p.hover);
        }
        icons::paint(ui.painter(), egui::Rect::from_center_size(r2.center(), vec2(16.0, 16.0)), Icon::Plus, p.text);
        if add.on_hover_text("Новый раздел (Ctrl+T)").clicked() {
            app.new_section();
        }
    });
    ui.add_space(4.0);
    let Some(ni) = app.cur_nb else { return };
    if app.notebooks[ni].sealed {
        ui.vertical_centered(|ui| {
            ui.add_space(20.0);
            ui.label("🔒 Записная книжка защищена паролем");
            if ui.button("Разблокировать…").clicked() {
                actions.push(NavAction::UnlockNotebook(ni));
            }
        });
    } else if app.settings.sections_on_top {
        ui.label(RichText::new("Разделы показаны вкладками над страницей").small().color(p.weak));
    } else {
        let bottom = add_row_rect(ui);
        egui::ScrollArea::vertical().id_salt("nav_scroll").max_height(ui.available_height() - bottom.height()).auto_shrink([false, true]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let app = &*app;
            tree(ui, app, ni, &app.notebooks[ni].meta.tree, &mut actions, 0);
        });
        if add_row(ui, bottom, "Добавить раздел", "Ctrl+T").clicked() {
            app.new_section();
        }
    }
    for a in actions {
        apply_nav(app, ui.ctx(), a);
    }
}

/// Space for the "+ Добавить …" button pinned to the bottom of a column.
fn add_row_rect(ui: &Ui) -> egui::Rect {
    let full = ui.available_rect_before_wrap();
    egui::Rect::from_min_max(egui::pos2(full.min.x, full.max.y - 40.0), full.max)
}

fn add_row(ui: &mut Ui, r: egui::Rect, text: &str, tip: &str) -> egui::Response {
    use crate::icons::{self, Icon};
    let p = crate::chrome::cur(ui);
    let resp = ui.interact(r, ui.id().with(("add_row", text)), egui::Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(r, 0.0, p.hover);
    }
    ui.painter().hline(r.x_range(), r.min.y, egui::Stroke::new(1.0, p.border));
    icons::paint(ui.painter(), egui::Rect::from_center_size(egui::pos2(r.min.x + 20.0, r.center().y), vec2(14.0, 14.0)), Icon::Plus, p.accent);
    ui.painter().text(egui::pos2(r.min.x + 36.0, r.center().y), egui::Align2::LEFT_CENTER, text, egui::FontId::proportional(13.5), p.accent);
    resp.on_hover_text(tip)
}

/// One row of the sections list with a coloured tab on the left.
fn section_row(ui: &mut Ui, label: &str, color: Color32, selected: bool, depth: usize, locked: Option<bool>) -> egui::Response {
    let p = crate::chrome::cur(ui);
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), egui::Sense::click());
    if selected {
        ui.painter().rect_filled(r, 0.0, p.sel);
        ui.painter().rect_filled(egui::Rect::from_min_size(r.min, vec2(3.0, r.height())), 0.0, p.accent);
    } else if resp.hovered() {
        ui.painter().rect_filled(r, 0.0, p.hover);
    }
    let x = r.min.x + 10.0 + depth as f32 * 14.0;
    ui.painter().rect_filled(egui::Rect::from_min_size(egui::pos2(x, r.min.y + 6.0), vec2(6.0, 22.0)), 3.0, color);
    let mut tx = x + 16.0;
    if let Some(l) = locked {
        ui.painter().text(egui::pos2(tx, r.center().y), egui::Align2::LEFT_CENTER, if l { "🔒" } else { "🔓" }, egui::FontId::proportional(12.0), p.weak);
        tx += 18.0;
    }
    let font = if selected { crate::fonts::font("", 13.5, true, false, false) } else { egui::FontId::proportional(13.5) };
    let g = ui.painter().layout_no_wrap(label.to_owned(), font, p.text);
    ui.painter().with_clip_rect(r.shrink(2.0)).galley(egui::pos2(tx, r.center().y - g.size().y / 2.0), g, p.text);
    resp
}

enum NavAction {
    SelectNotebook(usize),
    SelectSection(usize, Id),
    UnlockNotebook(usize),
    OpenNotebook,
    Properties(usize),
    Rename(RenameTarget, String),
    NewSection(usize),
    NewGroup(usize),
    Color(usize, Id, u8),
    DeleteSection(Id),
    DeleteGroup(Id),
    DuplicateSection(Id),
    MergeSection(Id),
    MoveSection(Id, usize, Option<Id>),
    SetSectionPw(Id),
    RemoveSectionPw(usize, Id),
    LockSection(usize, Id),
    NoCopy(usize, Id),
    DefaultTemplate(usize, Id, Option<String>),
    ToggleGroup(usize, Id),
    SetNotebookPw(usize),
    Backups,
    Trash,
    Close(usize),
    DropPage(Id, usize, Id),
    ExportArchive(usize),
    Integrity(usize),
}

fn notebook_menu(ui: &mut Ui, ni: usize, actions: &mut Vec<NavAction>) {
    if ui.button("Переименовать").clicked() {
        actions.push(NavAction::Rename(RenameTarget::Notebook(ni), String::new()));
        ui.close();
    }
    if ui.button("Новый раздел").clicked() {
        actions.push(NavAction::NewSection(ni));
        ui.close();
    }
    if ui.button("Новая группа разделов").clicked() {
        actions.push(NavAction::NewGroup(ni));
        ui.close();
    }
    ui.separator();
    if ui.button("Пароль на блокнот…").clicked() {
        actions.push(NavAction::SetNotebookPw(ni));
        ui.close();
    }
    if ui.button("Экспорт архива…").clicked() {
        actions.push(NavAction::ExportArchive(ni));
        ui.close();
    }
    if ui.button("Резервные копии…").clicked() {
        actions.push(NavAction::Backups);
        ui.close();
    }
    if ui.button("Корзина…").clicked() {
        actions.push(NavAction::Trash);
        ui.close();
    }
    if ui.button("Проверить целостность").clicked() {
        actions.push(NavAction::Integrity(ni));
        ui.close();
    }
    ui.separator();
    if ui.button("Закрыть блокнот").clicked() {
        actions.push(NavAction::Close(ni));
        ui.close();
    }
}

fn tree(ui: &mut Ui, app: &App, ni: usize, nodes: &[Node], actions: &mut Vec<NavAction>, depth: usize) {
    let p = crate::chrome::cur(ui);
    for n in nodes {
        match n {
            Node::Group(g) => {
                let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), egui::Sense::click());
                if resp.hovered() {
                    ui.painter().rect_filled(r, 0.0, p.hover);
                }
                let x = r.min.x + 8.0 + depth as f32 * 14.0;
                crate::icons::paint(ui.painter(), egui::Rect::from_center_size(egui::pos2(x + 7.0, r.center().y), vec2(12.0, 12.0)), if g.collapsed { crate::icons::Icon::Chevron } else { crate::icons::Icon::ChevronUp }, p.weak);
                ui.painter().text(egui::pos2(x + 18.0, r.center().y), egui::Align2::LEFT_CENTER, format!("📁 {}", g.name), egui::FontId::proportional(13.5), p.text);
                if resp.clicked() {
                    actions.push(NavAction::ToggleGroup(ni, g.id));
                }
                resp.context_menu(|ui| {
                    if ui.button("Переименовать").clicked() {
                        actions.push(NavAction::Rename(RenameTarget::Group(g.id), g.name.clone()));
                        ui.close();
                    }
                    if ui.button("Удалить группу").clicked() {
                        actions.push(NavAction::DeleteGroup(g.id));
                        ui.close();
                    }
                });
                if !g.collapsed {
                    tree(ui, app, ni, &g.children, actions, depth + 1);
                }
            }
            Node::Section(s) => {
                let nb = &app.notebooks[ni];
                let selected = app.cur_section == Some(s.id) && app.cur_nb == Some(ni);
                let locked = s.lock.as_ref().map(|_| !nb.keys.contains_key(&s.id));
                let r = section_row(ui, &s.name, palette_color(s.color), selected, depth, locked);
                if r.clicked() {
                    actions.push(NavAction::SelectSection(ni, s.id));
                }
                if let Some(pd) = r.dnd_release_payload::<PageDrag>() {
                    actions.push(NavAction::DropPage(pd.page, ni, s.id));
                }
                if r.dnd_hover_payload::<PageDrag>().is_some() {
                    ui.painter().rect_stroke(r.rect, 2.0, egui::Stroke::new(2.0, p.accent), egui::StrokeKind::Inside);
                }
                r.context_menu(|ui| section_menu(ui, app, ni, s, actions));
            }
        }
    }
}

fn section_menu(ui: &mut Ui, app: &App, ni: usize, s: &Section, actions: &mut Vec<NavAction>) {
    if ui.button("Переименовать").clicked() {
        actions.push(NavAction::Rename(RenameTarget::Section(s.id), s.name.clone()));
        ui.close();
    }
    ui.menu_button("Цвет", |ui| {
        egui::Grid::new("pal").spacing(vec2(3.0, 3.0)).show(ui, |ui| {
            for i in 0..16u8 {
                let (r, resp) = ui.allocate_exact_size(vec2(20.0, 20.0), egui::Sense::click());
                ui.painter().rect_filled(r, 3.0, palette_color(i));
                if s.color == i {
                    ui.painter().rect_stroke(r, 3.0, egui::Stroke::new(2.0, Color32::BLACK), egui::StrokeKind::Inside);
                }
                if resp.clicked() {
                    actions.push(NavAction::Color(ni, s.id, i));
                    ui.close();
                }
                if i % 8 == 7 {
                    ui.end_row();
                }
            }
        });
    });
    if ui.button("Дублировать").clicked() {
        actions.push(NavAction::DuplicateSection(s.id));
        ui.close();
    }
    if ui.button("Объединить с другим разделом…").clicked() {
        actions.push(NavAction::MergeSection(s.id));
        ui.close();
    }
    ui.menu_button("Переместить в", |ui| {
        for (j, nb) in app.notebooks.iter().enumerate() {
            if nb.sealed {
                continue;
            }
            if ui.button(format!("📓 {}", nb.meta.name)).clicked() {
                actions.push(NavAction::MoveSection(s.id, j, None));
                ui.close();
            }
            fn groups(ui: &mut Ui, nodes: &[Node], j: usize, sid: Id, actions: &mut Vec<NavAction>, depth: usize) {
                for n in nodes {
                    if let Node::Group(g) = n {
                        if ui.button(format!("{}📁 {}", "   ".repeat(depth + 1), g.name)).clicked() {
                            actions.push(NavAction::MoveSection(sid, j, Some(g.id)));
                            ui.close();
                        }
                        groups(ui, &g.children, j, sid, actions, depth + 1);
                    }
                }
            }
            groups(ui, &nb.meta.tree, j, s.id, actions, 0);
        }
    });
    ui.menu_button("Шаблон новых страниц", |ui| {
        if ui.radio(s.default_template.is_none(), "Пустая страница").clicked() {
            actions.push(NavAction::DefaultTemplate(ni, s.id, None));
        }
        for t in crate::templates::BUILTIN.iter().map(|s| s.to_string()).chain(crate::templates::custom_list()) {
            if ui.radio(s.default_template.as_deref() == Some(&t), &t).clicked() {
                actions.push(NavAction::DefaultTemplate(ni, s.id, Some(t.clone())));
            }
        }
    });
    ui.separator();
    if s.lock.is_none() {
        if ui.button("Защитить паролем…").clicked() {
            actions.push(NavAction::SetSectionPw(s.id));
            ui.close();
        }
    } else {
        if ui.button("Заблокировать сейчас").clicked() {
            actions.push(NavAction::LockSection(ni, s.id));
            ui.close();
        }
        if ui.button("Снять пароль").clicked() {
            actions.push(NavAction::RemoveSectionPw(ni, s.id));
            ui.close();
        }
        let no_copy = s.lock.as_ref().is_some_and(|l| l.no_copy);
        if ui.checkbox(&mut no_copy.clone(), "Запретить копирование и печать").clicked() {
            actions.push(NavAction::NoCopy(ni, s.id));
            ui.close();
        }
    }
    ui.separator();
    if ui.button("Удалить раздел").clicked() {
        actions.push(NavAction::DeleteSection(s.id));
        ui.close();
    }
}

fn apply_nav(app: &mut App, ctx: &egui::Context, a: NavAction) {
    match a {
        NavAction::SelectNotebook(ni) => {
            if app.cur_nb != Some(ni) {
                app.open_page(None);
                app.cur_nb = Some(ni);
                app.cur_section = None;
                app.select_first_page();
            }
        }
        NavAction::SelectSection(ni, sid) => {
            app.open_page(None);
            app.cur_nb = Some(ni);
            app.cur_section = Some(sid);
            if app.notebooks[ni].section_accessible(sid) {
                app.select_first_page();
            }
        }
        NavAction::OpenNotebook => {
            if let Some(path) = rfd::FileDialog::new().set_title("Папка записной книжки").pick_folder() {
                app.open_notebook_folder(ctx, &path);
            }
        }
        NavAction::Properties(ni) => {
            let nb = &app.notebooks[ni];
            let (total, assets, pages) = nb.size_info();
            let lines = vec![
                format!("Папка: {}", nb.path.display()),
                format!("Страниц: {pages}"),
                format!("Общий размер: {}", crate::storage::human_size(total)),
                format!("Вложения: {}", crate::storage::human_size(assets)),
                format!("В корзине: {}", nb.meta.trash.len()),
                format!("Создан: {}", fmt_dt(&nb.meta.created)),
            ];
            app.dialogs.push(Dialog::Report(nb.meta.name.clone(), lines));
        }
        NavAction::UnlockNotebook(ni) => app.dialogs.push(Dialog::Password { target: PwTarget::UnlockNotebook(ni), text: String::new(), confirm: String::new(), error: String::new() }),
        NavAction::Rename(t, cur) => {
            let text = match &t {
                RenameTarget::Notebook(ni) => app.notebooks[*ni].meta.name.clone(),
                _ => cur,
            };
            app.dialogs.push(Dialog::Rename { target: t, text });
        }
        NavAction::NewSection(ni) => {
            app.cur_nb = Some(ni);
            app.new_section();
        }
        NavAction::NewGroup(ni) => {
            app.cur_nb = Some(ni);
            app.new_group();
        }
        NavAction::Color(ni, sid, c) => {
            if let Some(s) = app.notebooks[ni].meta.section_mut(sid) {
                s.color = c;
            }
            app.notebooks[ni].dirty = true;
        }
        NavAction::DeleteSection(sid) => app.dialogs.push(Dialog::Confirm { text: "Переместить раздел в корзину?".into(), action: ConfirmAction::DeleteSection(sid) }),
        NavAction::DeleteGroup(gid) => app.dialogs.push(Dialog::Confirm { text: "Удалить группу? Разделы попадут в корзину.".into(), action: ConfirmAction::DeleteGroup(gid) }),
        NavAction::DuplicateSection(sid) => app.duplicate_section(sid),
        NavAction::MergeSection(sid) => app.dialogs.push(Dialog::MergeSections { from: sid }),
        NavAction::MoveSection(sid, nb, g) => app.move_section(sid, nb, g),
        NavAction::SetSectionPw(sid) => app.dialogs.push(Dialog::Password { target: PwTarget::SetSection(sid), text: String::new(), confirm: String::new(), error: String::new() }),
        NavAction::RemoveSectionPw(ni, sid) => {
            if app.notebooks[ni].keys.contains_key(&sid) {
                app.save_all();
                app.notebooks[ni].remove_section_password(sid);
                app.set_status("Пароль снят");
            } else {
                app.set_status("Сначала разблокируйте раздел");
            }
        }
        NavAction::LockSection(ni, sid) => {
            app.save_all();
            app.notebooks[ni].keys.remove(&sid);
            let ids: Vec<Id> = app.notebooks[ni].meta.section(sid).map(|s| s.pages.iter().map(|p| p.id).collect()).unwrap_or_default();
            for id in ids {
                app.pages.remove(&id);
                app.editors.remove(&id);
                if app.cur_page == Some(id) {
                    app.cur_page = None;
                }
            }
        }
        NavAction::NoCopy(ni, sid) => {
            if let Some(l) = app.notebooks[ni].meta.section_mut(sid).and_then(|s| s.lock.as_mut()) {
                l.no_copy = !l.no_copy;
            }
            app.notebooks[ni].dirty = true;
        }
        NavAction::DefaultTemplate(ni, sid, t) => {
            if let Some(s) = app.notebooks[ni].meta.section_mut(sid) {
                s.default_template = t;
            }
            app.notebooks[ni].dirty = true;
        }
        NavAction::ToggleGroup(ni, gid) => {
            if let Some(g) = app.notebooks[ni].meta.group_mut(gid) {
                g.collapsed = !g.collapsed;
            }
            app.notebooks[ni].dirty = true;
        }
        NavAction::SetNotebookPw(ni) => app.dialogs.push(Dialog::Password { target: PwTarget::SetNotebook(ni), text: String::new(), confirm: String::new(), error: String::new() }),
        NavAction::Backups => app.dialogs.push(Dialog::Backups),
        NavAction::Trash => app.dialogs.push(Dialog::Trash),
        NavAction::Close(ni) => app.dialogs.push(Dialog::Confirm { text: "Закрыть блокнот? Файлы останутся на диске.".into(), action: ConfirmAction::CloseNotebook(ni) }),
        NavAction::DropPage(pid, ni, sid) => {
            if app.notebooks[ni].meta.section_of_page(pid) != Some(sid) {
                app.move_page(pid, ni, sid);
                app.rebuild_index(ctx);
            }
        }
        NavAction::ExportArchive(ni) => {
            let name = app.notebooks[ni].meta.name.clone();
            if let Some(p) = rfd::FileDialog::new().add_filter("Архив блокнота", &["nnbz"]).set_file_name(format!("{name}.nnbz")).save_file() {
                app.export_archive(ni, &p);
            }
        }
        NavAction::Integrity(ni) => {
            let r = app.notebooks[ni].check_integrity();
            let name = app.notebooks[ni].meta.name.clone();
            app.dialogs.push(Dialog::Report(format!("Проверка «{name}»"), r));
        }
    }
}

// ----------------------------------------------------------------------------
// Page list

fn page_list(app: &mut App, ui: &mut Ui) {
    use crate::icons::{self, Icon};
    let p = crate::chrome::cur(ui);
    let Some(ni) = app.cur_nb else { return };
    let Some(sid) = app.cur_section else {
        ui.add_space(12.0);
        ui.label(RichText::new("Выберите раздел").color(p.weak));
        return;
    };
    let Some(sec) = app.notebooks[ni].meta.section(sid).cloned() else { return };
    ui.add_space(6.0);
    if !app.notebooks[ni].section_accessible(sid) {
        ui.vertical_centered(|ui| {
            ui.add_space(30.0);
            ui.label(RichText::new("🔒 Раздел защищён паролем").color(p.weak));
            if ui.button("Разблокировать…").clicked() {
                app.dialogs.push(Dialog::Password { target: PwTarget::UnlockSection(sid), text: String::new(), confirm: String::new(), error: String::new() });
            }
        });
        return;
    }
    // Sort (with the filter inside its menu).
    let mut sort = sec.sort;
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        if !app.page_filter.trim().is_empty() {
            let chip = ui.add(egui::Button::new(RichText::new(format!("Фильтр: {}  ✕", app.page_filter.trim())).size(12.0)).corner_radius(10.0));
            if chip.on_hover_text("Сбросить фильтр").clicked() {
                app.page_filter.clear();
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(4.0);
            let (r2, resp) = ui.allocate_exact_size(vec2(30.0, 30.0), egui::Sense::click());
            if resp.hovered() {
                ui.painter().rect_filled(r2, 4.0, p.hover);
            }
            icons::paint(ui.painter(), egui::Rect::from_center_size(r2.center(), vec2(16.0, 16.0)), Icon::Sort, p.text);
            egui::Popup::menu(&resp.on_hover_text("Сортировка и фильтр страниц")).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
                ui.set_min_width(220.0);
                ui.add(egui::TextEdit::singleline(&mut app.page_filter).hint_text("🔍 Фильтр страниц").desired_width(210.0));
                ui.separator();
                ui.selectable_value(&mut sort, PageSort::Manual, "Вручную");
                ui.selectable_value(&mut sort, PageSort::Title, "По названию");
                ui.selectable_value(&mut sort, PageSort::Created, "По дате создания");
                ui.selectable_value(&mut sort, PageSort::Modified, "По дате изменения");
                ui.separator();
                if ui.checkbox(&mut app.settings.page_list_details, "Подробный список").changed() {
                    app.settings.save();
                }
            });
        });
    });
    if sort != sec.sort {
        if let Some(s) = app.notebooks[ni].meta.section_mut(sid) {
            s.sort = sort;
        }
        app.notebooks[ni].dirty = true;
    }
    ui.add_space(2.0);
    let bottom = add_row_rect(ui);
    if add_row(ui, bottom, "Добавить страницу", "Ctrl+N; Ctrl+Alt+Shift+N — подстраница").clicked() {
        app.new_page(false);
    }
    let pages: Vec<&PageMeta> = sorted_pages(&sec);
    let filter = app.page_filter.trim().to_lowercase();
    let details = app.settings.page_list_details;
    // Page text for empty titles and the detailed view: derived once per index change.
    let generation = app.index.generation();
    if app.snippets.0 != generation {
        let entries = app.index.entries.read().unwrap();
        let map = entries
            .iter()
            .map(|e| {
                let body: String = e.text.lines().skip(1).map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(" ");
                (e.page, body.chars().take(140).collect())
            })
            .collect();
        drop(entries);
        app.snippets = (generation, map);
    }
    let snippets = std::mem::take(&mut app.snippets.1);
    let mut open = None;
    let mut ops: Vec<(Id, &'static str)> = Vec::new();
    let mut reorder: Option<(Id, Id)> = None;
    let mut move_to: Option<(Id, usize, Id)> = None;
    egui::ScrollArea::vertical().id_salt("pages_scroll").max_height(ui.available_height() - bottom.height()).auto_shrink([false, true]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        let mut hidden_below: Option<u8> = None;
        for (i, pm) in pages.iter().enumerate() {
            if let Some(l) = hidden_below {
                if pm.level > l {
                    continue;
                }
                hidden_below = None;
            }
            let has_children = pages.get(i + 1).is_some_and(|n| n.level > pm.level) && sec.sort == PageSort::Manual;
            if pm.collapsed && has_children {
                hidden_below = Some(pm.level);
            }
            let snippet = snippets.get(&pm.id).cloned().unwrap_or_default();
            let title = if pm.title.trim().is_empty() || pm.title == "Без названия" {
                let first: String = snippet.chars().take(50).collect();
                if first.is_empty() { "Без заголовка".to_string() } else { first }
            } else {
                pm.title.clone()
            };
            if !filter.is_empty() && !title.to_lowercase().contains(&filter) && !snippet.to_lowercase().contains(&filter) {
                continue;
            }
            let selected = app.cur_page == Some(pm.id);
            let h = if details { 54.0 } else { 34.0 };
            // Rows outside the visible part of the list only take up space.
            let w = ui.available_width();
            if !ui.is_rect_visible(egui::Rect::from_min_size(ui.cursor().min, vec2(w, h))) {
                ui.allocate_space(vec2(w, h));
                continue;
            }
            let drag_id = egui::Id::new(("pdrag", pm.id));
            let row = ui.dnd_drag_source(drag_id, PageDrag { page: pm.id }, |ui| ui.allocate_exact_size(vec2(ui.available_width(), h), egui::Sense::click()));
            let (rect, r) = row.inner;
            let r = r.union(row.response.clone());
            if selected {
                ui.painter().rect_filled(rect, 0.0, p.sel);
                ui.painter().rect_filled(egui::Rect::from_min_size(rect.min, vec2(3.0, rect.height())), 0.0, p.accent);
            } else if r.hovered() {
                ui.painter().rect_filled(rect, 0.0, p.hover);
            }
            let mut x = rect.min.x + 14.0 + pm.level as f32 * 16.0;
            if has_children {
                let cr = egui::Rect::from_center_size(egui::pos2(x + 5.0, rect.min.y + 17.0), vec2(14.0, 14.0));
                icons::paint(ui.painter(), cr, if pm.collapsed { Icon::Chevron } else { Icon::ChevronUp }, p.weak);
                let cresp = ui.interact(cr.expand(3.0), egui::Id::new(("collapse", pm.id)), egui::Sense::click());
                if cresp.clicked() {
                    ops.push((pm.id, "collapse"));
                }
                x += 16.0;
            }
            if pm.pinned {
                ui.painter().text(egui::pos2(x, rect.min.y + 17.0), egui::Align2::LEFT_CENTER, "📌", egui::FontId::proportional(11.0), p.weak);
                x += 16.0;
            }
            let font = if selected { crate::fonts::font("", 13.5, true, false, false) } else { egui::FontId::proportional(13.5) };
            let clip = ui.painter().with_clip_rect(egui::Rect::from_min_max(egui::pos2(x, rect.min.y), egui::pos2(rect.max.x - 6.0, rect.max.y)));
            clip.text(egui::pos2(x, rect.min.y + 17.0), egui::Align2::LEFT_CENTER, &title, font, p.text);
            if details {
                let sub = format!("{}  {}", pm.modified.format("%d.%m.%Y"), snippet);
                clip.text(egui::pos2(x, rect.min.y + 38.0), egui::Align2::LEFT_CENTER, sub, egui::FontId::proportional(11.5), p.weak);
            }
            if r.clicked() {
                open = Some(pm.id);
            }
            if r.secondary_clicked() {
                open = Some(pm.id);
            }
            r.clone().on_hover_text(format!("Создана: {}\nИзменена: {}\nАвтор: {}", fmt_dt(&pm.created), fmt_dt(&pm.modified), pm.author));
            if let Some(d) = r.dnd_release_payload::<PageDrag>() {
                if d.page != pm.id {
                    reorder = Some((d.page, pm.id));
                }
            }
            if r.dnd_hover_payload::<PageDrag>().is_some() {
                ui.painter().hline(rect.x_range(), rect.top(), egui::Stroke::new(2.0, p.accent));
            }
            r.context_menu(|ui| {
                if ui.button("Новая страница").clicked() {
                    ops.push((pm.id, "new"));
                    ui.close();
                }
                if ui.button("Новая подстраница").clicked() {
                    ops.push((pm.id, "newsub"));
                    ui.close();
                }
                ui.separator();
                for (label, op) in [
                    ("Переименовать", "rename"),
                    (if pm.pinned { "Открепить" } else { "Закрепить вверху" }, "pin"),
                    ("Сделать подстраницей", "indent"),
                    ("Повысить уровень", "outdent"),
                    ("Дублировать", "dup"),
                    ("Копировать ссылку на страницу", "link"),
                    ("Закрепить в окне", "window"),
                    ("Версии страницы", "versions"),
                    ("Экспорт…", "export"),
                    ("Печать…", "print"),
                ] {
                    if ui.button(label).clicked() {
                        ops.push((pm.id, op));
                        ui.close();
                    }
                }
                ui.menu_button("Переместить или копировать", |ui| {
                    for (j, nb) in app.notebooks.iter().enumerate() {
                        if nb.sealed {
                            continue;
                        }
                        ui.label(RichText::new(&nb.meta.name).strong());
                        for s in nb.meta.sections() {
                            if s.id == sid || !nb.section_accessible(s.id) {
                                continue;
                            }
                            if ui.button(format!("   {}", s.name)).clicked() {
                                move_to = Some((pm.id, j, s.id));
                                ui.close();
                            }
                        }
                    }
                });
                ui.separator();
                if ui.button("Удалить").clicked() {
                    ops.push((pm.id, "delete"));
                    ui.close();
                }
            });
        }
    });
    app.snippets.1 = snippets;
    if let Some(pg) = open {
        app.open_page(Some(pg));
    }
    if let Some((what, before)) = reorder {
        if let Some(s) = app.notebooks[ni].meta.section_mut(sid) {
            if let Some(i) = s.pages.iter().position(|x| x.id == what) {
                let pm = s.pages.remove(i);
                let j = s.pages.iter().position(|x| x.id == before).unwrap_or(s.pages.len());
                s.pages.insert(j, pm);
                s.sort = PageSort::Manual;
            }
        }
        app.notebooks[ni].dirty = true;
    }
    if let Some((pid, j, s)) = move_to {
        app.move_page(pid, j, s);
    }
    for (pid, op) in ops {
        let ctx = ui.ctx().clone();
        match op {
            "new" => {
                app.open_page(Some(pid));
                app.new_page(false);
            }
            "newsub" => {
                app.open_page(Some(pid));
                app.new_page(true);
            }
            "rename" => {
                let t = app.notebooks[ni].meta.page_meta(pid).map(|x| x.title.clone()).unwrap_or_default();
                app.dialogs.push(Dialog::Rename { target: RenameTarget::Page(pid), text: t });
            }
            "pin" | "indent" | "outdent" | "collapse" => {
                if let Some(pm) = app.notebooks[ni].meta.page_meta_mut(pid) {
                    match op {
                        "pin" => pm.pinned = !pm.pinned,
                        "indent" => pm.level = (pm.level + 1).min(2),
                        "outdent" => pm.level = pm.level.saturating_sub(1),
                        _ => pm.collapsed = !pm.collapsed,
                    }
                }
                app.notebooks[ni].dirty = true;
            }
            "dup" => app.duplicate_page(pid),
            "link" => {
                ctx.copy_text(format!("note://page/{pid}"));
                app.set_status("Ссылка на страницу скопирована");
            }
            "window" => {
                app.open_page(Some(pid));
                app.pin_current(false);
            }
            "versions" => {
                app.open_page(Some(pid));
                app.right = Some(RightPanel::Versions);
            }
            "export" => {
                app.open_page(Some(pid));
                app.dialogs.push(Dialog::Print(PrintOpts { print: false, ..Default::default() }));
            }
            "print" => {
                app.open_page(Some(pid));
                app.dialogs.push(Dialog::Print(PrintOpts::default()));
            }
            "delete" => app.delete_page(pid),
            _ => {}
        }
    }
}

/// Recently opened pages (rail "Последние").
fn recent_list(app: &mut App, ui: &mut Ui) {
    let p = crate::chrome::cur(ui);
    ui.add_space(8.0);
    ui.label(RichText::new("Последние заметки").strong().size(15.0));
    ui.add_space(6.0);
    let mut open = None;
    egui::ScrollArea::vertical().id_salt("recent").show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for pid in app.recent.clone() {
            let Some(nb) = app.notebooks.iter().find(|n| n.meta.page_meta(pid).is_some()) else { continue };
            let pm = nb.meta.page_meta(pid).cloned().unwrap();
            let sec = nb.meta.section_of_page(pid).and_then(|s| nb.meta.section(s)).map(|s| (s.name.clone(), s.color)).unwrap_or_default();
            let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 46.0), egui::Sense::click());
            if app.cur_page == Some(pid) {
                ui.painter().rect_filled(r, 0.0, p.sel);
            } else if resp.hovered() {
                ui.painter().rect_filled(r, 0.0, p.hover);
            }
            ui.painter().rect_filled(egui::Rect::from_min_size(r.min + vec2(10.0, 10.0), vec2(5.0, 26.0)), 2.5, palette_color(sec.1));
            let title = if pm.title.trim().is_empty() { "Без заголовка".to_string() } else { pm.title };
            let clip = ui.painter().with_clip_rect(r);
            clip.text(r.min + vec2(24.0, 15.0), egui::Align2::LEFT_CENTER, title, egui::FontId::proportional(13.5), p.text);
            clip.text(r.min + vec2(24.0, 33.0), egui::Align2::LEFT_CENTER, format!("{} › {} · {}", nb.meta.name, sec.0, pm.modified.format("%d.%m %H:%M")), egui::FontId::proportional(11.0), p.weak);
            if resp.clicked() {
                open = Some(pid);
            }
        }
    });
    if let Some(pid) = open {
        app.open_page(Some(pid));
    }
}

/// Coloured section tabs above the page (desktop OneNote layout).
fn section_tabs(app: &mut App, ui: &mut Ui) {
    let p = crate::chrome::cur(ui);
    let Some(ni) = app.cur_nb else { return };
    if app.notebooks[ni].sealed {
        return;
    }
    let secs: Vec<Section> = app.notebooks[ni].meta.sections().into_iter().cloned().collect();
    let mut actions: Vec<NavAction> = Vec::new();
    let bg = ui.visuals().panel_fill;
    egui::ScrollArea::horizontal().id_salt("sec_tabs").scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.add_space(8.0);
            for s in &secs {
                let selected = app.cur_section == Some(s.id);
                let base = palette_color(s.color);
                let fill = if selected { base } else { base.linear_multiply(0.45).blend(bg.linear_multiply(0.55)) };
                let font = if selected { crate::fonts::font("", 13.0, true, false, false) } else { egui::FontId::proportional(13.0) };
                let g = ui.painter().layout_no_wrap(s.name.clone(), font, Color32::from_gray(25));
                let (r, resp) = ui.allocate_exact_size(vec2(g.size().x + 26.0, 28.0), egui::Sense::click());
                ui.painter().rect_filled(r, egui::CornerRadius { nw: 5, ne: 5, sw: 0, se: 0 }, fill);
                if resp.hovered() && !selected {
                    ui.painter().rect_stroke(r, egui::CornerRadius { nw: 5, ne: 5, sw: 0, se: 0 }, egui::Stroke::new(1.0, base), egui::StrokeKind::Inside);
                }
                ui.painter().galley(egui::pos2(r.center().x - g.size().x / 2.0, r.center().y - g.size().y / 2.0), g, Color32::from_gray(25));
                if resp.clicked() {
                    actions.push(NavAction::SelectSection(ni, s.id));
                }
                if let Some(pd) = resp.dnd_release_payload::<PageDrag>() {
                    actions.push(NavAction::DropPage(pd.page, ni, s.id));
                }
                resp.context_menu(|ui| section_menu(ui, app, ni, s, &mut actions));
            }
            let (r, resp) = ui.allocate_exact_size(vec2(28.0, 28.0), egui::Sense::click());
            if resp.hovered() {
                ui.painter().rect_filled(r, 4.0, p.hover);
            }
            crate::icons::paint(ui.painter(), egui::Rect::from_center_size(r.center(), vec2(14.0, 14.0)), crate::icons::Icon::Plus, p.text);
            if resp.on_hover_text("Новый раздел (Ctrl+T)").clicked() {
                actions.push(NavAction::NewSection(ni));
            }
        });
    });
    for a in actions {
        apply_nav(app, ui.ctx(), a);
    }
}

// ----------------------------------------------------------------------------
// Central area

fn central(app: &mut App, ui: &mut Ui, ctx: &egui::Context) {
    let Some(pid) = app.cur_page else {
        ui.centered_and_justified(|ui| {
            let locked = app.cur_section.zip(app.cur_nb).is_some_and(|(s, n)| !app.notebooks[n].section_accessible(s));
            if locked {
                ui.vertical_centered(|ui| {
                    ui.add_space(80.0);
                    ui.label(RichText::new("🔒 Раздел защищён паролем").size(20.0));
                    if ui.button("Разблокировать…").clicked() {
                        if let Some(s) = app.cur_section {
                            app.dialogs.push(Dialog::Password { target: PwTarget::UnlockSection(s), text: String::new(), confirm: String::new(), error: String::new() });
                        }
                    }
                });
            } else {
                ui.label(RichText::new("Выберите или создайте страницу (Ctrl+N)").size(16.0).weak());
            }
        });
        return;
    };
    if !app.ensure_loaded(pid) {
        ui.centered_and_justified(|ui| ui.label("Страница недоступна"));
        return;
    }
    let Some(ni) = app.nb_index_of_page(pid) else { return };
    let dark = ui.visuals().dark_mode;
    let bg = canvas::page_bg(app.pages.get(&pid).unwrap(), dark);
    // The whole page area uses the page colour.
    ui.painter().rect_filled(ui.max_rect(), 0.0, bg);
    // Header with title.
    if !app.presentation {
        egui::Frame::new().fill(bg).inner_margin(egui::Margin { left: 60, right: 12, top: 26, bottom: 6 }).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            let mut title = app.notebooks[ni].meta.page_meta(pid).map(|p| p.title.clone()).unwrap_or_default();
            let tc = if dark { Color32::from_gray(230) } else { Color32::from_gray(30) };
            let title_w = (ui.available_width() - 40.0).clamp(200.0, 560.0);
            let r = ui.add(
                egui::TextEdit::singleline(&mut title)
                    .font(egui::FontId::proportional(26.0))
                    .text_color(tc)
                    .frame(egui::Frame::NONE)
                    .hint_text("")
                    .desired_width(title_w),
            );
            // OneNote-style underline under the title.
            let ly = r.rect.max.y + 3.0;
            ui.painter().hline(r.rect.min.x..=r.rect.min.x + title_w, ly, egui::Stroke::new(1.0, if dark { Color32::from_gray(90) } else { Color32::from_gray(190) }));
            ui.add_space(8.0);
            if r.changed() {
                if let Some(pm) = app.notebooks[ni].meta.page_meta_mut(pid) {
                    pm.title = title;
                }
                app.mark_dirty(pid);
            }
            let meta = app.notebooks[ni].meta.page_meta(pid).cloned();
            ui.horizontal(|ui| {
                if let Some(m) = meta {
                    let wc = if dark { Color32::from_gray(150) } else { Color32::from_gray(110) };
                    ui.label(RichText::new(crate::chrome::long_date(&m.created)).color(wc).size(19.0));
                    ui.add_space(26.0);
                    ui.label(RichText::new(m.created.format("%-H:%M").to_string()).color(wc).size(19.0));
                    if !m.author.is_empty() {
                        ui.add_space(14.0);
                        ui.label(RichText::new(&m.author).color(wc).small());
                    }
                }
                if let Some(who) = &app.locked_by {
                    ui.label(RichText::new(format!("⚠ Страницу сейчас редактирует {who} — открыта только для чтения")).color(Color32::from_rgb(220, 120, 0)));
                }
                if app.current_no_copy() {
                    ui.label(RichText::new("🚫 копирование запрещено").small().color(Color32::from_rgb(200, 60, 60)));
                }
            });
        });
        find_bar(app, ui, pid);
    }
    // Canvas.
    let highlights = compute_highlights(app, pid);
    let readonly = app.locked_by.is_some() || app.presentation;
    let no_copy = app.current_no_copy();
    let mut actions = Vec::new();
    let changed = {
        let App { pages, editors, notebooks, textures, speller, settings, clip, .. } = app;
        let (Some(page), Some(ed)) = (pages.get_mut(&pid), editors.get_mut(&pid)) else { return };
        ed.readonly = readonly;
        let mut env = canvas::Env {
            settings,
            nb: &notebooks[ni],
            textures,
            speller,
            dark,
            actions: &mut actions,
            clip,
            highlights: &highlights,
            no_copy,
        };
        canvas::show(ui, page, ed, &mut env)
    };
    if changed && !readonly {
        app.mark_dirty(pid);
    }
    for a in actions {
        app.handle_action(ctx, a);
    }
}

fn compute_highlights(app: &App, pid: Id) -> canvas::Highlights {
    let mut h = canvas::Highlights::new();
    let Some(ed) = app.editors.get(&pid) else { return h };
    let q = if ed.find.open && !ed.find.query.is_empty() {
        Some((ed.find.query.clone(), ed.find.case, ed.find.whole))
    } else if app.right == Some(RightPanel::Search) && !app.search_q.trim().is_empty() {
        Some((app.search_q.trim().to_owned(), false, false))
    } else {
        None
    };
    let Some((q, case, whole)) = q else { return h };
    if let Some(page) = app.pages.get(&pid) {
        for it in &page.items {
            if let ItemKind::Text(t) = &it.kind {
                let m = richtext::find_all(&t.text, &q, case, whole);
                if !m.is_empty() {
                    h.insert(it.id, m);
                }
            }
        }
    }
    h
}

fn find_bar(app: &mut App, ui: &mut Ui, pid: Id) {
    let open = app.editors.get(&pid).is_some_and(|e| e.find.open);
    if !open {
        return;
    }
    let mut replace_all = false;
    let mut replace_one = false;
    let mut close = false;
    let mut goto: Option<i32> = None;
    let total: usize = compute_highlights(app, pid).values().map(|v| v.len()).sum();
    if let Some(ed) = app.editors.get_mut(&pid) {
        ui.horizontal(|ui| {
            ui.add_space(20.0);
            let r = ui.add(egui::TextEdit::singleline(&mut ed.find.query).hint_text("Найти на странице").desired_width(200.0));
            if ui.memory(|m| m.focused().is_none()) {
                r.request_focus();
            }
            ui.label(format!("{total} совп."));
            if ui.small_button("▲").clicked() {
                goto = Some(-1);
            }
            if ui.small_button("▼").clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                goto = Some(1);
            }
            ui.checkbox(&mut ed.find.case, "Регистр");
            ui.checkbox(&mut ed.find.whole, "Слово целиком");
            ui.separator();
            ui.add(egui::TextEdit::singleline(&mut ed.find.replace).hint_text("Заменить на").desired_width(150.0));
            if ui.button("Заменить").clicked() {
                replace_one = true;
            }
            if ui.button("Заменить все").clicked() {
                replace_all = true;
            }
            if ui.button("✕").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                close = true;
            }
        });
    }
    if let Some(dir) = goto {
        let hl = compute_highlights(app, pid);
        let (page, ed) = (app.pages.get(&pid), app.editors.get(&pid));
        if let (Some(page), Some(ed)) = (page, ed) {
            let mut items: Vec<&Item> = page.items.iter().filter(|i| hl.contains_key(&i.id)).collect();
            items.sort_by(|a, b| a.pos[1].partial_cmp(&b.pos[1]).unwrap_or(std::cmp::Ordering::Equal));
            if !items.is_empty() {
                let n = items.len() as i32;
                let cur = (ed.find.current as i32 + dir).rem_euclid(n) as usize;
                let pos = items[cur].pos2();
                if let Some(ed) = app.editors.get_mut(&pid) {
                    ed.find.current = cur;
                    ed.scroll_to = Some(pos);
                }
            }
        }
    }
    if replace_one || replace_all {
        let (q, rep, case, whole) = app.editors.get(&pid).map(|e| (e.find.query.clone(), e.find.replace.clone(), e.find.case, e.find.whole)).unwrap_or_default();
        let mut count = 0;
        app.with_page(|page, ed| {
            ed.snapshot(page);
            'outer: for it in page.items.iter_mut() {
                if let ItemKind::Text(t) = &mut it.kind {
                    loop {
                        let m = richtext::find_all(&t.text, &q, case, whole);
                        let Some(&(a, b)) = m.first() else { break };
                        let (ba, bb) = (richtext::char_to_byte(&t.text, a), richtext::char_to_byte(&t.text, b));
                        t.text.replace_range(ba..bb, &rep);
                        count += 1;
                        if !replace_all {
                            break 'outer;
                        }
                        if rep.contains(&q) {
                            // Avoid endless loops when the replacement contains the query.
                            break;
                        }
                    }
                }
            }
        });
        if count > 0 {
            app.mark_dirty(pid);
        }
        app.set_status(format!("Заменено: {count}"));
    }
    if close {
        if let Some(ed) = app.editors.get_mut(&pid) {
            ed.find.open = false;
        }
    }
}

/// Without the status bar, messages pop up at the bottom of the window.
fn status_toast(app: &App, ctx: &egui::Context) {
    let age = now_secs() - app.status.1;
    if app.status.0.is_empty() || age > 6.0 {
        return;
    }
    let p = crate::chrome::pal(ctx.global_style().visuals.dark_mode);
    let alpha = ((6.0 - age) / 0.6).clamp(0.0, 1.0) as f32;
    egui::Area::new(egui::Id::new("status_toast"))
        .anchor(egui::Align2::CENTER_BOTTOM, vec2(0.0, -28.0))
        .interactable(false)
        .order(egui::Order::Tooltip)
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(p.ribbon_bg.gamma_multiply(alpha))
                .stroke(egui::Stroke::new(1.0, p.border.gamma_multiply(alpha)))
                .corner_radius(6.0)
                .inner_margin(egui::Margin::symmetric(14, 8))
                .shadow(egui::epaint::Shadow { offset: [0, 2], blur: 10, spread: 0, color: Color32::from_black_alpha((60.0 * alpha) as u8) })
                .show(ui, |ui| ui.label(RichText::new(&app.status.0).color(p.text.gamma_multiply(alpha))));
        });
    ctx.request_repaint_after(std::time::Duration::from_millis(100));
}

fn status_bar(app: &mut App, ui: &mut Ui) {
    let p = crate::chrome::cur(ui);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 14.0;
        let small = |t: String| RichText::new(t).size(12.0).color(p.weak);
        let t = now_secs();
        let save = if !app.dirty.is_empty() { "● Сохранение…" } else { "✔ Сохранено" };
        ui.label(small(save.into()));
        // Words and characters of the selection or page.
        if let Some(page) = app.cur_page.and_then(|pid| app.pages.get(&pid)) {
            let ed = app.cur_page.and_then(|pid| app.editors.get(&pid));
            let text = match ed.and_then(|e| e.editing.or_else(|| e.selection.items.first().copied())).and_then(|id| page.item(id)) {
                Some(it) => it.plain_text(),
                None if page.items.len() < 400 => page.plain_text(),
                None => String::new(),
            };
            let words = text.split_whitespace().count();
            let chars = text.chars().filter(|c| !c.is_whitespace()).count();
            ui.label(small(format!("Слов: {words} · Символов: {chars}")));
        }
        ui.label(small(app.settings.spell_lang.clone())).on_hover_text("Язык проверки правописания");
        let any_locked = app.notebooks.iter().any(|nb| nb.meta.sections().iter().any(|s| s.lock.is_some()));
        if any_locked && ui.add(egui::Label::new(small("🔒 Заблокировать".into())).sense(egui::Sense::click())).on_hover_text("Ctrl+Alt+L").clicked() {
            app.lock_all();
        }
        if t - app.status.1 < 8.0 {
            ui.label(RichText::new(&app.status.0).size(12.0));
        } else if app.index.building.load(std::sync::atomic::Ordering::Relaxed) {
            ui.label(small("Индексация…".into()));
        } else if app.export.is_some() {
            ui.label(small("Экспорт…".into()));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            if app.settings.offline_only {
                ui.label(small("⛔ Офлайн".into())).on_hover_text("Только офлайн: программа не обращается к сети");
            }
            if let Some(pid) = app.cur_page {
                if let Some(ed) = app.editors.get_mut(&pid) {
                    let mut pct = ed.zoom * 100.0;
                    ui.label(small(format!("{:.0}%", pct)));
                    if ui.add(egui::Slider::new(&mut pct, 10.0..=800.0).logarithmic(true).show_value(false)).changed() {
                        let r = ed.last_rect;
                        ed.zoom_at(r, r.center(), pct / 100.0 / ed.zoom);
                    }
                }
            }
            let mut save = false;
            for (on, icon, tip) in [(&mut app.settings.mouse_draws, "Мышь", "Мышь рисует"), (&mut app.settings.touch_draws, "Палец", "Палец рисует"), (&mut app.settings.pen_draws, "Перо", "Перо рисует")] {
                let text = RichText::new(icon).size(12.0).color(if *on { p.accent_text } else { p.weak });
                if ui.add(egui::Label::new(text).sense(egui::Sense::click())).on_hover_text(tip).clicked() {
                    *on = !*on;
                    save = true;
                }
            }
            if save {
                app.settings.save();
            }
            if crate::pen::pen_in_range() {
                ui.label(small("Перо рядом".into()));
            }
            ui.label(small(format!("👤 {}", app.settings.author)));
            if app.presentation {
                ui.label(small("Esc — выход из презентации".into()));
            }
        });
    });
}

impl App {
    pub fn paste_from_clipboard(&mut self, ctx: &egui::Context) {
        // Ignore the internal clipboard: take what the system clipboard has.
        let ours = std::mem::take(&mut self.clip);
        self.paste_on_canvas(ctx, None);
        self.clip = ours;
    }

    pub fn run_backups_now(&mut self) {
        self.run_backups(true);
    }
}
