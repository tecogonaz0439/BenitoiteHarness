//! 再帰を使わず HTML の本文を読みやすい文字にする（設計書 01-03「web_fetch」）。

use super::url::{self, Url};

const KEPT_BYTES: usize = 51200;

// 入れ子のリストの字下げは入力より大きくなりうる。先頭だけを保持し、残りは
// 量だけ数えて、入力の長さに比例する処理と小さなメモリ使用量を保つ（設計書 01-03「web_fetch」）。
#[derive(Default)]
struct Writer {
    text: String,
    bytes: usize,
    last: Option<char>,
    space: bool,
    pending_line: bool,
    full: bool,
}

impl Writer {
    fn raw(&mut self, c: char) {
        self.bytes = self.bytes.saturating_add(c.len_utf8());
        if !self.full && self.text.len().saturating_add(c.len_utf8()) <= KEPT_BYTES {
            self.text.push(c);
        } else {
            self.full = true;
        }
        self.last = Some(c);
    }
    fn emit(&mut self, c: char) {
        if self.pending_line {
            self.pending_line = false;
            self.raw('\n');
        }
        self.raw(c);
    }
    fn write(&mut self, value: &str, pre: bool) {
        for c in value.chars() {
            if c.is_control() && c != '\n' && c != '\t' {
                continue;
            }
            if pre {
                self.space = false;
                self.emit(c);
            } else if c.is_whitespace() {
                self.space = true;
            } else {
                if self.space
                    && self.last.is_some_and(|c| c != '\n' && c != ' ')
                    && !self.pending_line
                {
                    self.emit(' ');
                }
                self.space = false;
                self.emit(c);
            }
        }
    }
    fn line(&mut self) {
        self.space = false;
        self.pending_line = self.last.is_some_and(|c| c != '\n');
    }
    fn literal(&mut self, value: &str) {
        self.space = false;
        for c in value.chars() {
            if !c.is_control() || c == '\n' || c == '\t' {
                self.emit(c);
            }
        }
    }
    fn indent(&mut self, count: usize) {
        if count == 0 {
            return;
        }
        if self.pending_line {
            self.pending_line = false;
            self.raw('\n');
        }
        let keep = if self.full {
            0
        } else {
            KEPT_BYTES.saturating_sub(self.text.len()).min(count)
        };
        self.text.push_str(&" ".repeat(keep));
        if keep < count {
            self.full = true;
        }
        self.bytes = self.bytes.saturating_add(count);
        self.last = Some(' ');
    }
}

struct Tag<'a> {
    name: String,
    closing: bool,
    empty: bool,
    attributes: &'a str,
}

fn tag_start(input: &str) -> bool {
    let rest = input.strip_prefix("</").or_else(|| input.strip_prefix('<'));
    rest.and_then(|s| s.bytes().next())
        .is_some_and(|b| b.is_ascii_alphabetic())
}

fn tag_end(input: &str) -> Option<usize> {
    let mut quote = None;
    input.char_indices().skip(1).find_map(|(index, c)| {
        if let Some(current) = quote {
            if c == current {
                quote = None;
            }
        } else if c == '\'' || c == '"' {
            quote = Some(c);
        } else if c == '>' {
            return Some(index.saturating_add(1));
        }
        None
    })
}

fn tag(input: &str) -> Option<Tag<'_>> {
    let inner = input.strip_prefix('<')?.strip_suffix('>')?.trim_end();
    let closing = inner.starts_with('/');
    let inner = inner.strip_prefix('/').unwrap_or(inner);
    let length = inner
        .bytes()
        .take_while(|b| b.is_ascii_alphanumeric() || *b == b'-')
        .count();
    let (name, attributes) = inner.split_at(length);
    if !attributes.is_empty()
        && !attributes.starts_with(|c: char| c.is_ascii_whitespace() || c == '/')
    {
        return None;
    }
    Some(Tag {
        name: name.to_ascii_lowercase(),
        closing,
        empty: inner.ends_with('/'),
        attributes,
    })
}

fn raw_text(name: &str) -> bool {
    matches!(name, "script" | "style" | "iframe" | "noscript")
}

fn excluded_empty(tag: &Tag<'_>) -> bool {
    // HTML の非 void 要素は '/>' でも内容が続く。svg の空要素と embed だけを例外にする。
    tag.empty && matches!(tag.name.as_str(), "svg" | "embed")
}

fn raw_content<'a>(input: &'a str, name: &str) -> Option<(&'a str, &'a str)> {
    let mut position = 0_usize;
    while let Some(offset) = input.get(position..)?.find('<') {
        position = position.saturating_add(offset);
        let candidate = input.get(position..)?.strip_prefix("</");
        if let Some(candidate) = candidate
            && candidate
                .get(..name.len())
                .is_some_and(|s| s.eq_ignore_ascii_case(name))
        {
            let after_name = candidate.get(name.len()..)?;
            let after_space = after_name.trim_start_matches(|c: char| c.is_ascii_whitespace());
            if after_space.starts_with('>') {
                let length = 2_usize
                    .saturating_add(name.len())
                    .saturating_add(after_name.len().saturating_sub(after_space.len()))
                    .saturating_add(1);
                return Some((
                    input.get(..position)?,
                    input.get(position.saturating_add(length)..)?,
                ));
            }
        }
        position = position.saturating_add(1);
    }
    None
}

fn attribute<'a>(mut input: &'a str, name: &str) -> Option<&'a str> {
    while !input.is_empty() {
        input = input.trim_start_matches(|c: char| c.is_ascii_whitespace() || c == '/');
        let length = input
            .bytes()
            .take_while(|b| !b.is_ascii_whitespace() && *b != b'=')
            .count();
        let (key, rest) = input.split_at(length);
        input = rest.trim_start();
        if let Some(rest) = input.strip_prefix('=') {
            input = rest.trim_start();
            let value;
            if let Some(quote @ ('\'' | '"')) = input.chars().next() {
                input = input.get(1..)?;
                let end = input.find(quote)?;
                value = input.get(..end)?;
                input = input.get(end.saturating_add(1)..)?;
            } else {
                let end = input.find(char::is_whitespace).unwrap_or(input.len());
                value = input.get(..end)?;
                input = input.get(end..)?;
            }
            if key.eq_ignore_ascii_case(name) {
                return Some(value);
            }
        } else if length == 0 {
            return None;
        }
    }
    None
}

fn excluded(name: &str) -> bool {
    matches!(
        name,
        "script"
            | "style"
            | "noscript"
            | "template"
            | "svg"
            | "head"
            | "iframe"
            | "object"
            | "embed"
            | "canvas"
    )
}

fn block(name: &str) -> bool {
    matches!(
        name,
        "p" | "div"
            | "br"
            | "tr"
            | "section"
            | "article"
            | "header"
            | "footer"
            | "blockquote"
            | "hr"
            | "main"
            | "aside"
            | "nav"
            | "dl"
            | "dt"
            | "dd"
            | "figure"
            | "figcaption"
            | "address"
    )
}

pub(super) fn convert(mut input: &str, base: &Url) -> super::content::ConvertedText {
    let mut output = Writer::default();
    let mut title = Writer::default();
    let mut title_seen = false;
    let mut suppressed: Vec<String> = Vec::new();
    let mut pre = 0_usize;
    let mut lists = 0_usize;
    let mut cell = false;
    let mut links = Vec::new();
    while !input.is_empty() {
        // raw-text はコメントや宣言に見える文字列も本文とする。対応する閉じタグだけを先に探す。
        if let Some(name) = suppressed.last().filter(|name| raw_text(name)) {
            let Some((_, rest)) = raw_content(input, name) else {
                break;
            };
            input = rest;
            suppressed.pop();
            continue;
        }
        if !input.starts_with('<') {
            let end = input.find('<').unwrap_or(input.len());
            let (chunk, rest) = input.split_at(end);
            if suppressed.is_empty() {
                output.write(&entities(chunk), pre > 0);
            }
            input = rest;
            continue;
        }
        if input.starts_with("<!--") || input.starts_with("<![CDATA[") {
            let (prefix, close) = if input.starts_with("<!--") {
                (4_usize, "-->")
            } else {
                (9, "]]>")
            };
            let rest = input.get(prefix..).unwrap_or_default();
            input = rest
                .find(close)
                .and_then(|end| rest.get(end.saturating_add(close.len())..))
                .unwrap_or_default();
            continue;
        }
        if input.starts_with("<!") || input.starts_with("<?") {
            input = input
                .find('>')
                .and_then(|end| input.get(end.saturating_add(1)..))
                .unwrap_or_default();
            continue;
        }
        if !tag_start(input) {
            if suppressed.is_empty() {
                output.write("<", pre > 0);
            }
            input = input.get(1..).unwrap_or_default();
            continue;
        }
        // 閉じていない引用符を繰り返し走査せず、未完成のタグの内容は本文へ出さない。
        let Some(length) = tag_end(input) else {
            break;
        };
        let (markup, rest) = input.split_at(length);
        input = rest;
        let Some(tag) = tag(markup) else {
            continue;
        };
        let name = tag.name.as_str();
        if let Some(hidden) = suppressed.last() {
            if name == hidden && tag.closing {
                suppressed.pop();
                continue;
            }
            if hidden == "head" && name == "body" && !tag.closing {
                suppressed.pop();
            } else if !(hidden == "head" && name == "title" && !tag.closing) {
                if !tag.closing && excluded(name) && !excluded_empty(&tag) {
                    suppressed.push(tag.name);
                }
                continue;
            }
        }
        if excluded(name) {
            if !tag.closing && !excluded_empty(&tag) {
                suppressed.push(tag.name);
            }
            continue;
        }
        if name == "title" && !tag.closing {
            // 題は閉じタグがあった場合だけ採用する。中の '<' では通常のタグの解析に戻らない。
            let Some((contents, rest)) = raw_content(input, "title") else {
                break;
            };
            if !title_seen {
                title.write(&entities(contents), false);
                title_seen = true;
            }
            input = rest;
            continue;
        }
        if block(name) {
            output.line();
            if name == "tr" {
                cell = false;
            }
        }
        match name {
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                output.line();
                if !tag.closing {
                    let level = name
                        .as_bytes()
                        .get(1)
                        .copied()
                        .unwrap_or(b'1')
                        .saturating_sub(b'0');
                    output.literal(&format!("{} ", "#".repeat(usize::from(level))));
                }
            }
            "ul" | "ol" => {
                output.line();
                lists = if tag.closing {
                    lists.saturating_sub(1)
                } else {
                    lists.saturating_add(1)
                };
            }
            "li" => {
                output.line();
                if !tag.closing {
                    output.indent(lists.saturating_sub(1).saturating_mul(2));
                    output.literal("- ");
                }
            }
            "td" | "th" if !tag.closing => {
                if cell {
                    output.literal(" | ");
                }
                cell = true;
            }
            "pre" => {
                output.line();
                pre = if tag.closing {
                    pre.saturating_sub(1)
                } else {
                    pre.saturating_add(1)
                };
            }
            "a" => {
                if tag.closing {
                    if let Some(Some(target)) = links.pop() {
                        output.literal(&format!("]({target})"));
                    }
                } else {
                    let target = attribute(tag.attributes, "href")
                        .and_then(|href| url::link(base, &entities(href)));
                    if target.is_some() {
                        output.write("[", false);
                    }
                    links.push(target);
                }
            }
            _ => {}
        }
    }
    if title.bytes == 0 {
        super::content::ConvertedText {
            text: output.text,
            bytes: output.bytes,
        }
    } else {
        let combined = if title.bytes > title.text.len() {
            format!("# {}", title.text)
        } else {
            format!("# {}\n\n{}", title.text, output.text)
        };
        super::content::ConvertedText {
            text: crate::tools::truncate::prefix(&combined, KEPT_BYTES).to_owned(),
            bytes: title.bytes.saturating_add(4).saturating_add(output.bytes),
        }
    }
}

pub(super) fn entities(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut remaining = input;
    while let Some(start) = remaining.find('&') {
        let (before, rest) = remaining.split_at(start);
        output.push_str(before);
        let rest = rest.strip_prefix('&').unwrap_or_default();
        let length = rest
            .bytes()
            .take_while(|b| b.is_ascii_alphanumeric() || *b == b'#')
            .count();
        let (name, after) = rest.split_at(length);
        if let Some(after) = after.strip_prefix(';') {
            let decoded = match name {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" | "#39" => Some('\''),
                "nbsp" => Some('\u{a0}'),
                _ if name.starts_with('#') => {
                    let number = name
                        .strip_prefix("#x")
                        .or_else(|| name.strip_prefix("#X"))
                        .map_or_else(
                            || name.strip_prefix('#').and_then(|s| s.parse::<u32>().ok()),
                            |s| u32::from_str_radix(s, 16).ok(),
                        );
                    Some(
                        number
                            .filter(|&n| n != 0)
                            .and_then(char::from_u32)
                            .unwrap_or('\u{fffd}'),
                    )
                }
                _ => None,
            };
            if let Some(c) = decoded {
                output.push(c);
            } else {
                output.push('&');
                output.push_str(name);
                output.push(';');
            }
            remaining = after;
        } else {
            output.push('&');
            output.push_str(name);
            remaining = after;
        }
    }
    output.push_str(remaining);
    output
}
