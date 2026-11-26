//! Modelo e parser/serializador leve de HTML rico para a ferramenta Documento (`doc`).
//! Compativel com o formato sanitizado de `group-sharescreen/lib/docHtml.ts` e `DocView.tsx`
//! (tags `<p>`, `<h1>`..`<h3>`, `<b>`/`<strong>`, `<i>`/`<em>`, `<u>`, `<s>`/`<strike>`,
//! `<span style="color:...">`, `<ul><li>`, `<ol><li>`, `<blockquote>`, `<a href="...">`,
//! `<table><tbody><tr><td>...`, `<hr>`, `style="text-align:..."`), sincronizado via UTF-16 OT.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DocAlign {
    #[default]
    Left,
    Center,
    Right,
}

impl DocAlign {
    pub fn as_css(self) -> &'static str {
        match self {
            DocAlign::Left => "left",
            DocAlign::Center => "center",
            DocAlign::Right => "right",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InlineStyle {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub color: Option<String>,
    pub link: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocRun {
    pub text: String,
    pub style: InlineStyle,
}

impl DocRun {
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            style: InlineStyle::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocBlockKind {
    Paragraph,
    Heading1,
    Heading2,
    Heading3,
    BulletList,
    OrderedList,
    Blockquote,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocBlock {
    Paragraph {
        align: DocAlign,
        runs: Vec<DocRun>,
    },
    Heading {
        level: u8,
        align: DocAlign,
        runs: Vec<DocRun>,
    },
    BulletList {
        items: Vec<Vec<DocRun>>,
    },
    OrderedList {
        items: Vec<Vec<DocRun>>,
    },
    Blockquote {
        runs: Vec<DocRun>,
    },
    Table {
        rows: Vec<Vec<String>>,
    },
    HorizontalRule,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocDocument {
    pub blocks: Vec<DocBlock>,
}

impl Default for DocDocument {
    fn default() -> Self {
        Self {
            blocks: vec![DocBlock::Paragraph {
                align: DocAlign::Left,
                runs: Vec::new(),
            }],
        }
    }
}

pub fn parse_css_hex_color(s: &str) -> Option<[u8; 3]> {
    let t = s.trim();
    if let Some(hex) = t.strip_prefix('#') {
        if hex.len() == 3 {
            let r = u8::from_str_radix(&hex[0..1], 16).ok()? * 17;
            let g = u8::from_str_radix(&hex[1..2], 16).ok()? * 17;
            let b = u8::from_str_radix(&hex[2..3], 16).ok()? * 17;
            return Some([r, g, b]);
        }
        if hex.len() >= 6 {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            return Some([r, g, b]);
        }
        return None;
    }
    let lower = t.to_ascii_lowercase();
    if let Some(inner) = lower
        .strip_prefix("rgb(")
        .and_then(|v| v.strip_suffix(')'))
    {
        let mut parts = inner.split(',').filter_map(|p| p.trim().parse::<u8>().ok());
        let r = parts.next()?;
        let g = parts.next()?;
        let b = parts.next()?;
        return Some([r, g, b]);
    }
    if let Some(inner) = lower
        .strip_prefix("rgba(")
        .and_then(|v| v.strip_suffix(')'))
    {
        let mut parts = inner.split(',').filter_map(|p| p.trim().parse::<f32>().ok());
        let r = parts.next()?.clamp(0.0, 255.0) as u8;
        let g = parts.next()?.clamp(0.0, 255.0) as u8;
        let b = parts.next()?.clamp(0.0, 255.0) as u8;
        return Some([r, g, b]);
    }
    None
}

pub fn normalize_color_hex(s: &str) -> Option<String> {
    parse_css_hex_color(s).map(|[r, g, b]| format!("#{:02x}{:02x}{:02x}", r, g, b))
}

fn sanitize_href(raw: &str) -> Option<String> {
    let t = raw.trim();
    let lower = t.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("mailto:") {
        Some(t.to_string())
    } else {
        None
    }
}

fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut i = 0usize;
    let bytes = s.as_bytes();
    while i < bytes.len() {
        if bytes[i] == b'&' {
            if let Some(rel_end) = s[i..].find(';') {
                if rel_end <= 12 {
                    let ent = &s[i + 1..i + rel_end];
                    let ch = match ent {
                        "amp" => Some('&'),
                        "lt" => Some('<'),
                        "gt" => Some('>'),
                        "quot" => Some('"'),
                        "apos" | "#39" => Some('\''),
                        "nbsp" => Some(' '),
                        _ if ent.starts_with("#x") || ent.starts_with("#X") => {
                            u32::from_str_radix(&ent[2..], 16)
                                .ok()
                                .and_then(char::from_u32)
                        }
                        _ if ent.starts_with('#') => {
                            ent[1..].parse::<u32>().ok().and_then(char::from_u32)
                        }
                        _ => None,
                    };
                    if let Some(c) = ch {
                        out.push(c);
                        i += rel_end + 1;
                        continue;
                    }
                }
            }
        }
        let ch = s[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn escape_html_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(ch),
        }
    }
    out
}

fn escape_html_attr(s: &str) -> String {
    escape_html_text(s).replace('"', "&quot;")
}

fn extract_attr(attrs: &str, name: &str) -> Option<String> {
    let lower = attrs.to_ascii_lowercase();
    let needle = format!("{}=", name.to_ascii_lowercase());
    let mut search_from = 0usize;
    while let Some(pos) = lower[search_from..].find(&needle) {
        let abs = search_from + pos;
        if abs > 0 {
            let prev = lower.as_bytes()[abs - 1];
            if !prev.is_ascii_whitespace() && prev != b'<' {
                search_from = abs + needle.len();
                continue;
            }
        }
        let val_start = abs + needle.len();
        let rest = attrs[val_start..].trim_start();
        if let Some(q) = rest.chars().next().filter(|&c| c == '"' || c == '\'') {
            let after_q = &rest[1..];
            if let Some(end_q) = after_q.find(q) {
                return Some(decode_entities(&after_q[..end_q]));
            }
        } else {
            let end = rest
                .find(|c: char| c.is_ascii_whitespace() || c == '>')
                .unwrap_or(rest.len());
            return Some(decode_entities(&rest[..end]));
        }
        break;
    }
    None
}

fn parse_align_from_attrs(attrs: &str) -> DocAlign {
    if let Some(style) = extract_attr(attrs, "style") {
        let low = style.to_ascii_lowercase();
        if low.contains("text-align") {
            if low.contains("center") {
                return DocAlign::Center;
            }
            if low.contains("right") {
                return DocAlign::Right;
            }
        }
    }
    if let Some(al) = extract_attr(attrs, "align") {
        let low = al.trim().to_ascii_lowercase();
        if low == "center" {
            return DocAlign::Center;
        }
        if low == "right" {
            return DocAlign::Right;
        }
    }
    DocAlign::Left
}

fn parse_color_from_attrs(attrs: &str) -> Option<String> {
    if let Some(style) = extract_attr(attrs, "style") {
        for decl in style.split(';') {
            if let Some((k, v)) = decl.split_once(':') {
                if k.trim().eq_ignore_ascii_case("color") {
                    if let Some(hex) = normalize_color_hex(v) {
                        return Some(hex);
                    }
                }
            }
        }
    }
    if let Some(col) = extract_attr(attrs, "color") {
        return normalize_color_hex(&col);
    }
    None
}

fn merge_runs(runs: Vec<DocRun>) -> Vec<DocRun> {
    let mut out: Vec<DocRun> = Vec::new();
    for r in runs {
        if r.text.is_empty() {
            continue;
        }
        if let Some(last) = out.last_mut() {
            if last.style == r.style {
                last.text.push_str(&r.text);
                continue;
            }
        }
        out.push(r);
    }
    out
}

pub fn runs_plain_text(runs: &[DocRun]) -> String {
    let mut out = String::new();
    for r in runs {
        out.push_str(&r.text);
    }
    out
}

#[derive(Debug, Clone)]
enum Token<'a> {
    Text(&'a str),
    Tag {
        name: String,
        attrs: &'a str,
        closing: bool,
        self_closing: bool,
    },
}

fn tokenize_html(input: &str) -> Vec<Token<'_>> {
    let mut tokens = Vec::new();
    let mut pos = 0usize;
    while pos < input.len() {
        let Some(lt) = input[pos..].find('<') else {
            tokens.push(Token::Text(&input[pos..]));
            break;
        };
        if lt > 0 {
            tokens.push(Token::Text(&input[pos..pos + lt]));
        }
        let tag_start = pos + lt + 1;
        // Comment <!-- ... -->
        if input[tag_start..].starts_with("!--") {
            if let Some(end_c) = input[tag_start..].find("-->") {
                pos = tag_start + end_c + 3;
                continue;
            }
        }
        // Respect quotes inside tag attributes
        let mut in_quote: Option<char> = None;
        let mut gt_pos = None;
        for (off, ch) in input[tag_start..].char_indices() {
            match in_quote {
                Some(q) if ch == q => in_quote = None,
                Some(_) => {}
                None if ch == '"' || ch == '\'' => in_quote = Some(ch),
                None if ch == '>' => {
                    gt_pos = Some(tag_start + off);
                    break;
                }
                None => {}
            }
        }
        let Some(gt) = gt_pos else {
            tokens.push(Token::Text(&input[pos + lt..]));
            break;
        };
        let raw_inner = input[tag_start..gt].trim();
        pos = gt + 1;
        if raw_inner.is_empty() {
            continue;
        }
        let closing = raw_inner.starts_with('/');
        let after_slash = if closing { raw_inner[1..].trim_start() } else { raw_inner };
        let self_closing = after_slash.ends_with('/');
        let body = if self_closing {
            after_slash[..after_slash.len() - 1].trim_end()
        } else {
            after_slash
        };
        let name_end = body
            .find(|c: char| c.is_ascii_whitespace())
            .unwrap_or(body.len());
        let name = body[..name_end].to_ascii_lowercase();
        let attrs = &body[name_end..];
        tokens.push(Token::Tag {
            name,
            attrs,
            closing,
            self_closing,
        });
    }
    tokens
}

pub fn parse_doc_html(html: &str) -> DocDocument {
    let trimmed = html.trim();
    if trimmed.is_empty() {
        return DocDocument::default();
    }
    if !trimmed.contains('<') {
        let blocks = html
            .split('\n')
            .map(|line| DocBlock::Paragraph {
                align: DocAlign::Left,
                runs: if line.is_empty() {
                    Vec::new()
                } else {
                    vec![DocRun::plain(line.to_string())]
                },
            })
            .collect();
        return DocDocument { blocks };
    }

    let tokens = tokenize_html(html);
    let mut blocks: Vec<DocBlock> = Vec::new();
    let mut style_stack: Vec<(String, InlineStyle)> = Vec::new();
    let mut cur_style = InlineStyle::default();

    enum ActiveBlock {
        None,
        Paragraph { align: DocAlign, runs: Vec<DocRun> },
        Heading { level: u8, align: DocAlign, runs: Vec<DocRun> },
        Blockquote { runs: Vec<DocRun> },
        BulletList { items: Vec<Vec<DocRun>>, cur_item: Option<Vec<DocRun>> },
        OrderedList { items: Vec<Vec<DocRun>>, cur_item: Option<Vec<DocRun>> },
        Table { rows: Vec<Vec<String>>, cur_row: Option<Vec<String>>, cur_cell: Option<String> },
    }

    let flush_active = |active: &mut ActiveBlock, blocks: &mut Vec<DocBlock>| {
        let old = std::mem::replace(active, ActiveBlock::None);
        match old {
            ActiveBlock::None => {}
            ActiveBlock::Paragraph { align, runs } => {
                blocks.push(DocBlock::Paragraph {
                    align,
                    runs: merge_runs(runs),
                });
            }
            ActiveBlock::Heading { level, align, runs } => {
                blocks.push(DocBlock::Heading {
                    level,
                    align,
                    runs: merge_runs(runs),
                });
            }
            ActiveBlock::Blockquote { runs } => {
                blocks.push(DocBlock::Blockquote {
                    runs: merge_runs(runs),
                });
            }
            ActiveBlock::BulletList { mut items, cur_item } => {
                if let Some(it) = cur_item {
                    items.push(merge_runs(it));
                }
                if !items.is_empty() {
                    blocks.push(DocBlock::BulletList { items });
                }
            }
            ActiveBlock::OrderedList { mut items, cur_item } => {
                if let Some(it) = cur_item {
                    items.push(merge_runs(it));
                }
                if !items.is_empty() {
                    blocks.push(DocBlock::OrderedList { items });
                }
            }
            ActiveBlock::Table { mut rows, mut cur_row, cur_cell } => {
                if let Some(cell) = cur_cell {
                    let row = cur_row.get_or_insert_with(Vec::new);
                    row.push(cell);
                }
                if let Some(row) = cur_row {
                    if !row.is_empty() {
                        rows.push(row);
                    }
                }
                if !rows.is_empty() {
                    blocks.push(DocBlock::Table { rows });
                }
            }
        }
    };

    let push_run = |active: &mut ActiveBlock, txt: String, st: &InlineStyle| {
        if txt.is_empty() {
            return;
        }
        match active {
            ActiveBlock::None => {
                *active = ActiveBlock::Paragraph {
                    align: DocAlign::Left,
                    runs: vec![DocRun {
                        text: txt,
                        style: st.clone(),
                    }],
                };
            }
            ActiveBlock::Paragraph { runs, .. }
            | ActiveBlock::Heading { runs, .. }
            | ActiveBlock::Blockquote { runs } => {
                runs.push(DocRun {
                    text: txt,
                    style: st.clone(),
                });
            }
            ActiveBlock::BulletList { cur_item, .. }
            | ActiveBlock::OrderedList { cur_item, .. } => {
                let it = cur_item.get_or_insert_with(Vec::new);
                it.push(DocRun {
                    text: txt,
                    style: st.clone(),
                });
            }
            ActiveBlock::Table { cur_cell, .. } => {
                if let Some(c) = cur_cell.as_mut() {
                    c.push_str(&txt);
                }
            }
        }
    };

    let recompute_style = |stack: &[(String, InlineStyle)]| -> InlineStyle {
        let mut s = InlineStyle::default();
        for (_, item) in stack {
            s.bold |= item.bold;
            s.italic |= item.italic;
            s.underline |= item.underline;
            s.strike |= item.strike;
            if item.color.is_some() {
                s.color = item.color.clone();
            }
            if item.link.is_some() {
                s.link = item.link.clone();
            }
        }
        s
    };

    let mut active = ActiveBlock::None;
    for tok in tokens {
        match tok {
            Token::Text(raw) => {
                let decoded = decode_entities(raw);
                if matches!(active, ActiveBlock::None) && decoded.trim().is_empty() {
                    continue;
                }
                push_run(&mut active, decoded, &cur_style);
            }
            Token::Tag {
                name,
                attrs,
                closing,
                self_closing,
            } => {
                match name.as_str() {
                    "b" | "strong" | "i" | "em" | "u" | "s" | "strike" | "del" | "span" | "font" | "a" => {
                        if closing {
                            if let Some(pos) = style_stack.iter().rposition(|(n, _)| n == &name) {
                                style_stack.remove(pos);
                                cur_style = recompute_style(&style_stack);
                            }
                        } else if !self_closing {
                            let mut delta = InlineStyle::default();
                            match name.as_str() {
                                "b" | "strong" => delta.bold = true,
                                "i" | "em" => delta.italic = true,
                                "u" => delta.underline = true,
                                "s" | "strike" | "del" => delta.strike = true,
                                "span" | "font" => delta.color = parse_color_from_attrs(attrs),
                                "a" => {
                                    delta.link = extract_attr(attrs, "href").and_then(|h| sanitize_href(&h));
                                    delta.underline = true;
                                }
                                _ => {}
                            }
                            style_stack.push((name, delta));
                            cur_style = recompute_style(&style_stack);
                        }
                    }
                    "br" => {
                        // Ignore trailing lone <br> inside empty block (ProseMirror <p><br></p>)
                        let is_trailing_empty = matches!(
                            &active,
                            ActiveBlock::Paragraph { runs, .. } if runs.is_empty()
                        );
                        if !is_trailing_empty {
                            push_run(&mut active, "\n".to_string(), &cur_style);
                        }
                    }
                    "hr" => {
                        flush_active(&mut active, &mut blocks);
                        blocks.push(DocBlock::HorizontalRule);
                    }
                    "p" | "div" => {
                        if let ActiveBlock::Blockquote { ref mut runs } = active {
                            if !closing && !runs.is_empty() {
                                runs.push(DocRun::plain("\n"));
                            }
                            continue;
                        }
                        if matches!(
                            active,
                            ActiveBlock::BulletList { .. }
                                | ActiveBlock::OrderedList { .. }
                                | ActiveBlock::Table { .. }
                        ) {
                            // Web ProseMirror wraps list/table items in <p>...</p>
                            continue;
                        }
                        if closing {
                            flush_active(&mut active, &mut blocks);
                        } else {
                            flush_active(&mut active, &mut blocks);
                            active = ActiveBlock::Paragraph {
                                align: parse_align_from_attrs(attrs),
                                runs: Vec::new(),
                            };
                        }
                    }
                    "h1" | "h2" | "h3" => {
                        let lvl = match name.as_str() {
                            "h1" => 1,
                            "h2" => 2,
                            _ => 3,
                        };
                        if closing {
                            flush_active(&mut active, &mut blocks);
                        } else {
                            flush_active(&mut active, &mut blocks);
                            active = ActiveBlock::Heading {
                                level: lvl,
                                align: parse_align_from_attrs(attrs),
                                runs: Vec::new(),
                            };
                        }
                    }
                    "blockquote" => {
                        if closing {
                            flush_active(&mut active, &mut blocks);
                        } else {
                            flush_active(&mut active, &mut blocks);
                            active = ActiveBlock::Blockquote { runs: Vec::new() };
                        }
                    }
                    "ul" => {
                        if closing {
                            flush_active(&mut active, &mut blocks);
                        } else {
                            flush_active(&mut active, &mut blocks);
                            active = ActiveBlock::BulletList {
                                items: Vec::new(),
                                cur_item: None,
                            };
                        }
                    }
                    "ol" => {
                        if closing {
                            flush_active(&mut active, &mut blocks);
                        } else {
                            flush_active(&mut active, &mut blocks);
                            active = ActiveBlock::OrderedList {
                                items: Vec::new(),
                                cur_item: None,
                            };
                        }
                    }
                    "li" => {
                        match &mut active {
                            ActiveBlock::BulletList { items, cur_item }
                            | ActiveBlock::OrderedList { items, cur_item } => {
                                if closing {
                                    let it = cur_item.take().unwrap_or_default();
                                    items.push(merge_runs(it));
                                } else {
                                    if let Some(prev) = cur_item.take() {
                                        items.push(merge_runs(prev));
                                    }
                                    *cur_item = Some(Vec::new());
                                }
                            }
                            _ => {}
                        }
                    }
                    "table" => {
                        if closing {
                            flush_active(&mut active, &mut blocks);
                        } else {
                            flush_active(&mut active, &mut blocks);
                            active = ActiveBlock::Table {
                                rows: Vec::new(),
                                cur_row: None,
                                cur_cell: None,
                            };
                        }
                    }
                    "tr" => {
                        if let ActiveBlock::Table {
                            rows,
                            cur_row,
                            cur_cell,
                        } = &mut active
                        {
                            if closing {
                                if let Some(c) = cur_cell.take() {
                                    cur_row.get_or_insert_with(Vec::new).push(c);
                                }
                                if let Some(r) = cur_row.take() {
                                    rows.push(r);
                                }
                            } else {
                                if let Some(r) = cur_row.take() {
                                    rows.push(r);
                                }
                                *cur_row = Some(Vec::new());
                            }
                        }
                    }
                    "td" | "th" => {
                        if let ActiveBlock::Table {
                            cur_row, cur_cell, ..
                        } = &mut active
                        {
                            if closing {
                                let c = cur_cell.take().unwrap_or_default();
                                cur_row.get_or_insert_with(Vec::new).push(c);
                            } else {
                                if let Some(c) = cur_cell.take() {
                                    cur_row.get_or_insert_with(Vec::new).push(c);
                                }
                                *cur_cell = Some(String::new());
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    flush_active(&mut active, &mut blocks);
    if blocks.is_empty() {
        DocDocument::default()
    } else {
        DocDocument { blocks }
    }
}

fn serialize_runs(runs: &[DocRun]) -> String {
    let mut out = String::new();
    for r in runs {
        if r.text.is_empty() {
            continue;
        }
        let mut inner = escape_html_text(&r.text).replace('\n', "<br>");
        if let Some(ref href) = r.style.link {
            if let Some(safe) = sanitize_href(href) {
                inner = format!(
                    "<a href=\"{}\" target=\"_blank\" rel=\"noopener noreferrer\">{}</a>",
                    escape_html_attr(&safe),
                    inner
                );
            }
        }
        if let Some(ref col) = r.style.color {
            if let Some(hex) = normalize_color_hex(col) {
                inner = format!("<span style=\"color: {}\">{}</span>", hex, inner);
            }
        }
        if r.style.strike {
            inner = format!("<s>{}</s>", inner);
        }
        if r.style.underline && r.style.link.is_none() {
            inner = format!("<u>{}</u>", inner);
        }
        if r.style.italic {
            inner = format!("<i>{}</i>", inner);
        }
        if r.style.bold {
            inner = format!("<b>{}</b>", inner);
        }
        out.push_str(&inner);
    }
    out
}

pub fn serialize_doc_html(doc: &DocDocument) -> String {
    let mut out = String::new();
    for block in &doc.blocks {
        match block {
            DocBlock::Paragraph { align, runs } => {
                let align_attr = if *align != DocAlign::Left {
                    format!(" style=\"text-align: {}\"", align.as_css())
                } else {
                    String::new()
                };
                out.push_str(&format!("<p{}>{}</p>", align_attr, serialize_runs(runs)));
            }
            DocBlock::Heading { level, align, runs } => {
                let tag = match level {
                    1 => "h1",
                    2 => "h2",
                    _ => "h3",
                };
                let align_attr = if *align != DocAlign::Left {
                    format!(" style=\"text-align: {}\"", align.as_css())
                } else {
                    String::new()
                };
                out.push_str(&format!(
                    "<{}{}>{}</{}>",
                    tag,
                    align_attr,
                    serialize_runs(runs),
                    tag
                ));
            }
            DocBlock::BulletList { items } => {
                out.push_str("<ul>");
                for it in items {
                    out.push_str(&format!("<li><p>{}</p></li>", serialize_runs(it)));
                }
                out.push_str("</ul>");
            }
            DocBlock::OrderedList { items } => {
                out.push_str("<ol>");
                for it in items {
                    out.push_str(&format!("<li><p>{}</p></li>", serialize_runs(it)));
                }
                out.push_str("</ol>");
            }
            DocBlock::Blockquote { runs } => {
                out.push_str(&format!(
                    "<blockquote><p>{}</p></blockquote>",
                    serialize_runs(runs)
                ));
            }
            DocBlock::Table { rows } => {
                out.push_str("<table><tbody>");
                for row in rows {
                    out.push_str("<tr>");
                    for cell in row {
                        out.push_str(&format!("<td><p>{}</p></td>", escape_html_text(cell)));
                    }
                    out.push_str("</tr>");
                }
                out.push_str("</tbody></table>");
            }
            DocBlock::HorizontalRule => {
                out.push_str("<hr>");
            }
        }
    }
    out
}

pub fn toggle_runs_style(
    runs: &[DocRun],
    char_start: usize,
    char_end: usize,
    mut update: impl FnMut(&mut InlineStyle, bool),
    is_set: impl Fn(&InlineStyle) -> bool,
) -> Vec<DocRun> {
    let total_chars: usize = runs.iter().map(|r| r.text.chars().count()).sum();
    if total_chars == 0 {
        let mut s = InlineStyle::default();
        update(&mut s, true);
        return vec![DocRun {
            text: String::new(),
            style: s,
        }];
    }
    let (lo, hi) = if char_start == char_end {
        (0, total_chars)
    } else {
        (char_start.min(char_end), char_start.max(char_end).min(total_chars))
    };

    // Determine if entire selected range already has flag set -> then turn OFF, else ON
    let mut pos = 0usize;
    let mut all_set = true;
    let mut any_in_range = false;
    for r in runs {
        let len = r.text.chars().count();
        let r_lo = pos;
        let r_hi = pos + len;
        if r_hi > lo && r_lo < hi && !r.text.is_empty() {
            any_in_range = true;
            if !is_set(&r.style) {
                all_set = false;
                break;
            }
        }
        pos = r_hi;
    }
    let target_on = !(any_in_range && all_set);

    let mut out = Vec::new();
    let mut cur = 0usize;
    for r in runs {
        let chars: Vec<char> = r.text.chars().collect();
        let len = chars.len();
        let r_lo = cur;
        let r_hi = cur + len;
        cur = r_hi;
        if r_hi <= lo || r_lo >= hi {
            out.push(r.clone());
            continue;
        }
        let s_lo = lo.saturating_sub(r_lo).min(len);
        let s_hi = hi.saturating_sub(r_lo).min(len);
        if s_lo > 0 {
            out.push(DocRun {
                text: chars[..s_lo].iter().collect(),
                style: r.style.clone(),
            });
        }
        if s_hi > s_lo {
            let mut next_st = r.style.clone();
            update(&mut next_st, target_on);
            out.push(DocRun {
                text: chars[s_lo..s_hi].iter().collect(),
                style: next_st,
            });
        }
        if s_hi < len {
            out.push(DocRun {
                text: chars[s_hi..].iter().collect(),
                style: r.style.clone(),
            });
        }
    }
    merge_runs(out)
}

/// Atualiza o texto plano de uma lista de `DocRun` preservando a formatacao existente via dif,
/// opcionalmente aplicando `pending_style` apenas aos novos caracteres inseridos.
#[cfg(test)]
pub fn update_runs_text_preserving_styles(old_runs: &[DocRun], new_text: &str) -> Vec<DocRun> {
    update_runs_with_typing_style(old_runs, new_text, None)
}

pub fn update_runs_with_typing_style(
    old_runs: &[DocRun],
    new_text: &str,
    pending_style: Option<&InlineStyle>,
) -> Vec<DocRun> {
    let old_text = runs_plain_text(old_runs);
    if old_text == new_text {
        return old_runs.to_vec();
    }
    if new_text.is_empty() {
        return Vec::new();
    }
    if old_runs.is_empty() {
        return vec![DocRun {
            text: new_text.to_string(),
            style: pending_style.cloned().unwrap_or_default(),
        }];
    }
    // Single uniform run? Fast path only if no overriding pending_style!
    if old_runs.len() == 1 && pending_style.is_none() {
        return vec![DocRun {
            text: new_text.to_string(),
            style: old_runs[0].style.clone(),
        }];
    }
    let a: Vec<char> = old_text.chars().collect();
    let b: Vec<char> = new_text.chars().collect();
    let mut pref = 0usize;
    let max_pref = a.len().min(b.len());
    while pref < max_pref && a[pref] == b[pref] {
        pref += 1;
    }
    let mut end_a = a.len();
    let mut end_b = b.len();
    while end_a > pref && end_b > pref && a[end_a - 1] == b[end_b - 1] {
        end_a -= 1;
        end_b -= 1;
    }

    let mut out = Vec::new();
    let mut mid_style = pending_style.cloned().unwrap_or_else(|| old_runs[0].style.clone());
    let mut pos = 0usize;
    for r in old_runs {
        let len = r.text.chars().count();
        let r_lo = pos;
        let r_hi = pos + len;
        pos = r_hi;
        if pending_style.is_none() && ((pref == 0 && r_lo == 0) || (pref > r_lo && pref <= r_hi)) {
            mid_style = r.style.clone();
        }
        if r_lo < pref {
            let take = (pref - r_lo).min(len);
            let s: String = r.text.chars().take(take).collect();
            if !s.is_empty() {
                out.push(DocRun {
                    text: s,
                    style: r.style.clone(),
                });
            }
        }
    }
    if end_b > pref {
        let ins: String = b[pref..end_b].iter().collect();
        out.push(DocRun {
            text: ins,
            style: mid_style,
        });
    }
    let mut pos2 = 0usize;
    for r in old_runs {
        let len = r.text.chars().count();
        let r_lo = pos2;
        let r_hi = pos2 + len;
        pos2 = r_hi;
        if r_hi > end_a {
            let skip = end_a.saturating_sub(r_lo);
            let s: String = r.text.chars().skip(skip).collect();
            if !s.is_empty() {
                out.push(DocRun {
                    text: s,
                    style: r.style.clone(),
                });
            }
        }
    }
    merge_runs(out)
}

const EXPORT_PRINT_STYLE: &str = "body{font-family:Calibri,Arial,sans-serif;font-size:11pt;max-width:780px;margin:24px auto;color:#111}h1{font-size:24pt}h2{font-size:18pt}h3{font-size:14pt}table{border-collapse:collapse}td,th{border:1px solid #999;padding:4px 8px}blockquote{border-left:4px solid #ccc;margin-left:0;padding-left:12px;color:#555}";

pub fn doc_to_word_html(raw_html: &str, title: &str) -> String {
    let body = serialize_doc_html(&parse_doc_html(raw_html));
    let safe_title = escape_html_text(if title.trim().is_empty() {
        "Documento"
    } else {
        title.trim()
    });
    format!(
        "<html xmlns:o=\"urn:schemas-microsoft-com:office:office\" xmlns:w=\"urn:schemas-microsoft-com:office:word\" xmlns=\"http://www.w3.org/TR/REC-html40\"><head><meta charset=\"utf-8\"><title>{}</title><!--[if gte mso 9]><xml><w:WordDocument><w:View>Print</w:View><w:Zoom>100</w:Zoom></w:WordDocument></xml><![endif]--><style>{}</style></head><body>{}</body></html>",
        safe_title, EXPORT_PRINT_STYLE, body
    )
}

pub fn doc_to_printable_html(raw_html: &str, title: &str) -> String {
    let body = serialize_doc_html(&parse_doc_html(raw_html));
    let safe_title = escape_html_text(if title.trim().is_empty() {
        "Documento"
    } else {
        title.trim()
    });
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>{}</title><style>{}</style></head><body>{}</body></html>",
        safe_title, EXPORT_PRINT_STYLE, body
    )
}

/// Quantidade de caracteres (`char`) do texto dos runs — o indice que o cursor do egui usa.
pub fn runs_char_len(runs: &[DocRun]) -> usize {
    runs.iter().map(|r| r.text.chars().count()).sum()
}

/// Divide os runs no caractere `at` preservando o estilo de cada lado
/// (o Enter do editor: o que fica antes vai para o bloco atual, o resto para o novo).
pub fn split_runs_at(runs: &[DocRun], at: usize) -> (Vec<DocRun>, Vec<DocRun>) {
    let mut left = Vec::new();
    let mut right = Vec::new();
    let mut seen = 0usize;
    for r in runs {
        let len = r.text.chars().count();
        if seen + len <= at {
            left.push(r.clone());
        } else if seen >= at {
            right.push(r.clone());
        } else {
            let cut = r
                .text
                .char_indices()
                .nth(at - seen)
                .map(|(b, _)| b)
                .unwrap_or(r.text.len());
            left.push(DocRun {
                text: r.text[..cut].to_string(),
                style: r.style.clone(),
            });
            right.push(DocRun {
                text: r.text[cut..].to_string(),
                style: r.style.clone(),
            });
        }
        seen += len;
    }
    left.retain(|r| !r.text.is_empty());
    right.retain(|r| !r.text.is_empty());
    (merge_runs(left), merge_runs(right))
}

/// Junta `b` no fim de `a` (o Backspace no inicio de um bloco).
pub fn concat_runs(a: &[DocRun], b: &[DocRun]) -> Vec<DocRun> {
    let mut out = a.to_vec();
    out.extend(b.iter().cloned());
    merge_runs(out)
}

/// "Imprimir ou salvar em PDF" do site: abre o documento numa pagina propria que chama
/// `window.print()`. Aqui a pagina vai para a pasta temporaria e abre no navegador padrao,
/// cujo dialogo de impressao tem "Salvar como PDF". Nada fica rodando no Go Live Lite.
pub fn print_doc_via_browser(raw_html: &str, title: &str) -> Result<std::path::PathBuf, String> {
    let page = doc_to_printable_html(raw_html, title).replacen(
        "</body>",
        "<script>window.addEventListener('load',function(){setTimeout(function(){window.print()},150)})</script></body>",
        1,
    );
    let stem: String = title
        .trim()
        .chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | ' '))
        .take(40)
        .collect();
    let stem = if stem.trim().is_empty() { "documento".to_string() } else { stem.trim().to_string() };
    let path = std::env::temp_dir().join(format!("golive-lite-{}.html", stem));
    std::fs::write(&path, page.as_bytes()).map_err(|e| format!("nao foi possivel salvar: {e}"))?;
    crate::net::protocol::open_local_file(&path)?;
    Ok(path)
}

pub fn save_export_to_downloads(base_name: &str, ext: &str, bytes: &[u8]) -> Option<std::path::PathBuf> {
    let sanitized: String = base_name
        .trim()
        .chars()
        .map(|c| {
            if matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let stem = if sanitized.is_empty() {
        "golive-export".to_string()
    } else {
        sanitized
    };
    let dir = std::env::var_os("USERPROFILE")
        .map(|u| std::path::PathBuf::from(u).join("Downloads"))
        .filter(|d| d.is_dir())
        .or_else(|| std::env::current_dir().ok())?;
    for n in 0..100usize {
        let fname = if n == 0 {
            format!("{}.{}", stem, ext)
        } else {
            format!("{} ({}).{}", stem, n, ext)
        };
        let cand = dir.join(fname);
        if !cand.exists() {
            if std::fs::write(&cand, bytes).is_ok() {
                return Some(cand);
            }
            return None;
        }
    }
    None
}

impl DocBlock {
    pub fn kind(&self) -> Option<DocBlockKind> {
        match self {
            DocBlock::Paragraph { .. } => Some(DocBlockKind::Paragraph),
            DocBlock::Heading { level: 1, .. } => Some(DocBlockKind::Heading1),
            DocBlock::Heading { level: 2, .. } => Some(DocBlockKind::Heading2),
            DocBlock::Heading { .. } => Some(DocBlockKind::Heading3),
            DocBlock::BulletList { .. } => Some(DocBlockKind::BulletList),
            DocBlock::OrderedList { .. } => Some(DocBlockKind::OrderedList),
            DocBlock::Blockquote { .. } => Some(DocBlockKind::Blockquote),
            DocBlock::Table { .. } | DocBlock::HorizontalRule => None,
        }
    }

    pub fn align(&self) -> DocAlign {
        match self {
            DocBlock::Paragraph { align, .. } | DocBlock::Heading { align, .. } => *align,
            _ => DocAlign::Left,
        }
    }

    pub fn runs(&self) -> Option<&[DocRun]> {
        match self {
            DocBlock::Paragraph { runs, .. }
            | DocBlock::Heading { runs, .. }
            | DocBlock::Blockquote { runs } => Some(runs),
            _ => None,
        }
    }

    pub fn runs_mut(&mut self) -> Option<&mut Vec<DocRun>> {
        match self {
            DocBlock::Paragraph { runs, .. }
            | DocBlock::Heading { runs, .. }
            | DocBlock::Blockquote { runs } => Some(runs),
            _ => None,
        }
    }
}

impl DocDocument {
    pub fn set_block_kind(&mut self, idx: usize, target: DocBlockKind) {
        let Some(blk) = self.blocks.get_mut(idx) else {
            return;
        };
        let align = blk.align();
        let runs: Vec<DocRun> = match blk {
            DocBlock::Paragraph { runs, .. }
            | DocBlock::Heading { runs, .. }
            | DocBlock::Blockquote { runs } => std::mem::take(runs),
            DocBlock::BulletList { items } | DocBlock::OrderedList { items } => {
                let mut combined = Vec::new();
                for (i, it) in items.iter().enumerate() {
                    if i > 0 {
                        combined.push(DocRun::plain("\n"));
                    }
                    combined.extend(it.iter().cloned());
                }
                merge_runs(combined)
            }
            DocBlock::Table { .. } | DocBlock::HorizontalRule => return,
        };
        *blk = match target {
            DocBlockKind::Paragraph => DocBlock::Paragraph { align, runs },
            DocBlockKind::Heading1 => DocBlock::Heading { level: 1, align, runs },
            DocBlockKind::Heading2 => DocBlock::Heading { level: 2, align, runs },
            DocBlockKind::Heading3 => DocBlock::Heading { level: 3, align, runs },
            DocBlockKind::Blockquote => DocBlock::Blockquote { runs },
            DocBlockKind::BulletList => DocBlock::BulletList {
                items: split_runs_by_newline(runs),
            },
            DocBlockKind::OrderedList => DocBlock::OrderedList {
                items: split_runs_by_newline(runs),
            },
        };
    }

    pub fn set_block_align(&mut self, idx: usize, new_align: DocAlign) {
        if let Some(blk) = self.blocks.get_mut(idx) {
            match blk {
                DocBlock::Paragraph { align, .. } | DocBlock::Heading { align, .. } => {
                    *align = new_align;
                }
                _ => {}
            }
        }
    }

    pub fn clear_block_formatting(&mut self, idx: usize) {
        let Some(blk) = self.blocks.get_mut(idx) else {
            return;
        };
        match blk {
            DocBlock::Paragraph { align, runs } | DocBlock::Heading { align, runs, .. } => {
                let txt = runs_plain_text(runs);
                let _ = align;
                *blk = DocBlock::Paragraph {
                    align: DocAlign::Left,
                    runs: if txt.is_empty() { Vec::new() } else { vec![DocRun::plain(txt)] },
                };
            }
            DocBlock::Blockquote { runs } => {
                let txt = runs_plain_text(runs);
                *blk = DocBlock::Paragraph {
                    align: DocAlign::Left,
                    runs: if txt.is_empty() { Vec::new() } else { vec![DocRun::plain(txt)] },
                };
            }
            _ => {}
        }
    }

    pub fn insert_hr_after(&mut self, idx: usize) {
        let pos = (idx + 1).min(self.blocks.len());
        self.blocks.insert(pos, DocBlock::HorizontalRule);
        if pos + 1 >= self.blocks.len() {
            self.blocks.push(DocBlock::Paragraph {
                align: DocAlign::Left,
                runs: Vec::new(),
            });
        }
    }

    pub fn insert_table_after(&mut self, idx: usize, rows: usize, cols: usize) {
        let pos = (idx + 1).min(self.blocks.len());
        let tbl = DocBlock::Table {
            rows: vec![vec![String::new(); cols.max(1)]; rows.max(1)],
        };
        self.blocks.insert(pos, tbl);
        if pos + 1 >= self.blocks.len() {
            self.blocks.push(DocBlock::Paragraph {
                align: DocAlign::Left,
                runs: Vec::new(),
            });
        }
    }
}

/// Item "nenhum" no alvo de foco `(bloco, item, cursor)` dos blocos que nao sao lista.
pub const DOC_NO_ITEM: usize = usize::MAX;

/// Para onde o cursor vai depois de uma edicao estrutural: `(bloco, item, caractere)`.
pub type DocFocus = (usize, usize, usize);

impl DocBlock {
    fn list_items_mut(&mut self) -> Option<&mut Vec<Vec<DocRun>>> {
        match self {
            DocBlock::BulletList { items } | DocBlock::OrderedList { items } => Some(items),
            _ => None,
        }
    }

    fn same_list_kind(&self, items: Vec<Vec<DocRun>>) -> DocBlock {
        match self {
            DocBlock::OrderedList { .. } => DocBlock::OrderedList { items },
            _ => DocBlock::BulletList { items },
        }
    }
}

/// Edicoes de teclado que mudam a estrutura do documento, como o contentEditable do site faz:
/// Enter divide o bloco, Backspace no inicio junta com o anterior, Enter em item vazio sai da lista.
impl DocDocument {
    /// Enter num paragrafo, titulo ou citacao (cursor em `at`).
    pub fn split_text_block(&mut self, idx: usize, at: usize) -> Option<DocFocus> {
        // Citacao vazia + Enter: sai da citacao, como no editor do site.
        if matches!(self.blocks.get(idx)?, DocBlock::Blockquote { runs } if runs.is_empty()) {
            self.blocks[idx] = DocBlock::Paragraph { align: DocAlign::Left, runs: Vec::new() };
            return Some((idx, DOC_NO_ITEM, 0));
        }
        let blk = self.blocks.get_mut(idx)?;
        let align = blk.align();
        let total = runs_char_len(blk.runs()?);
        let new_blk = match blk {
            DocBlock::Blockquote { runs } => {
                let (l, r) = split_runs_at(runs, at);
                *runs = l;
                DocBlock::Blockquote { runs: r }
            }
            // Enter no fim de um titulo abre texto normal; no meio, divide o titulo.
            DocBlock::Heading { level, runs, .. } => {
                let level = *level;
                let (l, r) = split_runs_at(runs, at);
                *runs = l;
                if at >= total {
                    DocBlock::Paragraph { align: DocAlign::Left, runs: r }
                } else {
                    DocBlock::Heading { level, align, runs: r }
                }
            }
            DocBlock::Paragraph { runs, .. } => {
                let (l, r) = split_runs_at(runs, at);
                *runs = l;
                DocBlock::Paragraph { align, runs: r }
            }
            _ => return None,
        };
        self.blocks.insert(idx + 1, new_blk);
        Some((idx + 1, DOC_NO_ITEM, 0))
    }

    /// Backspace no inicio de um paragrafo, titulo ou citacao.
    pub fn merge_text_block_back(&mut self, idx: usize) -> Option<DocFocus> {
        let cur = self.blocks.get(idx)?.runs()?.to_vec();
        if idx == 0 {
            // No topo: titulo/citacao viram texto normal; paragrafo nao tem para onde ir.
            let promotable = matches!(
                self.blocks[0],
                DocBlock::Heading { .. } | DocBlock::Blockquote { .. }
            );
            if !promotable {
                return None;
            }
            self.set_block_kind(0, DocBlockKind::Paragraph);
            return Some((0, DOC_NO_ITEM, 0));
        }
        let focus = match &mut self.blocks[idx - 1] {
            DocBlock::Paragraph { runs, .. }
            | DocBlock::Heading { runs, .. }
            | DocBlock::Blockquote { runs } => {
                let len = runs_char_len(runs);
                *runs = concat_runs(runs, &cur);
                (idx - 1, DOC_NO_ITEM, len)
            }
            DocBlock::BulletList { items } | DocBlock::OrderedList { items } => {
                if items.is_empty() {
                    items.push(Vec::new());
                }
                let last = items.len() - 1;
                let len = runs_char_len(&items[last]);
                items[last] = concat_runs(&items[last], &cur);
                (idx - 1, last, len)
            }
            DocBlock::HorizontalRule => {
                // Apaga a linha divisoria que estava logo acima.
                self.blocks.remove(idx - 1);
                return Some((idx - 1, DOC_NO_ITEM, 0));
            }
            DocBlock::Table { .. } => {
                if !cur.is_empty() {
                    return None;
                }
                self.blocks.remove(idx);
                return Some((idx - 1, DOC_NO_ITEM, 0));
            }
        };
        self.blocks.remove(idx);
        Some(focus)
    }

    /// Enter num item de lista. Item vazio sai da lista (vira paragrafo), como no site.
    pub fn split_list_item(&mut self, idx: usize, item: usize, at: usize) -> Option<DocFocus> {
        let items = self.blocks.get_mut(idx)?.list_items_mut()?;
        if item >= items.len() {
            return None;
        }
        if items[item].is_empty() {
            return self.exit_list_at(idx, item);
        }
        let (l, r) = split_runs_at(&items[item], at);
        items[item] = l;
        items.insert(item + 1, r);
        Some((idx, item + 1, 0))
    }

    /// Backspace no inicio de um item: junta com o item de cima; no primeiro item, tira da lista.
    pub fn list_item_backspace(&mut self, idx: usize, item: usize) -> Option<DocFocus> {
        let items = self.blocks.get_mut(idx)?.list_items_mut()?;
        if item >= items.len() {
            return None;
        }
        if item > 0 {
            let cur = items.remove(item);
            let len = runs_char_len(&items[item - 1]);
            items[item - 1] = concat_runs(&items[item - 1], &cur);
            return Some((idx, item - 1, len));
        }
        self.exit_list_at(idx, 0)
    }

    /// Tira o item `item` da lista `idx` como paragrafo, dividindo a lista se preciso.
    fn exit_list_at(&mut self, idx: usize, item: usize) -> Option<DocFocus> {
        let blk = self.blocks.get_mut(idx)?;
        let mut items = std::mem::take(blk.list_items_mut()?);
        if item >= items.len() {
            *blk.list_items_mut()? = items;
            return None;
        }
        let after: Vec<Vec<DocRun>> = items.split_off(item + 1);
        let runs = items.pop().unwrap_or_default();
        let before = items;
        let make_after = blk.same_list_kind(after.clone());
        let para = DocBlock::Paragraph { align: DocAlign::Left, runs };
        let mut at = idx;
        if before.is_empty() {
            self.blocks[idx] = para;
        } else {
            *self.blocks[idx].list_items_mut()? = before;
            at = idx + 1;
            self.blocks.insert(at, para);
        }
        if !after.is_empty() {
            self.blocks.insert(at + 1, make_after);
        }
        Some((at, DOC_NO_ITEM, 0))
    }

    /// Clique abaixo do texto: continua no ultimo paragrafo ou abre um novo.
    pub fn focus_end_or_append(&mut self) -> DocFocus {
        if let Some(DocBlock::Paragraph { runs, .. }) = self.blocks.last() {
            return (self.blocks.len() - 1, DOC_NO_ITEM, runs_char_len(runs));
        }
        self.blocks.push(DocBlock::Paragraph { align: DocAlign::Left, runs: Vec::new() });
        (self.blocks.len() - 1, DOC_NO_ITEM, 0)
    }
}

fn split_runs_by_newline(runs: Vec<DocRun>) -> Vec<Vec<DocRun>> {
    let mut items: Vec<Vec<DocRun>> = vec![Vec::new()];
    for r in runs {
        let mut first = true;
        for part in r.text.split('\n') {
            if !first {
                items.push(Vec::new());
            }
            first = false;
            if !part.is_empty() {
                items.last_mut().unwrap().push(DocRun {
                    text: part.to_string(),
                    style: r.style.clone(),
                });
            }
        }
    }
    if items.is_empty() {
        items.push(Vec::new());
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    fn para(text: &str) -> DocBlock {
        DocBlock::Paragraph { align: DocAlign::Left, runs: if text.is_empty() { vec![] } else { vec![DocRun::plain(text)] } }
    }

    #[test]
    fn test_split_runs_at_keeps_styles_and_utf8() {
        let bold = InlineStyle { bold: true, ..Default::default() };
        let runs = vec![DocRun::plain("Olá "), DocRun { text: "mundão".into(), style: bold.clone() }];
        let (l, r) = split_runs_at(&runs, 6);
        assert_eq!(runs_plain_text(&l), "Olá mu");
        assert_eq!(runs_plain_text(&r), "ndão");
        assert!(r[0].style.bold && l[1].style.bold);
        assert_eq!(concat_runs(&l, &r), merge_runs(runs.clone()));
        let (l0, r0) = split_runs_at(&runs, 0);
        assert!(l0.is_empty());
        assert_eq!(runs_char_len(&r0), 10);
    }

    #[test]
    fn test_enter_and_backspace_round_trip_paragraphs() {
        let mut doc = DocDocument { blocks: vec![para("abcdef")] };
        assert_eq!(doc.split_text_block(0, 3), Some((1, DOC_NO_ITEM, 0)));
        assert_eq!(serialize_doc_html(&doc), "<p>abc</p><p>def</p>");
        assert_eq!(doc.merge_text_block_back(1), Some((0, DOC_NO_ITEM, 3)));
        assert_eq!(serialize_doc_html(&doc), "<p>abcdef</p>");
        assert_eq!(doc.merge_text_block_back(0), None);
    }

    #[test]
    fn test_heading_enter_at_end_opens_paragraph() {
        let mut doc = parse_doc_html("<h1>Titulo</h1>");
        doc.split_text_block(0, 6);
        assert_eq!(serialize_doc_html(&doc), "<h1>Titulo</h1><p></p>");
        let mut doc = parse_doc_html("<h2>AB</h2>");
        doc.split_text_block(0, 1);
        assert_eq!(serialize_doc_html(&doc), "<h2>A</h2><h2>B</h2>");
    }

    #[test]
    fn test_list_enter_on_empty_item_exits_and_splits_list() {
        let mut doc = parse_doc_html("<ul><li><p>a</p></li><li><p></p></li><li><p>c</p></li></ul>");
        assert_eq!(doc.split_list_item(0, 0, 1), Some((0, 1, 0)));
        assert_eq!(doc.split_list_item(0, 1, 0), Some((1, DOC_NO_ITEM, 0)));
        assert_eq!(
            serialize_doc_html(&doc),
            "<ul><li><p>a</p></li></ul><p></p><ul><li><p></p></li><li><p>c</p></li></ul>"
        );
    }

    #[test]
    fn test_list_backspace_merges_and_outdents_first_item() {
        let mut doc = parse_doc_html("<ol><li><p>a</p></li><li><p>b</p></li></ol>");
        assert_eq!(doc.list_item_backspace(0, 1), Some((0, 0, 1)));
        assert_eq!(serialize_doc_html(&doc), "<ol><li><p>ab</p></li></ol>");
        assert_eq!(doc.list_item_backspace(0, 0), Some((0, DOC_NO_ITEM, 0)));
        assert_eq!(serialize_doc_html(&doc), "<p>ab</p>");
        let mut doc = parse_doc_html("<ul><li><p>x</p></li></ul><p>y</p>");
        assert_eq!(doc.merge_text_block_back(1), Some((0, 0, 1)));
        assert_eq!(serialize_doc_html(&doc), "<ul><li><p>xy</p></li></ul>");
    }

    #[test]
    fn test_parse_and_serialize_golive_doc_html() {
        let html = "<h1>Titulo Principal</h1><p style=\"text-align: center\"><b>Negrito</b> <i>Italico</i> <u>Sub</u> <s>Risco</s> <span style=\"color: #ef4444\">Vermelho</span> <a href=\"https://golive.nemtudo.me\">Site</a></p><ul><li><p>Item 1</p></li><li><p>Item 2</p></li></ul><blockquote><p>Citacao importante</p></blockquote><table><tbody><tr><td><p>A</p></td><td><p>B</p></td></tr></tbody></table><hr>";
        let doc = parse_doc_html(html);
        assert_eq!(doc.blocks.len(), 6);
        assert!(matches!(&doc.blocks[0], DocBlock::Heading { level: 1, .. }));
        assert!(matches!(&doc.blocks[1], DocBlock::Paragraph { align: DocAlign::Center, .. }));
        assert!(matches!(&doc.blocks[2], DocBlock::BulletList { items } if items.len() == 2));
        assert!(matches!(&doc.blocks[3], DocBlock::Blockquote { .. }));
        assert!(matches!(&doc.blocks[4], DocBlock::Table { rows } if rows.len() == 1 && rows[0] == vec!["A", "B"]));
        assert!(matches!(&doc.blocks[5], DocBlock::HorizontalRule));

        let round = serialize_doc_html(&doc);
        let doc2 = parse_doc_html(&round);
        assert_eq!(doc, doc2);
    }

    #[test]
    fn test_toggle_inline_style_on_selection_range() {
        let runs = vec![DocRun::plain("Ola mundo bonito")];
        let toggled = toggle_runs_style(
            &runs,
            4,
            9,
            |s, on| s.bold = on,
            |s| s.bold,
        );
        assert_eq!(toggled.len(), 3);
        assert_eq!(toggled[0].text, "Ola ");
        assert!(!toggled[0].style.bold);
        assert_eq!(toggled[1].text, "mundo");
        assert!(toggled[1].style.bold);
        assert_eq!(toggled[2].text, " bonito");
        assert!(!toggled[2].style.bold);

        // Toggling same range again removes bold and merges back
        let untoggled = toggle_runs_style(&toggled, 4, 9, |s, on| s.bold = on, |s| s.bold);
        assert_eq!(untoggled, vec![DocRun::plain("Ola mundo bonito")]);
    }

    #[test]
    fn test_preserve_styles_on_text_edit() {
        let runs = vec![
            DocRun::plain("Oi "),
            DocRun {
                text: "mundo".to_string(),
                style: InlineStyle {
                    bold: true,
                    ..Default::default()
                },
            },
        ];
        let updated = update_runs_text_preserving_styles(&runs, "Oi mundo!");
        assert_eq!(runs_plain_text(&updated), "Oi mundo!");
        assert!(updated[1].style.bold);
    }

    #[test]
    fn test_utf8_accents_toggle_typing_style_and_word_export() {
        let runs = vec![DocRun::plain("Olá ação incrível!")];
        // "ação" starts at char 4, ends at char 8
        let toggled = toggle_runs_style(&runs, 4, 8, |s, on| s.bold = on, |s| s.bold);
        assert_eq!(toggled[0].text, "Olá ");
        assert_eq!(toggled[1].text, "ação");
        assert!(toggled[1].style.bold);
        assert_eq!(toggled[2].text, " incrível!");

        let pending = InlineStyle {
            italic: true,
            ..Default::default()
        };
        let typed = update_runs_with_typing_style(&toggled, "Olá ação super incrível!", Some(&pending));
        assert_eq!(runs_plain_text(&typed), "Olá ação super incrível!");
        assert!(typed.iter().any(|r| r.text.contains("super") && r.style.italic));

        let word = doc_to_word_html("<p><b>Olá</b> mundo</p>", "Ata <Reuniao>");
        assert!(word.contains("urn:schemas-microsoft-com:office:word"));
        assert!(word.contains("<title>Ata &lt;Reuniao&gt;</title>"));
        assert!(word.contains("<p><b>Olá</b> mundo</p>"));
    }
}
