//! Expression evaluation for table formulas and the math module:
//! arithmetic, functions, cell references, equation solving and plotting.

use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f64),
    Ident(String),
    Op(char),
    LParen,
    RParen,
    Comma,
    Colon,
}

fn tokenize(s: &str) -> Result<Vec<Tok>, String> {
    let cs: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit() || (c == '.' && cs.get(i + 1).is_some_and(|d| d.is_ascii_digit())) || (c == ',' && false) {
            let st = i;
            while i < cs.len() && (cs[i].is_ascii_digit() || cs[i] == '.') {
                i += 1;
            }
            if i < cs.len() && (cs[i] == 'e' || cs[i] == 'E') && cs.get(i + 1).is_some_and(|d| d.is_ascii_digit() || *d == '-' || *d == '+') {
                i += 2;
                while i < cs.len() && cs[i].is_ascii_digit() {
                    i += 1;
                }
            }
            let t: String = cs[st..i].iter().collect();
            out.push(Tok::Num(t.parse().map_err(|_| format!("Неверное число {t}"))?));
        } else if c.is_alphabetic() || c == '_' || c == 'π' || c == '√' {
            if c == '√' {
                out.push(Tok::Ident("sqrt".into()));
                i += 1;
                continue;
            }
            if c == 'π' {
                out.push(Tok::Ident("pi".into()));
                i += 1;
                continue;
            }
            let st = i;
            while i < cs.len() && (cs[i].is_alphanumeric() || cs[i] == '_' || cs[i] == '$') {
                i += 1;
            }
            out.push(Tok::Ident(cs[st..i].iter().collect::<String>().replace('$', "")));
        } else {
            match c {
                '+' | '-' | '*' | '/' | '^' | '%' => out.push(Tok::Op(c)),
                '×' | '·' => out.push(Tok::Op('*')),
                '÷' | ':' if c == '÷' => out.push(Tok::Op('/')),
                '²' => {
                    out.push(Tok::Op('^'));
                    out.push(Tok::Num(2.0));
                }
                '³' => {
                    out.push(Tok::Op('^'));
                    out.push(Tok::Num(3.0));
                }
                '(' => out.push(Tok::LParen),
                ')' => out.push(Tok::RParen),
                ',' | ';' => out.push(Tok::Comma),
                ':' => out.push(Tok::Colon),
                _ => return Err(format!("Неизвестный символ «{c}»")),
            }
            i += 1;
        }
    }
    Ok(out)
}

pub trait Env {
    fn var(&self, name: &str) -> Option<f64>;
    fn range(&self, _a: &str, _b: &str) -> Option<Vec<f64>> {
        None
    }
}

pub struct Vars<'a>(pub &'a HashMap<String, f64>);

impl Env for Vars<'_> {
    fn var(&self, name: &str) -> Option<f64> {
        self.0.get(name).copied()
    }
}

struct Parser<'a> {
    toks: Vec<Tok>,
    pos: usize,
    env: &'a dyn Env,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }
    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).cloned();
        self.pos += 1;
        t
    }
    fn expr(&mut self) -> Result<f64, String> {
        let mut v = self.term()?;
        while let Some(Tok::Op(c)) = self.peek().cloned() {
            if c == '+' || c == '-' {
                self.pos += 1;
                let r = self.term()?;
                v = if c == '+' { v + r } else { v - r };
            } else {
                break;
            }
        }
        Ok(v)
    }
    fn term(&mut self) -> Result<f64, String> {
        let mut v = self.unary()?;
        loop {
            match self.peek().cloned() {
                Some(Tok::Op(c)) if c == '*' || c == '/' || c == '%' => {
                    self.pos += 1;
                    let r = self.unary()?;
                    v = match c {
                        '*' => v * r,
                        '/' => v / r,
                        _ => v % r,
                    };
                }
                // Implicit multiplication: 2x, 2(x+1), (a)(b)
                Some(Tok::Ident(_)) | Some(Tok::LParen) | Some(Tok::Num(_)) => {
                    let r = self.unary()?;
                    v *= r;
                }
                _ => break,
            }
        }
        Ok(v)
    }
    fn unary(&mut self) -> Result<f64, String> {
        if let Some(Tok::Op('-')) = self.peek() {
            self.pos += 1;
            return Ok(-self.unary()?);
        }
        if let Some(Tok::Op('+')) = self.peek() {
            self.pos += 1;
            return self.unary();
        }
        self.power()
    }
    fn power(&mut self) -> Result<f64, String> {
        let b = self.atom()?;
        if let Some(Tok::Op('^')) = self.peek() {
            self.pos += 1;
            let e = self.unary()?;
            return Ok(b.powf(e));
        }
        Ok(b)
    }
    fn args(&mut self) -> Result<Vec<f64>, String> {
        let mut out = Vec::new();
        if let Some(Tok::RParen) = self.peek() {
            self.pos += 1;
            return Ok(out);
        }
        loop {
            // Range A1:B3
            if let (Some(Tok::Ident(a)), Some(Tok::Colon), Some(Tok::Ident(b))) =
                (self.toks.get(self.pos).cloned(), self.toks.get(self.pos + 1).cloned(), self.toks.get(self.pos + 2).cloned())
            {
                let vals = self.env.range(&a, &b).ok_or_else(|| format!("Неверный диапазон {a}:{b}"))?;
                out.extend(vals);
                self.pos += 3;
            } else {
                out.push(self.expr()?);
            }
            match self.next() {
                Some(Tok::Comma) => continue,
                Some(Tok::RParen) => break,
                _ => return Err("Ожидалась «)»".into()),
            }
        }
        Ok(out)
    }
    fn atom(&mut self) -> Result<f64, String> {
        match self.next() {
            Some(Tok::Num(n)) => {
                if let Some(Tok::Op('%')) = self.peek() {
                    // 50% → 0.5 only when nothing follows that could be an operand.
                    if !matches!(self.toks.get(self.pos + 1), Some(Tok::Num(_)) | Some(Tok::Ident(_)) | Some(Tok::LParen)) {
                        self.pos += 1;
                        return Ok(n / 100.0);
                    }
                }
                Ok(n)
            }
            Some(Tok::LParen) => {
                let v = self.expr()?;
                match self.next() {
                    Some(Tok::RParen) => Ok(v),
                    _ => Err("Ожидалась «)»".into()),
                }
            }
            Some(Tok::Ident(name)) => {
                let lname = name.to_lowercase();
                if let Some(Tok::LParen) = self.peek() {
                    self.pos += 1;
                    let a = self.args()?;
                    return call(&lname, &a);
                }
                match lname.as_str() {
                    "pi" => return Ok(std::f64::consts::PI),
                    "e" if self.env.var("e").is_none() => return Ok(std::f64::consts::E),
                    _ => {}
                }
                // Functions without parentheses: sqrt 4, sin x
                if matches!(lname.as_str(), "sqrt" | "sin" | "cos" | "tan" | "ln" | "log" | "abs" | "exp") {
                    let a = self.power()?;
                    return call(&lname, &[a]);
                }
                self.env
                    .var(&name)
                    .or_else(|| self.env.var(&name.to_uppercase()))
                    .ok_or_else(|| format!("Неизвестное имя «{name}»"))
            }
            Some(t) => Err(format!("Неожиданный элемент {t:?}")),
            None => Err("Выражение оборвано".into()),
        }
    }
}

fn call(name: &str, a: &[f64]) -> Result<f64, String> {
    let one = || a.first().copied().ok_or_else(|| format!("{name}: нет аргумента"));
    Ok(match name {
        "sum" | "сумм" => a.iter().sum(),
        "avg" | "average" | "срзнач" => {
            if a.is_empty() {
                0.0
            } else {
                a.iter().sum::<f64>() / a.len() as f64
            }
        }
        "min" | "мин" => a.iter().copied().fold(f64::INFINITY, f64::min),
        "max" | "макс" => a.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        "count" | "счёт" | "счет" => a.len() as f64,
        "sqrt" => one()?.sqrt(),
        "abs" => one()?.abs(),
        "sin" => one()?.sin(),
        "cos" => one()?.cos(),
        "tan" | "tg" => one()?.tan(),
        "asin" => one()?.asin(),
        "acos" => one()?.acos(),
        "atan" => one()?.atan(),
        "ln" => one()?.ln(),
        "log" | "lg" => one()?.log10(),
        "exp" => one()?.exp(),
        "round" => {
            let d = a.get(1).copied().unwrap_or(0.0);
            let k = 10f64.powf(d);
            (one()? * k).round() / k
        }
        "floor" => one()?.floor(),
        "ceil" => one()?.ceil(),
        "pow" => one()?.powf(a.get(1).copied().unwrap_or(1.0)),
        _ => return Err(format!("Неизвестная функция {name}")),
    })
}

pub fn eval(expr: &str, env: &dyn Env) -> Result<f64, String> {
    let toks = tokenize(expr)?;
    if toks.is_empty() {
        return Err("Пустое выражение".into());
    }
    let mut p = Parser { toks, pos: 0, env };
    let v = p.expr()?;
    if p.pos < p.toks.len() {
        return Err("Лишние символы в выражении".into());
    }
    Ok(v)
}

pub fn fmt_num(v: f64) -> String {
    if !v.is_finite() {
        return "#ОШИБКА".into();
    }
    if (v - v.round()).abs() < 1e-9 && v.abs() < 1e15 {
        return format!("{}", v.round() as i64);
    }
    let s = format!("{v:.6}");
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

// ----------------------------------------------------------------------------
// Tables

/// "B12" → (row 11, col 1)
pub fn parse_cell(s: &str) -> Option<(usize, usize)> {
    let s = s.to_uppercase();
    let letters: String = s.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
    let digits = &s[letters.len()..];
    if letters.is_empty() || digits.is_empty() {
        return None;
    }
    let mut col = 0usize;
    for c in letters.chars() {
        col = col * 26 + (c as usize - 'A' as usize + 1);
    }
    let row: usize = digits.parse().ok()?;
    Some((row.checked_sub(1)?, col - 1))
}

pub fn col_name(mut c: usize) -> String {
    let mut s = String::new();
    c += 1;
    while c > 0 {
        let r = (c - 1) % 26;
        s.insert(0, (b'A' + r as u8) as char);
        c = (c - 1) / 26;
    }
    s
}

struct TableEnv<'a> {
    cells: &'a [Vec<String>],
    /// Cells being evaluated (cycle detection) and finished results.
    visiting: std::cell::RefCell<Vec<(usize, usize)>>,
    memo: std::cell::RefCell<HashMap<(usize, usize), Option<f64>>>,
}

impl<'a> TableEnv<'a> {
    fn new(cells: &'a [Vec<String>]) -> Self {
        Self { cells, visiting: Default::default(), memo: Default::default() }
    }
    fn value(&self, r: usize, c: usize) -> Option<f64> {
        let s = self.cells.get(r)?.get(c)?.trim();
        if let Some(f) = s.strip_prefix('=') {
            if let Some(v) = self.memo.borrow().get(&(r, c)) {
                return *v;
            }
            if self.visiting.borrow().contains(&(r, c)) {
                return None;
            }
            self.visiting.borrow_mut().push((r, c));
            let v = eval(f, self).ok();
            self.visiting.borrow_mut().pop();
            self.memo.borrow_mut().insert((r, c), v);
            return v;
        }
        parse_number(s)
    }
}

pub fn parse_number(s: &str) -> Option<f64> {
    let t: String = s.chars().filter(|c| !c.is_whitespace() && *c != '\u{a0}').collect();
    t.replace(',', ".").trim_end_matches('%').parse().ok()
}

impl Env for TableEnv<'_> {
    fn var(&self, name: &str) -> Option<f64> {
        let (r, c) = parse_cell(name)?;
        Some(self.value(r, c).unwrap_or(0.0))
    }
    fn range(&self, a: &str, b: &str) -> Option<Vec<f64>> {
        let (r1, c1) = parse_cell(a)?;
        let (r2, c2) = parse_cell(b)?;
        let mut out = Vec::new();
        for r in r1.min(r2)..=r1.max(r2) {
            for c in c1.min(c2)..=c1.max(c2) {
                if let Some(v) = self.value(r, c) {
                    out.push(v);
                }
            }
        }
        Some(out)
    }
}

/// Display value of a table cell (formulas evaluated).
pub fn cell_display(cells: &[Vec<String>], r: usize, c: usize) -> String {
    let s = &cells[r][c];
    if s.trim().starts_with('=') {
        match TableEnv::new(cells).value(r, c) {
            Some(v) => fmt_num(v),
            None => "#ОШИБКА".into(),
        }
    } else {
        s.clone()
    }
}

// ----------------------------------------------------------------------------
// Equations

pub struct Solution {
    pub steps: Vec<String>,
    pub roots: Vec<f64>,
}

struct XEnv(f64);
impl Env for XEnv {
    fn var(&self, name: &str) -> Option<f64> {
        (name == "x" || name == "X").then_some(self.0)
    }
}

pub fn f_of_x(expr: &str, x: f64) -> Result<f64, String> {
    eval(expr, &XEnv(x))
}

/// Solves `lhs = rhs` in x. Exact steps for linear and quadratic equations,
/// numeric bisection otherwise.
pub fn solve(equation: &str) -> Result<Solution, String> {
    let (l, r) = equation.split_once('=').ok_or("Нужен знак «=»")?;
    let g = |x: f64| -> Result<f64, String> { Ok(f_of_x(l, x)? - f_of_x(r, x)?) };
    let mut steps = vec![format!("Переносим всё в левую часть: {} − ({}) = 0", l.trim(), r.trim())];
    // Fit a quadratic through 3 points and check it on others.
    let (f0, f1, fm) = (g(0.0)?, g(1.0)?, g(-1.0)?);
    let a = (f1 + fm) / 2.0 - f0;
    let b = (f1 - fm) / 2.0;
    let c = f0;
    let is_poly = [2.0, -2.0, 3.5, -4.25, 10.0].iter().all(|&x| {
        g(x).map(|v| (v - (a * x * x + b * x + c)).abs() < 1e-6 * (1.0 + v.abs())).unwrap_or(false)
    });
    if is_poly {
        if a.abs() < 1e-12 {
            if b.abs() < 1e-12 {
                steps.push(if c.abs() < 1e-12 { "Тождество: x — любое число".into() } else { "Противоречие: решений нет".into() });
                return Ok(Solution { steps, roots: vec![] });
            }
            steps.push(format!("Линейное уравнение: {}·x + {} = 0", fmt_num(b), fmt_num(c)));
            steps.push(format!("x = −({}) / {}", fmt_num(c), fmt_num(b)));
            let x = -c / b;
            steps.push(format!("x = {}", fmt_num(x)));
            return Ok(Solution { steps, roots: vec![x] });
        }
        steps.push(format!("Квадратное уравнение: {}·x² + {}·x + {} = 0", fmt_num(a), fmt_num(b), fmt_num(c)));
        let d = b * b - 4.0 * a * c;
        steps.push(format!("Дискриминант D = b² − 4ac = {}", fmt_num(d)));
        if d < -1e-12 {
            steps.push("D < 0: действительных корней нет".into());
            return Ok(Solution { steps, roots: vec![] });
        }
        if d.abs() < 1e-12 {
            let x = -b / (2.0 * a);
            steps.push(format!("D = 0: x = −b / 2a = {}", fmt_num(x)));
            return Ok(Solution { steps, roots: vec![x] });
        }
        let sd = d.sqrt();
        let x1 = (-b - sd) / (2.0 * a);
        let x2 = (-b + sd) / (2.0 * a);
        steps.push(format!("x₁ = (−b − √D) / 2a = {}", fmt_num(x1)));
        steps.push(format!("x₂ = (−b + √D) / 2a = {}", fmt_num(x2)));
        return Ok(Solution { steps, roots: vec![x1, x2] });
    }
    steps.push("Численное решение: поиск смены знака на [−1000; 1000] и деление отрезка пополам".into());
    let mut roots: Vec<f64> = Vec::new();
    let n = 20000;
    let (lo, hi) = (-1000.0f64, 1000.0f64);
    let mut px = lo;
    let mut pv = g(px).unwrap_or(f64::NAN);
    for i in 1..=n {
        let x = lo + (hi - lo) * i as f64 / n as f64;
        let v = g(x).unwrap_or(f64::NAN);
        if pv.is_finite() && v.is_finite() && (pv == 0.0 || pv.signum() != v.signum()) {
            let (mut a, mut b) = (px, x);
            for _ in 0..80 {
                let m = (a + b) / 2.0;
                let gm = g(m).unwrap_or(f64::NAN);
                if gm.signum() == g(a).unwrap_or(f64::NAN).signum() {
                    a = m;
                } else {
                    b = m;
                }
            }
            let root = (a + b) / 2.0;
            if g(root).is_ok_and(|v| v.abs() < 1e-6) && !roots.iter().any(|r| (r - root).abs() < 1e-6) {
                roots.push(root);
            }
        }
        px = x;
        pv = v;
        if roots.len() > 20 {
            break;
        }
    }
    for r in &roots {
        steps.push(format!("x ≈ {}", fmt_num(*r)));
    }
    if roots.is_empty() {
        steps.push("Корни на отрезке не найдены".into());
    }
    Ok(Solution { steps, roots })
}

/// Turns a typed formula into nicer math notation.
pub fn prettify(src: &str) -> String {
    let mut s = src.to_owned();
    for (a, b) in [
        ("\\alpha", "α"), ("\\beta", "β"), ("\\gamma", "γ"), ("\\delta", "δ"), ("\\Delta", "Δ"), ("\\pi", "π"),
        ("\\theta", "θ"), ("\\lambda", "λ"), ("\\mu", "μ"), ("\\sigma", "σ"), ("\\Sigma", "Σ"), ("\\omega", "ω"),
        ("\\phi", "φ"), ("\\epsilon", "ε"), ("\\infty", "∞"), ("\\sum", "∑"), ("\\int", "∫"), ("\\prod", "∏"),
        ("\\pm", "±"), ("\\times", "×"), ("\\cdot", "·"), ("\\div", "÷"), ("\\leq", "≤"), ("\\geq", "≥"),
        ("\\neq", "≠"), ("\\approx", "≈"), ("\\to", "→"), ("\\in", "∈"), ("\\forall", "∀"), ("\\exists", "∃"),
        ("\\partial", "∂"), ("\\nabla", "∇"), ("sqrt", "√"), ("<=", "≤"), (">=", "≥"), ("!=", "≠"), ("*", "·"),
    ] {
        s = s.replace(a, b);
    }
    // ^n → superscript, _n → subscript
    let sup = |c: char| match c {
        '0' => '⁰', '1' => '¹', '2' => '²', '3' => '³', '4' => '⁴', '5' => '⁵', '6' => '⁶', '7' => '⁷', '8' => '⁸',
        '9' => '⁹', '+' => '⁺', '-' => '⁻', 'n' => 'ⁿ', 'x' => 'ˣ', 'i' => 'ⁱ', _ => c,
    };
    let sub = |c: char| match c {
        '0' => '₀', '1' => '₁', '2' => '₂', '3' => '₃', '4' => '₄', '5' => '₅', '6' => '₆', '7' => '₇', '8' => '₈',
        '9' => '₉', '+' => '₊', '-' => '₋', 'i' => 'ᵢ', 'n' => 'ₙ', 'x' => 'ₓ', _ => c,
    };
    let cs: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < cs.len() {
        if (cs[i] == '^' || cs[i] == '_') && i + 1 < cs.len() {
            let f: &dyn Fn(char) -> char = if cs[i] == '^' { &sup } else { &sub };
            if cs[i + 1] == '{' {
                let mut j = i + 2;
                while j < cs.len() && cs[j] != '}' {
                    out.push(f(cs[j]));
                    j += 1;
                }
                i = j + 1;
            } else {
                out.push(f(cs[i + 1]));
                i += 2;
            }
            continue;
        }
        out.push(cs[i]);
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn basics() {
        let v = HashMap::new();
        assert_eq!(eval("2+3*4", &Vars(&v)).unwrap(), 14.0);
        assert_eq!(eval("2^3^2", &Vars(&v)).unwrap(), 512.0);
        assert_eq!(eval("sum(1,2,3)", &Vars(&v)).unwrap(), 6.0);
        let cells = vec![vec!["1".into(), "2".into()], vec!["=A1+B1".into(), "=SUM(A1:B2)".into()]];
        assert_eq!(cell_display(&cells, 1, 0), "3");
        assert_eq!(cell_display(&cells, 1, 1), "6");
        let s = solve("x^2 - 5x + 6 = 0").unwrap();
        assert_eq!(s.roots.len(), 2);
        let s = solve("2x + 4 = 10").unwrap();
        assert!((s.roots[0] - 3.0).abs() < 1e-9);
    }
}
