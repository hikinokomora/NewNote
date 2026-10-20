//! Main window layout: menus, ribbon, notebook tree, page list, page header,
//! find bar, canvas and status bar.

use egui::{Color32, RichText, Ui, vec2};

use crate::app::*;
use crate::canvas::{self, EraserMode, Tool};
use crate::ink::ShapeKind;
use crate::items::FormatCmd;
use crate::model::*;
use crate::richtext;

pub fn draw(app: &mut App, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    let hide = app.focus_mode || app.presentation;
    if !hide {
        egui::Panel::top("menu").show(ui, |ui| menu_bar(app, ui));
        egui::Panel::top("ribbon").show(ui, |ui| ribbon(app, ui));
    }
    if !app.presentation {
        egui::Panel::bottom("status").show(ui, |ui| status_bar(app, ui));
    }
    if !hide {
        egui::Panel::left("nav").resizable(true).default_size(230.0).show(ui, |ui| nav_tree(app, ui));
        egui::Panel::left("pages").resizable(true).default_size(220.0).show(ui, |ui| page_list(app, ui));
    }
    if let (Some(_), false) = (app.right, app.presentation) {
        egui::Panel::right("right").resizable(true).default_size(330.0).show(ui, |ui| crate::ui_panels::draw(app, ui));
    }
    egui::CentralPanel::no_frame().show(ui, |ui| central(app, ui, &ctx));
}

fn menu_bar(app: &mut App, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    egui::MenuBar::new().ui(ui, |ui| {
        ui.menu_button("Файл", |ui| {
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
            if ui.button("Импорт (HTML, MD, TXT, ENEX, DOCX, PDF, CSV, XLSX)…").clicked() {
                if let Some(files) = rfd::FileDialog::new()
                    .add_filter("Документы", &["html", "htm", "md", "markdown", "txt", "enex", "docx", "pdf", "csv", "xlsx", "xls", "ods"])
                    .pick_files()
                {
                    for f in files {
                        app.import_document(&ctx, &f);
                    }
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
        });
        ui.menu_button("Правка", |ui| {
            if ui.button("Отменить   Ctrl+Z").clicked() {
                if let Some(p) = app.cur_page {
                    if app.with_page(|pg, ed| ed.undo(pg)) == Some(true) {
                        app.mark_dirty(p);
                    }
                }
                ui.close();
            }
            if ui.button("Повторить   Ctrl+Y").clicked() {
                if let Some(p) = app.cur_page {
                    if app.with_page(|pg, ed| ed.redo(pg)) == Some(true) {
                        app.mark_dirty(p);
                    }
                }
                ui.close();
            }
            ui.separator();
            if ui.button("Найти на странице   Ctrl+F").clicked() {
                app.with_page(|_, ed| ed.find.open = true);
                ui.close();
            }
            if ui.button("Поиск везде   Ctrl+E").clicked() {
                app.right = Some(RightPanel::Search);
                app.search_focus = true;
                ui.close();
            }
            if ui.button("Выделить всё на странице").clicked() {
                app.with_page(|pg, ed| {
                    ed.selection.strokes = (0..pg.strokes.len()).collect();
                    ed.selection.items = pg.items.iter().map(|i| i.id).collect();
                });
                ui.close();
            }
        });
        ui.menu_button("Вставка", |ui| insert_menu(app, ui));
        ui.menu_button("Страница", |ui| page_menu(app, ui));
        ui.menu_button("Вид", |ui| view_menu(app, ui));
        ui.menu_button("Инструменты", |ui| {
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
        });
    });
}

fn export_menu(app: &mut App, ui: &mut Ui, section: bool) {
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
            for b in Background::ALL {
                if ui.radio(page.background == b, b.label()).clicked() {
                    page.background = b;
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
// Ribbon

fn ribbon(app: &mut App, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    ui.horizontal(|ui| {
        for (r, l) in [(Ribbon::Home, "Главная"), (Ribbon::Insert, "Вставка"), (Ribbon::Draw, "Рисование"), (Ribbon::View, "Вид")] {
            if ui.selectable_label(app.ribbon == r, RichText::new(l).strong()).clicked() {
                app.ribbon = r;
                if r == Ribbon::Draw {
                    app.with_page(|_, ed| {
                        if !ed.tool.is_ink() {
                            ed.tool = Tool::Pen;
                        }
                    });
                }
            }
        }
    });
    ui.separator();
    egui::ScrollArea::horizontal().id_salt("ribbon_scroll").show(ui, |ui| {
        ui.horizontal(|ui| match app.ribbon {
            Ribbon::Home => home_ribbon(app, ui, &ctx),
            Ribbon::Insert => insert_ribbon(app, ui, &ctx),
            Ribbon::Draw => draw_ribbon(app, ui),
            Ribbon::View => view_ribbon(app, ui, &ctx),
        });
    });
}

fn tbtn(ui: &mut Ui, text: &str, tip: &str) -> bool {
    ui.button(text).on_hover_text(tip).clicked()
}

fn home_ribbon(app: &mut App, ui: &mut Ui, ctx: &egui::Context) {
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
    let shown = if font.is_empty() { "Segoe UI".to_string() } else { font.clone() };
    egui::ComboBox::from_id_salt("font").width(130.0).selected_text(shown).show_ui(ui, |ui| {
        for f in crate::fonts::font_names() {
            if ui.selectable_label(f == font, RichText::new(&f).font(crate::fonts::font(&f, 14.0, false, false, false))).clicked() {
                app.format(ctx, FormatCmd::Font(f));
            }
        }
    });
    egui::ComboBox::from_id_salt("size").width(48.0).selected_text(format!("{size:.0}")).show_ui(ui, |ui| {
        for s in [8.0, 9.0, 10.0, 11.0, 12.0, 14.0, 15.0, 16.0, 18.0, 20.0, 24.0, 28.0, 32.0, 36.0, 48.0, 72.0] {
            if ui.selectable_label(s == size, format!("{s:.0}")).clicked() {
                app.format(ctx, FormatCmd::Size(s));
            }
        }
    });
    ui.separator();
    for (t, tip, cmd) in [
        ("B", "Жирный (Ctrl+B)", FormatCmd::Wrap("**", "**")),
        ("I", "Курсив (Ctrl+I)", FormatCmd::Wrap("*", "*")),
        ("U", "Подчёркнутый (Ctrl+U)", FormatCmd::Wrap("__", "__")),
        ("S", "Зачёркнутый (Ctrl+-)", FormatCmd::Wrap("~~", "~~")),
        ("x²", "Верхний индекс (Ctrl+Shift+=)", FormatCmd::Wrap("^", "^")),
        ("x₂", "Нижний индекс (Ctrl+=)", FormatCmd::Wrap(",,", ",,")),
    ] {
        let rt = match t {
            "B" => RichText::new(t).strong(),
            "I" => RichText::new(t).italics(),
            "U" => RichText::new(t).underline(),
            "S" => RichText::new(t).strikethrough(),
            _ => RichText::new(t),
        };
        if ui.button(rt).on_hover_text(tip).clicked() {
            app.format(ctx, cmd);
        }
    }
    ui.menu_button(RichText::new("A").color(Color32::from_rgb(210, 40, 40)), |ui| {
        ui.label("Цвет текста");
        color_grid(ui, |c| app.format(ctx, FormatCmd::Color(c)));
    })
    .response
    .on_hover_text("Цвет текста");
    ui.menu_button(RichText::new("🖍").background_color(Color32::from_rgb(255, 240, 120)), |ui| {
        ui.label("Цвет выделения");
        if ui.button("Жёлтое выделение").clicked() {
            app.format(ctx, FormatCmd::Wrap("==", "=="));
        }
        color_grid(ui, |c| app.format(ctx, FormatCmd::WrapOwned(format!("{{h:#{:02x}{:02x}{:02x}|", c[0], c[1], c[2]), "}".into())));
    })
    .response
    .on_hover_text("Цвет выделения");
    if tbtn(ui, "⌫A", "Очистить форматирование (Ctrl+Пробел)") {
        app.format(ctx, FormatCmd::Clear);
    }
    let painter_on = app.cur_page.and_then(|p| app.editors.get(&p)).is_some_and(|e| e.format_painter.is_some());
    if ui.selectable_label(painter_on, "🖌").on_hover_text("Формат по образцу: поставьте курсор в образец, нажмите кнопку, затем щёлкните по блоку").clicked() {
        let st = app.cur_page.and_then(|p| Some(crate::items::style_under_cursor(ctx, app.pages.get(&p)?, app.editors.get(&p)?))).flatten();
        app.with_page(|_, ed| {
            ed.format_painter = if painter_on { None } else { st };
            ed.editing = None;
        });
    }
    ui.separator();
    egui::ComboBox::from_id_salt("style").width(110.0).selected_text("Стиль").show_ui(ui, |ui| {
        let styles: [(&str, &str); 10] = [
            ("Обычный текст", ""),
            ("Заголовок 1", "# "),
            ("Заголовок 2", "## "),
            ("Заголовок 3", "### "),
            ("Заголовок 4", "#### "),
            ("Заголовок 5", "##### "),
            ("Заголовок 6", "###### "),
            ("Цитата", "> "),
            ("Примечание", "!> "),
            ("Блок кода", "```"),
        ];
        for (l, p) in styles {
            if ui.selectable_label(false, l).clicked() {
                if p == "```" {
                    app.format(ctx, FormatCmd::WrapOwned("```\n".into(), "\n```".into()));
                } else {
                    app.format(ctx, FormatCmd::LinePrefix(p.into()));
                }
            }
        }
    });
    if tbtn(ui, "•", "Маркированный список (Ctrl+.)") {
        app.format(ctx, FormatCmd::LinePrefix("- ".into()));
    }
    if tbtn(ui, "1.", "Нумерованный список (Ctrl+/)") {
        app.format(ctx, FormatCmd::LinePrefix("1. ".into()));
    }
    if tbtn(ui, "☐", "Список с флажками") {
        app.format(ctx, FormatCmd::LinePrefix("[ ] ".into()));
    }
    if tbtn(ui, "⇤", "Уменьшить отступ") {
        app.format(ctx, FormatCmd::Indent(true));
    }
    if tbtn(ui, "⇥", "Увеличить отступ") {
        app.format(ctx, FormatCmd::Indent(false));
    }
    ui.separator();
    for (t, tip, a) in [("⯇", "По левому краю", Align::Left), ("≡", "По центру", Align::Center), ("⯈", "По правому краю", Align::Right), ("☰", "По ширине", Align::Justify)] {
        if tbtn(ui, t, tip) {
            app.format(ctx, FormatCmd::Align(a));
        }
    }
    ui.menu_button("↕", |ui| {
        ui.label("Межстрочный интервал");
        for s in [1.0, 1.15, 1.5, 2.0, 2.5] {
            if ui.button(format!("{s}")).clicked() {
                app.format(ctx, FormatCmd::Spacing(s));
                ui.close();
            }
        }
        ui.separator();
        if ui.button("Справа налево / слева направо").clicked() {
            app.format(ctx, FormatCmd::Rtl);
            ui.close();
        }
        let pid = app.cur_page;
        if let Some(pid) = pid {
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
    ui.separator();
    ui.menu_button("🏷 Теги", |ui| {
        for t in app.settings.tags.clone() {
            let label = match t.shortcut {
                Some(d) => format!("{} {}   Ctrl+{d}", t.icon, t.name),
                None => format!("{} {}", t.icon, t.name),
            };
            if ui.button(label).clicked() {
                app.toggle_tag_current(&t);
                ui.close();
            }
        }
        ui.separator();
        if ui.button("Настроить теги…").clicked() {
            app.dialogs.push(Dialog::Tags);
            ui.close();
        }
    });
    if tbtn(ui, "🔎", "Найти на странице (Ctrl+F)") {
        app.with_page(|_, ed| ed.find.open = true);
    }
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

fn insert_ribbon(app: &mut App, ui: &mut Ui, ctx: &egui::Context) {
    if tbtn(ui, "▦ Таблица", "Вставить таблицу") {
        app.dialogs.push(Dialog::Table { rows: 3, cols: 3 });
    }
    if tbtn(ui, "🖼 Изображение", "Из файла") {
        pick_images(app);
    }
    if tbtn(ui, "📋 Из буфера", "Вставить изображение или текст из буфера обмена") {
        app.paste_from_clipboard(ctx);
    }
    if tbtn(ui, "✂ Скриншот", "Снимок области экрана (Win+Shift+S)") {
        app.start_screenshot(ctx);
    }
    if tbtn(ui, "📎 Файл", "Вложение") {
        pick_files(app);
    }
    if tbtn(ui, "📕 PDF", "Печатная копия PDF") {
        if let Some(p) = rfd::FileDialog::new().add_filter("PDF", &["pdf"]).pick_file() {
            app.pdf_printout_from(p, [0.0, 0.0]);
        }
    }
    ui.separator();
    if tbtn(ui, "🔗 Ссылка", "Ctrl+K") {
        app.dialogs.push(Dialog::Link { text: String::new(), url: String::new() });
    }
    if tbtn(ui, "∑ Формула", "Редактор формул и решение уравнений") {
        app.insert_item(ItemKind::Equation(String::new()), 420.0);
    }
    if tbtn(ui, "{} Код", "Блок кода с подсветкой") {
        app.insert_item(ItemKind::Code(CodeItem { lang: "rust".into(), code: String::new() }), 560.0);
    }
    if tbtn(ui, "⧉ Диаграмма", "Ментальная карта или блок-схема") {
        app.insert_item(ItemKind::Diagram(Diagram { kind: DiagramKind::MindMap, source: "Тема\n  Идея 1\n  Идея 2".into() }), 560.0);
    }
    ui.separator();
    if tbtn(ui, "📅 Дата", "Alt+Shift+D") {
        app.insert_text_at_cursor(ctx, chrono::Local::now().format("%d.%m.%Y").to_string());
    }
    if tbtn(ui, "🕑 Время", "Alt+Shift+T") {
        app.insert_text_at_cursor(ctx, chrono::Local::now().format("%H:%M").to_string());
    }
    if tbtn(ui, "Ω Символ", "Специальные знаки и эмодзи") {
        app.dialogs.push(Dialog::Symbols);
    }
    if tbtn(ui, "― Линия", "Горизонтальная линия") {
        app.insert_item(ItemKind::Divider { page_break: false }, 600.0);
        app.with_page(|_, ed| ed.editing = None);
    }
}

fn draw_ribbon(app: &mut App, ui: &mut Ui) {
    let presets = app.settings.pen_presets.clone();
    let Some(pid) = app.cur_page else {
        ui.label("Откройте страницу");
        return;
    };
    let mut save_settings = false;
    let settings = &mut app.settings;
    let Some(ed) = app.editors.get_mut(&pid) else { return };
    for (t, label, tip) in [
        (Tool::Select, "⬉ Выбор", "Выделение и ввод текста"),
        (Tool::Text, "T Текст", "Щелчок создаёт текстовый блок"),
        (Tool::Lasso, "◌ Лассо", "Выделение рукописных объектов (или кнопка пера)"),
        (Tool::Pan, "✋ Прокрутка", "Перемещение по холсту"),
    ] {
        if ui.selectable_label(ed.tool == t, label).on_hover_text(tip).clicked() {
            ed.tool = t;
        }
    }
    ui.separator();
    if ui.selectable_label(ed.tool == Tool::Pen, "✒ Перо").clicked() {
        ed.tool = Tool::Pen;
    }
    for p in presets.iter().filter(|p| p.favorite) {
        let c = Color32::from_rgba_unmultiplied(p.color[0], p.color[1], p.color[2], 255);
        let active = (ed.tool == Tool::Pen && !p.highlighter && ed.pen_color == p.color && ed.pen_width == p.width)
            || (ed.tool == Tool::Highlighter && p.highlighter && ed.hl_color == p.color && ed.hl_width == p.width);
        if ui.selectable_label(active, RichText::new(if p.highlighter { "▮" } else { "●" }).color(c)).on_hover_text(&p.name).clicked() {
            if p.highlighter {
                ed.tool = Tool::Highlighter;
                ed.hl_color = p.color;
                ed.hl_width = p.width;
            } else {
                ed.tool = Tool::Pen;
                ed.pen_color = p.color;
                ed.pen_width = p.width;
            }
        }
    }
    let mut pc = Color32::from_rgba_unmultiplied(ed.pen_color[0], ed.pen_color[1], ed.pen_color[2], ed.pen_color[3]);
    if ui.color_edit_button_srgba(&mut pc).on_hover_text("Цвет и прозрачность пера").changed() {
        ed.pen_color = pc.to_srgba_unmultiplied();
    }
    ui.add(egui::DragValue::new(&mut ed.pen_width).range(0.3..=40.0).speed(0.1).suffix(" px")).on_hover_text("Толщина пера");
    ui.menu_button("⭐", |ui| {
        ui.label("Наборы перьев");
        for (i, p) in settings.pen_presets.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                if ui.checkbox(&mut p.favorite, "").on_hover_text("В избранное").changed() {
                    save_settings = true;
                }
                let c = Color32::from_rgba_unmultiplied(p.color[0], p.color[1], p.color[2], 255);
                ui.label(RichText::new(if p.highlighter { "▮" } else { "●" }).color(c));
                ui.label(format!("{} ({:.1})", p.name, p.width));
                let _ = i;
            });
        }
        ui.separator();
        if ui.button("Сохранить текущее перо в наборы").clicked() {
            let hl = ed.tool == Tool::Highlighter;
            settings.pen_presets.push(crate::settings::PenPreset {
                name: format!("Перо {}", settings.pen_presets.len() + 1),
                color: if hl { ed.hl_color } else { ed.pen_color },
                width: if hl { ed.hl_width } else { ed.pen_width },
                highlighter: hl,
                favorite: true,
            });
            save_settings = true;
        }
    });
    ui.separator();
    if ui.selectable_label(ed.tool == Tool::Highlighter, "🖍 Маркер").clicked() {
        ed.tool = Tool::Highlighter;
    }
    if ed.tool == Tool::Highlighter {
        let mut hc = Color32::from_rgba_unmultiplied(ed.hl_color[0], ed.hl_color[1], ed.hl_color[2], ed.hl_color[3]);
        if ui.color_edit_button_srgba(&mut hc).changed() {
            ed.hl_color = hc.to_srgba_unmultiplied();
        }
        ui.add(egui::DragValue::new(&mut ed.hl_width).range(2.0..=60.0).suffix(" px"));
    }
    ui.separator();
    for (m, l) in [(EraserMode::Stroke, "⌫ Ластик: штрих"), (EraserMode::Point, "⌫ Ластик: точки")] {
        if ui.selectable_label(ed.tool == Tool::Eraser(m), l).clicked() {
            ed.tool = Tool::Eraser(m);
        }
    }
    if matches!(ed.tool, Tool::Eraser(_)) {
        ui.add(egui::DragValue::new(&mut ed.eraser_radius).range(2.0..=80.0).suffix(" px"));
    }
    ui.separator();
    let shape_label = match ed.tool {
        Tool::Shape(k) => k.label(),
        _ => "◇ Фигуры",
    };
    ui.menu_button(shape_label, |ui| {
        for k in ShapeKind::ALL {
            if ui.button(k.label()).clicked() {
                ed.tool = Tool::Shape(k);
                ui.close();
            }
        }
        ui.separator();
        let mut fill = ed.shape_fill.is_some();
        if ui.checkbox(&mut fill, "Заливка").changed() {
            ed.shape_fill = fill.then_some([ed.pen_color[0], ed.pen_color[1], ed.pen_color[2], 70]);
        }
        if let Some(f) = ed.shape_fill.as_mut() {
            let mut c = Color32::from_rgba_unmultiplied(f[0], f[1], f[2], f[3]);
            if ui.color_edit_button_srgba(&mut c).changed() {
                *f = c.to_srgba_unmultiplied();
            }
        }
    });
    if ui.selectable_label(settings.auto_shapes, "✨ Автофигуры").on_hover_text("Выравнивать нарисованные от руки линии, прямоугольники, овалы и треугольники").clicked() {
        settings.auto_shapes = !settings.auto_shapes;
        save_settings = true;
    }
    if ui.selectable_label(ed.ruler.is_some_and(|r| !r.protractor), "📏 Линейка").on_hover_text("Перетаскивайте мышью или пальцем, вращайте двумя пальцами или Alt+колесо").clicked() {
        ed.ruler = if ed.ruler.is_some_and(|r| !r.protractor) {
            None
        } else {
            Some(canvas::Ruler { center: ed.offset.to_pos2() + vec2(400.0, 300.0) / ed.zoom, angle: 0.0, protractor: false })
        };
    }
    if ui.selectable_label(ed.ruler.is_some_and(|r| r.protractor), "◠ Транспортир").clicked() {
        ed.ruler = if ed.ruler.is_some_and(|r| r.protractor) {
            None
        } else {
            Some(canvas::Ruler { center: ed.offset.to_pos2() + vec2(400.0, 300.0) / ed.zoom, angle: 0.0, protractor: true })
        };
    }
    ui.separator();
    ui.label("Рисует:");
    save_settings |= ui.toggle_value(&mut settings.pen_draws, "перо").changed();
    save_settings |= ui.toggle_value(&mut settings.touch_draws, "палец").changed();
    save_settings |= ui.toggle_value(&mut settings.mouse_draws, "мышь").changed();
    ui.separator();
    let want_ocr = ui.button("Аа В текст").on_hover_text("Распознать рукописный текст (выделенный или весь)").clicked();
    if save_settings {
        app.settings.save();
    }
    if want_ocr {
        let ctx = ui.ctx().clone();
        app.handle_action(&ctx, crate::canvas::Action::OcrInk);
    }
}

fn view_ribbon(app: &mut App, ui: &mut Ui, ctx: &egui::Context) {
    let Some(pid) = app.cur_page else {
        ui.label("Откройте страницу");
        return;
    };
    if let Some(ed) = app.editors.get_mut(&pid) {
        if tbtn(ui, "−", "Уменьшить") {
            ed.zoom = (ed.zoom / 1.2).max(0.1);
        }
        ui.label(format!("{:.0}%", ed.zoom * 100.0));
        if tbtn(ui, "+", "Увеличить") {
            ed.zoom = (ed.zoom * 1.2).min(8.0);
        }
        if tbtn(ui, "100%", "Исходный масштаб") {
            ed.zoom = 1.0;
        }
    }
    if tbtn(ui, "По ширине", "Вписать содержимое") {
        let bounds = app.pages.get(&pid).map(|p| canvas::export_region(p));
        let w = ctx.content_rect().width() - if app.focus_mode { 0.0 } else { 480.0 };
        if let (Some(b), Some(ed)) = (bounds, app.editors.get_mut(&pid)) {
            ed.zoom = (w / b.width().max(100.0)).clamp(0.1, 4.0);
            ed.offset = b.min.to_vec2() - vec2(20.0, 20.0) / ed.zoom;
        }
    }
    ui.separator();
    page_look_inline(app, ui);
    ui.separator();
    if ui.selectable_label(app.focus_mode, "Фокус (F11)").clicked() {
        app.focus_mode = !app.focus_mode;
    }
    if tbtn(ui, "Презентация (F5)", "Страница на весь экран") {
        app.presentation = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
    }
    if tbtn(ui, "📌 В окне", "Закрепить страницу в отдельном окне поверх других") {
        app.pin_current(true);
    }
    if tbtn(ui, "🔊 Читать", "Чтение вслух") {
        app.right = Some(RightPanel::Reading);
    }
    if tbtn(ui, "🎤 Диктовка", "Голосовой ввод Windows (Win+H)") {
        crate::winapi::start_dictation();
    }
}

fn page_look_inline(app: &mut App, ui: &mut Ui) {
    let Some(pid) = app.cur_page else { return };
    let mut changed = false;
    if let Some(page) = app.pages.get_mut(&pid) {
        egui::ComboBox::from_id_salt("bg").selected_text(page.background.label()).show_ui(ui, |ui| {
            for b in Background::ALL {
                if ui.selectable_label(page.background == b, b.label()).clicked() {
                    page.background = b;
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
    ui.horizontal(|ui| {
        ui.heading("Блокноты");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("+").on_hover_text("Новый раздел (Ctrl+T)").clicked() {
                app.new_section();
            }
        });
    });
    ui.separator();
    let mut actions: Vec<NavAction> = Vec::new();
    egui::ScrollArea::vertical().id_salt("nav_scroll").show(ui, |ui| {
        for ni in 0..app.notebooks.len() {
            let nb = &app.notebooks[ni];
            let selected_nb = app.cur_nb == Some(ni);
            let id = ui.make_persistent_id(("nb", ni, nb.path.to_string_lossy().to_string()));
            let header = egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, true);
            let nb_name = if nb.sealed { format!("🔒 {}", nb.meta.name) } else { nb.meta.name.clone() };
            header
                .show_header(ui, |ui| {
                    let r = ui.selectable_label(selected_nb, RichText::new(format!("📓 {nb_name}")).strong());
                    if r.clicked() {
                        actions.push(NavAction::SelectNotebook(ni));
                    }
                    r.context_menu(|ui| notebook_menu(ui, ni, &mut actions));
                })
                .body(|ui| {
                    if nb.sealed {
                        if ui.button("Разблокировать…").clicked() {
                            actions.push(NavAction::UnlockNotebook(ni));
                        }
                        return;
                    }
                    tree(ui, app, ni, &nb.meta.tree.clone(), &mut actions, 0);
                });
        }
        ui.add_space(8.0);
        if ui.button("+ Новый блокнот").clicked() {
            app.dialogs.push(Dialog::NewNotebook("Новый блокнот".into()));
        }
    });
    for a in actions {
        apply_nav(app, ui.ctx(), a);
    }
}

enum NavAction {
    SelectNotebook(usize),
    SelectSection(usize, Id),
    UnlockNotebook(usize),
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
    for n in nodes {
        match n {
            Node::Group(g) => {
                let r = ui.horizontal(|ui| {
                    ui.add_space(depth as f32 * 12.0);
                    let arrow = if g.collapsed { "▸" } else { "▾" };
                    let r = ui.selectable_label(false, format!("{arrow} 📁 {}", g.name));
                    if r.clicked() {
                        actions.push(NavAction::ToggleGroup(ni, g.id));
                    }
                    r
                });
                r.inner.context_menu(|ui| {
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
                let locked = s.lock.is_some() && !nb.keys.contains_key(&s.id);
                let resp = ui.horizontal(|ui| {
                    ui.add_space(depth as f32 * 12.0);
                    let (rect, _) = ui.allocate_exact_size(vec2(10.0, 16.0), egui::Sense::hover());
                    ui.painter().rect_filled(rect, 2.0, palette_color(s.color));
                    let label = if s.lock.is_some() { format!("{}{}", if locked { "🔒 " } else { "🔓 " }, s.name) } else { s.name.clone() };
                    ui.selectable_label(selected, label)
                });
                let r = resp.inner;
                if r.clicked() {
                    actions.push(NavAction::SelectSection(ni, s.id));
                }
                // Dropping a page onto a section moves it.
                if let Some(p) = r.dnd_release_payload::<PageDrag>() {
                    actions.push(NavAction::DropPage(p.page, ni, s.id));
                }
                if r.dnd_hover_payload::<PageDrag>().is_some() {
                    ui.painter().rect_stroke(r.rect, 2.0, egui::Stroke::new(2.0, Color32::from_rgb(60, 130, 255)), egui::StrokeKind::Outside);
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
    let Some(ni) = app.cur_nb else { return };
    let Some(sid) = app.cur_section else {
        ui.label("Выберите раздел");
        return;
    };
    let Some(sec) = app.notebooks[ni].meta.section(sid).cloned() else { return };
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(6.0, 20.0), egui::Sense::hover());
        ui.painter().rect_filled(rect, 2.0, palette_color(sec.color));
        ui.heading(&sec.name);
    });
    if !app.notebooks[ni].section_accessible(sid) {
        ui.label("🔒 Раздел защищён паролем");
        if ui.button("Разблокировать…").clicked() {
            app.dialogs.push(Dialog::Password { target: PwTarget::UnlockSection(sid), text: String::new(), confirm: String::new(), error: String::new() });
        }
        return;
    }
    let mut sort = sec.sort;
    ui.horizontal(|ui| {
        if ui.button("+ Страница").on_hover_text("Ctrl+N").clicked() {
            app.new_page(false);
        }
        if ui.small_button("+ под").on_hover_text("Подстраница (Ctrl+Alt+Shift+N)").clicked() {
            app.new_page(true);
        }
        egui::ComboBox::from_id_salt("sort").width(90.0).selected_text(match sort {
            PageSort::Manual => "Вручную",
            PageSort::Title => "По названию",
            PageSort::Created => "По созданию",
            PageSort::Modified => "По изменению",
        })
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut sort, PageSort::Manual, "Вручную");
            ui.selectable_value(&mut sort, PageSort::Title, "По названию");
            ui.selectable_value(&mut sort, PageSort::Created, "По дате создания");
            ui.selectable_value(&mut sort, PageSort::Modified, "По дате изменения");
        });
    });
    if sort != sec.sort {
        if let Some(s) = app.notebooks[ni].meta.section_mut(sid) {
            s.sort = sort;
        }
        app.notebooks[ni].dirty = true;
    }
    ui.separator();
    let pages: Vec<PageMeta> = sorted_pages(&sec).into_iter().cloned().collect();
    let mut open = None;
    let mut ops: Vec<(Id, &'static str)> = Vec::new();
    let mut reorder: Option<(Id, Id)> = None;
    let mut move_to: Option<(Id, usize, Id)> = None;
    egui::ScrollArea::vertical().id_salt("pages_scroll").show(ui, |ui| {
        let mut hidden_below: Option<u8> = None;
        for (i, p) in pages.iter().enumerate() {
            if let Some(l) = hidden_below {
                if p.level > l {
                    continue;
                }
                hidden_below = None;
            }
            let has_children = pages.get(i + 1).is_some_and(|n| n.level > p.level) && sec.sort == PageSort::Manual;
            if p.collapsed && has_children {
                hidden_below = Some(p.level);
            }
            let selected = app.cur_page == Some(p.id);
            let row = ui.horizontal(|ui| {
                ui.add_space(p.level as f32 * 16.0);
                if has_children {
                    if ui.small_button(if p.collapsed { "▸" } else { "▾" }).clicked() {
                        ops.push((p.id, "collapse"));
                    }
                }
                let title = if p.title.trim().is_empty() { "Без названия".to_string() } else { p.title.clone() };
                let label = if p.pinned { format!("📌 {title}") } else { title };
                let drag_id = egui::Id::new(("pdrag", p.id));
                let r = ui
                    .dnd_drag_source(drag_id, PageDrag { page: p.id }, |ui| {
                        ui.add(egui::Button::selectable(selected, label).min_size(vec2(ui.available_width(), 0.0)).truncate())
                    })
                    .inner;
                r
            });
            let r = row.inner;
            if r.clicked() {
                open = Some(p.id);
            }
            r.clone().on_hover_text(format!("Создана: {}\nИзменена: {}\nАвтор: {}", fmt_dt(&p.created), fmt_dt(&p.modified), p.author));
            if let Some(d) = r.dnd_release_payload::<PageDrag>() {
                if d.page != p.id {
                    reorder = Some((d.page, p.id));
                }
            }
            if r.dnd_hover_payload::<PageDrag>().is_some() {
                ui.painter().hline(r.rect.x_range(), r.rect.top(), egui::Stroke::new(2.0, Color32::from_rgb(60, 130, 255)));
            }
            r.context_menu(|ui| {
                for (label, op) in [
                    ("Переименовать", "rename"),
                    (if p.pinned { "Открепить" } else { "Закрепить вверху" }, "pin"),
                    ("Сделать подстраницей", "indent"),
                    ("Повысить уровень", "outdent"),
                    ("Дублировать", "dup"),
                    ("Копировать ссылку", "link"),
                    ("Закрепить в окне", "window"),
                ] {
                    if ui.button(label).clicked() {
                        ops.push((p.id, op));
                        ui.close();
                    }
                }
                ui.menu_button("Переместить в", |ui| {
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
                                move_to = Some((p.id, j, s.id));
                                ui.close();
                            }
                        }
                    }
                });
                ui.separator();
                if ui.button("Удалить").clicked() {
                    ops.push((p.id, "delete"));
                    ui.close();
                }
            });
        }
    });
    if let Some(p) = open {
        app.open_page(Some(p));
    }
    if let Some((what, before)) = reorder {
        if let Some(s) = app.notebooks[ni].meta.section_mut(sid) {
            if let Some(i) = s.pages.iter().position(|p| p.id == what) {
                let pm = s.pages.remove(i);
                let j = s.pages.iter().position(|p| p.id == before).unwrap_or(s.pages.len());
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
            "rename" => {
                let t = app.notebooks[ni].meta.page_meta(pid).map(|p| p.title.clone()).unwrap_or_default();
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
            "delete" => app.delete_page(pid),
            _ => {}
        }
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
    // Header with title.
    if !app.presentation {
        egui::Frame::new().fill(bg).inner_margin(egui::Margin { left: 24, right: 12, top: 10, bottom: 4 }).show(ui, |ui| {
            let mut title = app.notebooks[ni].meta.page_meta(pid).map(|p| p.title.clone()).unwrap_or_default();
            let tc = if dark { Color32::from_gray(230) } else { Color32::from_gray(30) };
            let r = ui.add(
                egui::TextEdit::singleline(&mut title)
                    .font(egui::FontId::proportional(26.0))
                    .text_color(tc)
                    .frame(egui::Frame::NONE)
                    .hint_text("Заголовок страницы")
                    .desired_width(f32::INFINITY),
            );
            if r.changed() {
                if let Some(pm) = app.notebooks[ni].meta.page_meta_mut(pid) {
                    pm.title = title;
                }
                app.mark_dirty(pid);
            }
            let meta = app.notebooks[ni].meta.page_meta(pid).cloned();
            ui.horizontal(|ui| {
                if let Some(m) = meta {
                    ui.label(RichText::new(m.created.format("%d.%m.%Y   %H:%M").to_string()).weak().small());
                    if !m.author.is_empty() {
                        ui.label(RichText::new(format!("· {}", m.author)).weak().small());
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

fn status_bar(app: &mut App, ui: &mut Ui) {
    ui.horizontal(|ui| {
        let t = now_secs();
        if t - app.status.1 < 8.0 {
            ui.label(&app.status.0);
        } else if app.index.building.load(std::sync::atomic::Ordering::Relaxed) {
            ui.label("Индексация…");
        } else if app.export.is_some() {
            ui.label("Экспорт…");
        } else {
            ui.label(RichText::new("Готово · только локальное хранение").weak());
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if let Some(ed) = app.cur_page.and_then(|p| app.editors.get(&p)) {
                ui.label(format!("{:.0}%", ed.zoom * 100.0));
            }
            ui.label(if app.dirty.is_empty() { "✔ Сохранено" } else { "● Изменено" });
            if crate::pen::pen_in_range() {
                ui.label("✒ Перо");
            }
            ui.label(RichText::new(format!("👤 {}", app.settings.author)).weak());
            if app.presentation {
                ui.label("Esc — выход из презентации");
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
