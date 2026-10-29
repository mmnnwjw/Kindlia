//! Convert text runs once at rebuild time, preserving rich spans and inline-object positions.
use kn_text::Block;
use std::collections::BTreeMap;
pub fn apply(block: &mut Block, mode: &str) {
    let target = match mode {
        "t2s" => zhconv::Variant::ZhHans,
        "s2t" => zhconv::Variant::ZhHant,
        _ => return,
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
    let mut span_index=0;
    for pair in bounds.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        positions.insert(a, count);
        let text: String = chars[a..b].iter().collect();
        while span_index+1<block.spans.len() && block.spans[span_index].end<=a {span_index+=1;}
        let original=block.spans.get(span_index).is_some_and(|s|s.start<=a&&s.end>=b&&(s.style.muted||s.style.ruby||s.style.ruby_base));
        let text = if original || text.contains(kn_text::OBJECT_CHAR) {
            text
        } else {
            zhconv::zhconv(&text, target)
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
