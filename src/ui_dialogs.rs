//! Dialog windows and the settings window.

use egui::{Context, RichText, Ui};

use crate::app::*;
use crate::model::*;

enum Res {
    Keep,
    Close,
}

pub fn draw(app: &mut App, ctx: &Context) {
    let Some(mut d) = app.dialogs.pop() else { return };
    let title = match &d {
        Dialog::NewNotebook(_) => "Новый блокнот",
        Dialog::Rename { .. } => "Переименовать",
        Dialog::Password { target, .. } => match target {
            PwTarget::UnlockSection(_) | PwTarget::UnlockNotebook(_) => "Введите пароль",
            _ => "Защита паролем",
        },
        Dialog::Settings(_) => "Параметры",
        Dialog::Trash => "Корзина",
        Dialog::Backups => "Резервные копии",
        Dialog::Report(..) => "Отчёт",
        Dialog::TaskDue { .. } => "Срок задачи",
        Dialog::Link { .. } => "Вставить ссылку",
        Dialog::Symbols => "Символы",
        Dialog::Templates => "Шаблоны страниц",
        Dialog::Tags => "Теги",
        Dialog::Merge { .. } => "Объединение блокнотов",
        Dialog::MergeSections { .. } => "Объединить разделы",
        Dialog::Hotkeys => "Горячие клавиши",
        Dialog::About => "О программе",
        Dialog::Table { .. } => "Вставить таблицу",
        Dialog::Print(_) => "Печать и PDF",
        Dialog::OcrResult { .. } => "Распознанный текст",
        Dialog::Confirm { .. } => "Подтверждение",
        Dialog::BackupPassword(_) => "Пароль резервных копий",
        Dialog::Flashcards { .. } => "Карточки",
        Dialog::ClearInk => "Очистить рукопись",
    };
    let mut open = true;
    let mut res = Res::Keep;
    let wide = matches!(d, Dialog::Settings(_) | Dialog::Trash | Dialog::Backups | Dialog::Symbols | Dialog::Tags | Dialog::Hotkeys);
    egui::Window::new(title)
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_width(if wide { 620.0 } else { 380.0 })
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            res = body(app, ui, ctx, &mut d);
        });
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        open = false;
    }
    if open && matches!(res, Res::Keep) {
        app.dialogs.push(d);
    }
}

fn body(app: &mut App, ui: &mut Ui, ctx: &Context, d: &mut Dialog) -> Res {
    let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
    match d {
        Dialog::NewNotebook(name) => {
            ui.label("Название:");
            let r = ui.text_edit_singleline(name);
            r.request_focus();
            ui.label(RichText::new(format!("Папка: {}", app.settings.notebooks_dir.display())).small().weak());
            if (ui.button("Создать").clicked() || enter) && !name.trim().is_empty() {
                let n = name.trim().to_owned();
                app.new_notebook(&n);
                return Res::Close;
            }
        }
        Dialog::Rename { target, text } => {
            let r = ui.text_edit_singleline(text);
            if !r.has_focus() {
                r.request_focus();
            }
            if ui.button("Готово").clicked() || enter {
                let t = text.trim().to_owned();
                if t.is_empty() && !matches!(target, RenameTarget::Page(_)) {
                    return Res::Keep;
                }
                match target.clone() {
                    RenameTarget::Notebook(ni) => {
                        app.notebooks[ni].meta.name = t;
                        app.notebooks[ni].dirty = true;
                    }
                    RenameTarget::Section(sid) => {
                        if let Some(ni) = app.notebooks.iter().position(|n| n.meta.section(sid).is_some()) {
                            app.notebooks[ni].meta.section_mut(sid).unwrap().name = t;
                            app.notebooks[ni].dirty = true;
                        }
                    }
                    RenameTarget::Group(gid) => {
                        for nb in app.notebooks.iter_mut() {
                            if let Some(g) = nb.meta.group_mut(gid) {
                                g.name = t.clone();
                                nb.dirty = true;
                            }
                        }
                    }
                    RenameTarget::Page(pid) => {
                        if let Some(ni) = app.nb_index_of_page(pid) {
                            if let Some(pm) = app.notebooks[ni].meta.page_meta_mut(pid) {
                                pm.title = if t.is_empty() { "Без названия".into() } else { t };
                            }
                            app.mark_dirty(pid);
                        }
                    }
                }
                return Res::Close;
            }
        }
        Dialog::Password { target, text, confirm, error } => {
            let setting = matches!(target, PwTarget::SetSection(_) | PwTarget::SetNotebook(_));
            if setting {
                ui.label("Пароль хранится только у вас. Если его забыть, данные восстановить невозможно.");
                ui.label(RichText::new("Шифрование AES-256-GCM, ключ из пароля через Argon2id.").small().weak());
            }
            ui.label("Пароль:");
            let r = ui.add(egui::TextEdit::singleline(text).password(true));
            if !r.has_focus() && confirm.is_empty() {
                r.request_focus();
            }
            if setting {
                ui.label("Повторите:");
                ui.add(egui::TextEdit::singleline(confirm).password(true));
            }
            if !error.is_empty() {
                ui.colored_label(egui::Color32::from_rgb(210, 40, 40), error.as_str());
            }
            if ui.button(if setting { "Установить" } else { "Разблокировать" }).clicked() || enter {
                match target.clone() {
                    PwTarget::UnlockSection(sid) => {
                        let ni = app.notebooks.iter().position(|n| n.meta.section(sid).is_some());
                        let ok = ni.and_then(|ni| {
                            let lock = app.notebooks[ni].meta.section(sid)?.lock.clone()?;
                            crate::crypto::unlock(&lock, text).map(|k| (ni, k))
                        });
                        match ok {
                            Some((ni, key)) => {
                                app.notebooks[ni].keys.insert(sid, key);
                                app.cur_nb = Some(ni);
                                app.cur_section = Some(sid);
                                app.select_first_page();
                                app.rebuild_index(ctx);
                                return Res::Close;
                            }
                            None => *error = "Неверный пароль".into(),
                        }
                    }
                    PwTarget::UnlockNotebook(ni) => {
                        if app.notebooks[ni].unseal(text) {
                            app.cur_nb = Some(ni);
                            app.cur_section = None;
                            app.select_first_page();
                            app.rebuild_index(ctx);
                            return Res::Close;
                        }
                        *error = "Неверный пароль".into();
                    }
                    PwTarget::SetSection(sid) => {
                        if text.chars().count() < 4 {
                            *error = "Минимум 4 символа".into();
                        } else if text != confirm {
                            *error = "Пароли не совпадают".into();
                        } else if let Some(ni) = app.notebooks.iter().position(|n| n.meta.section(sid).is_some()) {
                            app.safety_backup(ni, "перед защитой раздела");
                            app.notebooks[ni].lock_section(sid, text);
                            app.set_status("Раздел защищён паролем");
                            return Res::Close;
                        }
                    }
                    PwTarget::SetNotebook(ni) => {
                        if text.is_empty() && confirm.is_empty() {
                            app.safety_backup(ni, "перед снятием пароля");
                            app.notebooks[ni].set_password(None);
                            app.set_status("Пароль блокнота снят");
                            return Res::Close;
                        } else if text.chars().count() < 4 {
                            *error = "Минимум 4 символа (пустой пароль снимает защиту)".into();
                        } else if text != confirm {
                            *error = "Пароли не совпадают".into();
                        } else {
                            app.safety_backup(ni, "перед установкой пароля");
                            app.notebooks[ni].set_password(Some(text));
                            app.set_status("Блокнот защищён паролем");
                            return Res::Close;
                        }
                    }
                }
            }
        }
        Dialog::Settings(tab) => return settings_ui(app, ui, ctx, tab),
        Dialog::Trash => return trash_ui(app, ui),
        Dialog::Backups => return backups_ui(app, ui),
        Dialog::Report(title, lines) => {
            ui.heading(title.as_str());
            egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                for l in lines.iter() {
                    ui.label(l);
                }
            });
            if ui.button("OK").clicked() {
                return Res::Close;
            }
        }
        Dialog::TaskDue { item, date, time } => {
            ui.horizontal(|ui| {
                ui.label("Дата (ДД.ММ.ГГГГ):");
                ui.text_edit_singleline(date);
            });
            ui.horizontal(|ui| {
                ui.label("Время (ЧЧ:ММ):");
                ui.text_edit_singleline(time);
            });
            if ui.button("Установить").clicked() || enter {
                let dt = chrono::NaiveDateTime::parse_from_str(&format!("{} {}", date.trim(), time.trim()), "%d.%m.%Y %H:%M");
                match dt {
                    Ok(dt) => {
                        let id = *item;
                        if let Some(p) = app.cur_page {
                            app.with_page(|page, ed| {
                                ed.snapshot(page);
                                if let Some(it) = page.item_mut(id) {
                                    let t = it.task.get_or_insert(Task { done: false, due: None, priority: 0, notified: false });
                                    t.due = Some(dt);
                                    t.notified = false;
                                }
                            });
                            app.mark_dirty(p);
                        }
                        return Res::Close;
                    }
                    Err(_) => {
                        ui.colored_label(egui::Color32::RED, "Неверный формат");
                    }
                }
            }
        }
        Dialog::Link { text, url } => {
            ui.label("Текст ссылки:");
            ui.text_edit_singleline(text);
            ui.label("Адрес (сайт, файл, почта):");
            ui.text_edit_singleline(url);
            ui.horizontal(|ui| {
                if ui.button("Файл…").clicked() {
                    if let Some(p) = rfd::FileDialog::new().pick_file() {
                        *url = format!("file:///{}", p.display().to_string().replace('\\', "/"));
                        if text.is_empty() {
                            *text = p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                        }
                    }
                }
            });
            ui.separator();
            ui.label("Или ссылка на заметку:");
            egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                for nb in &app.notebooks {
                    if nb.sealed {
                        continue;
                    }
                    if ui.link(format!("📓 {}", nb.meta.name)).clicked() {
                        *url = format!("note://notebook/{}", nb.meta.id);
                        if text.is_empty() {
                            *text = nb.meta.name.clone();
                        }
                    }
                    for s in nb.meta.sections() {
                        if ui.link(format!("    ▪ {}", s.name)).clicked() {
                            *url = format!("note://section/{}", s.id);
                            if text.is_empty() {
                                *text = s.name.clone();
                            }
                        }
                        if nb.section_accessible(s.id) {
                            for p in &s.pages {
                                if ui.link(format!("        {}", p.title)).clicked() {
                                    *url = format!("note://page/{}", p.id);
                                    if text.is_empty() {
                                        *text = p.title.clone();
                                    }
                                }
                            }
                        }
                    }
                }
            });
            if (ui.button("Вставить").clicked() || enter) && !url.trim().is_empty() {
                let t = if text.trim().is_empty() { url.clone() } else { text.clone() };
                let s = format!("[{}]({})", t.replace([']', '['], ""), url.trim().replace(' ', "%20"));
                app.insert_text_at_cursor(ctx, s);
                return Res::Close;
            }
        }
        Dialog::Symbols => {
            const GROUPS: &[(&str, &str)] = &[
                ("Типографика", "—–«»„“”‘’…•·§¶©®™°±×÷≈≠≤≥∞√∑∫∂∆∇∏πΩµ←→↑↓↔⇒⇔✓✔✗✘★☆♥♦♣♠№€£¥₽¢"),
                ("Греческие", "αβγδεζηθικλμνξοπρστυφχψωΑΒΓΔΕΖΗΘΙΚΛΜΝΞΟΠΡΣΤΥΦΧΨΩ"),
                ("Индексы", "⁰¹²³⁴⁵⁶⁷⁸⁹⁺⁻⁼⁽⁾ⁿ₀₁₂₃₄₅₆₇₈₉₊₋₌₍₎½⅓⅔¼¾⅛"),
                ("Эмодзи", "😀😃😄😁😆😅😂🙂😉😊😍😘😎🤔😐😴😢😭😡👍👎👏🙏💪👀🔥⭐✨💡📌📎📅📝✅❌⚠️❓❗💬🎉🎁🏠🚗✈️☕🍕📚💻📱🎵🎬"),
            ];
            egui::ScrollArea::vertical().max_height(380.0).show(ui, |ui| {
                for (name, chars) in GROUPS {
                    ui.label(RichText::new(*name).strong());
                    ui.horizontal_wrapped(|ui| {
                        for c in chars.chars() {
                            if c == '\u{fe0f}' {
                                continue;
                            }
                            if ui.add(egui::Button::new(RichText::new(c.to_string()).size(18.0)).min_size(egui::vec2(30.0, 30.0))).clicked() {
                                app.insert_text_at_cursor(ctx, c.to_string());
                            }
                        }
                    });
                }
            });
            if ui.button("Закрыть").clicked() {
                return Res::Close;
            }
        }
        Dialog::Templates => {
            ui.label(RichText::new("Встроенные").strong());
            ui.horizontal_wrapped(|ui| {
                for t in crate::templates::BUILTIN {
                    if ui.button(*t).clicked() {
                        app.new_page_from_template(t);
                    }
                }
            });
            let custom = crate::templates::custom_list();
            if !custom.is_empty() {
                ui.label(RichText::new("Мои шаблоны").strong());
                let mut del = None;
                for t in &custom {
                    ui.horizontal(|ui| {
                        if ui.button(t).clicked() {
                            app.new_page_from_template(t);
                        }
                        if ui.small_button("🗑").clicked() {
                            del = Some(t.clone());
                        }
                    });
                }
                if let Some(t) = del {
                    let _ = std::fs::remove_file(crate::settings::Settings::templates_dir().join(format!("{t}.json")));
                }
            }
            ui.label(RichText::new("Свой шаблон: «Страница → Сохранить как шаблон».").small().weak());
            if ui.button("Закрыть").clicked() {
                return Res::Close;
            }
        }
        Dialog::Tags => return tags_editor(app, ui),
        Dialog::Merge { src, policy } => {
            ui.label(format!("Архив: {}", src.display()));
            ui.label("Если страница есть в обоих блокнотах:");
            for (i, l) in ["Оставить более новую", "Оставить мою", "Взять из архива", "Сохранить обе"].iter().enumerate() {
                ui.radio_value(policy, i, *l);
            }
            if let Some(ni) = app.cur_nb {
                ui.label(format!("Объединить с: {}", app.notebooks[ni].meta.name));
                if ui.button("Объединить").clicked() {
                    let (s, p) = (src.clone(), *policy);
                    app.import_archive(ctx, &s, Some((ni, p)));
                    return Res::Close;
                }
            }
        }
        Dialog::MergeSections { from } => {
            let from = *from;
            ui.label("Все страницы раздела будут перенесены в выбранный раздел, а сам раздел удалён.");
            let targets: Vec<(Id, String)> = app.nb().map(|nb| nb.meta.sections().iter().filter(|s| s.id != from && nb.section_accessible(s.id)).map(|s| (s.id, s.name.clone())).collect()).unwrap_or_default();
            for (id, name) in targets {
                if ui.button(&name).clicked() {
                    app.merge_sections(from, id);
                    return Res::Close;
                }
            }
        }
        Dialog::Hotkeys => {
            egui::Grid::new("hk").striped(true).show(ui, |ui| {
                for (a, k) in [
                    ("Новая страница", "Ctrl+N"), ("Новая подстраница", "Ctrl+Alt+Shift+N"), ("Новый раздел", "Ctrl+T"), ("Новое окно", "Ctrl+M"),
                    ("Поиск на странице", "Ctrl+F"), ("Поиск везде", "Ctrl+E"), ("Теги", "Ctrl+1 … Ctrl+9"), ("Жирный / курсив / подчёркнутый", "Ctrl+B / I / U"),
                    ("Зачёркнутый", "Ctrl+-"), ("Маркированный / нумерованный список", "Ctrl+. / Ctrl+/"), ("Заголовок 1–3", "Ctrl+Alt+1 … 3"),
                    ("Верхний / нижний индекс", "Ctrl+Shift+= / Ctrl+="), ("Очистить формат", "Ctrl+Пробел"), ("Ссылка", "Ctrl+K"),
                    ("Дата / время / дата и время", "Alt+Shift+D / T / F"), ("Снимок экрана", "Win+Shift+S (вставка из буфера)"), ("Быстрая заметка", "Win+Alt+N"),
                    ("Заблокировать разделы", "Ctrl+Alt+L"), ("Печать", "Ctrl+P"), ("Сохранить сейчас", "Ctrl+S"), ("Отменить / повторить", "Ctrl+Z / Ctrl+Y"),
                    ("Дублировать выделение", "Ctrl+D"), ("Масштаб", "Ctrl+колесо, щипок"), ("Прокрутка холста", "Пробел + перетаскивание, средняя кнопка"),
                    ("Фокус / презентация", "F11 / F5"), ("Кнопка пера", "лассо"), ("Обратный конец пера", "ластик"),
                ] {
                    ui.label(a);
                    ui.label(RichText::new(k).monospace());
                    ui.end_row();
                }
            });
        }
        Dialog::About => {
            ui.heading("NewNote");
            ui.label(format!("Версия {}", env!("CARGO_PKG_VERSION")));
            ui.label("Локальные заметки со свободным холстом. Без облака, учётной записи и телеметрии.");
            ui.label(format!("Блокноты: {}", app.settings.notebooks_dir.display()));
            ui.label(format!("Настройки: {}", crate::settings::Settings::path().display()));
            if ui.button("OK").clicked() {
                return Res::Close;
            }
        }
        Dialog::Table { rows, cols } => {
            ui.add(egui::Slider::new(rows, 1..=50).text("Строк"));
            ui.add(egui::Slider::new(cols, 1..=20).text("Столбцов"));
            if ui.button("Вставить").clicked() || enter {
                let t = Table::new(*rows, *cols);
                let w = t.col_widths.iter().sum();
                app.insert_item(ItemKind::Table(t), w);
                return Res::Close;
            }
        }
        Dialog::Print(o) => {
            if app.current_no_copy() {
                ui.label("Печать и экспорт запрещены для этого раздела.");
                if ui.button("OK").clicked() {
                    return Res::Close;
                }
                return Res::Keep;
            }
            ui.horizontal(|ui| {
                ui.radio_value(&mut o.whole_section, false, "Текущая страница");
                ui.radio_value(&mut o.whole_section, true, "Весь раздел");
            });
            ui.horizontal(|ui| {
                ui.radio_value(&mut o.landscape, false, "Книжная");
                ui.radio_value(&mut o.landscape, true, "Альбомная");
            });
            ui.add(egui::Slider::new(&mut o.margins, 0.0..=40.0).text("Поля, мм"));
            ui.add(egui::Slider::new(&mut o.scale, 0.3..=1.0).text("Масштаб"));
            ui.checkbox(&mut o.numbers, "Номера страниц");
            ui.checkbox(&mut o.header, "Заголовок и метка конфиденциальности сверху");
            ui.label(RichText::new("Печатаются текст, рукописные заметки и разметка фона.").small().weak());
            ui.horizontal(|ui| {
                if ui.button("🖨 Печать").clicked() {
                    let path = crate::settings::Settings::temp_dir().join(format!("print-{}.pdf", new_id()));
                    let _ = std::fs::create_dir_all(path.parent().unwrap());
                    let mut opts = o.clone();
                    opts.print = true;
                    app.start_export(ExportKind::Print, path, opts);
                    app.dialogs.clear();
                }
                if ui.button("Сохранить PDF…").clicked() {
                    let name = app.cur_page.and_then(|p| app.nb().and_then(|nb| nb.meta.page_meta(p))).map(|p| p.title.clone()).unwrap_or_default();
                    if let Some(p) = rfd::FileDialog::new().add_filter("PDF", &["pdf"]).set_file_name(format!("{}.pdf", crate::storage::sanitize(&name))).save_file() {
                        let opts = o.clone();
                        app.start_export(ExportKind::Pdf, p, opts);
                        app.dialogs.clear();
                    }
                }
            });
        }
        Dialog::OcrResult { title, text, from_ink } => {
            ui.label(RichText::new(title.as_str()).strong());
            egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                ui.add(egui::TextEdit::multiline(text).desired_width(f32::INFINITY));
            });
            let mut close = false;
            ui.horizontal_wrapped(|ui| {
                if ui.button("Копировать").clicked() {
                    ctx.copy_text(text.clone());
                }
                if ui.button("Вставить как текст").clicked() {
                    let t = text.clone();
                    app.insert_text_at_cursor(ctx, t);
                    app.with_page(|_, ed| ed.editing = None);
                }
                if *from_ink {
                    if ui.button("Заменить рукопись текстом").clicked() {
                        let t = text.clone();
                        if let Some(pid) = app.cur_page {
                            let (size, font, author) = (app.settings.default_size, app.settings.default_font.clone(), app.settings.author.clone());
                            app.with_page(|page, ed| {
                                ed.snapshot(page);
                                let idx: Vec<usize> = if ed.selection.strokes.is_empty() { (0..page.strokes.len()).collect() } else { ed.selection.strokes.clone() };
                                let mut bb = egui::Rect::NOTHING;
                                for &i in &idx {
                                    if let Some(s) = page.strokes.get(i) {
                                        bb = bb.union(s.bbox());
                                    }
                                }
                                let mut sorted = idx;
                                sorted.sort_unstable();
                                for i in sorted.into_iter().rev() {
                                    if i < page.strokes.len() {
                                        page.strokes.remove(i);
                                    }
                                }
                                let pos = if bb.is_positive() { bb.min } else { egui::pos2(0.0, 0.0) };
                                page.items.push(Item::new(pos, bb.width().max(300.0), ItemKind::Text(TextBlock::new(&t, size, &font)), &author));
                                ed.selection = Default::default();
                            });
                            app.mark_dirty(pid);
                        }
                        close = true;
                    }
                    if ui.button("Как формулу").clicked() {
                        let t: String = text.lines().collect::<Vec<_>>().join(" ").replace('×', "*").replace('—', "-");
                        app.insert_item(ItemKind::Equation(t), 420.0);
                        close = true;
                    }
                    if ui.button("Как таблицу").clicked() {
                        // Columns are separated by two or more spaces or tabs in the recognized text.
                        let rows: Vec<Vec<String>> = text
                            .lines()
                            .filter(|l| !l.trim().is_empty())
                            .map(|l| l.split(['\t', '|']).flat_map(|p| p.split("  ")).map(|c| c.trim().to_owned()).filter(|c| !c.is_empty()).collect())
                            .collect();
                        let t = crate::export::table_from_cells(rows);
                        let w = t.col_widths.iter().sum();
                        app.insert_item(ItemKind::Table(t), w);
                        close = true;
                    }
                }
            });
            if close || ui.button("Закрыть").clicked() {
                return Res::Close;
            }
        }
        Dialog::Confirm { text, action } => {
            ui.label(text.as_str());
            ui.horizontal(|ui| {
                if ui.button("Да").clicked() {
                    match action.clone() {
                        ConfirmAction::DeleteSection(s) => app.delete_section(s),
                        ConfirmAction::DeleteGroup(g) => app.delete_group(g),
                        ConfirmAction::EmptyTrash => {
                            if let Some(ni) = app.cur_nb {
                                app.safety_backup(ni, "перед очисткой корзины");
                                let entries: Vec<TrashEntry> = app.notebooks[ni].meta.trash.drain(..).collect();
                                for t in &entries {
                                    app.notebooks[ni].delete_trash_files(t);
                                }
                                app.notebooks[ni].dirty = true;
                            }
                        }
                        ConfirmAction::CloseNotebook(ni) => app.close_notebook(ni),
                        ConfirmAction::RestoreBackup(p) => {
                            if let Some(ni) = app.cur_nb {
                                app.save_all();
                                let path = app.notebooks[ni].path.clone();
                                let key = app.backup_key.clone();
                                app.close_notebook(ni);
                                match crate::storage::restore_backup(&p, &path, key.as_ref()) {
                                    Ok(()) => app.set_status("Блокнот восстановлен из резервной копии"),
                                    Err(e) => app.set_status(format!("Ошибка восстановления: {e}")),
                                }
                                app.open_notebook_folder(ctx, &path);
                            }
                        }
                    }
                    app.dialogs.clear();
                }
                if ui.button("Нет").clicked() {
                    app.dialogs.clear();
                }
            });
            return Res::Close;
        }
        Dialog::BackupPassword(pw) => {
            ui.label("Пароль для шифрования резервных копий (хранится только в памяти до закрытия программы):");
            ui.add(egui::TextEdit::singleline(pw).password(true));
            if ui.button("OK").clicked() || enter {
                // Derive a stable key: salt stored once in the backup folder.
                let salt_path = app.settings.backup_dir.join("backup.salt");
                let salt = std::fs::read(&salt_path).unwrap_or_else(|_| {
                    let s: [u8; 16] = rand::random();
                    crate::storage::atomic_write(&salt_path, &s);
                    s.to_vec()
                });
                app.backup_key = Some(crate::crypto::derive_key(pw, &salt));
                app.run_backups_now();
                return Res::Close;
            }
        }
        Dialog::ClearInk => {
            ui.label("Удалить все рукописные объекты на странице? Действие можно отменить (Ctrl+Z).");
            ui.horizontal(|ui| {
                if ui.button("Удалить").clicked() {
                    if let Some(pid) = app.cur_page {
                        app.with_page(|page, ed| {
                            ed.snapshot(page);
                            page.strokes.retain(|s| s.locked);
                            ed.selection = Default::default();
                        });
                        app.mark_dirty(pid);
                    }
                    app.dialogs.clear();
                }
                if ui.button("Отмена").clicked() {
                    app.dialogs.clear();
                }
            });
            return Res::Close;
        }
        Dialog::Flashcards { cards, idx, flipped } => {
            let n = cards.len();
            let (q, a) = &cards[(*idx).min(n - 1)];
            ui.label(format!("Карточка {} из {n}", *idx + 1));
            let frame = egui::Frame::group(ui.style()).inner_margin(20.0);
            let r = frame
                .show(ui, |ui| {
                    ui.set_min_size(egui::vec2(340.0, 140.0));
                    ui.vertical_centered(|ui| {
                        if *flipped {
                            ui.label(RichText::new(a).size(16.0));
                        } else {
                            ui.label(RichText::new(q).size(20.0).strong());
                        }
                    });
                })
                .response
                .interact(egui::Sense::click());
            if r.clicked() {
                *flipped = !*flipped;
            }
            ui.horizontal(|ui| {
                if ui.button("◀").clicked() && *idx > 0 {
                    *idx -= 1;
                    *flipped = false;
                }
                if ui.button(if *flipped { "Вопрос" } else { "Ответ" }).clicked() {
                    *flipped = !*flipped;
                }
                if ui.button("▶").clicked() && *idx + 1 < n {
                    *idx += 1;
                    *flipped = false;
                }
                if ui.button("На страницу").clicked() {
                    let text = cards.iter().map(|(q, a)| format!("**{q}** — {a}")).collect::<Vec<_>>().join("\n");
                    app.insert_text_at_cursor(ctx, text);
                }
            });
        }
    }
    Res::Keep
}

fn trash_ui(app: &mut App, ui: &mut Ui) -> Res {
    let Some(ni) = app.cur_nb else { return Res::Close };
    ui.label(format!("Блокнот «{}». Удалённое хранится {} дн.", app.notebooks[ni].meta.name, app.settings.trash_days));
    let mut restore = None;
    let mut purge = None;
    egui::ScrollArea::vertical().max_height(380.0).show(ui, |ui| {
        if app.notebooks[ni].meta.trash.is_empty() {
            ui.label(RichText::new("Корзина пуста").weak());
        }
        for (i, t) in app.notebooks[ni].meta.trash.iter().enumerate() {
            ui.horizontal(|ui| {
                let name = match &t.kind {
                    TrashKind::Page { meta, .. } => format!("📄 {}", meta.title),
                    TrashKind::Section { section, .. } => format!("▪ Раздел «{}» ({} стр.)", section.name, section.pages.len()),
                };
                ui.label(name);
                ui.label(RichText::new(fmt_dt(&t.deleted)).small().weak());
                if ui.button("Восстановить").clicked() {
                    restore = Some(i);
                }
                if ui.button("Удалить навсегда").clicked() {
                    purge = Some(i);
                }
            });
        }
    });
    if let Some(i) = restore {
        app.restore_trash(ni, i);
    }
    if let Some(i) = purge {
        let t = app.notebooks[ni].meta.trash.remove(i);
        app.notebooks[ni].delete_trash_files(&t);
        app.notebooks[ni].dirty = true;
    }
    ui.horizontal(|ui| {
        if ui.button("Очистить корзину").clicked() && !app.notebooks[ni].meta.trash.is_empty() {
            app.dialogs.push(Dialog::Trash);
            app.dialogs.push(Dialog::Confirm { text: "Удалить всё из корзины без возможности восстановления?".into(), action: ConfirmAction::EmptyTrash });
        }
        ui.label("Срок хранения, дней:");
        if ui.add(egui::DragValue::new(&mut app.settings.trash_days).range(1..=3650)).changed() {
            app.settings.save();
        }
    });
    Res::Keep
}

fn backups_ui(app: &mut App, ui: &mut Ui) -> Res {
    let Some(ni) = app.cur_nb else { return Res::Close };
    let name = app.notebooks[ni].meta.name.clone();
    let warn = egui::Color32::from_rgb(220, 140, 0);
    ui.horizontal(|ui| {
        ui.label(RichText::new(format!("Записная книжка «{name}»")).strong());
        if ui.button("Создать копию сейчас").clicked() {
            app.run_backups_now();
        }
        if ui.button("Открыть папку").clicked() {
            let _ = std::fs::create_dir_all(&app.settings.backup_dir);
            crate::winapi::shell_open(&app.settings.backup_dir.to_string_lossy());
        }
        if ui.button("Настройки…").clicked() {
            app.dialogs.push(Dialog::Settings(3));
        }
    });
    if !app.settings.backup_enabled {
        ui.label(RichText::new("Автоматические копии выключены").color(warn));
    }
    if app.settings.backup_mirror.is_none() {
        ui.label(RichText::new("Совет: задайте вторую папку на другом диске или флешке — тогда копии переживут и поломку диска.").small().color(warn));
    }
    let running = app.backups_running.lock().map(|r| !r.is_empty()).unwrap_or(false);
    if running {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label("Идёт резервное копирование…");
        });
    }
    if let Some(l) = app.backup_log.iter().find(|l| l.name == name) {
        match &l.result {
            Ok(t) => ui.label(RichText::new(format!("Последняя копия: {} — {t}", l.time.format("%d.%m.%Y %H:%M"))).small()),
            Err(e) => ui.label(RichText::new(format!("Ошибка {}: {e}", l.time.format("%d.%m.%Y %H:%M"))).small().color(egui::Color32::from_rgb(220, 60, 60))),
        };
    }
    ui.separator();
    // Copies from both folders, newest first.
    let mut list: Vec<(std::path::PathBuf, bool)> = crate::storage::list_backups(&app.settings.backup_dir, &name).into_iter().map(|p| (p, false)).collect();
    if let Some(m) = &app.settings.backup_mirror {
        list.extend(crate::storage::list_backups(m, &name).into_iter().map(|p| (p, true)));
    }
    list.sort_by(|a, b| b.0.file_name().cmp(&a.0.file_name()));
    let mut browse: Option<std::path::PathBuf> = None;
    egui::ScrollArea::vertical().id_salt("backup_list").max_height(220.0).show(ui, |ui| {
        if list.is_empty() {
            ui.label(RichText::new("Резервных копий пока нет").weak());
        }
        for (p, mirror) in &list {
            let when = p
                .file_stem()
                .and_then(|s| chrono::NaiveDateTime::parse_from_str(s.to_string_lossy().get(..15)?, "%Y%m%d-%H%M%S").ok())
                .map(|t| t.format("%d.%m.%Y  %H:%M").to_string())
                .unwrap_or_else(|| p.file_name().unwrap_or_default().to_string_lossy().to_string());
            let size = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
            let selected = app.backup_view.as_ref().is_some_and(|v| &v.0 == p);
            ui.horizontal(|ui| {
                ui.label(RichText::new(when).monospace());
                ui.label(RichText::new(crate::storage::human_size(size)).weak());
                if *mirror {
                    ui.label(RichText::new("вторая папка").small().weak());
                }
                if ui.selectable_label(selected, "Страницы…").on_hover_text("Вернуть отдельные страницы из этой копии").clicked() {
                    browse = Some(p.clone());
                }
                if ui.button("Восстановить блокнот").on_hover_text("Вернуть всю записную книжку к состоянию этой копии").clicked() {
                    app.dialogs.push(Dialog::Confirm {
                        text: "Вернуть всю записную книжку к состоянию этой копии? Текущее состояние не удаляется — оно будет сохранено рядом в папке «…before-restore…».".into(),
                        action: ConfirmAction::RestoreBackup(p.clone()),
                    });
                }
            });
        }
    });
    if let Some(p) = browse {
        if app.backup_view.as_ref().is_some_and(|v| v.0 == p) {
            app.backup_view = None;
        } else {
            let pages = crate::storage::read_backup(&p, app.backup_key.as_ref()).and_then(|a| crate::storage::backup_pages(&a));
            app.backup_view = Some((p, pages, String::new()));
        }
    }
    let mut restore: Option<(std::path::PathBuf, PageMeta)> = None;
    if let Some((path, pages, filter)) = &mut app.backup_view {
        ui.separator();
        match pages {
            Err(e) => {
                ui.label(RichText::new(e.as_str()).color(warn));
            }
            Ok(pages) => {
                ui.horizontal(|ui| {
                    ui.label(format!("Страниц в копии: {}", pages.len()));
                    ui.add(egui::TextEdit::singleline(filter).hint_text("Найти страницу").desired_width(220.0));
                });
                let f = filter.to_lowercase();
                egui::ScrollArea::vertical().id_salt("backup_pages").max_height(220.0).show(ui, |ui| {
                    for (sec, pm) in pages.iter().filter(|(s, pm)| f.is_empty() || pm.title.to_lowercase().contains(&f) || s.to_lowercase().contains(&f)) {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(sec.as_str()).weak());
                            ui.label(if pm.title.is_empty() { "Без заголовка" } else { pm.title.as_str() });
                            ui.label(RichText::new(pm.modified.format("%d.%m.%Y %H:%M").to_string()).small().weak());
                            if ui.button("Вернуть копию").on_hover_text("Добавить эту страницу из копии рядом с текущими — ничего не заменяется").clicked() {
                                restore = Some((path.clone(), pm.clone()));
                            }
                        });
                    }
                });
            }
        }
    }
    if let Some((b, pm)) = restore {
        app.restore_page_from_backup(&b, &pm);
    }
    if !app.backup_log.is_empty() {
        ui.separator();
        egui::CollapsingHeader::new("Журнал резервного копирования").show(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("backup_log").max_height(140.0).show(ui, |ui| {
                for l in &app.backup_log {
                    let (t, c) = match &l.result {
                        Ok(t) => (format!("✔ {} · {} · {t}", l.time.format("%d.%m %H:%M"), l.name), ui.visuals().text_color()),
                        Err(e) => (format!("✖ {} · {} · {e}", l.time.format("%d.%m %H:%M"), l.name), egui::Color32::from_rgb(220, 60, 60)),
                    };
                    ui.label(RichText::new(t).small().color(c));
                }
            });
        });
    }
    Res::Keep
}

fn tags_editor(app: &mut App, ui: &mut Ui) -> Res {
    let mut changed = false;
    let mut del = None;
    egui::Grid::new("tags_grid").striped(true).show(ui, |ui| {
        ui.label("Значок");
        ui.label("Название");
        ui.label("Цвет");
        ui.label("Ctrl+");
        ui.label("Задача");
        ui.end_row();
        for (i, t) in app.settings.tags.iter_mut().enumerate() {
            changed |= ui.add(egui::TextEdit::singleline(&mut t.icon).desired_width(30.0)).changed();
            changed |= ui.add(egui::TextEdit::singleline(&mut t.name).desired_width(140.0)).changed();
            let mut c = egui::Color32::from_rgb(t.color[0], t.color[1], t.color[2]);
            if ui.color_edit_button_srgba(&mut c).changed() {
                t.color = [c.r(), c.g(), c.b()];
                changed = true;
            }
            let mut sc = t.shortcut.map(|d| d.to_string()).unwrap_or_default();
            if ui.add(egui::TextEdit::singleline(&mut sc).desired_width(24.0)).changed() {
                t.shortcut = sc.trim().parse::<u8>().ok().filter(|d| (1..=9).contains(d));
                changed = true;
            }
            changed |= ui.checkbox(&mut t.is_task, "").changed();
            if ui.small_button("🗑").clicked() {
                del = Some(i);
            }
            ui.end_row();
        }
    });
    if let Some(i) = del {
        app.settings.tags.remove(i);
        changed = true;
    }
    ui.horizontal(|ui| {
        if ui.button("+ Тег").clicked() {
            let id = app.settings.tags.iter().map(|t| t.id).max().unwrap_or(0) + 1;
            app.settings.tags.push(TagDef { id, name: "Новый тег".into(), icon: "●".into(), color: [100, 100, 220], shortcut: None, is_task: false });
            changed = true;
        }
        if ui.button("Стандартные теги").clicked() {
            app.settings.tags = builtin_tags();
            changed = true;
        }
    });
    if changed {
        app.settings.save();
    }
    Res::Keep
}

fn settings_ui(app: &mut App, ui: &mut Ui, ctx: &Context, tab: &mut usize) -> Res {
    ui.horizontal(|ui| {
        for (i, l) in ["Основные", "Ввод и перо", "Редактирование", "Данные", "Конфиденциальность"].iter().enumerate() {
            ui.selectable_value(tab, i, *l);
        }
    });
    ui.separator();
    let s = &mut app.settings;
    let mut theme_changed = false;
    let mut save = false;
    egui::ScrollArea::vertical().max_height(440.0).show(ui, |ui| match *tab {
        0 => {
            ui.horizontal(|ui| {
                ui.label("Тема:");
                for (t, l) in [(crate::settings::Theme::Light, "Светлая"), (crate::settings::Theme::Dark, "Тёмная"), (crate::settings::Theme::System, "Системная")] {
                    theme_changed |= ui.radio_value(&mut s.theme, t, l).changed();
                }
            });
            ui.horizontal(|ui| {
                ui.label("Язык интерфейса:");
                ui.label("Русский");
            });
            theme_changed |= ui.add(egui::Slider::new(&mut s.ui_scale, 0.75..=2.0).text("Масштаб интерфейса")).changed();
            theme_changed |= ui.checkbox(&mut s.touch_mode, "Режим для сенсорных экранов (крупные элементы)").changed();
            ui.horizontal(|ui| {
                ui.label("Шрифт по умолчанию:");
                egui::ComboBox::from_id_salt("deffont").selected_text(s.default_font.clone()).show_ui(ui, |ui| {
                    for f in crate::fonts::font_names() {
                        save |= ui.selectable_value(&mut s.default_font, f.clone(), f).changed();
                    }
                });
                save |= ui.add(egui::DragValue::new(&mut s.default_size).range(8.0..=48.0)).changed();
            });
            ui.horizontal(|ui| {
                ui.label("Имя автора:");
                save |= ui.text_edit_singleline(&mut s.author).changed();
            });
            ui.horizontal(|ui| {
                ui.label("Папка блокнотов:");
                ui.label(s.notebooks_dir.display().to_string());
                if ui.button("Изменить…").clicked() {
                    if let Some(p) = rfd::FileDialog::new().pick_folder() {
                        s.notebooks_dir = p;
                        save = true;
                    }
                }
            });
            if ui.checkbox(&mut s.autostart, "Запускать вместе с Windows").changed() {
                crate::winapi::set_autostart(s.autostart);
                save = true;
            }
            ui.horizontal(|ui| {
                if ui.button("Добавить «Добавить файл в блокнот» в меню проводника").clicked() {
                    crate::winapi::set_explorer_menu(true);
                }
                if ui.button("Убрать").clicked() {
                    crate::winapi::set_explorer_menu(false);
                }
            });
        }
        1 => {
            ui.label(RichText::new("Графический планшет и сенсорный экран").strong());
            ui.label(RichText::new("Перо читается напрямую через Windows Ink (WM_POINTER): нажим, наклон, кнопка и ластик. Для планшетов Wacom включите «Windows Ink» в драйвере.").small().weak());
            save |= ui.checkbox(&mut s.pen_draws, "Перо рисует").changed();
            save |= ui.checkbox(&mut s.touch_draws, "Палец рисует (иначе — прокрутка и масштаб)").changed();
            save |= ui.checkbox(&mut s.mouse_draws, "Мышь рисует в режиме пера").changed();
            save |= ui.add(egui::Slider::new(&mut s.smoothing, 0.0..=0.85).text("Сглаживание линий")).changed();
            save |= ui.add(egui::Slider::new(&mut s.pressure_curve, 0.3..=3.0).text("Кривая нажима (меньше — мягче)")).changed();
            save |= ui.checkbox(&mut s.auto_shapes, "Выравнивать нарисованные от руки фигуры").changed();
            // Pressure test area.
            ui.label("Проверка нажима: проведите пером в рамке");
            let (rect, _) = ui.allocate_exact_size(egui::vec2(300.0, 60.0), egui::Sense::hover());
            ui.painter().rect_stroke(rect, 4.0, egui::Stroke::new(1.0, egui::Color32::GRAY), egui::StrokeKind::Inside);
            let samples = crate::pen::peek_last_pressure();
            if let Some(p) = samples {
                ui.painter().rect_filled(egui::Rect::from_min_size(rect.min, egui::vec2(rect.width() * p, rect.height())), 4.0, egui::Color32::from_rgba_unmultiplied(60, 130, 255, 80));
                ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, format!("нажим {:.0}%", p * 100.0), egui::FontId::proportional(14.0), ui.visuals().text_color());
                ctx.request_repaint();
            }
        }
        2 => {
            save |= ui.checkbox(&mut s.spellcheck, "Проверка орфографии (словари Windows)").changed();
            ui.horizontal(|ui| {
                ui.label("Язык проверки:");
                for l in ["ru-RU", "en-US", "de-DE", "fr-FR", "uk-UA"] {
                    save |= ui.radio_value(&mut s.spell_lang, l.to_string(), l).changed();
                }
            });
            save |= ui.checkbox(&mut s.autocorrect, "Автозамена и заглавные буквы в начале предложения").changed();
            save |= ui.checkbox(&mut s.auto_lists, "Автоматические списки и продолжение списков").changed();
            ui.label(RichText::new("Автозамена").strong());
            let mut del = None;
            let keys: Vec<String> = s.autocorrect_map.keys().cloned().collect();
            egui::Grid::new("ac").show(ui, |ui| {
                for k in keys {
                    ui.label(&k);
                    ui.label("→");
                    if let Some(v) = s.autocorrect_map.get_mut(&k) {
                        save |= ui.text_edit_singleline(v).changed();
                    }
                    if ui.small_button("✕").clicked() {
                        del = Some(k);
                    }
                    ui.end_row();
                }
            });
            if let Some(k) = del {
                s.autocorrect_map.remove(&k);
                save = true;
            }
            ui.horizontal(|ui| {
                let id = ui.id().with("newac");
                let mut pair: (String, String) = ui.data_mut(|d| d.get_temp(id).unwrap_or_default());
                ui.add(egui::TextEdit::singleline(&mut pair.0).desired_width(80.0).hint_text("что"));
                ui.add(egui::TextEdit::singleline(&mut pair.1).desired_width(80.0).hint_text("на что"));
                if ui.button("Добавить").clicked() && !pair.0.is_empty() {
                    s.autocorrect_map.insert(pair.0.clone(), pair.1.clone());
                    pair = Default::default();
                    save = true;
                }
                ui.data_mut(|d| d.insert_temp(id, pair));
            });
            ui.label(RichText::new("Пользовательский словарь").strong());
            let mut dict = s.user_dictionary.join("\n");
            if ui.add(egui::TextEdit::multiline(&mut dict).desired_rows(4)).changed() {
                s.user_dictionary = dict.lines().map(|l| l.trim().to_owned()).filter(|l| !l.is_empty()).collect();
                save = true;
            }
            ui.horizontal(|ui| {
                ui.label("Вставка из буфера:");
                save |= ui.radio_value(&mut s.paste_mode, crate::settings::PasteMode::KeepFormatting, "Сохранить форматирование").changed();
                save |= ui.radio_value(&mut s.paste_mode, crate::settings::PasteMode::TextOnly, "Только текст").changed();
                save |= ui.radio_value(&mut s.paste_mode, crate::settings::PasteMode::Merge, "Объединить").changed();
            });
        }
        3 => {
            save |= ui.add(egui::Slider::new(&mut s.autosave_secs, 1..=60).text("Автосохранение, сек")).changed();
            save |= ui.checkbox(&mut s.backup_enabled, "Автоматические резервные копии").changed();
            save |= ui.add(egui::Slider::new(&mut s.backup_minutes, 5..=1440).logarithmic(true).text("Проверять изменения каждые, мин")).changed();
            save |= ui.checkbox(&mut s.backup_on_exit, "Копия при выходе из программы").changed();
            ui.label(RichText::new("Копия создаётся, только если в блокноте что-то изменилось, и проверяется после записи.").small().weak());
            save |= ui.add(egui::Slider::new(&mut s.backup_recent, 1..=200).text("Последних копий хранить всегда")).changed();
            save |= ui.add(egui::Slider::new(&mut s.backup_days, 1..=365).text("По одной в день, дней")).changed();
            save |= ui.add(egui::Slider::new(&mut s.backup_weeks, 0..=260).text("По одной в неделю, недель")).changed();
            ui.horizontal(|ui| {
                ui.label(format!("Папка: {}", s.backup_dir.display()));
                if ui.button("Изменить…").clicked() {
                    if let Some(p) = rfd::FileDialog::new().pick_folder() {
                        s.backup_dir = p;
                        save = true;
                    }
                }
            });
            ui.horizontal(|ui| {
                match &s.backup_mirror {
                    Some(m) => ui.label(format!("Вторая папка: {}", m.display())),
                    None => ui.label(RichText::new("Вторая папка не задана — копии лежат на том же диске").color(egui::Color32::from_rgb(220, 140, 0))),
                };
                if ui.button("Выбрать…").on_hover_text("Другой диск, флешка или папка, которая синхронизируется с облаком").clicked() {
                    if let Some(p) = rfd::FileDialog::new().set_title("Вторая папка для резервных копий").pick_folder() {
                        s.backup_mirror = Some(p);
                        save = true;
                    }
                }
                if s.backup_mirror.is_some() && ui.button("Убрать").clicked() {
                    s.backup_mirror = None;
                    save = true;
                }
            });
            save |= ui.checkbox(&mut s.backup_encrypt, "Шифровать резервные копии паролем").changed();
            save |= ui.add(egui::Slider::new(&mut s.trash_days, 1..=365).text("Корзина, дней")).changed();
            save |= ui.add(egui::Slider::new(&mut s.version_minutes, 1..=120).text("Версия страницы каждые, мин")).changed();
            save |= ui.add(egui::Slider::new(&mut s.version_keep, 1..=500).text("Хранить версий")).changed();
            save |= ui.add(egui::Slider::new(&mut s.version_days, 1..=3650).text("Срок хранения версий, дней")).changed();
            save |= ui.checkbox(&mut s.compress_images, "Сжимать изображения при вставке").changed();
            save |= ui.add(egui::Slider::new(&mut s.image_max_dim, 800..=8000).text("Макс. размер стороны, px")).changed();
            save |= ui.add(egui::Slider::new(&mut s.jpeg_quality, 50..=100).text("Качество JPEG")).changed();
            ui.horizontal(|ui| {
                if ui.button("Очистить кэш изображений").clicked() {
                    let _ = std::fs::remove_dir_all(crate::settings::Settings::temp_dir());
                }
                if ui.button("Перестроить индекс поиска").clicked() {
                    app_rebuild_flag(ctx);
                }
            });
        }
        _ => {
            save |= ui.checkbox(&mut s.offline_only, "Режим «Только офлайн» (программа не обращается к сети)").changed();
            ui.label(RichText::new("Телеметрия и отправка данных отсутствуют. Ссылки на сайты открываются в браузере только по вашему щелчку.").small().weak());
            save |= ui.add(egui::Slider::new(&mut s.idle_lock_minutes, 0..=120).text("Блокировка разделов при бездействии, мин (0 — выкл.)")).changed();
            save |= ui.checkbox(&mut s.clear_temp_on_exit, "Удалять временные файлы при выходе").changed();
            ui.separator();
            ui.label(RichText::new("Языковые пакеты и модели распознавания").strong());
            let langs = crate::winapi::ocr_languages();
            ui.label(format!("Установлено языков OCR: {}", if langs.is_empty() { "нет".into() } else { langs.join(", ") }));
            if ui.button("Открыть параметры языков Windows").clicked() {
                crate::winapi::shell_open("ms-settings:regionlanguage");
            }
            if ui.button("Голоса для чтения вслух").clicked() {
                crate::winapi::shell_open("ms-settings:speech");
            }
        }
    });
    if theme_changed {
        save = true;
        app.apply_theme(ctx);
    }
    if save {
        app.settings.save();
        app.speller.set_language(&app.settings.spell_lang, &app.settings.user_dictionary);
    }
    if ctx.data(|d| d.get_temp::<bool>(egui::Id::new("rebuild_index")).unwrap_or(false)) {
        ctx.data_mut(|d| d.remove::<bool>(egui::Id::new("rebuild_index")));
        app.rebuild_index(ctx);
    }
    if ui.button("Закрыть").clicked() {
        return Res::Close;
    }
    Res::Keep
}

fn app_rebuild_flag(ctx: &Context) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new("rebuild_index"), true));
}
