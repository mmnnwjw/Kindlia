use super::*;
use crate::Kindlia;
use kn_platform::InputEvent;
use kn_render::Bitmap;
use kn_ui::{Cx, Page, PageId, RefreshHint, Transition};
pub struct TextPage {
    title: String,
    text: String,
    pages: Vec<kn_text::Page>,
    index: usize,
    backdrop: Option<Bitmap>,
}
impl TextPage {
    pub fn new(title: &str, text: &str) -> Self {
        Self {
            title: title.into(),
            text: text.into(),
            pages: vec![],
            index: 0,
            backdrop: None,
        }
    }
}
impl Page<Kindlia> for TextPage {
    fn id(&self) -> PageId {
        PageId(51)
    }
    fn wants_backdrop(&self) -> kn_ui::Backdrop {
        if self.backdrop.is_none() {
            kn_ui::Backdrop::Below
        } else {
            kn_ui::Backdrop::None
        }
    }
    fn set_backdrop(&mut self, bg: Option<Bitmap>) {
        self.backdrop = bg;
    }
    fn orientation(&self, _: &Kindlia) -> Option<kn_ui::Orientation> {
        None
    }
    fn flash_on_enter(&self) -> bool {
        false
    }
    fn render(&mut self, cx: &mut Cx<Kindlia>, f: &mut Bitmap) {
        if let Some(bg) = &self.backdrop {
            f.blit(bg, bg.bounds(), 0, 0);
        }
        cx.hits.clear();
        cx.hits
            .add(kn_ui::HitId(u32::MAX), f.bounds())
            .no_feedback();
        let m = cx.app.metrics;
        let w = cx.width * 4 / 5;
        let max_body = cx.height.saturating_sub(6 * m.margin + 3 * m.touch);
        if self.pages.is_empty() {
            let blocks: Vec<_> = self
                .text
                .lines()
                .map(|s| kn_text::Block {
                    text: s.into(),
                    ..Default::default()
                })
                .collect();
            let mut cache = Default::default();
            let mut measure = kn_text::FontMeasure::new(cx.fonts, &cx.app.ui_fonts, &mut cache);
            self.pages = kn_text::paginate_all(
                &blocks,
                kn_text::LayoutParams {
                    width: (w - 2 * m.margin) as i64,
                    height: max_body as i64,
                    font_size: m.body as u32,
                    margin: 0,
                    first_line_indent: false,
                    ..Default::default()
                },
                &mut measure,
            );
        }
        let body = self
            .pages
            .get(self.index)
            .into_iter()
            .flatten()
            .filter_map(|item| {
                if let kn_text::PageItem::Text { y, size, .. } = item {
                    Some((*y as u32).saturating_add(*size as u32 * 2))
                } else {
                    None
                }
            })
            .max()
            .unwrap_or(m.touch)
            .min(max_body);
        let h = 2 * m.margin + 2 * m.touch + body;
        let r = kn_render::Rect::new(
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
        label(
            cx,
            f,
            kn_render::Rect::new(r.x, r.y + m.margin as i32, w, m.touch),
            &self.title,
            false,
        );
        if let Some(page) = self.pages.get(self.index) {
            for i in page {
                if let kn_text::PageItem::Text {
                    text, x, y, size, ..
                } = i
                {
                    let mut ink = kn_ui::widgets::Ink {
                        fonts: cx.fonts,
                        glyphs: cx.glyphs,
                        chain: &cx.app.ui_fonts,
                    };
                    ink.text(
                        f,
                        *x as i32 + r.x + m.margin as i32,
                        *y as i32 + r.y + m.margin as i32 + m.touch as i32,
                        text,
                        *size as f32,
                        cx.theme.foreground,
                    );
                }
            }
        }
        let y = r.bottom() - m.margin as i32 - m.touch as i32;
        let inner = w - 2 * m.margin;
        if self.pages.len() > 1 {
            let bw = inner / 3;
            button(
                cx,
                f,
                kn_render::Rect::new(r.x + m.margin as i32, y, bw, m.touch),
                "上一页",
                2,
                self.index > 0,
            );
            button(
                cx,
                f,
                kn_render::Rect::new(r.x + m.margin as i32 + bw as i32, y, bw, m.touch),
                "关闭",
                1,
                true,
            );
            button(
                cx,
                f,
                kn_render::Rect::new(r.x + m.margin as i32 + 2 * bw as i32, y, bw, m.touch),
                "下一页",
                3,
                self.index + 1 < self.pages.len(),
            );
        } else {
            button(
                cx,
                f,
                kn_render::Rect::new(r.x + m.margin as i32, y, inner, m.touch),
                "关闭",
                1,
                true,
            );
        }
    }
    fn on_input(&mut self, cx: &mut Cx<Kindlia>, e: &InputEvent) -> Transition<Kindlia> {
        // 按下反馈由运行时负责；等完整手势再修改页面，避免按下/抬起各整页刷新一次。
        if matches!(e, InputEvent::Gesture(g) if g.kind == kn_platform::GestureKind::Down) {
            return Transition::None;
        }
        match hit(cx, e) {
            Some(1) => return Transition::Back,
            Some(2) => self.index = self.index.saturating_sub(1),
            Some(3) if self.index + 1 < self.pages.len() => self.index += 1,
            _ => {}
        }
        cx.request_redraw(RefreshHint::Ui);
        Transition::None
    }
}
