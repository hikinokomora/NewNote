//! "Smart helper": on-device text processing without any network or model
//! download. Extractive summaries, task and deadline extraction, cleanup,
//! outlines, question answering over the notebooks, tag/title suggestions
//! and flash cards.

use std::collections::HashMap;

const STOP: &[&str] = &[
    "и", "в", "во", "не", "что", "он", "на", "я", "с", "со", "как", "а", "то", "все", "она", "так", "его", "но", "да", "ты", "к", "у",
    "же", "вы", "за", "бы", "по", "только", "ее", "её", "мне", "было", "вот", "от", "меня", "еще", "ещё", "нет", "о", "из", "ему", "теперь",
    "когда", "даже", "ну", "вдруг", "ли", "если", "уже", "или", "ни", "быть", "был", "него", "до", "вас", "нибудь", "опять", "уж", "вам",
    "ведь", "там", "потом", "себя", "ничего", "ей", "может", "они", "тут", "где", "есть", "надо", "ней", "для", "мы", "тебя", "их", "чем",
    "была", "сам", "чтоб", "без", "будто", "чего", "раз", "тоже", "себе", "под", "будет", "ж", "тогда", "кто", "этот", "того", "потому",
    "этого", "какой", "совсем", "ним", "здесь", "этом", "один", "почти", "мой", "тем", "чтобы", "нее", "были", "куда", "зачем", "всех",
    "это", "эти", "эта", "при", "об", "также", "the", "a", "an", "and", "or", "of", "to", "in", "on", "for", "is", "are", "was", "were",
    "be", "it", "this", "that", "with", "as", "at", "by", "from", "not", "but", "we", "you", "i", "he", "she", "they", "them", "our",
];

fn is_stop(w: &str) -> bool {
    STOP.contains(&w)
}

pub fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric() && c != '-')
        .filter(|w| w.chars().count() > 1)
        .map(|w| w.to_lowercase())
        .collect()
}

/// Crude stemmer: cuts common Russian/English endings so word forms match.
pub fn stem(w: &str) -> String {
    let cs: Vec<char> = w.chars().collect();
    if cs.len() <= 4 {
        return w.to_owned();
    }
    const ENDS: &[&str] = &[
        "ами", "ями", "ого", "его", "ому", "ему", "ыми", "ими", "ой", "ей", "ий", "ый", "ая", "яя", "ое", "ее", "ые", "ие", "ов", "ев", "ах",
        "ях", "ам", "ям", "ом", "ем", "ую", "юю", "ть", "ла", "ли", "ло", "ет", "ит", "ут", "ют", "ат", "ят", "а", "я", "ы", "и", "о", "е",
        "у", "ю", "ь", "ing", "ed", "es", "s",
    ];
    for e in ENDS {
        if w.ends_with(e) && cs.len() - e.chars().count() >= 3 {
            return cs[..cs.len() - e.chars().count()].iter().collect();
        }
    }
    w.to_owned()
}

pub fn sentences(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let chars: Vec<char> = text.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        cur.push(c);
        let end = matches!(c, '.' | '!' | '?' | '\n') && chars.get(i + 1).is_none_or(|n| n.is_whitespace());
        if end {
            let t = cur.trim().to_owned();
            if t.chars().count() > 2 {
                out.push(t);
            }
            cur.clear();
        }
    }
    let t = cur.trim().to_owned();
    if t.chars().count() > 2 {
        out.push(t);
    }
    out
}

fn freqs(text: &str) -> HashMap<String, f32> {
    let mut f = HashMap::new();
    for w in words(text) {
        if !is_stop(&w) && !w.chars().all(|c| c.is_ascii_digit()) {
            *f.entry(stem(&w)).or_insert(0.0) += 1.0;
        }
    }
    f
}

/// Extractive summary: the highest-scoring sentences in original order.
pub fn summarize(text: &str, max_sentences: usize) -> String {
    let sents = sentences(text);
    if sents.len() <= max_sentences {
        return sents.join(" ");
    }
    let f = freqs(text);
    let maxf = f.values().copied().fold(1.0, f32::max);
    let mut scored: Vec<(usize, f32)> = sents
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let ws = words(s);
            let n = ws.len().max(1) as f32;
            let sc: f32 = ws.iter().map(|w| f.get(&stem(w)).copied().unwrap_or(0.0) / maxf).sum::<f32>() / n.sqrt();
            // Slight preference for early sentences.
            (i, sc * (1.0 + 0.3 / (1.0 + i as f32)))
        })
        .collect();
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let mut pick: Vec<usize> = scored.iter().take(max_sentences).map(|x| x.0).collect();
    pick.sort_unstable();
    pick.iter().map(|&i| sents[i].clone()).collect::<Vec<_>>().join(" ")
}

pub struct FoundTask {
    pub text: String,
    pub due: Option<chrono::NaiveDate>,
}

const TASK_WORDS: &[&str] = &[
    "надо", "нужно", "необходимо", "сделать", "подготовить", "отправить", "позвонить", "написать", "купить", "проверить", "согласовать",
    "встретиться", "закончить", "сдать", "оплатить", "забрать", "заказать", "обновить", "исправить", "todo", "to do", "must", "should",
    "need to", "call", "send", "prepare", "finish", "buy", "fix", "срок", "дедлайн", "до ",
];

/// Lines that look like tasks, with a due date if one is mentioned.
pub fn extract_tasks(text: &str) -> Vec<FoundTask> {
    let today = chrono::Local::now().date_naive();
    let mut out = Vec::new();
    for s in sentences(text) {
        let l = s.to_lowercase();
        let checkbox = l.starts_with("[ ]") || l.starts_with("- [ ]");
        if !(checkbox || TASK_WORDS.iter().any(|w| l.contains(w))) {
            continue;
        }
        let due = find_date(&l, today);
        let t = s.trim_start_matches("- ").trim_start_matches("[ ] ").to_owned();
        out.push(FoundTask { text: t, due });
    }
    out
}

pub fn find_date(l: &str, today: chrono::NaiveDate) -> Option<chrono::NaiveDate> {
    use chrono::Datelike;
    for tok in l.split(|c: char| c.is_whitespace() || c == ',' || c == '(' || c == ')') {
        let t = tok.trim_end_matches('.');
        for fmt in ["%d.%m.%Y", "%Y-%m-%d", "%d.%m.%y"] {
            if let Ok(d) = chrono::NaiveDate::parse_from_str(t, fmt) {
                return Some(d);
            }
        }
        if let Ok(d) = chrono::NaiveDate::parse_from_str(&format!("{t}.{}", today.year()), "%d.%m.%Y") {
            return Some(d);
        }
    }
    if l.contains("послезавтра") {
        return Some(today + chrono::Duration::days(2));
    }
    if l.contains("завтра") || l.contains("tomorrow") {
        return Some(today + chrono::Duration::days(1));
    }
    if l.contains("сегодня") || l.contains("today") {
        return Some(today);
    }
    let days = [
        ("понедельник", 0), ("вторник", 1), ("сред", 2), ("четверг", 3), ("пятниц", 4), ("суббот", 5), ("воскресен", 6),
    ];
    for (d, n) in days {
        if l.contains(d) {
            let cur = today.weekday().num_days_from_monday() as i64;
            let mut delta = n as i64 - cur;
            if delta <= 0 {
                delta += 7;
            }
            return Some(today + chrono::Duration::days(delta));
        }
    }
    if l.contains("через неделю") || l.contains("next week") {
        return Some(today + chrono::Duration::days(7));
    }
    None
}

/// Fixes spacing, punctuation, repeated words and capitalization.
pub fn cleanup(text: &str) -> String {
    let mut s = text.to_owned();
    while s.contains("  ") {
        s = s.replace("  ", " ");
    }
    for p in [" ,", " .", " !", " ?", " :", " ;"] {
        s = s.replace(p, &p[1..]);
    }
    for p in [",", ".", "!", "?", ":", ";"] {
        // Ensure a space after punctuation followed by a letter.
        let mut out = String::new();
        let cs: Vec<char> = s.chars().collect();
        for i in 0..cs.len() {
            out.push(cs[i]);
            if cs[i].to_string() == p && cs.get(i + 1).is_some_and(|c| c.is_alphabetic()) && !cs.get(i.wrapping_sub(1)).is_some_and(|c| c.is_ascii_digit()) {
                out.push(' ');
            }
        }
        s = out;
    }
    // Repeated words: "в в" → "в"
    let toks: Vec<&str> = s.split(' ').collect();
    let mut out: Vec<&str> = Vec::new();
    for t in toks {
        if out.last().is_some_and(|l| l.to_lowercase() == t.to_lowercase() && t.chars().all(|c| c.is_alphabetic())) {
            continue;
        }
        out.push(t);
    }
    let s = out.join(" ");
    // Capitalize sentences.
    let mut res = String::new();
    let mut cap = true;
    for c in s.chars() {
        if cap && c.is_alphabetic() {
            res.extend(c.to_uppercase());
            cap = false;
        } else {
            res.push(c);
        }
        if matches!(c, '.' | '!' | '?' | '\n') {
            cap = true;
        }
    }
    res
}

/// Simple paraphrase: replaces bureaucratic phrases with plain ones.
pub fn rephrase(text: &str) -> String {
    let mut s = cleanup(text);
    for (a, b) in [
        ("в связи с тем, что", "потому что"), ("в настоящее время", "сейчас"), ("на сегодняшний день", "сейчас"),
        ("осуществлять", "делать"), ("осуществить", "сделать"), ("является", "—"), ("в целях", "чтобы"),
        ("в случае если", "если"), ("в течение", "за"), ("при помощи", "с помощью"), ("данный", "этот"),
        ("данная", "эта"), ("данное", "это"), ("в рамках", "в"), ("производить", "делать"), ("in order to", "to"),
        ("due to the fact that", "because"), ("at this point in time", "now"),
    ] {
        s = s.replace(a, b);
        let cap = {
            let mut c = a.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        };
        let capb = {
            let mut c = b.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        };
        s = s.replace(&cap, &capb);
    }
    s
}

/// Outline skeleton for a topic, in markup.
pub fn outline(topic: &str, kind: usize) -> String {
    let t = topic.trim();
    match kind {
        0 => format!(
            "# {t}\n## 1. Введение\n- Что такое {t}\n- Почему это важно\n## 2. Основные понятия\n- \n## 3. Как это работает\n1. \n2. \n3. \n## 4. Примеры\n- \n## 5. Плюсы и минусы\n- Плюсы:\n- Минусы:\n## 6. Выводы\n- \n## Источники\n- "
        ),
        1 => format!(
            "# План проекта: {t}\n## Цель\n- \n## Результат\n- \n## Этапы\n[ ] Исследование\n[ ] Планирование\n[ ] Реализация\n[ ] Проверка\n[ ] Запуск\n## Сроки\n- Начало: \n- Окончание: \n## Риски\n- \n## Ресурсы\n- "
        ),
        _ => format!(
            "# {t}: доклад\n## Вступление (1 мин)\n- Приветствие, тема, зачем слушать\n## Проблема\n- \n## Решение\n- \n## Доказательства\n- \n## Итог и призыв к действию\n- \n## Вопросы"
        ),
    }
}

/// Answers a question by finding the best-matching passages in the notes.
pub fn answer<'a>(question: &str, docs: impl Iterator<Item = (&'a str, &'a str)>) -> Vec<(String, String, f32)> {
    let q: Vec<String> = words(question).into_iter().filter(|w| !is_stop(w)).map(|w| stem(&w)).collect();
    if q.is_empty() {
        return Vec::new();
    }
    let mut res = Vec::new();
    for (title, text) in docs {
        for s in sentences(text) {
            let ws: Vec<String> = words(&s).iter().map(|w| stem(w)).collect();
            let hit = q.iter().filter(|t| ws.contains(t)).count();
            if hit == 0 {
                continue;
            }
            let score = hit as f32 / q.len() as f32 + 0.1 / (1.0 + (ws.len() as f32 - 15.0).abs() / 15.0);
            res.push((title.to_owned(), s, score));
        }
    }
    res.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
    res.truncate(8);
    res
}

/// Keyword-based tag suggestions from the user's tag names and top terms.
pub fn suggest_tags(text: &str, tag_names: &[(u32, String)]) -> (Vec<u32>, Vec<String>) {
    let l = text.to_lowercase();
    let mut tags = Vec::new();
    let rules: &[(&str, &[&str])] = &[
        ("Задача", &["надо", "нужно", "сделать", "todo", "[ ]"]),
        ("Вопрос", &["?", "почему", "как ", "зачем"]),
        ("Важно", &["важно", "обязательно", "срочно"]),
        ("Критично", &["критично", "авария", "срочно"]),
        ("Телефон", &["+7", "тел.", "телефон"]),
        ("Адрес", &["ул.", "улица", "дом ", "проспект", "г. "]),
        ("Идея", &["идея", "можно было бы", "а что если"]),
        ("Прочитать", &["прочитать", "книга", "статья"]),
        ("Фильм", &["фильм", "сериал", "кино"]),
        ("Музыка", &["песня", "альбом", "музыка"]),
        ("Контакт", &["@", "контакт"]),
    ];
    for (name, keys) in rules {
        if keys.iter().any(|k| l.contains(k)) {
            if let Some((id, _)) = tag_names.iter().find(|(_, n)| n == name) {
                tags.push(*id);
            }
        }
    }
    let f = freqs(text);
    let mut top: Vec<(String, f32)> = f.into_iter().collect();
    top.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    // Map stems back to a real word for display.
    let all = words(text);
    let kw = top
        .iter()
        .take(6)
        .filter_map(|(st, _)| all.iter().find(|w| stem(w) == *st).cloned())
        .collect();
    (tags, kw)
}

pub fn suggest_title(text: &str) -> String {
    for line in text.lines() {
        let l = line.trim().trim_start_matches('#').trim();
        if l.chars().count() >= 3 {
            let t: String = l.chars().take(60).collect();
            return t;
        }
    }
    let (_, kw) = suggest_tags(text, &[]);
    let mut t = kw.into_iter().take(3).collect::<Vec<_>>().join(", ");
    if let Some(c) = t.get(..1) {
        t = c.to_uppercase() + &t[1..];
    }
    t
}

/// Flash cards from "term — definition", "term: definition" lines and
/// headings followed by text.
pub fn flashcards(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let lines: Vec<&str> = text.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let l = line.trim().trim_start_matches("- ").trim_start_matches("* ");
        for sep in [" — ", " - ", ": ", " – "] {
            if let Some((a, b)) = l.split_once(sep) {
                if a.chars().count() <= 60 && b.chars().count() >= 3 && !a.is_empty() {
                    out.push((a.trim().to_owned(), b.trim().to_owned()));
                    break;
                }
            }
        }
        if l.starts_with('#') {
            let q = l.trim_start_matches('#').trim().to_owned();
            let body: Vec<&str> = lines[i + 1..].iter().take_while(|x| !x.trim().starts_with('#')).copied().collect();
            let a = body.join(" ").trim().to_owned();
            if !q.is_empty() && !a.is_empty() {
                out.push((q, summarize(&a, 2)));
            }
        }
    }
    out
}

/// Russian syllable splitting for the reading mode.
pub fn syllables(word: &str) -> String {
    const V: &str = "аеёиоуыэюяaeiouyАЕЁИОУЫЭЮЯAEIOUY";
    let cs: Vec<char> = word.chars().collect();
    let vowels: Vec<usize> = cs.iter().enumerate().filter(|(_, c)| V.contains(**c)).map(|(i, _)| i).collect();
    if vowels.len() < 2 {
        return word.to_owned();
    }
    let mut cuts = Vec::new();
    for w in vowels.windows(2) {
        let (a, b) = (w[0], w[1]);
        let cons = b - a - 1;
        let cut = match cons {
            0 => a + 1,
            1 => a + 1,
            _ => {
                // Keep "й", "ь", "ъ" with the previous syllable.
                let mut c = a + 2;
                if matches!(cs[a + 1], 'й' | 'Й') {
                    c = a + 2;
                } else if cons >= 2 && matches!(cs[a + 2], 'ь' | 'ъ') {
                    c = a + 3;
                }
                c.min(b)
            }
        };
        cuts.push(cut);
    }
    let mut out = String::new();
    for (i, c) in cs.iter().enumerate() {
        if cuts.contains(&i) {
            out.push('·');
        }
        out.push(*c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn helpers() {
        assert!(!summarize("Кошки спят. Кошки едят. Собаки лают громко. Кошки играют.", 2).is_empty());
        assert_eq!(extract_tasks("Нужно позвонить Ивану завтра. Погода хорошая.").len(), 1);
        assert_eq!(syllables("молоко"), "мо·ло·ко");
        assert_eq!(flashcards("Атом — мельчайшая частица").len(), 1);
    }
}
