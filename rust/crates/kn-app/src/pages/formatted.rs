//! Centered read-only formatted document: comment syntax guide and draft preview.
use super::*;
use crate::comment_format::{self, Action, Line};
use kn_platform::InputEvent;
use kn_render::{Bitmap, Rect};
use kn_ui::{Cx, Page, PageId, RefreshHint, Transition};
use std::collections::HashSet;
pub const HELP:&str="**粗体文字**\n*斜体文字*\n~~删除的文字~~\n`代码`\n[链接文字](https://example.com)\nhttps://example.com\n- 无序列表项\n1. 有序列表项\n1. 下一项\n直接换行\n换行后的文字\n!!剧透内容!!\n::: star 4.5\n::: details 点击展开\n折叠内容\n:::";
pub struct FormattedPage {
    title: String,
    body: String,
    lines: Vec<Line>,
    page: usize,
    expanded: HashSet<String>,
    actions: Vec<Action>,
    bg: Option<Bitmap>,
}
impl FormattedPage {
    pub fn new(title: &str, body: &str) -> Self {
        Self {
            title: title.into(),
            body: body.into(),
            lines: vec![],
            page: 0,
            expanded: Default::default(),
            actions: vec![],
            bg: None,
        }
    }
}
impl Page<Kindlia> for FormattedPage {
    fn id(&self) -> PageId {
        PageId(138)
    }
    fn wants_backdrop(&self) -> kn_ui::Backdrop {
        if self.bg.is_none() {
            kn_ui::Backdrop::Below
        } else {
            kn_ui::Backdrop::None
        }
    }
    fn set_backdrop(&mut self, bg: Option<Bitmap>) {
        self.bg = bg;
    }
    fn flash_on_enter(&self) -> bool {
        false
    }
    fn render(&mut self, cx: &mut Cx<Kindlia>, f: &mut Bitmap) {
        if let Some(bg) = &self.bg {
            f.blit(bg, bg.bounds(), 0, 0);
        }
        cx.hits.clear();
        let m = cx.app.metrics;
        let w = cx.width * 4 / 5;
        let line_h = (m.body * 1.45).ceil() as u32;
        let max_body = cx.height.saturating_sub(6 * m.margin + 3 * m.touch);
        let rows = (max_body / line_h).max(1) as usize;
        let mut ink = kn_ui::widgets::Ink {
            fonts: cx.fonts,
            glyphs: cx.glyphs,
            chain: &cx.app.ui_fonts,
        };
        if self.lines.is_empty() {
            self.lines = comment_format::layout(
                &self.body,
                "document",
                &self.expanded,
                &ink,
                m.body,
                (w - 2 * m.margin) as f32,
            );
        }
        let pages = self.lines.len().div_ceil(rows).max(1);
        self.page = self.page.min(pages - 1);
        let count = self.lines.iter().skip(self.page * rows).take(rows).count();
        let h = 2 * m.margin + 2 * m.touch + count.max(1) as u32 * line_h;
        let r = Rect::new(
            ((cx.width - w) / 2) as i32,
            ((cx.height - h) / 2) as i32,
            w,
            h,
        );
        f.rounded_rect(
            r,
            m.radius,
            Some(cx.theme.background),
            Some(cx.theme.foreground),
            3,
        );
        ink.text_centered(
            f,
            Rect::new(r.x, r.y + m.margin as i32, w, m.touch),
            &self.title,
            m.body,
            cx.theme.foreground,
        );
        self.actions.clear();
        for (i, line) in self
            .lines
            .iter()
            .skip(self.page * rows)
            .take(rows)
            .enumerate()
        {
            for (rect, action) in comment_format::draw(
                &mut ink,
                f,
                line,
                r.x + m.margin as i32,
                r.y + (m.margin + m.touch + i as u32 * line_h) as i32,
                m.body,
                cx.theme.foreground,
                cx.theme.background,
            ) {
                let id = 500_000 + self.actions.len() as u32;
                self.actions.push(action);
                cx.hits.add(kn_ui::HitId(id), rect);
            }
        }
        let y = r.bottom() - m.margin as i32 - m.touch as i32;
        let bw = (w - 2 * m.margin) / 3;
        button(
            cx,
            f,
            Rect::new(r.x + m.margin as i32, y, bw, m.touch),
            "上一页",
            2,
            self.page > 0,
        );
        button(
            cx,
            f,
            Rect::new(r.x + m.margin as i32 + bw as i32, y, bw, m.touch),
            "关闭",
            1,
            true,
        );
        button(
            cx,
            f,
            Rect::new(r.x + m.margin as i32 + 2 * bw as i32, y, bw, m.touch),
            "下一页",
            3,
            self.page + 1 < pages,
        );
    }
    fn on_input(&mut self, cx: &mut Cx<Kindlia>, e: &InputEvent) -> Transition<Kindlia> {
        match hit(cx, e) {
            Some(1) => return Transition::Back,
            Some(2) => self.page = self.page.saturating_sub(1),
            Some(3) => self.page += 1,
            Some(id) if id >= 500_000 => {
                if let Some(a) = self.actions.get((id - 500_000) as usize).cloned() {
                    match a {
                        Action::Toggle(key) => {
                            if !self.expanded.remove(&key) {
                                self.expanded.insert(key);
                            }
                            self.lines.clear();
                        }
                        Action::Link(url) => {
                            return Transition::Push(Box::new(text::TextPage::new(
                                "链接地址",
                                &url,
                            )))
                        }
                    }
                }
            }
            _ => {}
        }
        cx.request_redraw(RefreshHint::Ui);
        Transition::None
    }
}
