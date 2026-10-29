//! Centered modal selectors, sharing KinNovel's rounded card geometry.
use super::*;
use kn_platform::InputEvent;
use kn_render::{Bitmap, Rect};
pub enum Event {
    Pending,
    Cancel,
    Selected(usize),
}
pub struct Choice {
    title: String,
    options: Vec<String>,
    selected: Option<usize>,
    page: usize,
    backdrop: Option<Bitmap>,
}
impl Choice {
    pub fn new(
        title: &str,
        options: impl IntoIterator<Item = impl Into<String>>,
        selected: Option<usize>,
    ) -> Self {
        Self {
            title: title.into(),
            options: options.into_iter().map(Into::into).collect(),
            selected,
            page: 0,
            backdrop: None,
        }
    }
    pub fn wants_backdrop(&self) -> kn_ui::Backdrop {
        if self.backdrop.is_none() {
            kn_ui::Backdrop::Screen
        } else {
            kn_ui::Backdrop::None
        }
    }
    pub fn set_backdrop(&mut self, image: Option<Bitmap>) {
        self.backdrop = image;
    }
    fn rows(cx: &Cx<Kindlia>) -> usize {
        let m = cx.app.metrics;
        (cx.height.saturating_sub(6 * m.margin + 3 * m.touch) / m.touch).max(1) as usize
    }
    pub fn render(&mut self, cx: &mut Cx<Kindlia>, frame: &mut Bitmap) {
        if let Some(bg) = &self.backdrop {
            frame.blit(bg, bg.bounds(), 0, 0);
        }
        cx.hits.clear();
        cx.hits.add(HitId(1), frame.bounds()).no_feedback();
        let m = cx.app.metrics;
        let rows = Self::rows(cx);
        let pages = self.options.len().div_ceil(rows).max(1);
        self.page = self.page.min(pages - 1);
        let visible = self
            .options
            .len()
            .saturating_sub(self.page * rows)
            .min(rows);
        let has_cancel = self
            .options
            .iter()
            .skip(self.page * rows)
            .take(rows)
            .any(|text| text == "取消");
        let w = cx.width * 4 / 5;
        let h = 2 * m.margin
            + m.touch * (visible as u32 + 1 + u32::from(!has_cancel) + u32::from(pages > 1));
        let r = Rect::new(
            ((cx.width - w) / 2) as i32,
            ((cx.height - h) / 2) as i32,
            w,
            h,
        );
        frame.rounded_rect(
            r,
            m.radius,
            Some(cx.theme.background),
            Some(cx.theme.foreground),
            3,
        );
        cx.hits.add(HitId(u32::MAX), r).no_feedback();
        label(
            cx,
            frame,
            Rect::new(
                r.x + m.margin as i32,
                r.y + m.margin as i32,
                w - 2 * m.margin,
                m.touch,
            ),
            &self.title,
            false,
        );
        let x = r.x + m.margin as i32;
        let mut y = r.y + m.margin as i32 + m.touch as i32;
        for (i, text) in self
            .options
            .iter()
            .enumerate()
            .skip(self.page * rows)
            .take(rows)
        {
            selected_button(
                cx,
                frame,
                Rect::new(x, y, w - 2 * m.margin, m.touch),
                text,
                710_000 + i as u32,
                self.selected == Some(i),
            );
            y += m.touch as i32;
        }
        if pages > 1 {
            let bw = (w - 2 * m.margin) / 3;
            button(
                cx,
                frame,
                Rect::new(x, y, bw, m.touch),
                "上一页",
                2,
                self.page > 0,
            );
            label(
                cx,
                frame,
                Rect::new(x + bw as i32, y, bw, m.touch),
                &format!("{}/{}", self.page + 1, pages),
                true,
            );
            button(
                cx,
                frame,
                Rect::new(x + 2 * bw as i32, y, bw, m.touch),
                "下一页",
                3,
                self.page + 1 < pages,
            );
            y += m.touch as i32;
        }
        if !has_cancel {
            button(
                cx,
                frame,
                Rect::new(x, y, w - 2 * m.margin, m.touch),
                "取消",
                1,
                true,
            );
        }
    }
    pub fn input(&mut self, cx: &mut Cx<Kindlia>, event: &InputEvent) -> Event {
        match hit(cx, event) {
            Some(1) => return Event::Cancel,
            Some(2) => self.page = self.page.saturating_sub(1),
            Some(3) if self.page + 1 < self.options.len().div_ceil(Self::rows(cx)) => {
                self.page += 1
            }
            Some(id) if id >= 710_000 && id - 710_000 < self.options.len() as u32 => {
                return Event::Selected((id - 710_000) as usize)
            }
            _ => return Event::Pending,
        }
        cx.request_redraw(kn_ui::RefreshHint::Ui);
        Event::Pending
    }
}
