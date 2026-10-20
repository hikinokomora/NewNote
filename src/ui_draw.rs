//! The "Рисование" ribbon: tools, pen gallery with sets, per-pen settings,
//! shapes, rulers, conversion, page background, layers and view.

use egui::{Color32, RichText, Ui, vec2};

use crate::app::App;
use crate::canvas::{EraserMode, Ruler, RulerKind, Tool};
use crate::model::*;
use crate::settings::{MM, PenPreset};
use crate::shapes::ShapeKind;

fn c32(c: [u8; 4]) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3])
}

/// One pen of the gallery, drawn like a real pen (selected pen is raised).
fn pen_slot(ui: &mut Ui, p: &PenPreset, active: bool) -> egui::Response {
    let pal = crate::chrome::cur(ui);
    let (rect, resp) = ui.allocate_exact_size(vec2(30.0, 40.0), egui::Sense::click());
    if active {
        ui.painter().rect_filled(rect, 4.0, pal.sel);
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, 4.0, pal.hover);
    }
    crate::icons::pen(ui.painter(), rect, p.brush(), p.color, p.effect, active, ui.visuals().dark_mode);
    resp
}

pub fn draw_ribbon(app: &mut App, ui: &mut Ui) {
    use crate::chrome::{flat, group};
    use crate::icons::Icon;
    let ctx = ui.ctx().clone();
    let Some(pid) = app.cur_page else {
        ui.label("Откройте страницу");
        return;
    };
    let full = app.settings.ribbon_full;
    let mut save = false;
    let mut actions: Vec<crate::canvas::Action> = Vec::new();
    let mut open_settings: Option<usize> = None;
    let mut clear_ink = false;
    {
        let App { settings, editors, pages, .. } = app;
        let (Some(ed), Some(page)) = (editors.get_mut(&pid), pages.get_mut(&pid)) else { return };

        // ---- Tools ----
        group(ui, full, "Инструменты", |ui| {
            if flat(ui, Some(Icon::Select), "", "Выбор и ввод текста (Alt+1)", ed.tool == Tool::Select).clicked() {
                ed.tool = Tool::Select;
            }
            if flat(ui, Some(Icon::Lasso), "", "Лассо (L, боковая кнопка пера)", ed.tool == Tool::Lasso).clicked() {
                ed.tool = Tool::Lasso;
            }
            if flat(ui, Some(Icon::Space), "", "Вставить пробел: потяните вниз, чтобы раздвинуть содержимое", ed.tool == Tool::Space).clicked() {
                ed.tool = Tool::Space;
            }
            let er = flat(ui, Some(Icon::Eraser), "", "Ластик (E, обратный конец пера); правый щелчок — режимы", ed.tool == Tool::Eraser);
            if er.clicked() {
                ed.tool = Tool::Eraser;
            }
            er.context_menu(|ui| {
                ui.label(RichText::new("Ластик").strong());
                for m in EraserMode::ALL {
                    if ui.radio(ed.eraser_mode == m, m.label()).clicked() {
                        ed.eraser_mode = m;
                        ed.tool = Tool::Eraser;
                    }
                }
                ui.add(egui::Slider::new(&mut ed.eraser_radius, 1.0..=100.0).text("Размер, px"));
                ui.separator();
                if ui.button("Очистить всю рукопись…").clicked() {
                    clear_ink = true;
                    ui.close();
                }
            });
        });

        // ---- Pens ----
        group(ui, full, "Перья", |ui| {
            let pens = settings.pens().to_vec();
            let mut action: Option<(usize, &str)> = None;
            for (i, p) in pens.iter().enumerate() {
                let active = ed.tool == Tool::Draw && ed.pen_slot == Some(i);
                let r = pen_slot(ui, p, active).on_hover_text(format!("{} — {}, {:.2} мм{}", p.name, p.brush().label(), p.width / MM, if i < 9 { format!(" · клавиша {}", i + 1) } else { String::new() }));
                if r.clicked() {
                    if active {
                        open_settings = Some(i);
                    } else {
                        ed.set_pen(p, Some(i));
                    }
                }
                r.context_menu(|ui| {
                    for (label, op) in [("Изменить", "edit"), ("Дублировать", "dup"), ("Сбросить", "reset"), ("← Влево", "left"), ("Вправо →", "right"), ("Удалить", "del")] {
                        if ui.button(label).clicked() {
                            action = Some((i, op));
                            ui.close();
                        }
                    }
                });
            }
            if flat(ui, Some(Icon::Plus), "", "Добавить перо", false).clicked() {
                let mut p = ed.pen.clone();
                p.name = format!("{} {}", p.brush().label(), settings.pens().len() + 1);
                settings.pens_mut().push(p);
                open_settings = Some(settings.pens().len() - 1);
                save = true;
            }
            let set_names: Vec<String> = settings.pen_sets.iter().map(|s| s.name.clone()).collect();
            let sr = flat(ui, None, &format!("{} ⌄", set_names.get(settings.active_set).cloned().unwrap_or_default()), "Набор перьев", false);
            egui::Popup::menu(&sr).show(|ui| {
                for (i, n) in set_names.iter().enumerate() {
                    if ui.selectable_label(settings.active_set == i, n).clicked() {
                        settings.active_set = i;
                        save = true;
                    }
                }
                ui.separator();
                if ui.button("Сохранить как новый набор").clicked() {
                    let pens = settings.pens().to_vec();
                    let n = settings.pen_sets.len() + 1;
                    settings.pen_sets.push(crate::settings::PenSet { name: format!("Набор {n}"), pens });
                    settings.active_set = settings.pen_sets.len() - 1;
                    save = true;
                }
                if ui.button("Вернуть стандартные наборы").clicked() {
                    settings.pen_sets = crate::settings::builtin_pen_sets();
                    settings.active_set = 0;
                    save = true;
                }
            });
            if let Some((i, op)) = action {
                let pens = settings.pens_mut();
                match op {
                    "edit" => open_settings = Some(i),
                    "dup" => {
                        let mut p = pens[i].clone();
                        p.name.push_str(" (копия)");
                        pens.insert(i + 1, p);
                    }
                    "reset" => {
                        if let Some(d) = crate::settings::builtin_pen_sets().get(settings.active_set).and_then(|s| s.pens.get(i)).cloned() {
                            settings.pens_mut()[i] = d;
                        }
                    }
                    "left" if i > 0 => pens.swap(i, i - 1),
                    "right" if i + 1 < pens.len() => pens.swap(i, i + 1),
                    "del" if pens.len() > 1 => {
                        pens.remove(i);
                    }
                    _ => {}
                }
                save = true;
            }
        });

        // ---- Shapes and conversion ----
        group(ui, full, "Фигуры и преобразование", |ui| {
            let selected_shape = matches!(ed.tool, Tool::Shape(_));
            let sr = flat(ui, Some(Icon::Shapes), "Фигуры", "Галерея фигур", selected_shape);
            egui::Popup::menu(&sr).show(|ui| {
                ui.set_min_width(330.0);
                for (grp, kinds) in ShapeKind::GROUPS {
                    ui.label(RichText::new(grp).small().strong());
                    ui.horizontal_wrapped(|ui| {
                        for k in kinds {
                            if ui.selectable_label(ed.tool == Tool::Shape(*k), k.label()).clicked() {
                                ed.tool = Tool::Shape(*k);
                                ui.close();
                            }
                        }
                    });
                }
                ui.separator();
                ui.collapsing("Стиль фигуры", |ui| shape_style_ui(ui, &mut ed.shape));
                if ui.checkbox(&mut settings.auto_shapes, "Умное рисование (Shift+S)").changed() {
                    save = true;
                }
                ui.label(RichText::new("Shift — пропорции и углы по 15°; ломаная — щелчками, двойной щелчок завершает").small().weak());
            });
            if flat(ui, Some(Icon::ToShape), "Преобразовать в фигуру", "Выделенные (или все) штрихи в аккуратные фигуры", false).clicked() {
                ed.snapshot(page);
                if ed.selection.strokes.is_empty() {
                    ed.select_all(page);
                    ed.selection.items.clear();
                }
                crate::selection::to_shapes(page, ed);
                actions.push(crate::canvas::Action::Dirty);
            }
            let has_ink = !page.strokes.is_empty();
            if ui.add_enabled_ui(has_ink, |ui| flat(ui, Some(Icon::ToText), "Преобразовать в текст", "Распознать рукописный текст", false)).inner.clicked() {
                actions.push(crate::canvas::Action::OcrInk);
            }
            if ui.add_enabled_ui(has_ink, |ui| flat(ui, Some(Icon::Math), "Математика", "Рукописная формула в набранную и её решение", false)).inner.clicked() {
                actions.push(crate::canvas::Action::InkToMath);
            }
        });

        // ---- Rulers ----
        group(ui, full, "Линейка", |ui| {
            let on = ed.ruler.is_some();
            let rr = flat(ui, Some(Icon::Ruler), "Линейка ⌄", "Линейка (R), транспортир, угольник, циркуль, измерение", on);
            egui::Popup::menu(&rr).show(|ui| {
                for (kind, text) in [(RulerKind::Ruler, "📏 Линейка   R"), (RulerKind::Protractor, "◠ Транспортир"), (RulerKind::SetSquare, "◺ Угольник 90°/45°")] {
                    let active = ed.ruler.is_some_and(|r| r.kind == kind);
                    if ui.selectable_label(active, text).clicked() {
                        ed.ruler = if active { None } else { Some(Ruler { center: ed.view_center(), angle: 0.0, kind }) };
                    }
                }
                if ed.ruler.is_some() && ui.button("Убрать линейку").clicked() {
                    ed.ruler = None;
                }
                ui.separator();
                if ui.selectable_label(ed.tool == Tool::Compass, "⊙ Циркуль").on_hover_text("От центра задайте радиус, Shift — рисовать дугу").clicked() {
                    ed.tool = Tool::Compass;
                }
                if ui.selectable_label(ed.tool == Tool::Measure, "↔ Измерить расстояние").clicked() {
                    ed.tool = Tool::Measure;
                }
                ui.separator();
                if ui.checkbox(&mut settings.snap_grid, "Привязка к сетке   G").changed() {
                    save = true;
                }
                let c = ed.view_center();
                if ui.button("Добавить горизонтальную направляющую").clicked() {
                    page.guides.push(Guide { vertical: false, at: c.y });
                    actions.push(crate::canvas::Action::Dirty);
                }
                if ui.button("Добавить вертикальную направляющую").clicked() {
                    page.guides.push(Guide { vertical: true, at: c.x });
                    actions.push(crate::canvas::Action::Dirty);
                }
                if !page.guides.is_empty() && ui.button("Убрать направляющие").clicked() {
                    page.guides.clear();
                    actions.push(crate::canvas::Action::Dirty);
                }
            });
        });

        // ---- Page ----
        group(ui, full, "Страница", |ui| {
            let br = flat(ui, Some(Icon::Page), "Фон ⌄", "Фон, цвет и размер страницы, подложка", false);
            egui::Popup::menu(&br).show(|ui| background_menu(ui, page, ed, &mut actions));
            let lr = flat(ui, Some(Icon::Layers), &format!("{} ⌄", LAYER_NAMES[ed.layer as usize]), "Слои", false);
            egui::Popup::menu(&lr).show(|ui| layers_menu(ui, page, ed, &mut actions));
        });

        // ---- More ----
        group(ui, full, "Ещё", |ui| {
            if flat(ui, Some(Icon::Laser), "", "Указка: исчезающие штрихи", ed.tool == Tool::Laser).clicked() {
                ed.tool = Tool::Laser;
            }
            let mr = flat(ui, None, "Ещё ⌄", "Другие инструменты и вид", false);
            egui::Popup::menu(&mr).show(|ui| {
                ui.set_min_width(260.0);
                for (t, l) in [
                    (Tool::Text, "T  Текст: щелчок создаёт блок"),
                    (Tool::RectSelect, "⬚  Выделение рамкой"),
                    (Tool::Pan, "✋  Прокрутка"),
                    (Tool::Marker, "①  Нумерованная метка"),
                    (Tool::Fill, "🪣  Заливка фигуры"),
                    (Tool::Redact, "▮  Скрыть область"),
                    (Tool::Eyedropper, "💧  Пипетка"),
                ] {
                    if ui.selectable_label(ed.tool == t, l).clicked() {
                        ed.tool = t;
                        ui.close();
                    }
                }
                ui.separator();
                ui.checkbox(&mut ed.annotate, "Аннотации: штрихи привязываются к картинкам и блокам");
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("⟳ 90°").on_hover_text("Ctrl+Alt+R").clicked() {
                        let r = ed.last_rect;
                        ed.rotate_by(r, r.center(), std::f32::consts::FRAC_PI_2);
                    }
                    if ui.selectable_label(ed.free_rotate, "↻ Двумя пальцами").clicked() {
                        ed.free_rotate = !ed.free_rotate;
                    }
                    if ui.selectable_label(ed.mirror, "⇋ Зеркало").clicked() {
                        ed.toggle_mirror();
                    }
                    if ed.rotated() && ui.button("Сброс").clicked() {
                        ed.reset_rotation();
                    }
                });
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("Рисует:");
                    save |= ui.toggle_value(&mut settings.pen_draws, "✒ перо").changed();
                    save |= ui.toggle_value(&mut settings.touch_draws, "☝ палец").changed();
                    save |= ui.toggle_value(&mut settings.mouse_draws, "🖱 мышь").changed();
                });
                ui.separator();
                if ui.button("▶ Воспроизвести рисование").clicked() {
                    ed.replay = Some(crate::canvas::Replay { start: ctx.input(|i| i.time), speed: 1.0, paused: None });
                    ui.close();
                }
                if ui.checkbox(&mut settings.minimap, "Мини-карта").changed() {
                    save = true;
                }
            });
        });
    }
    if clear_ink {
        app.dialogs.push(crate::app::Dialog::ClearInk);
    }
    if let Some(i) = open_settings {
        app.pen_settings = Some(i);
    }
    if save {
        app.settings.save();
    }
    for a in actions {
        app.handle_action(&ctx, a);
    }
}

fn background_menu(ui: &mut Ui, page: &mut Page, ed: &mut crate::canvas::Editor, actions: &mut Vec<crate::canvas::Action>) {
    let cur = page.pattern();
    ui.set_min_width(240.0);
    for p in Pattern::ALL {
        if ui.radio(cur == p, p.label()).clicked() {
            page.pattern = Some(p);
            page.pattern_step = 0.0;
            actions.push(crate::canvas::Action::Dirty);
        }
    }
    ui.separator();
    let mut step = if page.pattern_step > 0.0 { page.pattern_step } else { cur.default_step() };
    if ui.add(egui::Slider::new(&mut step, 2.0..=80.0).text("Размер ячейки")).changed() {
        page.pattern_step = step;
        actions.push(crate::canvas::Action::Dirty);
    }
    ui.horizontal(|ui| {
        ui.label("Цвет линий:");
        let mut c = page.line_color.map(c32).unwrap_or(Color32::from_rgba_unmultiplied(80, 130, 210, 55));
        if ui.color_edit_button_srgba(&mut c).changed() {
            page.line_color = Some(c.to_srgba_unmultiplied());
            actions.push(crate::canvas::Action::Dirty);
        }
        if ui.small_button("Сброс").clicked() {
            page.line_color = None;
            actions.push(crate::canvas::Action::Dirty);
        }
    });
    ui.horizontal(|ui| {
        ui.label("Цвет страницы:");
        let mut c = page.bg_color.map(c32).unwrap_or(Color32::WHITE);
        if ui.color_edit_button_srgba(&mut c).changed() {
            page.bg_color = Some(c.to_srgba_unmultiplied());
            actions.push(crate::canvas::Action::Dirty);
        }
        if ui.small_button("Сброс").clicked() {
            page.bg_color = None;
            actions.push(crate::canvas::Action::Dirty);
        }
    });
    ui.label("Размер:");
    ui.horizontal_wrapped(|ui| {
        for s in PageSize::ALL {
            if ui.selectable_label(page.size == s, s.label()).clicked() {
                page.size = s;
                actions.push(crate::canvas::Action::Dirty);
            }
        }
    });
    ui.separator();
    if ui.button("Подложка: изображение или PDF…").clicked() {
        actions.push(crate::canvas::Action::Status("underlay".into()));
        ui.close();
    }
    ui.checkbox(&mut ed.edit_underlay, "Редактировать подложку");
}

fn layers_menu(ui: &mut Ui, page: &mut Page, ed: &mut crate::canvas::Editor, actions: &mut Vec<crate::canvas::Action>) {
    for i in (0..3).rev() {
        ui.horizontal(|ui| {
            if ui.radio(ed.layer == i as u8, LAYER_NAMES[i]).on_hover_text("Рисовать в этом слое").clicked() {
                ed.layer = i as u8;
            }
            let l = &mut page.layers[i];
            if ui.selectable_label(l.visible, "👁").on_hover_text("Показать / скрыть").clicked() {
                l.visible = !l.visible;
                actions.push(crate::canvas::Action::Dirty);
            }
            if ui.selectable_label(l.locked, "🔒").on_hover_text("Заблокировать").clicked() {
                l.locked = !l.locked;
                actions.push(crate::canvas::Action::Dirty);
            }
            if ui.add(egui::Slider::new(&mut l.opacity, 0.05..=1.0).show_value(false)).on_hover_text("Прозрачность слоя").changed() {
                actions.push(crate::canvas::Action::Dirty);
            }
        });
    }
    ui.separator();
    if ui.button("Объединить все слои в «Рисунок»").clicked() {
        ed.snapshot(page);
        for s in page.strokes.iter_mut() {
            s.layer = 1;
        }
        actions.push(crate::canvas::Action::Dirty);
    }
    if ui.button("Разблокировать все объекты").clicked() {
        crate::selection::unlock_all(page);
        actions.push(crate::canvas::Action::Dirty);
    }
    ui.label(RichText::new("Маркер «под текстом» рисует в слое «Фон»").small().weak());
}

pub fn shape_style_ui(ui: &mut Ui, st: &mut crate::canvas::ShapeStyle) {
    ui.horizontal(|ui| {
        ui.label("Контур:");
        let mut c = c32(st.color);
        if ui.color_edit_button_srgba(&mut c).changed() {
            st.color = c.to_srgba_unmultiplied();
        }
        ui.add(egui::DragValue::new(&mut st.width).range(0.3..=40.0).speed(0.1).suffix(" px"));
    });
    crate::selection::swatches(ui, |c| st.color = c);
    ui.horizontal(|ui| {
        let mut fill = st.fill.is_some();
        if ui.checkbox(&mut fill, "Заливка").changed() {
            st.fill = fill.then_some([st.color[0], st.color[1], st.color[2], 70]);
        }
        if let Some(f) = st.fill.as_mut() {
            let mut c = c32(*f);
            if ui.color_edit_button_srgba(&mut c).changed() {
                *f = c.to_srgba_unmultiplied();
            }
        }
    });
    ui.horizontal(|ui| {
        for d in Dash::ALL {
            if ui.selectable_label(st.dash == d, d.label()).clicked() {
                st.dash = d;
            }
        }
    });
    ui.horizontal(|ui| {
        ui.toggle_value(&mut st.arrow_start, "← Стрелка в начале");
        ui.toggle_value(&mut st.arrow_end, "Стрелка в конце →");
    });
    ui.horizontal(|ui| {
        ui.toggle_value(&mut st.shadow, "Тень");
        ui.toggle_value(&mut st.glow, "Свечение");
    });
}

/// Window with the full settings of one pen slot.
pub fn pen_settings_window(app: &mut App, ctx: &egui::Context) {
    let Some(slot) = app.pen_settings else { return };
    if slot >= app.settings.pens().len() {
        app.pen_settings = None;
        return;
    }
    let mut open = true;
    let mut changed = false;
    let mut close = false;
    let mut pick_color = false;
    let fav = app.settings.fav_colors.clone();
    let mut add_fav: Option<[u8; 4]> = None;
    let mut new_pen: Option<PenPreset> = None;
    {
        let p = &mut app.settings.pens_mut()[slot];
        egui::Window::new("Настройки пера").open(&mut open).collapsible(false).resizable(false).default_width(360.0).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Название:");
                changed |= ui.text_edit_singleline(&mut p.name).changed();
            });
            ui.horizontal_wrapped(|ui| {
                for k in InkKind::BRUSHES {
                    if ui.selectable_label(p.brush() == k, format!("{} {}", k.icon(), k.label())).clicked() {
                        p.kind = k;
                        p.highlighter = k == InkKind::Highlighter;
                        if k == InkKind::Highlighter && p.color[3] > 160 {
                            p.color[3] = 105;
                        }
                        changed = true;
                    }
                }
            });
            ui.separator();
            ui.label(RichText::new("Цвет").strong());
            crate::selection::swatches(ui, |c| {
                p.color = [c[0], c[1], c[2], p.color[3]];
                changed = true;
            });
            ui.horizontal(|ui| {
                let mut c = Color32::from_rgb(p.color[0], p.color[1], p.color[2]);
                let mut rgb = [p.color[0], p.color[1], p.color[2]];
                if ui.color_edit_button_srgb(&mut rgb).changed() {
                    p.color = [rgb[0], rgb[1], rgb[2], p.color[3]];
                    c = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
                    changed = true;
                }
                let mut hex = format!("{:02X}{:02X}{:02X}", c.r(), c.g(), c.b());
                ui.label("#");
                if ui.add(egui::TextEdit::singleline(&mut hex).desired_width(60.0)).changed() {
                    if let Ok(v) = u32::from_str_radix(hex.trim().trim_start_matches('#'), 16) {
                        if hex.trim().trim_start_matches('#').len() == 6 {
                            p.color = [(v >> 16) as u8, (v >> 8) as u8, v as u8, p.color[3]];
                            changed = true;
                        }
                    }
                }
                if ui.button("💧 Пипетка").on_hover_text("Взять цвет со штриха на странице").clicked() {
                    pick_color = true;
                }
                if ui.button("☆").on_hover_text("В избранные цвета").clicked() {
                    add_fav = Some(p.color);
                }
            });
            if !fav.is_empty() {
                ui.horizontal_wrapped(|ui| {
                    ui.label("Избранные:");
                    for f in &fav {
                        let (r, resp) = ui.allocate_exact_size(vec2(18.0, 18.0), egui::Sense::click());
                        ui.painter().rect_filled(r, 9.0, Color32::from_rgb(f[0], f[1], f[2]));
                        if resp.clicked() {
                            p.color = [f[0], f[1], f[2], p.color[3]];
                            changed = true;
                        }
                    }
                });
            }
            let mut opacity = p.color[3] as f32 / 255.0 * 100.0;
            if ui.add(egui::Slider::new(&mut opacity, 5.0..=100.0).text("Непрозрачность, %")).changed() {
                p.color[3] = (opacity / 100.0 * 255.0) as u8;
                changed = true;
            }
            ui.separator();
            ui.label(RichText::new("Толщина").strong());
            let mut mm = p.width / MM;
            if ui.add(egui::Slider::new(&mut mm, 0.25..=24.0).logarithmic(true).text("мм")).changed() {
                p.width = mm * MM;
                changed = true;
            }
            ui.horizontal_wrapped(|ui| {
                for s in [0.25, 0.35, 0.5, 0.7, 1.0, 1.4, 2.0, 3.0, 4.5, 6.0, 9.0, 12.0] {
                    if ui.selectable_label((mm - s).abs() < 0.01, format!("{s}")).clicked() {
                        p.width = s * MM;
                        changed = true;
                    }
                }
            });
            ui.separator();
            ui.label(RichText::new("Чувствительность и сглаживание").strong());
            changed |= ui.checkbox(&mut p.pressure, "Учитывать нажим").changed();
            if p.pressure {
                changed |= ui.add(egui::Slider::new(&mut p.curve, 0.3..=3.0).text("Кривая нажима (меньше — мягче)")).changed();
                pressure_curve_preview(ui, p.curve);
            }
            changed |= ui.checkbox(&mut p.tilt, "Учитывать наклон (карандаш, кисти)").changed();
            let mut sm = p.smoothing * 100.0;
            if ui.add(egui::Slider::new(&mut sm, 0.0..=100.0).text("Сглаживание")).on_hover_text("Убирает дрожь линии; 25–40 — лёгкое сглаживание без задержки").changed() {
                p.smoothing = sm / 100.0;
                changed = true;
            }
            let mut st = p.stabilizer * 100.0;
            if ui.add(egui::Slider::new(&mut st, 0.0..=100.0).text("Стабилизатор")).on_hover_text("Перо следует за курсором «на нитке»: ровные медленные линии").changed() {
                p.stabilizer = st / 100.0;
                changed = true;
            }
            ui.label(RichText::new("Shift — прямая линия; задержите перо в конце линии — она станет ровной фигурой").small().weak());
            ui.separator();
            ui.label(RichText::new("Эффект").strong());
            ui.horizontal_wrapped(|ui| {
                for e in [Effect::None, Effect::Rainbow, Effect::Gold, Effect::Silver] {
                    if ui.selectable_label(p.effect == e, e.label()).clicked() {
                        p.effect = e;
                        changed = true;
                    }
                }
                let is_grad = matches!(p.effect, Effect::Gradient(_));
                if ui.selectable_label(is_grad, "Градиент").clicked() && !is_grad {
                    p.effect = Effect::Gradient([255, 120, 40, 255]);
                    changed = true;
                }
                if let Effect::Gradient(c2) = &mut p.effect {
                    let mut c = c32(*c2);
                    if ui.color_edit_button_srgba(&mut c).changed() {
                        *c2 = c.to_srgba_unmultiplied();
                        changed = true;
                    }
                }
            });
            if p.brush() == InkKind::Highlighter {
                changed |= ui.checkbox(&mut p.under_text, "Маркер под текстом").changed();
            }
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Сохранить как новое перо").clicked() {
                    let mut n = p.clone();
                    n.name.push_str(" *");
                    new_pen = Some(n);
                }
                if ui.button("Копировать настройки").clicked() {
                    if let Ok(s) = serde_json::to_string(&*p) {
                        ui.ctx().copy_text(s);
                    }
                }
                if ui.button("Вставить").clicked() {
                    if let Ok(mut cb) = arboard::Clipboard::new() {
                        if let Some(n) = cb.get_text().ok().and_then(|t| serde_json::from_str::<PenPreset>(&t).ok()) {
                            *p = n;
                            changed = true;
                        }
                    }
                }
                if ui.button("Готово").clicked() {
                    close = true;
                }
            });
        });
    }
    if let Some(c) = add_fav {
        if !app.settings.fav_colors.contains(&c) {
            app.settings.fav_colors.push(c);
            if app.settings.fav_colors.len() > 16 {
                app.settings.fav_colors.remove(0);
            }
        }
        changed = true;
    }
    if let Some(n) = new_pen {
        app.settings.pens_mut().insert(slot + 1, n);
        app.pen_settings = Some(slot + 1);
        changed = true;
    }
    if changed {
        app.settings.save();
        // Apply to the active pen right away.
        let p = app.settings.pens()[app.pen_settings.unwrap_or(slot).min(app.settings.pens().len() - 1)].clone();
        let slot_now = app.pen_settings.unwrap_or(slot);
        if let Some(ed) = app.cur_page.and_then(|pid| app.editors.get_mut(&pid)) {
            ed.set_pen(&p, Some(slot_now));
        }
    }
    if pick_color {
        if let Some(ed) = app.cur_page.and_then(|pid| app.editors.get_mut(&pid)) {
            ed.tool = Tool::Eyedropper;
        }
        app.pen_pick_slot = Some(slot);
    }
    if !open || close {
        app.pen_settings = None;
    }
}

fn pressure_curve_preview(ui: &mut Ui, curve: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(120.0, 60.0), egui::Sense::hover());
    let p = ui.painter();
    p.rect_stroke(rect, 3.0, egui::Stroke::new(1.0, Color32::GRAY), egui::StrokeKind::Inside);
    let pts: Vec<egui::Pos2> = (0..=30)
        .map(|i| {
            let x = i as f32 / 30.0;
            let y = 0.12 + 0.88 * x.powf(curve);
            egui::pos2(rect.min.x + x * rect.width(), rect.max.y - y * rect.height())
        })
        .collect();
    p.add(egui::Shape::line(pts, egui::Stroke::new(2.0, Color32::from_rgb(60, 130, 255))));
    p.text(rect.right_bottom() + vec2(4.0, 0.0), egui::Align2::LEFT_BOTTOM, "нажим → толщина", egui::FontId::proportional(10.0), Color32::GRAY);
}
