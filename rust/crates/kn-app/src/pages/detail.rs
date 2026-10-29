//! 书籍详情布局移植自 KinNovel pages/book.rs：封面 26%、信息右栏、简介及底部操作栏。
use super::*;
use kn_render::{Bitmap, Rect};
use kn_ui::{Cx, HitId};
pub struct Detail<'a> {
    pub title: &'a str,
    pub author: &'a str,
    pub meta: &'a [String],
    pub intro: &'a str,
    pub notice: &'a str,
}
pub fn cover_rect(cx: &Cx<Kindlia>) -> Rect {
    let m = cx.app.metrics;
    let w = (cx.width as f32 * 0.26) as u32;
    Rect::new(
        m.margin as i32,
        m.header_h() as i32 + m.margin as i32 / 2,
        w,
        (w as f32 * 1.42) as u32,
    )
}
pub fn render(
    cx: &mut Cx<Kindlia>,
    frame: &mut Bitmap,
    detail: Detail<'_>,
    cover: Option<&Bitmap>,
    actions: &[(&str, u32, bool)],
    intro_hit: u32,
    _refresh_hit: Option<u32>,
) {
    header(cx, frame, "书籍详情", true);
    let theme = cx.theme;
    let m = cx.app.metrics;
    let chain = cx.app.ui_fonts.clone();
    let side = m.margin as i32;
    let r = cover_rect(cx);
    let mut ink = Ink {
        fonts: cx.fonts,
        glyphs: cx.glyphs,
        chain: &chain,
    };
    match cover {
        Some(img) => frame.blit(
            img,
            img.bounds(),
            r.x + (r.w as i32 - img.width() as i32) / 2,
            r.y + (r.h as i32 - img.height() as i32) / 2,
        ),
        None => {
            frame.fill_rect(r, theme.mid);
            let inner = Rect::new(
                r.x + 2,
                r.y + 2,
                r.w.saturating_sub(4),
                r.h.saturating_sub(4),
            );
            frame.fill_rect(inner, theme.background);
            ink.text_centered(frame, r, "暂无封面", m.small, theme.muted);
        }
    }
    let info_x = r.right() + side;
    let info_w = (cx.width as i32 - info_x - side).max(0) as f32;
    let mut y = r.y;
    // 保持原版标题和作者区域，长标题在右侧有限区域内折行。
    for line in wrap_plain(detail.title, &ink, m.title, info_w, 3) {
        ink.text(frame, info_x, y, &line, m.title, theme.foreground);
        y += (m.title * 1.3) as i32;
    }
    let author = ink.fit(
        if detail.author.is_empty() {
            "未知作者"
        } else {
            detail.author
        },
        m.small,
        info_w,
    );
    ink.text(frame, info_x, y, &author, m.small, theme.foreground);
    if !detail.author.is_empty() {
        cx.hits.add(HitId(19),Rect::new(info_x,y,info_w.max(1.) as u32,(m.small*1.5) as u32));
    }
    y += (m.small * 1.5) as i32;
    for text in detail.meta.iter().take(4) {
        if text.starts_with("标签：") && text != "标签：" {
            cx.hits.add(
                HitId(18),
                Rect::new(info_x, y, info_w.max(1.) as u32, (m.small * 1.5) as u32),
            );
        }
        let text = ink.fit(text, m.small, info_w);
        ink.text(frame, info_x, y, &text, m.small, theme.muted);
        y += (m.small * 1.5) as i32;
    }
    let gap = side / 3;
    let btn_h = m.touch as i32;
    let bottom_y = cx.height as i32 - btn_h - side / 2;
    let status_y = bottom_y - side / 3;
    let intro_y = (r.bottom() + side / 2).max(y + side / 2);
    ink.text(frame, side, intro_y, "简介", m.body, theme.foreground);
    let mut iy = intro_y + (m.body * 1.4) as i32;
    let line_h = (m.small * 1.4) as i32;
    let max_lines = ((status_y - side / 2 - iy) / line_h).max(0) as usize;
    for line in wrap_plain(
        detail.intro,
        &ink,
        m.small,
        (cx.width - 2 * m.margin) as f32,
        max_lines,
    ) {
        ink.text(frame, side, iy, &line, m.small, theme.muted);
        iy += line_h;
    }
    cx.hits.add(
        HitId(intro_hit),
        Rect::new(
            side,
            intro_y,
            cx.width - 2 * m.margin,
            (status_y - intro_y).max(0) as u32,
        ),
    );
    if !detail.notice.is_empty()
        && (detail.notice.contains("失败")
            || detail.notice.contains("错误")
            || detail.notice.contains("中…"))
    {
        let notice = ink.fit(detail.notice, m.tiny, (cx.width - 2 * m.margin) as f32);
        ink.text(
            frame,
            side,
            bottom_y - (m.tiny * 1.4) as i32,
            &notice,
            m.tiny,
            theme.muted,
        );
    }
    let cols = actions.len() as i32;
    let col_w = (cx.width as i32 - 2 * side - gap * (cols - 1)) / cols;
    for (i, (label, id, enabled)) in actions.iter().enumerate() {
        let rect = Rect::new(
            side + i as i32 * (col_w + gap),
            bottom_y,
            col_w as u32,
            btn_h as u32,
        );
        widgets::button(
            &mut ink,
            frame,
            &theme,
            &m,
            rect,
            label,
            if !enabled {
                ButtonStyle::Disabled
            } else if i == 0 {
                ButtonStyle::Primary
            } else {
                ButtonStyle::Secondary
            },
        );
        cx.hits
            .add(HitId(*id), rect)
            .rounded(m.radius)
            .enabled(*enabled);
    }
}
// KinNovel 简介的有限行折行与省略策略，字宽通过同一个 Ink 测量。
fn wrap_plain(text: &str, ink: &Ink, size: f32, max_width: f32, max_lines: usize) -> Vec<String> {
    if max_lines == 0 {
        return vec![];
    }
    let mut lines = vec![];
    let mut current = String::new();
    let mut width = 0.;
    for ch in text.trim().chars() {
        if ch == '\r' {
            continue;
        }
        let cw = ink.width(&ch.to_string(), size);
        if ch == '\n' || (!current.is_empty() && width + cw > max_width) {
            lines.push(std::mem::take(&mut current));
            width = 0.;
            if lines.len() == max_lines {
                if let Some(last) = lines.last_mut() {
                    last.push('…');
                    *last = ink.fit(last, size, max_width);
                }
                return lines;
            }
            if ch == '\n' {
                continue;
            }
        }
        current.push(ch);
        width += cw;
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}
