//! Centered modal selectors, sharing KinNovel's rounded card geometry.
use super::*;
use kn_platform::InputEvent;
use kn_render::Bitmap;
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
            kn_ui::Backdrop::CleanFrame
        } else {
            kn_ui::Backdrop::None
        }
    }
    pub fn set_backdrop(&mut self, image: Option<Bitmap>) {
        self.backdrop = image;
    }
    fn rows(cx: &Cx<Kindlia>) -> usize {
        widgets::menu_dialog_rows(cx.height, &cx.app.metrics)
    }
    pub fn render(&mut self, cx: &mut Cx<Kindlia>, frame: &mut Bitmap) {
        if let Some(bg) = &self.backdrop {
            frame.blit(bg, bg.bounds(), 0, 0);
        }
        cx.hits.clear();
        let m = cx.app.metrics;
        let rows = Self::rows(cx);
        let pages = self.options.len().div_ceil(rows).max(1);
        self.page = self.page.min(pages - 1);
        let mut ink = Ink {
            fonts: cx.fonts,
            glyphs: cx.glyphs,
            chain: &cx.app.ui_fonts,
        };
        widgets::menu_dialog(
            &mut ink,
            frame,
            cx.hits,
            &cx.theme,
            &m,
            &self.title,
            &self.options,
            self.selected,
            self.page,
            HitId(1),
            HitId(2),
            HitId(3),
            710_000,
        );
    }
    pub fn input(&mut self, cx: &mut Cx<Kindlia>, event: &InputEvent) -> Event {
        match page_hit(cx, event) {
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
