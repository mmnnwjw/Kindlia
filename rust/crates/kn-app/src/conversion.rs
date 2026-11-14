//! Convert text runs once at rebuild time, preserving rich spans and inline-object positions.
use kn_text::Block;
use std::collections::{BTreeMap, HashMap};
use std::sync::OnceLock;

static T2S_DATA: &[u8] = include_bytes!("../data/opencc-t2s.br");
static S2T_DATA: &[u8] = include_bytes!("../data/opencc-s2t.br");

struct Dict {
    entries: HashMap<String, String>,
    max_chars: usize,
}

impl Dict {
    fn from_brotli(compressed: &[u8]) -> Self {
        let mut raw = Vec::new();
        let mut cursor = compressed;
        if let Err(e) = brotli_decompressor::BrotliDecompress(&mut cursor, &mut raw) {
            eprintln!("[convert] dict decompress error: {e}");
            return Self {
                entries: HashMap::new(),
                max_chars: 1,
            };
        }
        let text = String::from_utf8(raw).unwrap_or_default();
        let mut entries = HashMap::new();
        let mut max_chars = 1;
        for line in text.lines() {
            if let Some((k, v)) = line.split_once('\t') {
                max_chars = max_chars.max(k.chars().count());
                entries.entry(k.to_owned()).or_insert_with(|| v.to_owned());
            }
        }
        Self { entries, max_chars }
    }

    fn convert(&self, text: &str) -> String {
        if self.entries.is_empty() || text.is_empty() {
            return text.to_owned();
        }
        let bounds: Vec<usize> = text
            .char_indices()
            .map(|(i, _)| i)
            .chain(std::iter::once(text.len()))
            .collect();
        let count = bounds.len() - 1;
        let mut out = String::with_capacity(text.len());
        let mut i = 0;
        while i < count {
            let mut matched = false;
            for len in (1..=(count - i).min(self.max_chars)).rev() {
                if let Some(v) = self.entries.get(&text[bounds[i]..bounds[i + len]]) {
                    out.push_str(v);
                    i += len;
                    matched = true;
                    break;
                }
            }
            if !matched {
                out.push_str(&text[bounds[i]..bounds[i + 1]]);
                i += 1;
            }
        }
        out
    }
}

fn dict_for(mode: &str) -> Option<&'static Dict> {
    static T2S: OnceLock<Dict> = OnceLock::new();
    static S2T: OnceLock<Dict> = OnceLock::new();
    match mode {
        "t2s" => Some(T2S.get_or_init(|| Dict::from_brotli(T2S_DATA))),
        "s2t" => Some(S2T.get_or_init(|| Dict::from_brotli(S2T_DATA))),
        _ => None,
    }
}

pub fn apply(block: &mut Block, mode: &str) {
    let Some(dict) = dict_for(mode) else {
        return;
    };
    if block.path.starts_with("/jp/") || block.text.is_empty() {
        return;
    }
    let chars: Vec<_> = block.text.chars().collect();
    let mut bounds = vec![0, chars.len()];
    for s in &block.spans {
        bounds.extend([s.start.min(chars.len()), s.end.min(chars.len())]);
    }
    for (i, _) in &block.objects {
        bounds.extend([(*i).min(chars.len()), (i + 1).min(chars.len())]);
    }
    bounds.sort_unstable();
    bounds.dedup();
    let mut positions = BTreeMap::new();
    let mut output = String::with_capacity(block.text.len());
    let mut count = 0;
    let mut span_index = 0;
    for pair in bounds.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        positions.insert(a, count);
        let text: String = chars[a..b].iter().collect();
        while span_index + 1 < block.spans.len() && block.spans[span_index].end <= a {
            span_index += 1;
        }
        let original = block.spans.get(span_index).is_some_and(|s| {
            s.start <= a && s.end >= b && (s.style.muted || s.style.ruby || s.style.ruby_base)
        });
        let text = if original || text.contains(kn_text::OBJECT_CHAR) {
            text
        } else {
            dict.convert(&text)
        };
        count += text.chars().count();
        output.push_str(&text);
        positions.insert(b, count);
    }
    for s in &mut block.spans {
        s.start = *positions.get(&s.start).unwrap_or(&count);
        s.end = *positions.get(&s.end).unwrap_or(&count);
    }
    for (i, _) in &mut block.objects {
        *i = *positions.get(i).unwrap_or(&count);
    }
    block.text = output;
}
