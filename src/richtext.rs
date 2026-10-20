//! Lightweight rich text markup used by text containers.
//!
//! Line markers: `# `..`###### ` headings, `> ` quote, `!> ` note, fenced
//! ```` ``` ```` code, `- `/`* ` bullets, `1. ` numbers, `[ ] `/`[x] ` check
//! boxes, `---` rule; leading pairs of spaces nest list levels.
//! Inline: `**bold**`, `*italic*`, `__underline__`, `~~strike~~`, `^sup^`,
//! `,,sub,,`, `==highlight==`, `` `code` ``, `[text](url)`,
//! `{c:#rrggbb|text}` colour, `{h:#rrggbb|text}` highlight,
//! `{f:Font|text}` font, `{s:18|text}` size. `\` escapes a character.

use egui::text::{LayoutJob, TextFormat};
use egui::{Align, Color32, Stroke};

use crate::fonts;
use crate::model::TextBlock;

#[derive(Clone, Debug, PartialEq)]
pub enum LineKind {
    Normal,
    Heading(u8),
    Quote,
    Note,
    Code,
    Fence,
    Bullet(u8),
    Number(u8, u32),
    Check(u8, bool),
    Rule,
}

pub struct LineInfo {
    pub kind: LineKind,
    /// Char count of the line marker (including indentation).
    pub prefix: usize,
}

pub fn parse_lines(text: &str) -> Vec<LineInfo> {
    let mut out = Vec::new();
    let mut in_code = false;
    let mut counters = [0u32; 8];
    for line in text.split('\n') {
        let trimmed = line.trim_start_matches(' ');
        let indent = line.len() - trimmed.len();
        let level = (indent / 2).min(7) as u8;
        if trimmed.starts_with("```") {
            in_code = !in_code;
            out.push(LineInfo { kind: LineKind::Fence, prefix: line.chars().count() });
            continue;
        }
        if in_code {
            out.push(LineInfo { kind: LineKind::Code, prefix: 0 });
            continue;
        }
        let (kind, prefix) = if let Some(n) = heading_level(trimmed) {
            (LineKind::Heading(n), indent + n as usize + 1)
        } else if trimmed == "---" || trimmed == "***" {
            (LineKind::Rule, line.chars().count())
        } else if trimmed.starts_with("!> ") {
            (LineKind::Note, indent + 3)
        } else if trimmed.starts_with("> ") {
            (LineKind::Quote, indent + 2)
        } else if trimmed.starts_with("[ ] ") {
            (LineKind::Check(level, false), indent + 4)
        } else if trimmed.starts_with("[x] ") || trimmed.starts_with("[X] ") {
            (LineKind::Check(level, true), indent + 4)
        } else if trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("• ") {
            (LineKind::Bullet(level), indent + 2)
        } else if let Some(p) = number_prefix(trimmed) {
            let lvl = level as usize;
            counters[lvl] += 1;
            for c in counters.iter_mut().skip(lvl + 1) {
                *c = 0;
            }
            (LineKind::Number(level, counters[lvl]), indent + p)
        } else {
            (LineKind::Normal, 0)
        };
        if !matches!(kind, LineKind::Number(..)) && !matches!(kind, LineKind::Bullet(_) | LineKind::Check(..)) {
            counters = [0; 8];
        }
        out.push(LineInfo { kind, prefix });
    }
    out
}

fn heading_level(s: &str) -> Option<u8> {
    let n = s.chars().take_while(|c| *c == '#').count();
    ((1..=6).contains(&n) && s[n..].starts_with(' ')).then_some(n as u8)
}

fn number_prefix(s: &str) -> Option<usize> {
    let digits = s.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits > 0 && digits < 6 && s[digits..].starts_with(". ") {
        Some(digits + 2)
    } else {
        None
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Inline {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub sup: bool,
    pub sub: bool,
    pub mark: bool,
    pub code: bool,
    pub color: Option<Color32>,
    pub highlight: Option<Color32>,
    pub font: Option<String>,
    pub size: Option<f32>,
    pub link: Option<String>,
}

/// A run of text: either a substring of the source (with its start char index)
/// or decoration inserted only for display.
pub struct Run {
    pub text: String,
    pub src: Option<usize>,
    pub style: Inline,
    pub marker: bool,
}

fn parse_hex(s: &str) -> Option<Color32> {
    let s = s.trim_start_matches('#');
    if s.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(s, 16).ok()?;
    Some(Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8))
}

/// Parses inline markup of one line. `offset` is the char index of `line[0]`
/// in the whole text.
pub fn parse_inline(line: &[char], offset: usize, base: &Inline) -> Vec<Run> {
    let mut runs: Vec<Run> = Vec::new();
    let mut st = base.clone();
    let mut brace_stack: Vec<Inline> = Vec::new();
    let mut cur = String::new();
    let mut cur_start = 0usize;
    let n = line.len();
    let mut i = 0;

    macro_rules! flush {
        () => {
            if !cur.is_empty() {
                runs.push(Run { text: std::mem::take(&mut cur), src: Some(offset + cur_start), style: st.clone(), marker: false });
            }
        };
    }
    let marker = |runs: &mut Vec<Run>, from: usize, len: usize, st: &Inline| {
        let text: String = line[from..from + len].iter().collect();
        runs.push(Run { text, src: Some(offset + from), style: st.clone(), marker: true });
    };
    let can_open = |i: usize, len: usize| i + len < n && !line[i + len].is_whitespace();
    let can_close = |i: usize| i > 0 && !line[i - 1].is_whitespace();

    while i < n {
        let c = line[i];
        let next = line.get(i + 1).copied();
        if st.code && c != '`' {
            if cur.is_empty() {
                cur_start = i;
            }
            cur.push(c);
            i += 1;
            continue;
        }
        // Escapes.
        if c == '\\' && next.is_some_and(|x| "*_~^,=`[]{}\\|".contains(x)) {
            flush!();
            marker(&mut runs, i, 1, &st);
            cur_start = i + 1;
            cur.push(line[i + 1]);
            i += 2;
            continue;
        }
        // Two-char toggles.
        let pair = next.map(|x| (c, x));
        let toggle2 = match pair {
            Some(('*', '*')) => Some(0),
            Some(('_', '_')) => Some(1),
            Some(('~', '~')) => Some(2),
            Some(('=', '=')) => Some(3),
            Some((',', ',')) => Some(4),
            _ => None,
        };
        if let Some(k) = toggle2 {
            let on = match k {
                0 => st.bold,
                1 => st.underline,
                2 => st.strike,
                3 => st.mark,
                _ => st.sub,
            };
            if (on && can_close(i)) || (!on && can_open(i, 2)) {
                flush!();
                marker(&mut runs, i, 2, &st);
                match k {
                    0 => st.bold = !on,
                    1 => st.underline = !on,
                    2 => st.strike = !on,
                    3 => st.mark = !on,
                    _ => st.sub = !on,
                }
                i += 2;
                continue;
            }
        }
        if c == '*' || c == '^' {
            let on = if c == '*' { st.italic } else { st.sup };
            if (on && can_close(i)) || (!on && can_open(i, 1)) {
                flush!();
                marker(&mut runs, i, 1, &st);
                if c == '*' {
                    st.italic = !on;
                } else {
                    st.sup = !on;
                }
                i += 1;
                continue;
            }
        }
        if c == '`' {
            flush!();
            marker(&mut runs, i, 1, &st);
            st.code = !st.code;
            i += 1;
            continue;
        }
        // [text](url)
        if c == '[' {
            if let Some(close) = (i + 1..n).find(|&j| line[j] == ']') {
                if line.get(close + 1) == Some(&'(') {
                    if let Some(end) = (close + 2..n).find(|&j| line[j] == ')') {
                        flush!();
                        let url: String = line[close + 2..end].iter().collect();
                        marker(&mut runs, i, 1, &st);
                        let mut ls = st.clone();
                        ls.link = Some(url);
                        let inner = parse_inline(&line[i + 1..close], offset + i + 1, &ls);
                        runs.extend(inner);
                        marker(&mut runs, close, end - close + 1, &st);
                        i = end + 1;
                        cur_start = i;
                        continue;
                    }
                }
            }
        }
        // {x:value|
        if c == '{' && i + 2 < n && line[i + 2] == ':' && "chfs".contains(line[i + 1]) {
            if let Some(bar) = (i + 3..n.min(i + 40)).find(|&j| line[j] == '|') {
                let val: String = line[i + 3..bar].iter().collect();
                let mut ns = st.clone();
                let ok = match line[i + 1] {
                    'c' => parse_hex(&val).map(|c| ns.color = Some(c)).is_some(),
                    'h' => parse_hex(&val).map(|c| ns.highlight = Some(c)).is_some(),
                    'f' => {
                        ns.font = Some(val.trim().to_owned());
                        true
                    }
                    's' => val.trim().parse::<f32>().ok().map(|s| ns.size = Some(s.clamp(4.0, 200.0))).is_some(),
                    _ => false,
                };
                if ok {
                    flush!();
                    marker(&mut runs, i, bar - i + 1, &st);
                    brace_stack.push(st.clone());
                    st = ns;
                    i = bar + 1;
                    continue;
                }
            }
        }
        if c == '}' && !brace_stack.is_empty() {
            flush!();
            marker(&mut runs, i, 1, &st);
            st = brace_stack.pop().unwrap();
            i += 1;
            continue;
        }
        if cur.is_empty() {
            cur_start = i;
        }
        cur.push(c);
        i += 1;
    }
    flush!();
    runs
}

pub fn strip_markup(text: &str) -> String {
    let lines = parse_lines(text);
    let mut out = String::with_capacity(text.len());
    let mut offset = 0;
    for (li, line) in text.split('\n').enumerate() {
        let chars: Vec<char> = line.chars().collect();
        let info = &lines[li];
        if li > 0 {
            out.push('\n');
        }
        match info.kind {
            LineKind::Fence | LineKind::Rule => {}
            LineKind::Code => out.push_str(line),
            _ => {
                let p = info.prefix.min(chars.len());
                for r in parse_inline(&chars[p..], offset + p, &Inline::default()) {
                    if !r.marker {
                        out.push_str(&r.text);
                    }
                }
            }
        }
        offset += chars.len() + 1;
    }
    out
}

// ----------------------------------------------------------------------------
// Layout

#[derive(Clone, Debug)]
pub enum Click {
    Toggle(usize),
    Link(String),
}

pub struct Ctx<'a> {
    pub dark: bool,
    pub zoom: f32,
    pub edit: bool,
    pub highlights: &'a [(usize, usize)],
    pub misspelled: &'a dyn Fn(&str) -> bool,
    pub detect: bool,
}

pub struct Layout {
    pub job: LayoutJob,
    /// Clickable ranges in galley char indices.
    pub clicks: Vec<(usize, usize, Click)>,
}

pub fn default_text_color(dark: bool) -> Color32 {
    if dark { Color32::from_gray(225) } else { Color32::from_gray(25) }
}

fn heading_scale(n: u8) -> f32 {
    match n {
        1 => 2.0,
        2 => 1.6,
        3 => 1.35,
        4 => 1.15,
        5 => 1.05,
        _ => 1.0,
    }
}

pub fn layout(tb: &TextBlock, cx: &Ctx, wrap: f32) -> Layout {
    let mut job = LayoutJob::default();
    job.wrap.max_width = wrap;
    job.halign = match tb.align {
        crate::model::Align::Center => Align::Center,
        crate::model::Align::Right => Align::RIGHT,
        _ if tb.rtl => Align::RIGHT,
        _ => Align::LEFT,
    };
    job.justify = tb.align == crate::model::Align::Justify;
    let mut clicks = Vec::new();
    let base_color = if tb.color[3] == 0 {
        default_text_color(cx.dark)
    } else {
        Color32::from_rgba_unmultiplied(tb.color[0], tb.color[1], tb.color[2], tb.color[3])
    };
    let z = cx.zoom;
    let lines = parse_lines(&tb.text);
    let mut offset = 0usize;
    let mut out_chars = 0usize;
    let marker_color = if cx.dark { Color32::from_gray(110) } else { Color32::from_gray(160) };
    let text_lines: Vec<&str> = tb.text.split('\n').collect();
    for (li, line) in text_lines.iter().enumerate() {
        let chars: Vec<char> = line.chars().collect();
        let info = &lines[li];
        let last = li + 1 == text_lines.len();
        let mut base = Inline::default();
        let mut size = tb.size;
        match info.kind {
            LineKind::Heading(n) => {
                size *= heading_scale(n);
                base.bold = n <= 4;
            }
            LineKind::Code | LineKind::Fence => base.code = true,
            LineKind::Quote => base.italic = true,
            LineKind::Check(_, true) => base.strike = !cx.edit,
            _ => {}
        }
        let push = |job: &mut LayoutJob, text: &str, src: Option<usize>, st: &Inline, marker: bool, size: f32, out_chars: &mut usize| {
            let mut fmt = format_for(tb, st, size * z, base_color, cx.dark);
            if marker {
                fmt.color = marker_color;
                fmt.font_id.size = (size * z * 0.8).max(6.0);
            }
            match info.kind {
                LineKind::Quote => {
                    fmt.color = fmt.color.gamma_multiply(0.8);
                }
                LineKind::Note => {
                    fmt.background = if cx.dark { Color32::from_rgb(60, 55, 30) } else { Color32::from_rgb(255, 248, 210) };
                }
                LineKind::Code | LineKind::Fence => {
                    fmt.background = if cx.dark { Color32::from_gray(40) } else { Color32::from_gray(238) };
                }
                _ => {}
            }
            if tb.line_spacing != 1.0 {
                fmt.line_height = Some(size * z * 1.3 * tb.line_spacing);
            }
            // Split for search highlights and spelling.
            let cs: Vec<char> = text.chars().collect();
            let mut k = 0;
            while k < cs.len() {
                let gi = src.map(|s| s + k);
                let hl = gi.is_some_and(|g| cx.highlights.iter().any(|(a, b)| g >= *a && g < *b));
                // Word boundaries for the speller.
                let mut e = k + 1;
                if cs[k].is_alphabetic() {
                    while e < cs.len() && (cs[e].is_alphabetic() || cs[e] == '\'' || cs[e] == '-') {
                        e += 1;
                    }
                } else {
                    while e < cs.len() && !cs[e].is_alphabetic() {
                        e += 1;
                    }
                }
                // Further split where highlight state changes.
                if src.is_some() {
                    let mut e2 = k + 1;
                    while e2 < e {
                        let g2 = src.unwrap() + e2;
                        let h2 = cx.highlights.iter().any(|(a, b)| g2 >= *a && g2 < *b);
                        if h2 != hl {
                            break;
                        }
                        e2 += 1;
                    }
                    e = e2;
                }
                let word: String = cs[k..e].iter().collect();
                let mut f = fmt.clone();
                if hl {
                    f.background = Color32::from_rgb(255, 214, 0);
                    f.color = Color32::BLACK;
                }
                if !marker && !st.code && cs[k].is_alphabetic() && word.chars().count() > 1 && (cx.misspelled)(&word) {
                    f.underline = Stroke::new(1.5, Color32::from_rgb(230, 40, 40));
                }
                job.append(&word, 0.0, f);
                *out_chars += e - k;
                k = e;
            }
        };

        let display_prefix = |kind: &LineKind| -> Option<String> {
            match kind {
                LineKind::Bullet(l) => Some(format!("{}{} ", "    ".repeat(*l as usize), ["•", "◦", "▪"][*l as usize % 3])),
                LineKind::Number(l, n) => Some(format!("{}{}. ", "    ".repeat(*l as usize), n)),
                LineKind::Check(l, c) => Some(format!("{}{} ", "    ".repeat(*l as usize), if *c { "☑" } else { "☐" })),
                LineKind::Quote => Some("┃ ".into()),
                LineKind::Note => Some("ℹ ".into()),
                _ => None,
            }
        };

        match info.kind {
            LineKind::Rule if !cx.edit => {
                let w = ((wrap / (size * z * 0.5)).floor() as usize).clamp(3, 200);
                push(&mut job, &"─".repeat(w), None, &Inline::default(), true, size, &mut out_chars);
            }
            LineKind::Fence if !cx.edit => {}
            LineKind::Code => {
                push(&mut job, line, Some(offset), &base, false, size, &mut out_chars);
            }
            _ => {
                let p = info.prefix.min(chars.len());
                if cx.edit {
                    if p > 0 {
                        let t: String = chars[..p].iter().collect();
                        push(&mut job, &t, Some(offset), &base, true, size, &mut out_chars);
                    }
                } else if let Some(dp) = display_prefix(&info.kind) {
                    let start = out_chars;
                    let st = Inline { bold: matches!(info.kind, LineKind::Number(..)), ..Inline::default() };
                    push(&mut job, &dp, None, &st, false, size, &mut out_chars);
                    if let LineKind::Check(..) = info.kind {
                        clicks.push((start, out_chars, Click::Toggle(li)));
                    }
                }
                for r in parse_inline(&chars[p..], offset + p, &base) {
                    if r.marker && !cx.edit {
                        continue;
                    }
                    let start = out_chars;
                    if !cx.edit && r.style.link.is_none() && cx.detect && !r.style.code {
                        // Auto-detected links, mail, phones and dates.
                        let found = detect(&r.text);
                        if !found.is_empty() {
                            let cs: Vec<char> = r.text.chars().collect();
                            let mut pos = 0;
                            for (a, b, url) in found {
                                if a > pos {
                                    let t: String = cs[pos..a].iter().collect();
                                    push(&mut job, &t, r.src.map(|s| s + pos), &r.style, false, size, &mut out_chars);
                                }
                                let t: String = cs[a..b].iter().collect();
                                let mut ls = r.style.clone();
                                ls.link = Some(url.clone());
                                let s0 = out_chars;
                                push(&mut job, &t, r.src.map(|s| s + a), &ls, false, size, &mut out_chars);
                                clicks.push((s0, out_chars, Click::Link(url)));
                                pos = b;
                            }
                            if pos < cs.len() {
                                let t: String = cs[pos..].iter().collect();
                                push(&mut job, &t, r.src.map(|s| s + pos), &r.style, false, size, &mut out_chars);
                            }
                            continue;
                        }
                    }
                    push(&mut job, &r.text, r.src, &r.style, r.marker, size, &mut out_chars);
                    if let (Some(url), false) = (&r.style.link, cx.edit) {
                        clicks.push((start, out_chars, Click::Link(url.clone())));
                    }
                }
            }
        }
        if !last {
            let mut f = format_for(tb, &base, size * z, base_color, cx.dark);
            if tb.para_after > 0.0 || tb.para_before > 0.0 {
                f.line_height = Some(size * z * 1.3 + (tb.para_after + tb.para_before) * z);
            }
            job.append("\n", 0.0, f);
            out_chars += 1;
        }
        offset += chars.len() + 1;
    }
    if job.sections.is_empty() {
        job.append("", 0.0, format_for(tb, &Inline::default(), tb.size * z, base_color, cx.dark));
    }
    Layout { job, clicks }
}

pub fn format_for(tb: &TextBlock, st: &Inline, size: f32, base_color: Color32, dark: bool) -> TextFormat {
    let mut size = st.size.map(|s| s * size / tb.size.max(1.0)).unwrap_or(size);
    if st.sup || st.sub {
        size *= 0.65;
    }
    let font_name = st.font.as_deref().unwrap_or(&tb.font);
    let font_id = fonts::font(font_name, size, st.bold, st.italic, st.code);
    let mut f = TextFormat {
        font_id,
        color: st.color.unwrap_or(base_color),
        italics: st.italic && !fonts::has_italic(font_name) && !st.code,
        ..Default::default()
    };
    if st.sup {
        f.valign = Align::TOP;
    }
    if st.sub {
        f.valign = Align::BOTTOM;
    }
    if st.underline || st.link.is_some() {
        f.underline = Stroke::new(1.0, f.color);
    }
    if st.link.is_some() && st.color.is_none() {
        f.color = if dark { Color32::from_rgb(110, 170, 255) } else { Color32::from_rgb(20, 90, 200) };
        f.underline = Stroke::new(1.0, f.color);
    }
    if st.strike {
        f.strikethrough = Stroke::new(1.0, f.color);
    }
    if st.mark {
        f.background = Color32::from_rgb(255, 240, 120);
        if dark {
            f.color = Color32::BLACK;
        }
    }
    if let Some(h) = st.highlight {
        f.background = h;
    }
    if st.code {
        f.background = if dark { Color32::from_gray(45) } else { Color32::from_gray(232) };
    }
    f
}

/// Finds URLs, e-mails, phone numbers and dates: (start, end, target) in chars.
pub fn detect(text: &str) -> Vec<(usize, usize, String)> {
    let cs: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    let is_word_start = |i: usize| i == 0 || !cs[i - 1].is_alphanumeric();
    while i < cs.len() {
        let rest: String = cs[i..cs.len().min(i + 8)].iter().collect();
        let lower = rest.to_lowercase();
        if is_word_start(i) && (lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("www.") || lower.starts_with("file:///") || lower.starts_with("note://")) {
            let mut e = i;
            while e < cs.len() && !cs[e].is_whitespace() && !"<>\"".contains(cs[e]) {
                e += 1;
            }
            while e > i && ".,;:!?)".contains(cs[e - 1]) {
                e -= 1;
            }
            let s: String = cs[i..e].iter().collect();
            let url = if s.to_lowercase().starts_with("www.") { format!("https://{s}") } else { s };
            out.push((i, e, url));
            i = e;
            continue;
        }
        if cs[i] == '@' && i > 0 {
            let mut a = i;
            while a > 0 && (cs[a - 1].is_alphanumeric() || "._-+".contains(cs[a - 1])) {
                a -= 1;
            }
            let mut e = i + 1;
            while e < cs.len() && (cs[e].is_alphanumeric() || ".-".contains(cs[e])) {
                e += 1;
            }
            while e > i + 1 && cs[e - 1] == '.' {
                e -= 1;
            }
            let domain: String = cs[i + 1..e].iter().collect();
            if a < i && domain.contains('.') {
                // Drop anything already emitted inside the address.
                out.retain(|(s, _, _)| *s < a);
                let s: String = cs[a..e].iter().collect();
                out.push((a, e, format!("mailto:{s}")));
                i = e;
                continue;
            }
        }
        if is_word_start(i) && (cs[i].is_ascii_digit() || (cs[i] == '+' && cs.get(i + 1).is_some_and(|c| c.is_ascii_digit()))) {
            // Dates: dd.mm.yyyy or yyyy-mm-dd
            let mut e = i;
            while e < cs.len() && (cs[e].is_ascii_digit() || "+-() .".contains(cs[e])) {
                e += 1;
            }
            while e > i && !cs[e - 1].is_ascii_digit() {
                e -= 1;
            }
            let s: String = cs[i..e].iter().collect();
            let digits = s.chars().filter(|c| c.is_ascii_digit()).count();
            let is_date = is_date_str(&s);
            if is_date {
                out.push((i, e, format!("date:{s}")));
                i = e;
                continue;
            }
            if (7..=15).contains(&digits) && (s.contains(' ') || s.contains('-') || s.contains('(') || s.starts_with('+') || digits >= 10) && !s.contains('.') {
                let tel: String = s.chars().filter(|c| c.is_ascii_digit() || *c == '+').collect();
                out.push((i, e, format!("tel:{tel}")));
                i = e;
                continue;
            }
            // Skip number.
            while i < cs.len() && cs[i].is_alphanumeric() {
                i += 1;
            }
            continue;
        }
        i += 1;
    }
    out
}

pub fn is_date_str(s: &str) -> bool {
    let s = s.trim();
    chrono::NaiveDate::parse_from_str(s, "%d.%m.%Y").is_ok() || chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").is_ok()
}

// ----------------------------------------------------------------------------
// Editing commands working on char ranges of the source text.

pub fn char_to_byte(s: &str, ci: usize) -> usize {
    s.char_indices().nth(ci).map(|(b, _)| b).unwrap_or(s.len())
}

/// Wraps [a, b) in `open`/`close` markers, or removes them if already wrapped.
/// Returns the new selection.
pub fn toggle_wrap(text: &mut String, a: usize, b: usize, open: &str, close: &str) -> (usize, usize) {
    let (oa, ob) = (char_to_byte(text, a), char_to_byte(text, b));
    let ol = open.chars().count();
    let cl = close.chars().count();
    // Already wrapped outside the selection?
    if oa >= open.len() && text[..oa].ends_with(open) && text[ob..].starts_with(close) {
        text.replace_range(ob..ob + close.len(), "");
        text.replace_range(oa - open.len()..oa, "");
        return (a - ol, b - ol);
    }
    // Selection includes the markers?
    let sel = &text[oa..ob];
    if sel.len() >= open.len() + close.len() && sel.starts_with(open) && sel.ends_with(close) {
        let inner = sel[open.len()..sel.len() - close.len()].to_owned();
        text.replace_range(oa..ob, &inner);
        return (a, b - ol - cl);
    }
    text.insert_str(ob, close);
    text.insert_str(oa, open);
    (a + ol, b + ol)
}

/// Removes all inline markers inside [a, b).
pub fn clear_format(text: &mut String, a: usize, b: usize) -> (usize, usize) {
    let (oa, ob) = (char_to_byte(text, a), char_to_byte(text, b));
    let chars: Vec<char> = text[oa..ob].chars().collect();
    let mut plain = String::new();
    for r in parse_inline(&chars, 0, &Inline::default()) {
        if !r.marker {
            plain.push_str(&r.text);
        }
    }
    let n = plain.chars().count();
    text.replace_range(oa..ob, &plain);
    (a, a + n)
}

/// Inline style at a char index (used by the format painter).
pub fn style_at(text: &str, ci: usize) -> Inline {
    let mut offset = 0;
    let lines = parse_lines(text);
    for (li, line) in text.split('\n').enumerate() {
        let chars: Vec<char> = line.chars().collect();
        if ci <= offset + chars.len() {
            let p = lines[li].prefix.min(chars.len());
            let mut last = Inline::default();
            for r in parse_inline(&chars[p..], offset + p, &Inline::default()) {
                if let Some(s) = r.src {
                    if !r.marker && ci >= s && ci <= s + r.text.chars().count() {
                        return r.style;
                    }
                    if s <= ci {
                        last = r.style.clone();
                    }
                }
            }
            return last;
        }
        offset += chars.len() + 1;
    }
    Inline::default()
}

pub fn apply_style(text: &mut String, a: usize, b: usize, st: &Inline) -> (usize, usize) {
    let (mut a, mut b) = clear_format(text, a, b);
    let mut wrap = |open: String, close: &str, text: &mut String| {
        let r = toggle_wrap(text, a, b, &open, close);
        a = r.0;
        b = r.1;
    };
    if st.bold {
        wrap("**".into(), "**", text);
    }
    if st.italic {
        wrap("*".into(), "*", text);
    }
    if st.underline {
        wrap("__".into(), "__", text);
    }
    if st.strike {
        wrap("~~".into(), "~~", text);
    }
    if st.mark {
        wrap("==".into(), "==", text);
    }
    if let Some(c) = st.color {
        wrap(format!("{{c:#{:02x}{:02x}{:02x}|", c.r(), c.g(), c.b()), "}", text);
    }
    if let Some(c) = st.highlight {
        wrap(format!("{{h:#{:02x}{:02x}{:02x}|", c.r(), c.g(), c.b()), "}", text);
    }
    if let Some(f) = &st.font {
        wrap(format!("{{f:{f}|"), "}", text);
    }
    if let Some(s) = st.size {
        wrap(format!("{{s:{s}|"), "}", text);
    }
    (a, b)
}

/// Returns (line index, line start char, line char len) for a char index.
pub fn line_of(text: &str, ci: usize) -> (usize, usize, usize) {
    let mut start = 0;
    for (i, l) in text.split('\n').enumerate() {
        let n = l.chars().count();
        if ci <= start + n {
            return (i, start, n);
        }
        start += n + 1;
    }
    (0, 0, 0)
}

/// Replaces the line marker of every line touched by [a, b).
pub fn set_line_prefix(text: &mut String, a: usize, b: usize, new_prefix: &str) -> isize {
    let infos = parse_lines(text);
    let (la, _, _) = line_of(text, a);
    let (lb, _, _) = line_of(text, b);
    let mut lines: Vec<String> = text.split('\n').map(|s| s.to_owned()).collect();
    let mut delta: isize = 0;
    // Toggle off when every line already has this marker.
    let all_same = (la..=lb).all(|i| {
        let l = &lines[i];
        let t = l.trim_start_matches(' ');
        !new_prefix.is_empty() && t.starts_with(new_prefix.trim_start_matches(' '))
    });
    for i in la..=lb {
        let info = &infos[i];
        let chars: Vec<char> = lines[i].chars().collect();
        let indent = chars.iter().take_while(|c| **c == ' ').count();
        let old_p = if info.prefix > 0 && !matches!(info.kind, LineKind::Code | LineKind::Fence | LineKind::Rule) {
            info.prefix
        } else {
            indent
        };
        let rest: String = chars[old_p.min(chars.len())..].iter().collect();
        let ind: String = " ".repeat(indent);
        let np = if all_same { String::new() } else { new_prefix.to_owned() };
        let new_line = format!("{ind}{np}{rest}");
        delta += new_line.chars().count() as isize - chars.len() as isize;
        lines[i] = new_line;
    }
    *text = lines.join("\n");
    delta
}

/// Changes indentation (list level) of lines in [a, b).
pub fn indent_lines(text: &mut String, a: usize, b: usize, outdent: bool) -> isize {
    let (la, _, _) = line_of(text, a);
    let (lb, _, _) = line_of(text, b);
    let mut lines: Vec<String> = text.split('\n').map(|s| s.to_owned()).collect();
    let mut delta = 0isize;
    for l in lines.iter_mut().take(lb + 1).skip(la) {
        if outdent {
            if l.starts_with("  ") {
                l.replace_range(0..2, "");
                delta -= 2;
            }
        } else {
            l.insert_str(0, "  ");
            delta += 2;
        }
    }
    *text = lines.join("\n");
    delta
}

pub fn toggle_check_line(text: &mut String, line: usize) {
    let mut lines: Vec<String> = text.split('\n').map(|s| s.to_owned()).collect();
    if let Some(l) = lines.get_mut(line) {
        let indent = l.len() - l.trim_start_matches(' ').len();
        let t = &l[indent..];
        if t.starts_with("[ ] ") {
            l.replace_range(indent..indent + 3, "[x]");
        } else if t.starts_with("[x] ") || t.starts_with("[X] ") {
            l.replace_range(indent..indent + 3, "[ ]");
        }
    }
    *text = lines.join("\n");
}

/// Continues lists on Enter: after a newline is inserted at `ci`, copies the
/// previous line's marker. Returns the number of chars inserted.
pub fn continue_list(text: &mut String, ci: usize) -> usize {
    // ci is the char index right after the inserted '\n'.
    if ci == 0 {
        return 0;
    }
    let (li, _, _) = line_of(text, ci - 1);
    let lines: Vec<&str> = text.split('\n').collect();
    let Some(prev) = lines.get(li) else { return 0 };
    let infos = parse_lines(text);
    let info = &infos[li];
    let indent = prev.len() - prev.trim_start_matches(' ').len();
    let content_empty = prev.chars().count() <= info.prefix;
    let marker = match &info.kind {
        LineKind::Bullet(_) => format!("{}{} ", " ".repeat(indent), prev.trim_start().chars().next().unwrap_or('-')),
        LineKind::Number(_, n) => format!("{}{}. ", " ".repeat(indent), n + 1),
        LineKind::Check(..) => format!("{}[ ] ", " ".repeat(indent)),
        LineKind::Quote => "> ".into(),
        _ => return 0,
    };
    if content_empty {
        // Empty item: end the list instead.
        let start = char_to_byte(text, ci - 1 - prev.chars().count());
        let end = char_to_byte(text, ci - 1);
        text.replace_range(start..end, "");
        return 0;
    }
    let at = char_to_byte(text, ci);
    text.insert_str(at, &marker);
    marker.chars().count()
}

/// Turns "-", "*", "1.", "[]" typed at line start into list markers.
pub fn autoformat_line(text: &mut String, ci: usize) -> isize {
    let (_, start, _) = line_of(text, ci);
    let line_to_cursor: String = text.chars().skip(start).take(ci - start).collect();
    let t = line_to_cursor.trim_start_matches(' ');
    if t == "[] " {
        let at = char_to_byte(text, ci - 2);
        text.insert(at, ' ');
        return 1;
    }
    0
}

/// Applies autocorrect to the word before `ci` (called after a space/punctuation).
pub fn autocorrect(text: &mut String, ci: usize, map: &std::collections::BTreeMap<String, String>) -> isize {
    if ci < 2 {
        return 0;
    }
    let chars: Vec<char> = text.chars().collect();
    let end = ci - 1;
    let mut start = end;
    while start > 0 && !chars[start - 1].is_whitespace() {
        start -= 1;
    }
    let word: String = chars[start..end].iter().collect();
    if let Some(rep) = map.get(&word) {
        let (a, b) = (char_to_byte(text, start), char_to_byte(text, end));
        text.replace_range(a..b, rep);
        return rep.chars().count() as isize - (end - start) as isize;
    }
    // Capitalize first letter of a sentence.
    if start == 0 || (start >= 2 && ".!?".contains(chars[start - 2]) && chars[start - 1] == ' ') || (start >= 1 && chars[start - 1] == '\n') {
        if let Some(c) = chars.get(start) {
            if c.is_lowercase() && word.chars().count() > 1 && word.chars().all(|c| c.is_alphabetic()) {
                let up: String = c.to_uppercase().collect();
                let a = char_to_byte(text, start);
                let b = char_to_byte(text, start + 1);
                text.replace_range(a..b, &up);
            }
        }
    }
    0
}

/// Find matches in plain char positions.
pub fn find_all(text: &str, needle: &str, case: bool, whole: bool) -> Vec<(usize, usize)> {
    if needle.is_empty() {
        return Vec::new();
    }
    let hay: Vec<char> = if case { text.chars().collect() } else { text.to_lowercase().chars().collect() };
    let nd: Vec<char> = if case { needle.chars().collect() } else { needle.to_lowercase().chars().collect() };
    let mut out = Vec::new();
    if hay.len() < nd.len() {
        return out;
    }
    let mut i = 0;
    while i + nd.len() <= hay.len() {
        if hay[i..i + nd.len()] == nd[..] {
            let ok = !whole
                || ((i == 0 || !hay[i - 1].is_alphanumeric()) && (i + nd.len() == hay.len() || !hay[i + nd.len()].is_alphanumeric()));
            if ok {
                out.push((i, i + nd.len()));
                i += nd.len();
                continue;
            }
        }
        i += 1;
    }
    out
}

/// Converts a markup text to HTML (export).
pub fn to_html(text: &str) -> String {
    let lines = parse_lines(text);
    let mut out = String::new();
    let mut offset = 0;
    let mut list: Option<&str> = None;
    for (li, line) in text.split('\n').enumerate() {
        let chars: Vec<char> = line.chars().collect();
        let info = &lines[li];
        let want = match info.kind {
            LineKind::Bullet(_) | LineKind::Check(..) => Some("ul"),
            LineKind::Number(..) => Some("ol"),
            _ => None,
        };
        if list != want {
            if let Some(l) = list {
                out.push_str(&format!("</{l}>\n"));
            }
            if let Some(w) = want {
                out.push_str(&format!("<{w}>\n"));
            }
            list = want;
        }
        let p = info.prefix.min(chars.len());
        let mut inner = String::new();
        if !matches!(info.kind, LineKind::Code | LineKind::Fence | LineKind::Rule) {
            for r in parse_inline(&chars[p..], offset + p, &Inline::default()) {
                if r.marker {
                    continue;
                }
                let mut t = esc(&r.text);
                let s = &r.style;
                if s.code {
                    t = format!("<code>{t}</code>");
                }
                if s.bold {
                    t = format!("<b>{t}</b>");
                }
                if s.italic {
                    t = format!("<i>{t}</i>");
                }
                if s.underline {
                    t = format!("<u>{t}</u>");
                }
                if s.strike {
                    t = format!("<s>{t}</s>");
                }
                if s.sup {
                    t = format!("<sup>{t}</sup>");
                }
                if s.sub {
                    t = format!("<sub>{t}</sub>");
                }
                if s.mark {
                    t = format!("<mark>{t}</mark>");
                }
                let mut css = String::new();
                if let Some(c) = s.color {
                    css += &format!("color:#{:02x}{:02x}{:02x};", c.r(), c.g(), c.b());
                }
                if let Some(c) = s.highlight {
                    css += &format!("background:#{:02x}{:02x}{:02x};", c.r(), c.g(), c.b());
                }
                if let Some(f) = &s.font {
                    css += &format!("font-family:'{}';", esc(f));
                }
                if let Some(z) = s.size {
                    css += &format!("font-size:{z}px;");
                }
                if !css.is_empty() {
                    t = format!("<span style=\"{css}\">{t}</span>");
                }
                if let Some(u) = &s.link {
                    t = format!("<a href=\"{}\">{t}</a>", esc(u));
                }
                inner.push_str(&t);
            }
        }
        match &info.kind {
            LineKind::Heading(n) => out.push_str(&format!("<h{n}>{inner}</h{n}>\n")),
            LineKind::Quote => out.push_str(&format!("<blockquote>{inner}</blockquote>\n")),
            LineKind::Note => out.push_str(&format!("<p class=\"note\">{inner}</p>\n")),
            LineKind::Code => out.push_str(&format!("<pre><code>{}</code></pre>\n", esc(line))),
            LineKind::Fence => {}
            LineKind::Rule => out.push_str("<hr>\n"),
            LineKind::Bullet(_) | LineKind::Number(..) => out.push_str(&format!("<li>{inner}</li>\n")),
            LineKind::Check(_, c) => out.push_str(&format!("<li>{} {inner}</li>\n", if *c { "☑" } else { "☐" })),
            LineKind::Normal => {
                if inner.is_empty() {
                    out.push_str("<br>\n");
                } else {
                    out.push_str(&format!("<p>{inner}</p>\n"));
                }
            }
        }
        offset += chars.len() + 1;
    }
    if let Some(l) = list {
        out.push_str(&format!("</{l}>\n"));
    }
    out
}

/// Converts markup to standard Markdown (export).
pub fn to_markdown(text: &str) -> String {
    let mut out = String::new();
    for line in text.split('\n') {
        let mut l = line.to_owned();
        // Our extensions have no Markdown equivalent: keep the inner text.
        for (open, close) in [("__", "__"), ("==", "=="), (",,", ",,"), ("^", "^")] {
            let _ = close;
            l = l.replace(open, "");
        }
        let chars: Vec<char> = l.chars().collect();
        let mut s = String::new();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '{' && i + 2 < chars.len() && chars[i + 2] == ':' && "chfs".contains(chars[i + 1]) {
                if let Some(bar) = (i + 3..chars.len()).find(|&j| chars[j] == '|') {
                    i = bar + 1;
                    continue;
                }
            }
            if chars[i] == '}' {
                i += 1;
                continue;
            }
            if chars[i] == '!' && chars.get(i + 1) == Some(&'>') && i == l.len() - l.trim_start().len() {
                s.push('>');
                i += 2;
                continue;
            }
            s.push(chars[i]);
            i += 1;
        }
        let t = s.trim_start();
        let ind = &s[..s.len() - t.len()];
        if let Some(rest) = t.strip_prefix("[ ] ") {
            s = format!("{ind}- [ ] {rest}");
        } else if let Some(rest) = t.strip_prefix("[x] ") {
            s = format!("{ind}- [x] {rest}");
        }
        out.push_str(&s);
        out.push('\n');
    }
    out
}

pub fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip() {
        assert_eq!(strip_markup("**bold** and *it*"), "bold and it");
        assert_eq!(strip_markup("# Title"), "Title");
        assert_eq!(strip_markup("{c:#ff0000|red}"), "red");
        assert_eq!(strip_markup("2 * 3 = 6"), "2 * 3 = 6");
    }

    #[test]
    fn wrap_toggle() {
        let mut s = String::from("hello world");
        let r = toggle_wrap(&mut s, 0, 5, "**", "**");
        assert_eq!(s, "**hello** world");
        let _ = toggle_wrap(&mut s, r.0, r.1, "**", "**");
        assert_eq!(s, "hello world");
    }

    #[test]
    fn detects() {
        let d = detect("see https://example.com, mail a.b@c.ru tel +7 912 345-67-89 on 12.05.2026");
        assert_eq!(d.len(), 4);
    }
}
