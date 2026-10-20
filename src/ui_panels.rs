//! Right-hand panels: search, tag summary, my tasks, smart helper, comments,
//! version history and read-aloud.

use egui::{Color32, RichText, Ui};

use crate::app::*;
use crate::helper;
use crate::model::*;
use crate::search::{self, Scope};

#[derive(Default)]
pub struct ReadingState {
    pub large: bool,
    pub wide_spacing: bool,
    pub line_focus: bool,
    pub syllables: bool,
    pub main_on_top: bool,
    pub text: String,
    pub focus_line: usize,
}

pub fn draw(app: &mut App, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    ui.horizontal(|ui| {
        let title = match app.right {
            Some(RightPanel::Search) => "Поиск",
            Some(RightPanel::Tags) => "Сводка тегов",
            Some(RightPanel::Tasks) => "Мои задачи",
            Some(RightPanel::Helper) => "Умный помощник",
            Some(RightPanel::Comments) => "Комментарии",
            Some(RightPanel::Versions) => "История версий",
            Some(RightPanel::Reading) => "Чтение",
            None => "",
        };
        ui.heading(title);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("✕").clicked() {
                if app.right == Some(RightPanel::Reading) {
                    app.tts.stop();
                }
                app.right = None;
            }
        });
    });
    ui.separator();
    match app.right {
        Some(RightPanel::Search) => search_panel(app, ui, &ctx),
        Some(RightPanel::Tags) => tags_panel(app, ui, &ctx, false),
        Some(RightPanel::Tasks) => tags_panel(app, ui, &ctx, true),
        Some(RightPanel::Helper) => helper_panel(app, ui, &ctx),
        Some(RightPanel::Comments) => comments_panel(app, ui),
        Some(RightPanel::Versions) => versions_panel(app, ui),
        Some(RightPanel::Reading) => reading_panel(app, ui),
        None => {}
    }
}

fn search_panel(app: &mut App, ui: &mut Ui, ctx: &egui::Context) {
    let r = ui.add(egui::TextEdit::singleline(&mut app.search_q).hint_text("Искать во всех заметках…").desired_width(f32::INFINITY));
    if app.search_focus {
        r.request_focus();
        app.search_focus = false;
    }
    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && !app.search_q.trim().is_empty() {
        let q = app.search_q.trim().to_owned();
        app.settings.recent_searches.retain(|x| *x != q);
        app.settings.recent_searches.insert(0, q);
        app.settings.recent_searches.truncate(15);
        app.settings.save();
    }
    let f = &mut app.search_filters;
    ui.horizontal_wrapped(|ui| {
        egui::ComboBox::from_id_salt("scope").selected_text(f.scope.label()).show_ui(ui, |ui| {
            for s in Scope::ALL {
                ui.selectable_value(&mut f.scope, s, s.label());
            }
        });
        let tag_name = f.tag.and_then(|t| app.settings.tag(t)).map(|t| t.name.clone()).unwrap_or_else(|| "Любой тег".into());
        egui::ComboBox::from_id_salt("stag").selected_text(tag_name).show_ui(ui, |ui| {
            ui.selectable_value(&mut f.tag, None, "Любой тег");
            for t in &app.settings.tags {
                ui.selectable_value(&mut f.tag, Some(t.id), format!("{} {}", t.icon, t.name));
            }
        });
    });
    ui.horizontal_wrapped(|ui| {
        for (k, l) in [(search::K_TEXT, "Текст"), (search::K_INK, "Рукопись"), (search::K_IMAGE, "Изображения"), (search::K_FILE, "Файлы"), (search::K_TABLE, "Таблицы")] {
            let mut on = f.kinds & k != 0;
            if ui.toggle_value(&mut on, l).changed() {
                f.kinds ^= k;
            }
        }
    });
    ui.horizontal(|ui| {
        ui.label("Автор:");
        ui.add(egui::TextEdit::singleline(&mut f.author).desired_width(100.0));
        egui::ComboBox::from_id_salt("sdate")
            .selected_text(match f.from {
                None => "За всё время".to_string(),
                Some(d) => format!("С {}", d.format("%d.%m.%Y")),
            })
            .show_ui(ui, |ui| {
                let today = chrono::Local::now().date_naive();
                if ui.selectable_label(f.from.is_none(), "За всё время").clicked() {
                    f.from = None;
                }
                for (l, d) in [("Сегодня", 0), ("Неделя", 7), ("Месяц", 30), ("Год", 365)] {
                    if ui.selectable_label(false, l).clicked() {
                        f.from = Some(today - chrono::Duration::days(d));
                    }
                }
            });
    });
    ui.horizontal(|ui| {
        if ui.small_button("☆ Сохранить поиск").clicked() && !app.search_q.trim().is_empty() {
            app.settings.saved_searches.push(crate::settings::SavedSearch { name: app.search_q.clone(), query: app.search_q.clone() });
            app.settings.save();
        }
        if !app.settings.saved_searches.is_empty() {
            ui.menu_button("Сохранённые", |ui| {
                let mut del = None;
                for (i, s) in app.settings.saved_searches.iter().enumerate() {
                    ui.horizontal(|ui| {
                        if ui.button(&s.name).clicked() {
                            app.search_q = s.query.clone();
                            ui.close();
                        }
                        if ui.small_button("✕").clicked() {
                            del = Some(i);
                        }
                    });
                }
                if let Some(i) = del {
                    app.settings.saved_searches.remove(i);
                    app.settings.save();
                }
            });
        }
    });
    ui.separator();
    if app.search_q.trim().is_empty() && app.search_filters.tag.is_none() {
        if !app.settings.recent_searches.is_empty() {
            ui.label(RichText::new("Недавние").weak());
            for q in app.settings.recent_searches.clone() {
                if ui.link(&q).clicked() {
                    app.search_q = q;
                }
            }
        }
        return;
    }
    let cur_nb = app.nb().map(|n| n.path.clone());
    let hits = search::query(&app.index, &app.search_q, &app.search_filters, cur_nb.as_deref(), app.cur_section, app.cur_page);
    ui.label(format!("Найдено страниц: {}", hits.len()));
    let mut open = None;
    egui::ScrollArea::vertical().id_salt("search_res").show(ui, |ui| {
        for h in &hits {
            let r = ui.add(egui::Button::new(RichText::new(&h.title).strong()).frame(false));
            ui.label(RichText::new(&h.location).small().weak());
            ui.label(RichText::new(&h.snippet).small());
            ui.separator();
            if r.clicked() {
                open = Some(h.page);
            }
        }
    });
    if let Some(p) = open {
        app.open_page(Some(p));
        let _ = ctx;
    }
}

fn tags_panel(app: &mut App, ui: &mut Ui, ctx: &egui::Context, tasks_only: bool) {
    ui.horizontal_wrapped(|ui| {
        if !tasks_only {
            egui::ComboBox::from_id_salt("tscope").selected_text(app.summary_scope.label()).show_ui(ui, |ui| {
                for s in Scope::ALL {
                    ui.selectable_value(&mut app.summary_scope, s, s.label());
                }
            });
            let tag_name = app.summary_tag.and_then(|t| app.settings.tag(t)).map(|t| t.name.clone()).unwrap_or_else(|| "Все теги".into());
            egui::ComboBox::from_id_salt("ttag").selected_text(tag_name).show_ui(ui, |ui| {
                ui.selectable_value(&mut app.summary_tag, None, "Все теги");
                for t in &app.settings.tags {
                    ui.selectable_value(&mut app.summary_tag, Some(t.id), format!("{} {}", t.icon, t.name));
                }
            });
        }
        egui::ComboBox::from_id_salt("tstate").selected_text(["Все", "Невыполненные", "Выполненные"][app.summary_state]).show_ui(ui, |ui| {
            for (i, l) in ["Все", "Невыполненные", "Выполненные"].iter().enumerate() {
                ui.selectable_value(&mut app.summary_state, i, *l);
            }
        });
        if ui.button("⟳").on_hover_text("Обновить").clicked() {
            app.save_all();
            app.collect_summary(ctx);
        }
    });
    if !tasks_only && ui.button("Вставить сводку на новую страницу").clicked() {
        insert_summary_page(app, ctx);
    }
    ui.separator();
    let cur_nb = app.nb().map(|n| n.path.clone());
    let mut rows: Vec<SummaryRow> = app
        .summary
        .iter()
        .filter(|r| {
            if tasks_only {
                return r.task.as_ref().is_some_and(|t| match app.summary_state {
                    2 => t.done,
                    0 => true,
                    _ => !t.done,
                });
            }
            let scope_ok = match app.summary_scope {
                Scope::Page => Some(r.page) == app.cur_page,
                Scope::Section => app.nb().and_then(|nb| nb.meta.section_of_page(r.page)) == app.cur_section && app.cur_section.is_some(),
                Scope::Notebook => Some(&r.nb) == cur_nb.as_ref(),
                Scope::All => true,
            };
            let tag_ok = app.summary_tag.is_none_or(|t| r.tags.contains(&t));
            let state_ok = match app.summary_state {
                1 => r.task.as_ref().is_some_and(|t| !t.done),
                2 => r.task.as_ref().is_some_and(|t| t.done),
                _ => true,
            };
            scope_ok && tag_ok && state_ok && !r.tags.is_empty()
        })
        .cloned()
        .collect();
    if tasks_only {
        rows.sort_by(|a, b| {
            let da = a.task.as_ref().and_then(|t| t.due);
            let db = b.task.as_ref().and_then(|t| t.due);
            match (da, db) {
                (Some(x), Some(y)) => x.cmp(&y),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                _ => b.modified.cmp(&a.modified),
            }
            .then(a.task.as_ref().map(|t| if t.priority == 0 { 9 } else { t.priority }).cmp(&b.task.as_ref().map(|t| if t.priority == 0 { 9 } else { t.priority })))
        });
    } else {
        rows.sort_by(|a, b| a.tags.first().cmp(&b.tags.first()).then(b.modified.cmp(&a.modified)));
    }
    ui.label(format!("Записей: {}", rows.len()));
    let mut open: Option<(Id, Id)> = None;
    let mut toggle: Option<(Id, Id)> = None;
    egui::ScrollArea::vertical().id_salt("tags_res").show(ui, |ui| {
        let mut last_tag = None;
        for r in &rows {
            if !tasks_only && r.tags.first() != last_tag.as_ref() {
                last_tag = r.tags.first().copied();
                if let Some(t) = last_tag.and_then(|t| app.settings.tag(t)) {
                    ui.add_space(4.0);
                    ui.label(RichText::new(format!("{} {}", t.icon, t.name)).strong().color(Color32::from_rgb(t.color[0], t.color[1], t.color[2])));
                }
            }
            ui.horizontal(|ui| {
                if let Some(t) = &r.task {
                    let mut done = t.done;
                    if ui.checkbox(&mut done, "").changed() {
                        toggle = Some((r.page, r.item));
                    }
                }
                let text: String = r.text.lines().next().unwrap_or("").chars().take(70).collect();
                if ui.link(if text.is_empty() { "(пусто)".to_string() } else { text }).clicked() {
                    open = Some((r.page, r.item));
                }
            });
            let mut sub = format!("{} › {}", r.section, r.page_title);
            if let Some(due) = r.task.as_ref().and_then(|t| t.due) {
                sub = format!("⏰ {} · {sub}", due.format("%d.%m.%Y %H:%M"));
            }
            ui.label(RichText::new(sub).small().weak());
        }
    });
    if let Some((p, it)) = open {
        app.open_link(&format!("note://page/{p}#{it}"));
    }
    if let Some((p, it)) = toggle {
        if app.ensure_loaded(p) {
            if let Some(i) = app.pages.get_mut(&p).and_then(|pg| pg.item_mut(it)) {
                if let Some(t) = i.task.as_mut() {
                    t.done = !t.done;
                }
            }
            app.mark_dirty(p);
            for r in app.summary.iter_mut().filter(|r| r.item == it) {
                if let Some(t) = r.task.as_mut() {
                    t.done = !t.done;
                }
            }
        }
    }
}

pub fn insert_summary_page(app: &mut App, ctx: &egui::Context) {
    app.save_all();
    let mut text = format!("# Сводка тегов\n*{}*\n", chrono::Local::now().format("%d.%m.%Y %H:%M"));
    let tags = app.settings.tags.clone();
    for t in &tags {
        let rows: Vec<&SummaryRow> = app.summary.iter().filter(|r| r.tags.contains(&t.id)).collect();
        if rows.is_empty() {
            continue;
        }
        text.push_str(&format!("\n## {} {}\n", t.icon, t.name));
        for r in rows {
            let line: String = r.text.lines().next().unwrap_or("").chars().take(120).collect();
            let prefix = match &r.task {
                Some(tk) if tk.done => "[x] ",
                Some(_) => "[ ] ",
                None => "- ",
            };
            text.push_str(&format!("{prefix}{line} — [{}](note://page/{}#{})\n", r.page_title, r.page, r.item));
        }
    }
    let Some(pid) = app.quick_notes_page("Сводка тегов") else { return };
    let (size, font, author) = (app.settings.default_size, app.settings.default_font.clone(), app.settings.author.clone());
    if let Some(p) = app.pages.get_mut(&pid) {
        p.items.push(Item::new(egui::pos2(0.0, 0.0), 700.0, ItemKind::Text(TextBlock::new(&text, size, &font)), &author));
    }
    app.mark_dirty(pid);
    app.collect_summary(ctx);
}

fn current_text(app: &App) -> (String, String) {
    let Some(pid) = app.cur_page else { return (String::new(), String::new()) };
    let title = app.nb().and_then(|nb| nb.meta.page_meta(pid)).map(|p| p.title.clone()).unwrap_or_default();
    let Some(page) = app.pages.get(&pid) else { return (title, String::new()) };
    // Prefer the edited / selected text block.
    if let Some(ed) = app.editors.get(&pid) {
        if let Some(id) = ed.editing.or_else(|| ed.selection.items.first().copied()) {
            if let Some(it) = page.item(id) {
                return (title, it.plain_text());
            }
        }
    }
    (title, page.plain_text())
}

fn append_text(app: &mut App, text: &str) {
    if app.cur_page.is_none() {
        return;
    }
    let (size, font) = (app.settings.default_size, app.settings.default_font.clone());
    app.insert_item(ItemKind::Text(TextBlock::new(text, size, &font)), 560.0);
    app.with_page(|_, ed| ed.editing = None);
}

fn replace_selected_text(app: &mut App, f: impl Fn(&str) -> String) -> bool {
    let Some(pid) = app.cur_page else { return false };
    let done = app.with_page(|page, ed| {
        let Some(id) = ed.editing.or_else(|| ed.selection.items.first().copied()) else { return false };
        ed.snapshot(page);
        if let Some(ItemKind::Text(t)) = page.item_mut(id).map(|i| &mut i.kind) {
            t.text = f(&t.text);
            return true;
        }
        false
    });
    if done == Some(true) {
        app.mark_dirty(pid);
        true
    } else {
        false
    }
}

fn helper_panel(app: &mut App, ui: &mut Ui, _ctx: &egui::Context) {
    ui.label(RichText::new("Работает полностью на этом компьютере, данные никуда не отправляются.").small().weak());
    ui.add_space(4.0);
    ui.label(RichText::new("Текущая страница").strong());
    ui.horizontal_wrapped(|ui| {
        if ui.button("Краткая сводка").clicked() {
            let (_, t) = current_text(app);
            let s = helper::summarize(&t, 4);
            app.helper_out = vec![("Сводка".into(), if s.is_empty() { "Мало текста для сводки".into() } else { s })];
        }
        if ui.button("Сводка раздела").clicked() {
            let mut all = String::new();
            if let (Some(nb), Some(sid)) = (app.nb(), app.cur_section) {
                if let Some(s) = nb.meta.section(sid) {
                    for p in &s.pages {
                        let text = app.pages.get(&p.id).map(|pg| pg.plain_text()).or_else(|| nb.load_page(p.id).ok().map(|pg| pg.plain_text())).unwrap_or_default();
                        all.push_str(&text);
                        all.push('\n');
                    }
                }
            }
            app.helper_out = vec![("Сводка раздела".into(), helper::summarize(&all, 7))];
        }
        if ui.button("Задачи и сроки").clicked() {
            let (_, t) = current_text(app);
            let tasks = helper::extract_tasks(&t);
            app.helper_out = tasks
                .into_iter()
                .map(|t| (t.due.map(|d| format!("до {}", d.format("%d.%m.%Y"))).unwrap_or_else(|| "без срока".into()), t.text))
                .collect();
            if app.helper_out.is_empty() {
                app.helper_out.push(("Задачи".into(), "Задач не найдено".into()));
            }
        }
        if ui.button("Предложить теги и заголовок").clicked() {
            let (_, t) = current_text(app);
            let names: Vec<(u32, String)> = app.settings.tags.iter().map(|t| (t.id, t.name.clone())).collect();
            let (tags, kw) = helper::suggest_tags(&t, &names);
            let tag_names: Vec<String> = tags.iter().filter_map(|id| app.settings.tag(*id)).map(|t| format!("{} {}", t.icon, t.name)).collect();
            app.helper_out = vec![
                ("Заголовок".into(), helper::suggest_title(&t)),
                ("Теги".into(), if tag_names.is_empty() { "—".into() } else { tag_names.join(", ") }),
                ("Ключевые слова".into(), kw.join(", ")),
            ];
        }
        if ui.button("Карточки для повторения").clicked() {
            let (_, t) = current_text(app);
            let cards = helper::flashcards(&t);
            if cards.is_empty() {
                app.helper_out = vec![("Карточки".into(), "Не нашлось пар «термин — определение» или заголовков с текстом".into())];
            } else {
                app.dialogs.push(Dialog::Flashcards { cards, idx: 0, flipped: false });
            }
        }
    });
    ui.label(RichText::new("Выделенный текстовый блок").strong());
    ui.horizontal_wrapped(|ui| {
        if ui.button("Исправить").on_hover_text("Пробелы, знаки препинания, повторы слов, заглавные буквы").clicked() && !replace_selected_text(app, helper::cleanup) {
            app.set_status("Выберите текстовый блок");
        }
        if ui.button("Перефразировать").on_hover_text("Упрощает канцелярские обороты").clicked() && !replace_selected_text(app, helper::rephrase) {
            app.set_status("Выберите текстовый блок");
        }
    });
    ui.separator();
    ui.label(RichText::new("План или структура по теме").strong());
    ui.add(egui::TextEdit::singleline(&mut app.helper_topic).hint_text("Тема").desired_width(f32::INFINITY));
    ui.horizontal(|ui| {
        for (i, l) in ["Конспект", "Проект", "Доклад"].iter().enumerate() {
            if ui.button(*l).clicked() && !app.helper_topic.trim().is_empty() {
                let text = helper::outline(&app.helper_topic, i);
                append_text(app, &text);
            }
        }
    });
    ui.separator();
    ui.label(RichText::new("Вопрос по заметкам").strong());
    let r = ui.add(egui::TextEdit::singleline(&mut app.helper_question).hint_text("Например: когда встреча с поставщиком?").desired_width(f32::INFINITY));
    if (ui.button("Найти ответ").clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))) && !app.helper_question.trim().is_empty() {
        let entries = app.index.entries.read().unwrap();
        let docs = entries.iter().map(|e| (e.title.as_str(), e.text.as_str()));
        let ans = helper::answer(&app.helper_question, docs);
        drop(entries);
        app.helper_out = ans.into_iter().map(|(t, s, _)| (t, s)).collect();
        if app.helper_out.is_empty() {
            app.helper_out.push(("Ответ".into(), "В заметках ничего не нашлось".into()));
        }
    }
    ui.separator();
    if !app.helper_out.is_empty() {
        let mut insert = false;
        egui::ScrollArea::vertical().id_salt("helper_out").show(ui, |ui| {
            for (h, t) in &app.helper_out {
                ui.label(RichText::new(h).strong());
                ui.label(t);
                ui.add_space(4.0);
            }
            ui.horizontal(|ui| {
                if ui.button("Вставить на страницу").clicked() {
                    insert = true;
                }
                if ui.button("Копировать").clicked() {
                    let s: String = app.helper_out.iter().map(|(h, t)| format!("{h}: {t}")).collect::<Vec<_>>().join("\n");
                    ui.ctx().copy_text(s);
                }
            });
        });
        if insert {
            let tasks_like = app.helper_out.iter().all(|(h, _)| h.starts_with("до ") || h == "без срока");
            let text: String = app
                .helper_out
                .iter()
                .map(|(h, t)| if tasks_like { format!("[ ] {t} ({h})") } else { format!("**{h}:** {t}") })
                .collect::<Vec<_>>()
                .join("\n");
            append_text(app, &text);
        }
    }
}

fn comments_panel(app: &mut App, ui: &mut Ui) {
    let Some(pid) = app.cur_page else { return };
    let author = app.settings.author.clone();
    let mut del = None;
    let mut changed = false;
    // Known people for @mentions: authors seen in this notebook.
    let mut people: Vec<String> = app.nb().map(|nb| nb.meta.sections().iter().flat_map(|s| s.pages.iter().map(|p| p.author.clone())).collect()).unwrap_or_default();
    people.sort();
    people.dedup();
    people.retain(|p| !p.is_empty());
    if let Some(page) = app.pages.get_mut(&pid) {
        egui::ScrollArea::vertical().id_salt("comments").max_height(ui.available_height() - 110.0).show(ui, |ui| {
            if page.comments.is_empty() {
                ui.label(RichText::new("Комментариев пока нет").weak());
            }
            for (i, c) in page.comments.iter().enumerate() {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&c.author).strong());
                        ui.label(RichText::new(fmt_dt(&c.time)).small().weak());
                        if c.author == author && ui.small_button("✕").clicked() {
                            del = Some(i);
                        }
                    });
                    // Highlight @mentions.
                    let mut job = egui::text::LayoutJob::default();
                    for w in c.text.split_inclusive(' ') {
                        let col = if w.starts_with('@') { Color32::from_rgb(40, 110, 220) } else { ui.visuals().text_color() };
                        job.append(w, 0.0, egui::TextFormat { color: col, font_id: egui::FontId::proportional(14.0), ..Default::default() });
                    }
                    ui.label(job);
                });
            }
        });
        ui.separator();
        ui.add(egui::TextEdit::multiline(&mut app.comment_text).hint_text("Комментарий… (@имя — упоминание)").desired_rows(3).desired_width(f32::INFINITY));
        if app.comment_text.ends_with('@') && !people.is_empty() {
            ui.horizontal_wrapped(|ui| {
                for p in &people {
                    if ui.small_button(p).clicked() {
                        app.comment_text.push_str(&p.replace(' ', "_"));
                        app.comment_text.push(' ');
                    }
                }
            });
        }
        if ui.button("Добавить").clicked() && !app.comment_text.trim().is_empty() {
            page.comments.push(Comment { id: new_id(), author: author.clone(), time: now(), text: std::mem::take(&mut app.comment_text) });
            changed = true;
        }
        if let Some(i) = del {
            page.comments.remove(i);
            changed = true;
        }
    }
    if changed {
        app.mark_dirty(pid);
    }
}

fn versions_panel(app: &mut App, ui: &mut Ui) {
    let Some(pid) = app.cur_page else { return };
    let Some(ni) = app.nb_index_of_page(pid) else { return };
    if ui.button("Сохранить версию сейчас").clicked() {
        if let Some(p) = app.pages.get(&pid) {
            app.notebooks[ni].save_version(p, app.settings.version_keep, app.settings.version_days);
        }
    }
    let list = app.notebooks[ni].list_versions(pid);
    ui.label(format!("Версий: {}", list.len()));
    egui::ScrollArea::vertical().id_salt("versions").max_height(160.0).show(ui, |ui| {
        for (t, path) in &list {
            let sel = app.version_sel.as_ref() == Some(path);
            if ui.selectable_label(sel, fmt_dt(t)).clicked() {
                app.version_sel = Some(path.clone());
                app.version_page = app.notebooks[ni].load_version(pid, path);
            }
        }
    });
    ui.separator();
    let Some(old) = app.version_page.clone() else {
        ui.label(RichText::new("Выберите версию, чтобы сравнить её с текущей").weak());
        return;
    };
    let cur_text = app.pages.get(&pid).map(|p| p.plain_text()).unwrap_or_default();
    let old_text = old.plain_text();
    let mut restore_all = false;
    let mut restore_items: Vec<Item> = Vec::new();
    ui.horizontal(|ui| {
        if ui.button("Восстановить версию").clicked() {
            restore_all = true;
        }
    });
    ui.label(RichText::new("Сравнение (− было, + стало):").strong());
    egui::ScrollArea::vertical().id_salt("diff").max_height(ui.available_height() * 0.55).show(ui, |ui| {
        let diff = similar::TextDiff::from_lines(&old_text, &cur_text);
        for ch in diff.iter_all_changes() {
            let (sign, col) = match ch.tag() {
                similar::ChangeTag::Delete => ("−", Color32::from_rgb(200, 50, 50)),
                similar::ChangeTag::Insert => ("+", Color32::from_rgb(40, 150, 60)),
                similar::ChangeTag::Equal => (" ", ui.visuals().weak_text_color()),
            };
            let line = ch.to_string();
            let line = line.trim_end();
            if ch.tag() == similar::ChangeTag::Equal && line.is_empty() {
                continue;
            }
            ui.label(RichText::new(format!("{sign} {line}")).color(col).monospace().size(12.0));
        }
    });
    ui.separator();
    ui.label(RichText::new("Восстановить фрагмент:").strong());
    egui::ScrollArea::vertical().id_salt("frag").show(ui, |ui| {
        for it in &old.items {
            let t: String = it.plain_text().lines().next().unwrap_or("").chars().take(60).collect();
            ui.horizontal(|ui| {
                if ui.small_button("↩").on_hover_text("Вернуть этот блок на страницу").clicked() {
                    restore_items.push(it.clone());
                }
                ui.label(format!("[{}] {}", it.kind.type_name(), t));
            });
        }
    });
    if restore_all || !restore_items.is_empty() {
        app.with_page(|page, ed| {
            ed.snapshot(page);
            if restore_all {
                *page = old.clone();
            } else {
                for it in restore_items {
                    if let Some(cur) = page.item_mut(it.id) {
                        *cur = it;
                    } else {
                        page.items.push(it);
                    }
                }
            }
        });
        app.mark_dirty(pid);
        app.set_status("Версия восстановлена");
    }
}

fn reading_panel(app: &mut App, ui: &mut Ui) {
    let st = app.tts.status();
    ui.horizontal(|ui| {
        if !st.speaking {
            if ui.button("▶ Читать").clicked() {
                let (title, text) = current_text(app);
                let text = if text.trim().is_empty() { title } else { text };
                app.reading.text = text.clone();
                app.tts.speak(&text, app.settings.tts_rate, app.settings.tts_volume, &app.settings.tts_voice);
            }
        } else if st.paused {
            if ui.button("▶ Продолжить").clicked() {
                app.tts.resume();
            }
        } else if ui.button("⏸ Пауза").clicked() {
            app.tts.pause();
        }
        if ui.button("⏹ Стоп").clicked() {
            app.tts.stop();
        }
    });
    let mut save = false;
    if !st.voices.is_empty() {
        egui::ComboBox::from_label("Голос")
            .selected_text(if app.settings.tts_voice.is_empty() { "По умолчанию".to_string() } else { app.settings.tts_voice.clone() })
            .show_ui(ui, |ui| {
                if ui.selectable_label(app.settings.tts_voice.is_empty(), "По умолчанию").clicked() {
                    app.settings.tts_voice.clear();
                    save = true;
                }
                for v in &st.voices {
                    if ui.selectable_label(app.settings.tts_voice == *v, v).clicked() {
                        app.settings.tts_voice = v.clone();
                        save = true;
                    }
                }
            });
    }
    save |= ui.add(egui::Slider::new(&mut app.settings.tts_rate, -10..=10).text("Скорость")).changed();
    save |= ui.add(egui::Slider::new(&mut app.settings.tts_volume, 0..=100).text("Громкость")).changed();
    if save {
        app.settings.save();
    }
    ui.separator();
    ui.horizontal_wrapped(|ui| {
        ui.checkbox(&mut app.reading.large, "Крупный шрифт");
        ui.checkbox(&mut app.reading.wide_spacing, "Широкие интервалы");
        ui.checkbox(&mut app.reading.line_focus, "Фокус на строке");
        ui.checkbox(&mut app.reading.syllables, "Слоги");
    });
    if app.reading.text.is_empty() {
        let (_, t) = current_text(app);
        app.reading.text = t;
    }
    if ui.small_button("Обновить текст").clicked() {
        let (_, t) = current_text(app);
        app.reading.text = t;
    }
    ui.separator();
    let size = if app.reading.large { 22.0 } else { 16.0 };
    let extra = if app.reading.wide_spacing { 2.5 } else { 0.0 };
    let text = app.reading.text.clone();
    let word = st.word;
    let lines: Vec<&str> = text.lines().collect();
    // Which line holds the spoken word?
    let mut offset = 0usize;
    let mut word_line = None;
    for (i, l) in lines.iter().enumerate() {
        let n = l.chars().count();
        if let Some((w, _)) = word {
            if w >= offset && w <= offset + n {
                word_line = Some(i);
            }
        }
        offset += n + 1;
    }
    if let Some(l) = word_line {
        app.reading.focus_line = l;
    }
    egui::ScrollArea::vertical().id_salt("reading").show(ui, |ui| {
        let mut offset = 0usize;
        for (i, l) in lines.iter().enumerate() {
            let n = l.chars().count();
            let dim = app.reading.line_focus && i != app.reading.focus_line;
            let base = if dim { ui.visuals().weak_text_color().gamma_multiply(0.5) } else { ui.visuals().text_color() };
            let mut job = egui::text::LayoutJob::default();
            job.wrap.max_width = ui.available_width();
            let fmt = |c: Color32, bg: Color32| egui::TextFormat {
                font_id: egui::FontId::proportional(size),
                color: c,
                background: bg,
                extra_letter_spacing: extra * 0.4,
                line_height: Some(size * (1.4 + extra * 0.2)),
                ..Default::default()
            };
            // Split into words to highlight the spoken one.
            let chars: Vec<char> = l.chars().collect();
            let mut k = 0;
            while k < chars.len() {
                let mut e = k;
                if chars[k].is_alphanumeric() {
                    while e < chars.len() && chars[e].is_alphanumeric() {
                        e += 1;
                    }
                } else {
                    e += 1;
                }
                let w: String = chars[k..e].iter().collect();
                let gi = offset + k;
                let spoken = word.is_some_and(|(a, len)| gi >= a && gi < a + len.max(1));
                let w = if app.reading.syllables && w.chars().count() > 3 { helper::syllables(&w) } else { w };
                if spoken {
                    job.append(&w, 0.0, fmt(Color32::BLACK, Color32::from_rgb(255, 214, 0)));
                } else {
                    job.append(&w, 0.0, fmt(base, Color32::TRANSPARENT));
                }
                k = e;
            }
            let r = ui.label(job);
            if spoken_line(word_line, i) {
                r.scroll_to_me(Some(egui::Align::Center));
            }
            if r.clicked() {
                app.reading.focus_line = i;
            }
            offset += n + 1;
        }
    });
}

fn spoken_line(word_line: Option<usize>, i: usize) -> bool {
    word_line == Some(i)
}
