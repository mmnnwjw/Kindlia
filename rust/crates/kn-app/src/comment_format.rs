//! The source comment Markdown dialect. Raw HTML is always plain text.
use kn_ui::widgets::Ink;
use std::collections::HashSet;
#[derive(Clone, Default, PartialEq)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    pub code: bool,
    pub half: bool,
}
#[derive(Clone, PartialEq)]
pub enum Action {
    Toggle(String),
    Link(String),
}
#[derive(Clone, Default)]
pub struct Run {
    pub text: String,
    pub style: Style,
    pub action: Option<Action>,
    pub hidden: bool,
}
#[derive(Clone, Default)]
pub struct Line {
    pub runs: Vec<Run>,
}
fn inline(
    text: &str,
    style: Style,
    prefix: &str,
    offset: usize,
    expanded: &HashSet<String>,
    depth: usize,
    out: &mut Vec<Run>,
) {
    if depth > 12 {
        out.push(Run {
            text: text.into(),
            style,
            ..Default::default()
        });
        return;
    }
    let mut i = 0;
    while i < text.len() {
        let tail = &text[i..];
        let mut consumed = false;
        if tail.starts_with('\\') && tail.len() > 1 {
            let ch = tail[1..].chars().next().unwrap();
            out.push(Run {
                text: ch.to_string(),
                style: style.clone(),
                ..Default::default()
            });
            i += 1 + ch.len_utf8();
            continue;
        }
        for mark in ["`", "!!", "**", "~~", "*"] {
            if let Some(rest) = tail.strip_prefix(mark) {
                if let Some(end) = rest.find(mark).filter(|n| *n > 0) {
                    let inner = &rest[..end];
                    let mut st = style.clone();
                    match mark {
                        "**" => st.bold = true,
                        "*" => st.italic = true,
                        "~~" => st.strike = true,
                        "`" => st.code = true,
                        _ => {}
                    }
                    if mark == "!!" {
                        let key = format!("{prefix}:{}", offset + i);
                        let shown = expanded.contains(&key);
                        out.push(Run {
                            text: if shown {
                                inner.into()
                            } else {
                                "点击显示剧透".into()
                            },
                            style: st,
                            action: Some(Action::Toggle(key)),
                            hidden: !shown,
                        });
                    } else if mark == "`" {
                        out.push(Run {
                            text: inner.into(),
                            style: st,
                            ..Default::default()
                        });
                    } else {
                        inline(
                            inner,
                            st,
                            prefix,
                            offset + i + mark.len(),
                            expanded,
                            depth + 1,
                            out,
                        );
                    }
                    i += mark.len() * 2 + end;
                    consumed = true;
                    break;
                }
            }
        }
        if consumed {
            continue;
        }
        if let Some(t) = tail.strip_prefix('[') {
            if let Some(end) = t.find("](") {
                let href = &t[end + 2..];
                if let Some(close) = href.find(')') {
                    let url = &href[..close];
                    if safe_link(url) {
                        let mut label = vec![];
                        inline(
                            &t[..end],
                            style.clone(),
                            prefix,
                            offset + i + 1,
                            expanded,
                            depth + 1,
                            &mut label,
                        );
                        for mut run in label {
                            run.action = Some(Action::Link(url.into()));
                            out.push(run);
                        }
                        i += 1 + end + 2 + close + 1;
                        continue;
                    }
                }
            }
        }
        if tail.starts_with("https://") || tail.starts_with("http://") {
            let n = tail
                .find(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"'))
                .unwrap_or(tail.len());
            let url = tail[..n].trim_end_matches(['.', ',', ';', ')', '。', '，', '！']);
            if !url.is_empty() {
                out.push(Run {
                    text: url.into(),
                    style: style.clone(),
                    action: Some(Action::Link(url.into())),
                    hidden: false,
                });
                i += url.len();
                continue;
            }
        }
        let ch = tail.chars().next().unwrap();
        out.push(Run {
            text: ch.to_string(),
            style: style.clone(),
            ..Default::default()
        });
        i += ch.len_utf8();
    }
}
fn safe_link(s: &str) -> bool {
    url::Url::parse(s)
        .ok()
        .is_some_and(|u| matches!(u.scheme(), "http" | "https") && u.host_str().is_some())
}
fn wrap(runs: Vec<Run>, ink: &Ink, size: f32, width: f32) -> Vec<Line> {
    let mut lines = vec![];
    let mut line = Line::default();
    let mut used = 0.;
    for run in runs {
        for ch in run.text.chars() {
            let text = ch.to_string();
            let w = ink.width(&text, size);
            if used + w > width && !line.runs.is_empty() {
                lines.push(std::mem::take(&mut line));
                used = 0.;
            }
            if let Some(last) = line.runs.last_mut().filter(|r| {
                r.style == run.style && r.action == run.action && r.hidden == run.hidden
            }) {
                last.text.push(ch);
            } else {
                line.runs.push(Run {
                    text,
                    style: run.style.clone(),
                    action: run.action.clone(),
                    hidden: run.hidden,
                });
            }
            used += w;
        }
    }
    lines.push(line);
    lines
}
pub fn layout(
    text: &str,
    prefix: &str,
    expanded: &HashSet<String>,
    ink: &Ink,
    size: f32,
    width: f32,
) -> Vec<Line> {
    let mut result = vec![];
    let mut stack = vec![];
    let mut offset = 0;
    let mut ordered: Option<u64> = None;
    for source in text
        .chars()
        .take(32000)
        .collect::<String>()
        .split('\n')
        .take(2000)
    {
        let line = source.trim_end_matches('\r');
        let trimmed = line.trim();
        let visible = stack.iter().all(|x| *x);
        if let Some(title) = trimmed.strip_prefix("::: details") {
            let key = format!("{prefix}:details:{offset}");
            let open = expanded.contains(&key);
            if visible {
                result.extend(wrap(
                    vec![Run {
                        text: format!(
                            "{} {}",
                            if open { "▾" } else { "▸" },
                            if title.trim().is_empty() {
                                "点击展开"
                            } else {
                                title.trim()
                            }
                        ),
                        style: Style {
                            bold: true,
                            ..Default::default()
                        },
                        action: Some(Action::Toggle(key)),
                        hidden: false,
                    }],
                    ink,
                    size,
                    width,
                ));
            }
            stack.push(open);
        } else if trimmed == ":::" && !stack.is_empty() {
            stack.pop();
        } else if visible {
            let mut runs = vec![];
            if let Some(info) = trimmed.strip_prefix("::: star ") {
                let star = (info
                    .trim()
                    .parse::<f32>()
                    .ok()
                    .filter(|v| v.is_finite())
                    .unwrap_or(0.)
                    .clamp(0., 5.)
                    * 2.)
                    .round()
                    / 2.;
                for i in 0..5 {
                    let left = star - i as f32;
                    runs.push(Run {
                        text: if left >= 1. { "★" } else { "☆" }.into(),
                        style: Style {
                            half: left >= 0.5 && left < 1.,
                            ..Default::default()
                        },
                        ..Default::default()
                    });
                }
                runs.push(Run {
                    text: format!("  {star:.1} / 5"),
                    ..Default::default()
                });
                ordered = None;
            } else {
                let unordered = trimmed
                    .strip_prefix("- ")
                    .or_else(|| trimmed.strip_prefix("* "))
                    .or_else(|| trimmed.strip_prefix("+ "));
                let numbered = trimmed
                    .split_once(". ")
                    .and_then(|(n, t)| n.parse::<u64>().ok().map(|n| (n, t)));
                let content = if let Some(t) = unordered {
                    ordered = None;
                    runs.push(Run {
                        text: "• ".into(),
                        ..Default::default()
                    });
                    t
                } else if let Some((start, t)) = numbered {
                    let n = ordered.unwrap_or(start);
                    ordered = Some(n.saturating_add(1));
                    runs.push(Run {
                        text: format!("{n}. "),
                        ..Default::default()
                    });
                    t
                } else {
                    ordered = None;
                    line
                };
                inline(
                    content,
                    Style::default(),
                    prefix,
                    offset,
                    expanded,
                    0,
                    &mut runs,
                );
            }
            result.extend(wrap(runs, ink, size, width));
        }
        offset += source.len() + 1;
    }
    result
}
pub fn draw(
    ink: &mut Ink,
    frame: &mut kn_render::Bitmap,
    line: &Line,
    x: i32,
    y: i32,
    size: f32,
    color: u8,
    background: u8,
) -> Vec<(kn_render::Rect, Action)> {
    use kn_render::{Rect, Synth, TextStyle};
    let mut at = x as f32;
    let mut hits = vec![];
    for run in &line.runs {
        let w = ink.width(&run.text, size);
        let rect = Rect::new(
            at.floor() as i32,
            y,
            w.ceil().max(1.) as u32,
            (size * 1.45).ceil() as u32,
        );
        if run.style.code {
            frame.fill_rect(rect, 220);
        }
        let style = TextStyle {
            fonts: ink.chain,
            size_px: size,
        };
        let ascent = ink.fonts.line_metrics(&style).ascent;
        if run.hidden {
            frame.rounded_rect(rect, 4, Some(color), None, 0);
            ink.glyphs.draw_text(
                ink.fonts,
                frame,
                at,
                y as f32 + ascent,
                &run.text,
                &style,
                background,
            );
        } else {
            ink.glyphs.draw_text_synth(
                ink.fonts,
                frame,
                at,
                y as f32 + ascent,
                &run.text,
                &style,
                color,
                Synth {
                    bold: run.style.bold,
                    italic: run.style.italic,
                },
            );
        }
        if run.style.half {
            let mut star =
                kn_render::Bitmap::new((size * 2.) as u32, (size * 2.) as u32, background);
            ink.glyphs
                .draw_text(ink.fonts, &mut star, 0., ascent, "★", &style, color);
            frame.blit(
                &star,
                Rect::new(0, 0, (w / 2.).ceil() as u32, rect.h),
                rect.x,
                rect.y,
            );
        }
        if run.style.strike {
            frame.fill_rect(
                Rect::new(rect.x, y + (size * 0.55) as i32, rect.w, 2),
                color,
            );
        }
        if matches!(run.action, Some(Action::Link(_))) {
            frame.fill_rect(Rect::new(rect.x, y + size as i32 + 2, rect.w, 1), color);
        }
        if let Some(action) = &run.action {
            hits.push((rect, action.clone()));
        }
        at += w;
    }
    hits
}

/// Plain, bounded list preview. Conceal spoilers and collapsed bodies here too.
pub fn preview(text: &str) -> String {
    let mut out = String::new();
    let mut nesting = 0usize;
    for (i, line) in text.lines().take(1000).enumerate() {
        let t = line.trim();
        if let Some(title) = t.strip_prefix("::: details") {
            if nesting == 0 {
                out.push_str("▸ ");
                out.push_str(title.trim());
                out.push(' ');
            }
            nesting += 1;
            continue;
        }
        if t == ":::" && nesting > 0 {
            nesting -= 1;
            continue;
        }
        if nesting > 0 {
            continue;
        }
        if let Some(star) = t.strip_prefix("::: star ") {
            out.push_str("评分 ");
            out.push_str(star);
            out.push_str(" / 5 ");
            continue;
        }
        let mut runs = vec![];
        inline(
            t,
            Style::default(),
            "preview",
            i,
            &Default::default(),
            0,
            &mut runs,
        );
        for run in runs {
            out.push_str(&run.text);
        }
        out.push(' ');
        if out.chars().count() > 180 {
            break;
        }
    }
    out.chars().take(200).collect()
}
