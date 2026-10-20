//! Export (HTML, Markdown, TXT, DOCX, PDF, CSV, XLSX) and import
//! (HTML, Markdown, TXT, ENEX, DOCX, CSV, XLSX).

use std::io::{Read, Write};
use std::path::Path;

use crate::model::*;
use crate::richtext::{self, esc};
use crate::storage::Notebook;

pub fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len() * 4 / 3 + 4);
    for c in data.chunks(3) {
        let b = [c[0], *c.get(1).unwrap_or(&0), *c.get(2).unwrap_or(&0)];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

fn sorted_items(page: &Page) -> Vec<&Item> {
    let mut v: Vec<&Item> = page.items.iter().collect();
    v.sort_by(|a, b| {
        a.pos[1].partial_cmp(&b.pos[1]).unwrap_or(std::cmp::Ordering::Equal).then(a.pos[0].partial_cmp(&b.pos[0]).unwrap_or(std::cmp::Ordering::Equal))
    });
    v
}

fn mime(name: &str) -> &'static str {
    let l = name.to_lowercase();
    if l.ends_with(".png") {
        "image/png"
    } else if l.ends_with(".gif") {
        "image/gif"
    } else if l.ends_with(".webp") {
        "image/webp"
    } else if l.ends_with(".bmp") {
        "image/bmp"
    } else {
        "image/jpeg"
    }
}

fn table_html(t: &Table) -> String {
    let mut s = String::from("<table border=\"1\" cellspacing=\"0\" cellpadding=\"4\" style=\"border-collapse:collapse\">\n");
    for r in 0..t.rows() {
        s.push_str("<tr>");
        for c in 0..t.cols() {
            if t.merges.iter().any(|m| r >= m.r && r < m.r + m.rs && c >= m.c && c < m.c + m.cs && !(r == m.r && c == m.c)) {
                continue;
            }
            let span = t.merges.iter().find(|m| m.r == r && m.c == c).map(|m| format!(" rowspan=\"{}\" colspan=\"{}\"", m.rs, m.cs)).unwrap_or_default();
            let tag = if t.header && r == 0 { "th" } else { "td" };
            let mut style = String::new();
            if let Some(f) = t.fills.get(&format!("{r},{c}")) {
                style = format!(" style=\"background:#{:02x}{:02x}{:02x}\"", f[0], f[1], f[2]);
            } else if t.banded && r % 2 == 1 {
                style = " style=\"background:#f2f2f2\"".into();
            }
            s.push_str(&format!("<{tag}{span}{style}>{}</{tag}>", esc(&crate::calc::cell_display(&t.cells, r, c))));
        }
        s.push_str("</tr>\n");
    }
    s.push_str("</table>\n");
    s
}

pub fn ink_svg(strokes: &[Stroke]) -> String {
    crate::raster::strokes_svg(strokes, 1.0)
}

pub fn page_html_body(nb: &Notebook, page: &Page, title: &str) -> String {
    let mut out = format!("<h1>{}</h1>\n", esc(title));
    if page.sensitivity != Sensitivity::None {
        out.push_str(&format!("<p class=\"label\">{}</p>\n", page.sensitivity.label()));
    }
    for it in sorted_items(page) {
        match &it.kind {
            ItemKind::Text(t) => out.push_str(&format!("<div class=\"block\">{}</div>\n", richtext::to_html(&t.text))),
            ItemKind::Image(i) => {
                if let Ok(data) = std::fs::read(nb.asset_path(&i.asset)) {
                    let mut tr = String::new();
                    if i.rotation > 0 {
                        tr += &format!("rotate({}deg) ", i.rotation as u32 * 90);
                    }
                    if i.flip_h {
                        tr += "scaleX(-1) ";
                    }
                    if i.flip_v {
                        tr += "scaleY(-1) ";
                    }
                    out.push_str(&format!(
                        "<figure><img src=\"data:{};base64,{}\" alt=\"{}\" style=\"max-width:{:.0}px;transform:{tr}\"><figcaption>{}</figcaption></figure>\n",
                        mime(&i.asset), base64(&data), esc(&i.alt), it.width, esc(&i.caption)
                    ));
                }
            }
            ItemKind::File(f) => out.push_str(&format!("<p>📎 {} ({})</p>\n", esc(&f.name), crate::storage::human_size(f.size))),
            ItemKind::Table(t) => out.push_str(&table_html(t)),
            ItemKind::Equation(e) => out.push_str(&format!("<p class=\"math\">{}</p>\n", esc(&crate::calc::prettify(e)))),
            ItemKind::Code(c) => out.push_str(&format!("<pre><code class=\"language-{}\">{}</code></pre>\n", esc(&c.lang), esc(&c.code))),
            ItemKind::Diagram(d) => out.push_str(&format!("<pre class=\"diagram\">{}</pre>\n", esc(&d.source))),
            ItemKind::Divider { page_break } => {
                out.push_str(if *page_break { "<hr style=\"page-break-after:always\">\n" } else { "<hr>\n" })
            }
            ItemKind::Chart(c) => {
                out.push_str(&format!("<p><b>{}</b></p>\n", esc(&c.title)));
                for (l, v) in c.rows() {
                    out.push_str(&format!("<p>{}: {}</p>\n", esc(&l), crate::calc::fmt_num(v)));
                }
            }
        }
    }
    if !page.strokes.is_empty() {
        out.push_str(&format!("<div class=\"ink\">{}</div>\n", ink_svg(&page.strokes)));
    }
    out
}

pub fn html_document(title: &str, body: &str) -> String {
    format!(
        "<!DOCTYPE html>\n<html lang=\"ru\"><head><meta charset=\"utf-8\"><title>{}</title><style>\
body{{font-family:'Segoe UI',Arial,sans-serif;max-width:900px;margin:2em auto;line-height:1.5;color:#222}}\
.note{{background:#fff8d2;padding:.4em .8em;border-left:4px solid #e6c200}}\
blockquote{{border-left:4px solid #ccc;margin:0;padding-left:1em;color:#555}}\
pre{{background:#f4f4f4;padding:.6em;overflow:auto}}.label{{color:#b00;font-weight:bold}}\
th{{background:#e8e8e8}}figure{{margin:1em 0}}figcaption{{color:#666;font-size:.9em}}\
</style></head><body>\n{}\n</body></html>",
        esc(title),
        body
    )
}

pub fn page_markdown(nb: &Notebook, page: &Page, title: &str, assets_dir: Option<&Path>) -> String {
    let mut out = format!("# {title}\n\n");
    if page.sensitivity != Sensitivity::None {
        out.push_str(&format!("> **{}**\n\n", page.sensitivity.label()));
    }
    for it in sorted_items(page) {
        match &it.kind {
            ItemKind::Text(t) => out.push_str(&richtext::to_markdown(&t.text)),
            ItemKind::Image(i) => {
                if let Some(dir) = assets_dir {
                    let _ = std::fs::create_dir_all(dir);
                    let _ = std::fs::copy(nb.asset_path(&i.asset), dir.join(&i.asset));
                    let rel = format!("{}/{}", dir.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(), i.asset);
                    out.push_str(&format!("![{}]({})\n", i.alt, rel.replace(' ', "%20")));
                    if !i.caption.is_empty() {
                        out.push_str(&format!("*{}*\n", i.caption));
                    }
                }
            }
            ItemKind::File(f) => out.push_str(&format!("📎 {}\n", f.name)),
            ItemKind::Table(t) => {
                for r in 0..t.rows() {
                    let row: Vec<String> = (0..t.cols()).map(|c| crate::calc::cell_display(&t.cells, r, c).replace('|', "\\|")).collect();
                    out.push_str(&format!("| {} |\n", row.join(" | ")));
                    if r == 0 {
                        out.push_str(&format!("|{}\n", " --- |".repeat(t.cols())));
                    }
                }
            }
            ItemKind::Equation(e) => out.push_str(&format!("$${}$$\n", e)),
            ItemKind::Code(c) => out.push_str(&format!("```{}\n{}\n```\n", c.lang, c.code)),
            ItemKind::Diagram(d) => out.push_str(&format!("```\n{}\n```\n", d.source)),
            ItemKind::Divider { .. } => out.push_str("---\n"),
            ItemKind::Chart(c) => {
                out.push_str(&format!("**{}**\n", c.title));
                for (l, v) in c.rows() {
                    out.push_str(&format!("- {l}: {}\n", crate::calc::fmt_num(v)));
                }
            }
        }
        out.push('\n');
    }
    out
}

pub fn page_text(page: &Page, title: &str) -> String {
    let mut s = format!("{title}\n{}\n\n", "=".repeat(title.chars().count().max(3)));
    if page.sensitivity != Sensitivity::None {
        s.push_str(&format!("[{}]\n\n", page.sensitivity.label()));
    }
    for it in sorted_items(page) {
        let t = match &it.kind {
            ItemKind::Table(t) => (0..t.rows())
                .map(|r| (0..t.cols()).map(|c| crate::calc::cell_display(&t.cells, r, c)).collect::<Vec<_>>().join("\t"))
                .collect::<Vec<_>>()
                .join("\n"),
            _ => it.plain_text(),
        };
        if !t.trim().is_empty() {
            s.push_str(&t);
            s.push_str("\n\n");
        }
    }
    s
}

// ----------------------------------------------------------------------------
// DOCX (WordprocessingML written by hand)

fn docx_runs(line: &[char], offset: usize) -> String {
    let mut out = String::new();
    for r in richtext::parse_inline(line, offset, &richtext::Inline::default()) {
        if r.marker {
            continue;
        }
        let s = &r.style;
        let mut pr = String::new();
        if s.bold {
            pr += "<w:b/>";
        }
        if s.italic {
            pr += "<w:i/>";
        }
        if s.underline || s.link.is_some() {
            pr += "<w:u w:val=\"single\"/>";
        }
        if s.strike {
            pr += "<w:strike/>";
        }
        if s.sup {
            pr += "<w:vertAlign w:val=\"superscript\"/>";
        }
        if s.sub {
            pr += "<w:vertAlign w:val=\"subscript\"/>";
        }
        if s.mark {
            pr += "<w:highlight w:val=\"yellow\"/>";
        }
        if s.code {
            pr += "<w:rFonts w:ascii=\"Consolas\" w:hAnsi=\"Consolas\"/>";
        }
        if let Some(c) = s.color {
            pr += &format!("<w:color w:val=\"{:02X}{:02X}{:02X}\"/>", c.r(), c.g(), c.b());
        }
        if let Some(f) = &s.font {
            pr += &format!("<w:rFonts w:ascii=\"{0}\" w:hAnsi=\"{0}\" w:cs=\"{0}\"/>", esc(f));
        }
        if let Some(z) = s.size {
            pr += &format!("<w:sz w:val=\"{}\"/>", (z * 1.5) as u32);
        }
        out += &format!("<w:r><w:rPr>{pr}</w:rPr><w:t xml:space=\"preserve\">{}</w:t></w:r>", esc(&r.text));
    }
    out
}

fn docx_text(text: &str) -> String {
    let infos = richtext::parse_lines(text);
    let mut out = String::new();
    let mut offset = 0;
    for (li, line) in text.split('\n').enumerate() {
        let chars: Vec<char> = line.chars().collect();
        let info = &infos[li];
        let p = info.prefix.min(chars.len());
        let (style, prefix) = match &info.kind {
            richtext::LineKind::Heading(n) => (format!("Heading{}", n.min(&3)), String::new()),
            richtext::LineKind::Quote => ("Quote".into(), String::new()),
            richtext::LineKind::Bullet(l) => (String::new(), format!("{}• ", "    ".repeat(*l as usize))),
            richtext::LineKind::Number(l, n) => (String::new(), format!("{}{n}. ", "    ".repeat(*l as usize))),
            richtext::LineKind::Check(l, c) => (String::new(), format!("{}{} ", "    ".repeat(*l as usize), if *c { "☑" } else { "☐" })),
            _ => (String::new(), String::new()),
        };
        if matches!(info.kind, richtext::LineKind::Fence) {
            offset += chars.len() + 1;
            continue;
        }
        let ppr = if style.is_empty() { String::new() } else { format!("<w:pPr><w:pStyle w:val=\"{style}\"/></w:pPr>") };
        let body = if matches!(info.kind, richtext::LineKind::Code) {
            format!("<w:r><w:rPr><w:rFonts w:ascii=\"Consolas\" w:hAnsi=\"Consolas\"/></w:rPr><w:t xml:space=\"preserve\">{}</w:t></w:r>", esc(line))
        } else if matches!(info.kind, richtext::LineKind::Rule) {
            "<w:r><w:t>────────────────────</w:t></w:r>".into()
        } else {
            let pre = if prefix.is_empty() { String::new() } else { format!("<w:r><w:t xml:space=\"preserve\">{}</w:t></w:r>", esc(&prefix)) };
            pre + &docx_runs(&chars[p..], offset + p)
        };
        out += &format!("<w:p>{ppr}{body}</w:p>");
        offset += chars.len() + 1;
    }
    out
}

pub fn docx(nb: &Notebook, pages: &[(String, Page)]) -> Vec<u8> {
    let mut body = String::new();
    let mut media: Vec<(String, Vec<u8>, u32, u32)> = Vec::new();
    for (pi, (title, page)) in pages.iter().enumerate() {
        if pi > 0 {
            body += "<w:p><w:r><w:br w:type=\"page\"/></w:r></w:p>";
        }
        body += &format!("<w:p><w:pPr><w:pStyle w:val=\"Title\"/></w:pPr><w:r><w:t xml:space=\"preserve\">{}</w:t></w:r></w:p>", esc(title));
        if page.sensitivity != Sensitivity::None {
            body += &format!("<w:p><w:r><w:rPr><w:b/><w:color w:val=\"B00000\"/></w:rPr><w:t>{}</w:t></w:r></w:p>", page.sensitivity.label());
        }
        for it in sorted_items(page) {
            match &it.kind {
                ItemKind::Text(t) => body += &docx_text(&t.text),
                ItemKind::Table(t) => {
                    body += "<w:tbl><w:tblPr><w:tblStyle w:val=\"TableGrid\"/><w:tblW w:w=\"0\" w:type=\"auto\"/><w:tblBorders><w:top w:val=\"single\" w:sz=\"4\"/><w:left w:val=\"single\" w:sz=\"4\"/><w:bottom w:val=\"single\" w:sz=\"4\"/><w:right w:val=\"single\" w:sz=\"4\"/><w:insideH w:val=\"single\" w:sz=\"4\"/><w:insideV w:val=\"single\" w:sz=\"4\"/></w:tblBorders></w:tblPr>";
                    for r in 0..t.rows() {
                        body += "<w:tr>";
                        for c in 0..t.cols() {
                            let b = if t.header && r == 0 { "<w:rPr><w:b/></w:rPr>" } else { "" };
                            body += &format!(
                                "<w:tc><w:tcPr><w:tcW w:w=\"{}\" w:type=\"dxa\"/></w:tcPr><w:p><w:r>{b}<w:t xml:space=\"preserve\">{}</w:t></w:r></w:p></w:tc>",
                                (t.col_widths.get(c).copied().unwrap_or(100.0) * 15.0) as u32,
                                esc(&crate::calc::cell_display(&t.cells, r, c))
                            );
                        }
                        body += "</w:tr>";
                    }
                    body += "</w:tbl><w:p/>";
                }
                ItemKind::Image(i) => {
                    if let Ok(data) = std::fs::read(nb.asset_path(&i.asset)) {
                        if let Ok(img) = image::load_from_memory(&data) {
                            let mut png = Vec::new();
                            if img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).is_ok() {
                                let n = media.len() + 1;
                                let w = it.width.min(620.0);
                                let h = w * img.height() as f32 / img.width().max(1) as f32;
                                let (cx, cy) = ((w * 9525.0) as u32, (h * 9525.0) as u32);
                                media.push((format!("image{n}.png"), png, cx, cy));
                                body += &format!(
                                    "<w:p><w:r><w:drawing><wp:inline><wp:extent cx=\"{cx}\" cy=\"{cy}\"/><wp:docPr id=\"{n}\" name=\"Picture {n}\" descr=\"{}\"/>\
<a:graphic xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/picture\">\
<pic:pic xmlns:pic=\"http://schemas.openxmlformats.org/drawingml/2006/picture\"><pic:nvPicPr><pic:cNvPr id=\"{n}\" name=\"image{n}.png\"/><pic:cNvPicPr/></pic:nvPicPr>\
<pic:blipFill><a:blip r:embed=\"rIdImg{n}\"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill>\
<pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr></pic:pic>\
</a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>",
                                    esc(&i.alt)
                                );
                                if !i.caption.is_empty() {
                                    body += &format!("<w:p><w:r><w:rPr><w:i/></w:rPr><w:t xml:space=\"preserve\">{}</w:t></w:r></w:p>", esc(&i.caption));
                                }
                            }
                        }
                    }
                }
                ItemKind::Code(c) => {
                    for l in c.code.lines() {
                        body += &format!("<w:p><w:r><w:rPr><w:rFonts w:ascii=\"Consolas\" w:hAnsi=\"Consolas\"/></w:rPr><w:t xml:space=\"preserve\">{}</w:t></w:r></w:p>", esc(l));
                    }
                }
                ItemKind::Divider { page_break: true } => body += "<w:p><w:r><w:br w:type=\"page\"/></w:r></w:p>",
                _ => {
                    let t = it.plain_text();
                    if !t.is_empty() {
                        body += &format!("<w:p><w:r><w:t xml:space=\"preserve\">{}</w:t></w:r></w:p>", esc(&t));
                    }
                }
            }
        }
    }
    let document = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\">\
<w:body>{body}<w:sectPr><w:pgSz w:w=\"11906\" w:h=\"16838\"/><w:pgMar w:top=\"1134\" w:right=\"850\" w:bottom=\"1134\" w:left=\"1701\"/></w:sectPr></w:body></w:document>"
    );
    let styles = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<w:styles xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">\
<w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii=\"Calibri\" w:hAnsi=\"Calibri\" w:cs=\"Calibri\"/><w:sz w:val=\"22\"/></w:rPr></w:rPrDefault></w:docDefaults>\
<w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Normal\"><w:name w:val=\"Normal\"/></w:style>\
<w:style w:type=\"paragraph\" w:styleId=\"Title\"><w:name w:val=\"Title\"/><w:pPr><w:spacing w:after=\"200\"/></w:pPr><w:rPr><w:sz w:val=\"48\"/></w:rPr></w:style>\
<w:style w:type=\"paragraph\" w:styleId=\"Heading1\"><w:name w:val=\"heading 1\"/><w:pPr><w:outlineLvl w:val=\"0\"/></w:pPr><w:rPr><w:b/><w:sz w:val=\"36\"/></w:rPr></w:style>\
<w:style w:type=\"paragraph\" w:styleId=\"Heading2\"><w:name w:val=\"heading 2\"/><w:pPr><w:outlineLvl w:val=\"1\"/></w:pPr><w:rPr><w:b/><w:sz w:val=\"30\"/></w:rPr></w:style>\
<w:style w:type=\"paragraph\" w:styleId=\"Heading3\"><w:name w:val=\"heading 3\"/><w:pPr><w:outlineLvl w:val=\"2\"/></w:pPr><w:rPr><w:b/><w:sz w:val=\"26\"/></w:rPr></w:style>\
<w:style w:type=\"paragraph\" w:styleId=\"Quote\"><w:name w:val=\"Quote\"/><w:pPr><w:ind w:left=\"720\"/></w:pPr><w:rPr><w:i/><w:color w:val=\"555555\"/></w:rPr></w:style>\
</w:styles>";
    let mut rels = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rIdStyles\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\"/>");
    for (i, m) in media.iter().enumerate() {
        rels += &format!("<Relationship Id=\"rIdImg{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" Target=\"media/{}\"/>", i + 1, m.0);
    }
    rels += "</Relationships>";
    let ct = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/>\
<Default Extension=\"png\" ContentType=\"image/png\"/>\
<Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/>\
<Override PartName=\"/word/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml\"/></Types>";
    let root_rels = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/></Relationships>";
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut z = zip::ZipWriter::new(&mut buf);
        let o: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let mut put = |name: &str, data: &[u8]| {
            let _ = z.start_file(name, o);
            let _ = z.write_all(data);
        };
        put("[Content_Types].xml", ct.as_bytes());
        put("_rels/.rels", root_rels.as_bytes());
        put("word/document.xml", document.as_bytes());
        put("word/styles.xml", styles.as_bytes());
        put("word/_rels/document.xml.rels", rels.as_bytes());
        for m in &media {
            put(&format!("word/media/{}", m.0), &m.1);
        }
        let _ = z.finish();
    }
    buf.into_inner()
}

// ----------------------------------------------------------------------------
// PDF from page images

pub struct PdfOptions {
    pub landscape: bool,
    pub margin_mm: f32,
    pub scale: f32,
    pub page_numbers: bool,
    pub header: String,
}

/// Writes a PDF where each element of `pages` is one sheet image, placed
/// within the margins. Page numbers use the built-in Helvetica font.
pub fn pdf_from_images(pages: &[image::RgbImage], opt: &PdfOptions) -> Vec<u8> {
    let (pw, ph) = if opt.landscape { (842.0f32, 595.0f32) } else { (595.0f32, 842.0f32) };
    let m = opt.margin_mm * 72.0 / 25.4;
    let mut objs: Vec<Vec<u8>> = Vec::new();
    // 1: catalog, 2: pages, 3: font; then per page: page, content, image.
    let n = pages.len();
    let mut kids = String::new();
    for i in 0..n {
        kids += &format!("{} 0 R ", 4 + i * 3);
    }
    objs.push(b"<< /Type /Catalog /Pages 2 0 R >>".to_vec());
    objs.push(format!("<< /Type /Pages /Kids [{kids}] /Count {n} >>").into_bytes());
    objs.push(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec());
    let ascii_header: String = opt.header.chars().filter(|c| c.is_ascii() && *c != '(' && *c != ')' && *c != '\\').collect();
    for (i, img) in pages.iter().enumerate() {
        let (_pid, cid, iid) = (4 + i * 3, 5 + i * 3, 6 + i * 3);
        let mut jpg = Vec::new();
        let _ = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpg, 90).encode_image(img);
        let (iw, ih) = (img.width() as f32, img.height() as f32);
        let avail_w = pw - 2.0 * m;
        let avail_h = ph - 2.0 * m - 14.0;
        let k = (avail_w / iw).min(avail_h / ih) * opt.scale.clamp(0.1, 1.0);
        let (dw, dh) = (iw * k, ih * k);
        let (x, y) = (m, ph - m - dh);
        let mut content = format!("q {dw:.2} 0 0 {dh:.2} {x:.2} {y:.2} cm /Im{i} Do Q\n");
        if opt.page_numbers {
            content += &format!("BT /F1 9 Tf {:.2} {:.2} Td ({} / {}) Tj ET\n", pw / 2.0 - 10.0, m * 0.5, i + 1, n);
        }
        if !ascii_header.trim().is_empty() {
            content += &format!("BT /F1 9 Tf {:.2} {:.2} Td ({}) Tj ET\n", m, ph - m * 0.6, ascii_header);
        }
        objs.push(
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {pw} {ph}] /Resources << /Font << /F1 3 0 R >> /XObject << /Im{i} {iid} 0 R >> >> /Contents {cid} 0 R >>"
            )
            .into_bytes(),
        );
        let mut c = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
        c.extend_from_slice(content.as_bytes());
        c.extend_from_slice(b"\nendstream");
        objs.push(c);
        let mut im = format!(
            "<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode /Length {} >>\nstream\n",
            img.width(),
            img.height(),
            jpg.len()
        )
        .into_bytes();
        im.extend_from_slice(&jpg);
        im.extend_from_slice(b"\nendstream");
        objs.push(im);
    }
    let mut out = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(o);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
    for o in offsets {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1).as_bytes());
    out
}

/// Splits a tall page image into sheet-sized slices.
pub fn slice_pages(img: &image::RgbImage, landscape: bool) -> Vec<image::RgbImage> {
    let ratio = if landscape { 595.0 / 842.0 } else { 842.0 / 595.0 };
    let sheet_h = ((img.width() as f32) * ratio) as u32;
    if sheet_h == 0 {
        return vec![img.clone()];
    }
    let mut out = Vec::new();
    let mut y = 0;
    while y < img.height() {
        let h = sheet_h.min(img.height() - y);
        out.push(image::imageops::crop_imm(img, 0, y, img.width(), h).to_image());
        y += h;
    }
    out
}

// ----------------------------------------------------------------------------
// Tables: CSV / XLSX

pub fn table_to_csv(t: &Table) -> String {
    let mut s = String::new();
    for r in 0..t.rows() {
        let row: Vec<String> = (0..t.cols())
            .map(|c| {
                let v = crate::calc::cell_display(&t.cells, r, c);
                if v.contains([';', '"', '\n', ',']) { format!("\"{}\"", v.replace('"', "\"\"")) } else { v }
            })
            .collect();
        s.push_str(&row.join(";"));
        s.push_str("\r\n");
    }
    s
}

pub fn parse_csv(text: &str) -> Vec<Vec<String>> {
    let text = text.trim_start_matches('\u{feff}');
    let first = text.lines().next().unwrap_or("");
    let sep = [';', '\t', ','].into_iter().max_by_key(|c| first.matches(*c).count()).unwrap_or(';');
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut cur = String::new();
    let mut q = false;
    let cs: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        if q {
            if c == '"' {
                if cs.get(i + 1) == Some(&'"') {
                    cur.push('"');
                    i += 1;
                } else {
                    q = false;
                }
            } else {
                cur.push(c);
            }
        } else if c == '"' {
            q = true;
        } else if c == sep {
            row.push(std::mem::take(&mut cur));
        } else if c == '\n' {
            row.push(std::mem::take(&mut cur));
            rows.push(std::mem::take(&mut row));
        } else if c != '\r' {
            cur.push(c);
        }
        i += 1;
    }
    if !cur.is_empty() || !row.is_empty() {
        row.push(cur);
        rows.push(row);
    }
    normalize(rows)
}

fn normalize(mut rows: Vec<Vec<String>>) -> Vec<Vec<String>> {
    let w = rows.iter().map(|r| r.len()).max().unwrap_or(1).max(1);
    for r in rows.iter_mut() {
        r.resize(w, String::new());
    }
    if rows.is_empty() {
        rows.push(vec![String::new(); w]);
    }
    rows
}

pub fn table_from_cells(cells: Vec<Vec<String>>) -> Table {
    let cells = normalize(cells);
    let mut t = Table::new(cells.len(), cells[0].len());
    t.cells = cells;
    t
}

pub fn table_to_xlsx(t: &Table, path: &Path) -> Result<(), String> {
    use rust_xlsxwriter::{Format, Workbook};
    let mut wb = Workbook::new();
    let ws = wb.add_worksheet();
    let bold = Format::new().set_bold();
    for r in 0..t.rows() {
        for c in 0..t.cols() {
            let raw = &t.cells[r][c];
            let (rr, cc) = (r as u32, c as u16);
            let res = if let Some(f) = raw.trim().strip_prefix('=') {
                ws.write_formula(rr, cc, format!("={f}").as_str()).map(|_| ())
            } else if let Some(n) = crate::calc::parse_number(raw) {
                ws.write_number(rr, cc, n).map(|_| ())
            } else if t.header && r == 0 {
                ws.write_string_with_format(rr, cc, raw, &bold).map(|_| ())
            } else {
                ws.write_string(rr, cc, raw).map(|_| ())
            };
            res.map_err(|e| e.to_string())?;
        }
        let _ = ws.set_column_width(0, 12);
    }
    for (c, w) in t.col_widths.iter().enumerate() {
        let _ = ws.set_column_width(c as u16, (*w / 7.0) as f64);
    }
    wb.save(path).map_err(|e| e.to_string())
}

pub fn table_from_xlsx(path: &Path) -> Result<Vec<Vec<String>>, String> {
    use calamine::{Reader, open_workbook_auto};
    let mut wb = open_workbook_auto(path).map_err(|e| e.to_string())?;
    let name = wb.sheet_names().first().cloned().ok_or("Пустая книга")?;
    let range = wb.worksheet_range(&name).map_err(|e| e.to_string())?;
    let rows = range.rows().map(|r| r.iter().map(|c| c.to_string()).collect()).collect();
    Ok(normalize(rows))
}

// ----------------------------------------------------------------------------
// Import

pub fn html_to_markup(html: &str) -> String {
    let mut out = String::new();
    let mut i = 0;
    let b = html.as_bytes();
    let mut list_num: Vec<u32> = Vec::new();
    let mut skip_depth = 0;
    while i < html.len() {
        if b[i] == b'<' {
            let end = html[i..].find('>').map(|e| i + e).unwrap_or(html.len() - 1);
            let tag = html[i + 1..end].trim().to_lowercase();
            let name: String = tag.trim_start_matches('/').chars().take_while(|c| c.is_alphanumeric()).collect();
            let closing = tag.starts_with('/');
            match name.as_str() {
                "script" | "style" | "head" => skip_depth += if closing { -1 } else { 1 },
                "br" => out.push('\n'),
                "p" | "div" | "tr" | "en-note" if closing => out.push('\n'),
                "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                    if closing {
                        out.push('\n');
                    } else {
                        let n = name[1..].parse::<usize>().unwrap_or(1);
                        out.push_str(&format!("\n{} ", "#".repeat(n)));
                    }
                }
                "b" | "strong" => out.push_str("**"),
                "i" | "em" => out.push('*'),
                "u" => out.push_str("__"),
                "s" | "strike" | "del" => out.push_str("~~"),
                "code" => out.push('`'),
                "td" | "th" if closing => out.push('\t'),
                "ul" if !closing => list_num.push(0),
                "ol" if !closing => list_num.push(1),
                "ul" | "ol" => {
                    list_num.pop();
                    out.push('\n');
                }
                "li" if !closing => {
                    let ind = "  ".repeat(list_num.len().saturating_sub(1));
                    match list_num.last_mut() {
                        Some(n) if *n > 0 => {
                            out.push_str(&format!("\n{ind}{}. ", n));
                            *n += 1;
                        }
                        _ => out.push_str(&format!("\n{ind}- ")),
                    }
                }
                "en-todo" => {
                    out.push_str(if tag.contains("checked=\"true\"") { "\n[x] " } else { "\n[ ] " });
                }
                "hr" => out.push_str("\n---\n"),
                "blockquote" if !closing => out.push_str("\n> "),
                _ => {}
            }
            i = end + 1;
            continue;
        }
        let next = html[i..].find('<').map(|e| i + e).unwrap_or(html.len());
        if skip_depth <= 0 {
            out.push_str(&decode_entities(&html[i..next]).replace(['\n', '\r'], " "));
        }
        i = next;
    }
    // Collapse blank runs.
    let mut res = String::new();
    let mut blank = 0;
    for line in out.lines() {
        let l = line.trim_end();
        if l.trim().is_empty() {
            blank += 1;
            if blank > 1 {
                continue;
            }
        } else {
            blank = 0;
        }
        res.push_str(l.trim_start_matches(' '));
        res.push('\n');
    }
    res.trim().to_owned()
}

fn decode_entities(s: &str) -> String {
    let mut out = s.replace("&nbsp;", " ").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#39;", "'").replace("&apos;", "'");
    // Numeric entities.
    while let Some(a) = out.find("&#") {
        let Some(e) = out[a..].find(';') else { break };
        let num = &out[a + 2..a + e];
        let v = if let Some(h) = num.strip_prefix('x') { u32::from_str_radix(h, 16).ok() } else { num.parse().ok() };
        let rep = v.and_then(char::from_u32).map(|c| c.to_string()).unwrap_or_default();
        out.replace_range(a..a + e + 1, &rep);
    }
    out.replace("&amp;", "&")
}

/// Markdown → our markup (most syntax is shared).
pub fn markdown_to_markup(md: &str) -> String {
    md.lines()
        .map(|l| {
            let t = l.trim_start();
            let ind = &l[..l.len() - t.len()];
            if let Some(r) = t.strip_prefix("- [ ] ") {
                format!("{ind}[ ] {r}")
            } else if let Some(r) = t.strip_prefix("- [x] ").or_else(|| t.strip_prefix("- [X] ")) {
                format!("{ind}[x] {r}")
            } else {
                l.replace("<u>", "__").replace("</u>", "__")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Evernote ENEX: (title, markup) per note.
pub fn parse_enex(xml: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(a) = rest.find("<note>") {
        let Some(e) = rest[a..].find("</note>") else { break };
        let note = &rest[a..a + e];
        let title = between(note, "<title>", "</title>").map(decode_entities).unwrap_or_else(|| "Без названия".into());
        let mut content = between(note, "<content>", "</content>").unwrap_or("").to_owned();
        if let Some(c) = content.strip_prefix("<![CDATA[") {
            content = c.trim_end_matches("]]>").to_owned();
        }
        out.push((title, html_to_markup(&content)));
        rest = &rest[a + e + 7..];
    }
    out
}

fn between<'a>(s: &'a str, a: &str, b: &str) -> Option<&'a str> {
    let i = s.find(a)? + a.len();
    let j = s[i..].find(b)? + i;
    Some(&s[i..j])
}

pub fn docx_to_markup(path: &Path) -> Result<String, String> {
    let f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut z = zip::ZipArchive::new(f).map_err(|e| e.to_string())?;
    let mut xml = String::new();
    z.by_name("word/document.xml").map_err(|e| e.to_string())?.read_to_string(&mut xml).map_err(|e| e.to_string())?;
    let mut out = String::new();
    for para in xml.split("</w:p>") {
        let style = between(para, "<w:pStyle w:val=\"", "\"").unwrap_or("");
        let mut line = String::new();
        for run in para.split("</w:r>") {
            let bold = run.contains("<w:b/>") || run.contains("<w:b ");
            let ital = run.contains("<w:i/>") || run.contains("<w:i ");
            let mut text = String::new();
            let mut r = run;
            while let Some(a) = r.find("<w:t") {
                let Some(gt) = r[a..].find('>') else { break };
                let start = a + gt + 1;
                let Some(end) = r[start..].find("</w:t>") else { break };
                text.push_str(&decode_entities(&r[start..start + end]));
                r = &r[start + end..];
            }
            if run.contains("<w:tab/>") {
                text.push('\t');
            }
            if text.is_empty() {
                continue;
            }
            let mut t = text;
            if bold && !t.trim().is_empty() {
                t = format!("**{t}**");
            }
            if ital && !t.trim().is_empty() {
                t = format!("*{t}*");
            }
            line.push_str(&t);
        }
        let lower = style.to_lowercase();
        if let Some(n) = lower.strip_prefix("heading").and_then(|n| n.parse::<usize>().ok()) {
            line = format!("{} {}", "#".repeat(n.min(6)), line);
        } else if lower == "title" {
            line = format!("# {line}");
        } else if para.contains("<w:numPr>") {
            line = format!("- {line}");
        }
        if para.contains("<w:p") {
            out.push_str(&line);
            out.push('\n');
        }
    }
    Ok(out.trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn csv_and_html() {
        let rows = parse_csv("a;b\n\"x;y\";2\n");
        assert_eq!(rows[1][0], "x;y");
        assert_eq!(base64(b"Man"), "TWFu");
        let m = html_to_markup("<h1>Hi</h1><p><b>bold</b> text</p><ul><li>one</li></ul>");
        assert!(m.contains("# Hi") && m.contains("**bold**") && m.contains("- one"));
    }
}
