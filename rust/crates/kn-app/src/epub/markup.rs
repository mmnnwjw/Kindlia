//! Resolve EPUB resources and CSS once on the I/O worker, never during pagination.
use super::{resolve, unescape, xml, Archive};
use roxmltree::Node;
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Clone, Copy)]
struct Element<'a, 'b>(Node<'a, 'b>);
impl simplecss::Element for Element<'_, '_> {
    fn parent_element(&self) -> Option<Self> {
        self.0.parent_element().map(Self)
    }
    fn prev_sibling_element(&self) -> Option<Self> {
        self.0
            .prev_siblings()
            .skip(1)
            .find(|n| n.is_element())
            .map(Self)
    }
    fn has_local_name(&self, name: &str) -> bool {
        self.0.has_tag_name(name)
    }
    fn attribute_matches(&self, name: &str, op: simplecss::AttributeOperator<'_>) -> bool {
        self.0.attribute(name).is_some_and(|v| op.matches(v))
    }
    fn pseudo_class_matches(&self, class: simplecss::PseudoClass<'_>) -> bool {
        match class {
            simplecss::PseudoClass::FirstChild => self.prev_sibling_element().is_none(),
            simplecss::PseudoClass::Link => {
                self.0.has_tag_name("a") && self.0.attribute("href").is_some()
            }
            simplecss::PseudoClass::Lang(lang) => self
                .0
                .ancestors()
                .find_map(|n| {
                    n.attribute("lang")
                        .or_else(|| n.attribute(("http://www.w3.org/XML/1998/namespace", "lang")))
                })
                .is_some_and(|l| {
                    l == lang || l.strip_prefix(lang).is_some_and(|r| r.starts_with('-'))
                }),
            _ => false,
        }
    }
}
fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
}
fn media_ok(value: Option<&str>) -> bool {
    value.is_none_or(|s| s.is_empty() || s.split(',').any(|v| matches!(v.trim(), "all" | "screen")))
}
fn noteref(n: Node<'_, '_>) -> bool {
    n.attribute(("http://www.idpf.org/2007/ops", "type"))
        .unwrap_or("")
        .split_whitespace()
        .any(|v| v == "noteref")
        || n.attribute("role") == Some("doc-noteref")
        || n.attribute("class")
            .unwrap_or("")
            .split_whitespace()
            .any(|v| v == "duokan-footnote")
        || n.parent_element().is_some_and(|p| {
            p.has_tag_name("sup")
                && p.attribute("class")
                    .unwrap_or("")
                    .split_whitespace()
                    .any(|v| v == "footnote-ref")
        })
}
pub(super) fn prepare(
    archive: &mut Archive,
    path: &str,
    html: &str,
    original_only: bool,
) -> Result<String, String> {
    let Ok(doc) = xml(html) else {
        return Ok(html.into());
    };
    let mut css = String::new();
    let mut sheets = HashSet::new();
    for n in doc.descendants().filter(|n| n.is_element()) {
        if n.has_tag_name("style") && media_ok(n.attribute("media")) {
            css.push_str(n.text().unwrap_or(""));
            css.push('\n');
        } else if n.has_tag_name("link")
            && media_ok(n.attribute("media"))
            && n.attribute("rel")
                .unwrap_or("")
                .split_whitespace()
                .any(|s| s == "stylesheet")
        {
            if let Some(name) = n.attribute("href").and_then(|h| resolve(path, h).ok()) {
                if sheets.len() < 32 && sheets.insert(name.clone()) {
                    if let Ok(bytes) = archive.read(&name, 512 * 1024) {
                        if let Ok(text) = super::decode_text(bytes) {
                            css.push_str(&text);
                            css.push('\n');
                        }
                    }
                }
            }
        }
        if css.len() > 1024 * 1024 {
            return Err("EPUB 章节样式超过 1 MiB".into());
        }
    }
    let mut sheet = simplecss::StyleSheet::parse(&css);
    sheet.rules.retain(|r| {
        let selector = r.selector.to_string();
        selector.len() <= 1024 && selector.split_whitespace().count() <= 64
    });
    let nodes: Vec<_> = doc.descendants().filter(|n| n.is_element()).collect();
    if nodes.len().saturating_mul(sheet.rules.len()) > 8_000_000 {
        return Err("EPUB 章节样式匹配过于复杂".into());
    }
    let mut edits = vec![];
    // XHTML permits self-closing script/style tags. HTML fragment parsing treats
    // script as raw text and can swallow/escape the rest of the document.
    // CSS has already been collected; omit non-content nodes before HTML parsing.
    let mut omitted: Vec<_> = doc
        .descendants()
        .filter(|n| n.has_tag_name("head") || n.has_tag_name("script") || n.has_tag_name("style"))
        .map(|n| n.range())
        .collect();
    if original_only {
        for n in doc.descendants().filter(|n| {
            n.has_tag_name("p")
                && n.attribute("style")
                    .is_some_and(|s| s.replace(' ', "").contains("opacity:0.4"))
        }) {
            if let Some(prev) = n.prev_siblings().skip(1).find(|n| n.is_element()) {
                if prev.has_tag_name("p") {
                    omitted.push(prev.range());
                }
            }
        }
    }
    omitted.sort_by_key(|r| (r.start, std::cmp::Reverse(r.end)));
    let mut end = 0;
    omitted.retain(|r| {
        if r.start < end {
            false
        } else {
            end = r.end;
            true
        }
    });
    for r in &omitted {
        edits.push((r.clone(), String::new()));
    }
    let mut requests: BTreeMap<String, Vec<(String, usize)>> = BTreeMap::new();
    let mut note_links = vec![];
    for n in nodes {
        if n.ancestors().take(130).count() > 128 || {
            let i = omitted.partition_point(|r| r.start <= n.range().start);
            i > 0 && omitted[i - 1].contains(&n.range().start)
        } {
            continue;
        }
        if n.ancestors().skip(1).any(|a| a.has_tag_name("svg")) {
            continue;
        }
        if n.has_tag_name("svg") {
            let mut images = String::new();
            for image in n.descendants().filter(|i| i.has_tag_name("image")) {
                if let Some(src) = image
                    .attribute("href")
                    .or_else(|| image.attribute(("http://www.w3.org/1999/xlink", "href")))
                {
                    let src = resolve(path, src)
                        .map(|p| {
                            format!(
                                "https://epub.invalid/{}",
                                p.split('/')
                                    .map(crate::api::encode)
                                    .collect::<Vec<_>>()
                                    .join("/")
                            )
                        })
                        .unwrap_or_else(|_| src.into());
                    let dimension = |name| image.attribute(name)
                        .and_then(|s| s.parse::<f64>().ok())
                        .filter(|n| n.is_finite() && *n >= 1.0 && *n <= 1_000_000.0)
                        .map(|n| n.round() as u32);
                    let size = match (dimension("width"), dimension("height")) {
                        (Some(w), Some(h)) => format!(" width=\"{w}\" height=\"{h}\""),
                        _ => String::new(),
                    };
                    images.push_str(&format!("<img src=\"{}\"{size}/>", escape(&src)));
                }
            }
            if !images.is_empty() {
                edits.push((n.range(), images));
            }
            continue;
        }
        let mut normal = String::new();
        let mut important = String::new();
        let append =
            |d: simplecss::Declaration<'_>, normal: &mut String, important: &mut String| {
                if original_only && d.name == "opacity" && d.value.trim() == "0.4" {
                    return;
                }
                let dst = if d.important { important } else { normal };
                dst.push_str(d.name);
                dst.push(':');
                dst.push_str(d.value);
                dst.push(';');
            };
        for rule in &sheet.rules {
            if rule.selector.matches(&Element(n)) {
                for d in &rule.declarations {
                    append(*d, &mut normal, &mut important);
                }
            }
        }
        for d in simplecss::DeclarationTokenizer::from(n.attribute("style").unwrap_or("")) {
            append(d, &mut normal, &mut important);
        }
        normal.push_str(&important);
        if !normal.is_empty() || (original_only && n.attribute("style").is_some()) {
            if normal.len() > 64 * 1024 {
                return Err("EPUB 单元素样式过长".into());
            }
            if let Some(attr) = n.attributes().find(|a| a.name() == "style") {
                edits.push((attr.range_value(), escape(&normal)));
            } else {
                let start = n.range().start + 1;
                let end = html[start..]
                    .find(|c: char| c.is_ascii_whitespace() || c == '/' || c == '>')
                    .unwrap()
                    + start;
                edits.push((end..end, format!(" style=\"{}\"", escape(&normal))));
            }
        }
        if n.has_tag_name("img") {
            if let Some(attr) = n.attributes().find(|a| a.name() == "src") {
                if let Ok(name) = resolve(path, attr.value()) {
                    let encoded = name
                        .split('/')
                        .map(crate::api::encode)
                        .collect::<Vec<_>>()
                        .join("/");
                    edits.push((
                        attr.range_value(),
                        format!("https://epub.invalid/{encoded}"),
                    ));
                }
            }
        }
        if n.has_tag_name("a") && noteref(n) {
            if let Some(attr) = n.attributes().find(|a| a.name() == "href") {
                if let Some((file, fragment)) = attr.value().split_once('#') {
                    if let Ok(name) = resolve(path, file) {
                        if name == path {
                            edits.push((attr.range_value(), format!("#{}", escape(fragment))));
                        } else if note_links.len() < 1024
                            && (requests.contains_key(&name) || requests.len() < 8)
                        {
                            let id = note_links.len();
                            requests
                                .entry(name)
                                .or_default()
                                .push((unescape(fragment), id));
                            note_links.push((attr.range_value(), id));
                        }
                    }
                }
            }
        }
    }
    let mut notes = String::new();
    let mut found = HashSet::new();
    for (name, ids) in requests {
        if let Ok(bytes) = archive.read(&name, 1024 * 1024) {
            if let Ok(content) = super::decode_text(bytes) {
                if let Ok(targets) = xml(&content) {
                    let mut pending: HashMap<&str, Vec<usize>> = HashMap::new();
                    for (fragment, id) in &ids {
                        pending.entry(fragment).or_default().push(*id);
                    }
                    for node in targets.descendants().filter(|n| n.is_element()) {
                        if let Some((_, links)) =
                            node.attribute("id").and_then(|s| pending.remove_entry(s))
                        {
                            let text = node
                                .descendants()
                                .filter(|n| n.is_text())
                                .filter_map(|n| n.text())
                                .collect::<String>();
                            if notes.len() + text.len().saturating_mul(links.len()) > 1024 * 1024 {
                                break;
                            }
                            for id in links {
                                notes.push_str(&format!(
                                    "<aside id=\"kindlia-external-note-{id}\">{}</aside>",
                                    escape(&text)
                                ));
                                found.insert(id);
                            }
                        }
                    }
                }
            }
        }
    }
    for (range, id) in note_links {
        if found.contains(&id) {
            edits.push((range, format!("#kindlia-external-note-{id}")));
        }
    }
    // Stitch source slices once: repeated replace_range would move O(nodes × bytes).
    edits.sort_by_key(|(r, _)| r.start);
    let added = edits.iter().map(|(_, s)| s.len()).sum::<usize>();
    if html.len().saturating_add(added) > 16 * 1024 * 1024 {
        return Err("EPUB 章节展开后过大".into());
    }
    let mut output = String::with_capacity(html.len() + added);
    let mut cursor = 0;
    for (range, value) in edits {
        if range.start < cursor {
            continue;
        }
        output.push_str(&html[cursor..range.start]);
        output.push_str(&value);
        cursor = range.end;
    }
    output.push_str(&html[cursor..]);
    let mut html = output;
    if !notes.is_empty() {
        let at = html.rfind("</body>").unwrap_or(html.len());
        html.insert_str(at, &notes);
    }
    Ok(html)
}
