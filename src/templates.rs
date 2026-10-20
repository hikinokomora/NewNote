//! Page templates: built-in and user-defined (saved pages).

use crate::model::*;
use crate::settings::Settings;

pub const BUILTIN: &[&str] = &["Встреча", "План", "Список дел", "Конспект", "Дневник", "Проект"];

fn text(y: f32, s: &str, st: &Settings) -> Item {
    Item::new(egui::pos2(0.0, y), 620.0, ItemKind::Text(TextBlock::new(s, st.default_size, &st.default_font)), &st.author)
}

pub fn builtin(name: &str, id: Id, st: &Settings) -> Page {
    let mut p = Page::new(id);
    let today = chrono::Local::now().format("%d.%m.%Y").to_string();
    match name {
        "Встреча" => {
            p.items.push(text(0.0, &format!("## Встреча {today}\n**Участники:** \n**Место:** \n\n### Повестка\n1. \n2. \n\n### Решения\n- \n\n### Задачи\n[ ] \n[ ] "), st));
        }
        "План" => {
            p.items.push(text(0.0, "## План\n**Цель:** \n\n### Шаги\n1. \n2. \n3. \n\n### Сроки\n- \n\n### Ресурсы\n- ", st));
        }
        "Список дел" => {
            p.pattern = Some(Pattern::LinesMedium);
            p.items.push(text(0.0, &format!("## Дела на {today}\n[ ] \n[ ] \n[ ] \n\n### Позже\n[ ] "), st));
        }
        "Конспект" => {
            p.pattern = Some(Pattern::GridMedium);
            p.items.push(text(0.0, &format!("# Тема\n*{today}*\n\n## Ключевые понятия\n- Понятие — определение\n\n## Записи\n\n## Вопросы\n- \n\n## Итог\n"), st));
        }
        "Дневник" => {
            p.pattern = Some(Pattern::LinesMedium);
            p.items.push(text(0.0, &format!("## {today}\n**Настроение:** \n\n### Что произошло\n\n### За что благодарен\n- \n\n### Планы на завтра\n[ ] "), st));
        }
        "Проект" => {
            p.items.push(text(0.0, "# Проект\n**Описание:** \n**Ответственный:** \n**Срок:** \n\n## Этапы", st));
            let mut t = Table::new(5, 4);
            t.cells[0] = vec!["Этап".into(), "Ответственный".into(), "Срок".into(), "Статус".into()];
            t.col_widths = vec![200.0, 150.0, 110.0, 110.0];
            p.items.push(Item::new(egui::pos2(0.0, 170.0), 570.0, ItemKind::Table(t), &st.author));
            p.items.push(text(330.0, "## Риски\n- \n\n## Заметки\n", st));
        }
        _ => {}
    }
    p
}

pub fn welcome(id: Id, st: &Settings) -> Page {
    let mut p = Page::new(id);
    p.items.push(text(
        0.0,
        "# Добро пожаловать в NewNote\n\
Все заметки хранятся **только на этом компьютере** — без облака и учётной записи.\n\n\
## С чего начать\n\
- Щёлкните в любом месте страницы, чтобы начать писать.\n\
- Перо планшета рисует сразу: выберите инструмент «Перо» на вкладке **Рисование**.\n\
- Кнопка пера (боковая) включает лассо, обратный конец пера — ластик.\n\
- `Ctrl+колесо` или щипок двумя пальцами — масштаб, пробел + перетаскивание — прокрутка.\n\
- Перетащите файлы или картинки прямо на страницу.\n\n\
## Форматирование\n\
**жирный** (Ctrl+B), *курсив* (Ctrl+I), __подчёркнутый__ (Ctrl+U), ~~зачёркнутый~~ (Ctrl+-), ==выделение==\n\
[ ] Список с флажками: начните строку с [] и пробела\n\
[x] Выполненная задача\n\
- Маркированный список: «- » в начале строки\n\
1. Нумерованный: «1. »\n\n\
!> Быстрая заметка из любого места: Win+Alt+N",
        st,
    ));
    p
}

pub fn custom_list() -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(Settings::templates_dir())
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| {
                    let p = e.path();
                    (p.extension().is_some_and(|x| x == "json")).then(|| p.file_stem().unwrap().to_string_lossy().to_string())
                })
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

pub fn save_custom(name: &str, page: &Page) -> bool {
    let mut p = page.clone();
    p.comments.clear();
    let path = Settings::templates_dir().join(format!("{}.json", crate::storage::sanitize(name)));
    serde_json::to_vec(&p).map(|b| crate::storage::atomic_write(&path, &b)).unwrap_or(false)
}

/// Builds a page from a template name (built-in or custom). Assets of custom
/// templates are not copied, so images are dropped.
pub fn instantiate(name: &str, id: Id, st: &Settings) -> Page {
    if BUILTIN.contains(&name) {
        return builtin(name, id, st);
    }
    let path = Settings::templates_dir().join(format!("{name}.json"));
    match std::fs::read(path).ok().and_then(|b| serde_json::from_slice::<Page>(&b).ok()) {
        Some(mut p) => {
            p.id = id;
            p.items.retain(|i| !matches!(i.kind, ItemKind::Image(_) | ItemKind::File(_)));
            for it in p.items.iter_mut() {
                it.id = new_id();
                it.author = st.author.clone();
                it.modified = now();
            }
            p
        }
        None => Page::new(id),
    }
}
