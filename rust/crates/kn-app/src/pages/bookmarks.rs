use super::*;
use crate::{model::NovelKey, Kindlia};
use kn_platform::{GestureKind, InputEvent};
use kn_render::{Bitmap, Point, Rect};
use kn_ui::{Cx, Page, PageId, RefreshHint, Transition};
pub struct BookmarksPage {
    key: NovelKey,
    from_catalog: bool,
    items: Vec<(String, String, usize)>,
    page: usize,
    rows: usize,
    confirm: Option<usize>,
}
impl BookmarksPage {
    pub fn catalog(key: NovelKey) -> Self {
        Self {
            from_catalog: true,
            ..Self::new(key)
        }
    }
    pub fn new(key: NovelKey) -> Self {
        Self {
            key,
            from_catalog: false,
            items: vec![],
            page: 0,
            rows: 6,
            confirm: None,
        }
    }
}
impl Page<Kindlia> for BookmarksPage {
    fn id(&self) -> PageId {
        PageId(55)
    }
    fn enter(&mut self, cx: &mut Cx<Kindlia>, _: bool) {
        self.items = cx.app.repo.store.bookmarks(&self.key);
    }
    fn render(&mut self, cx: &mut Cx<Kindlia>, f: &mut Bitmap) {
        header(cx, f, "目录", true);
        let m = cx.app.metrics;
        let y = m.header_h() + m.touch;
        self.rows = (cx.height.saturating_sub(y + m.pager_h()) / m.row_h()).max(1) as usize;
        let mut ink = Ink {
            fonts: cx.fonts,
            glyphs: cx.glyphs,
            chain: &cx.app.ui_fonts,
        };
        widgets::segmented(
            &mut ink,
            f,
            cx.hits,
            &cx.theme,
            &m,
            Rect::new(
                m.margin as i32,
                m.header_h() as i32,
                cx.width - 2 * m.margin,
                m.touch,
            ),
            &["目录", "书签"],
            1,
            70,
        );
        for (i, (chapter, path, offset)) in self
            .items
            .iter()
            .skip(self.page * self.rows)
            .take(self.rows)
            .enumerate()
        {
            row(
                cx,
                f,
                Rect::new(0, (y + i as u32 * m.row_h()) as i32, cx.width, m.row_h()),
                &format!("章节 {chapter}"),
                &format!("{path} · {offset}"),
                "",
                Some(200 + i as u32),
            );
        }
        pager(
            cx,
            f,
            self.page,
            self.items.len().div_ceil(self.rows).max(1),
            0,
        );
    }
    fn on_input(&mut self, cx: &mut Cx<Kindlia>, e: &InputEvent) -> Transition<Kindlia> {
        // 按下反馈由运行时负责；等完整手势再修改页面，避免按下/抬起各整页刷新一次。
        if matches!(e, InputEvent::Gesture(g) if g.kind == kn_platform::GestureKind::Down) {
            return Transition::None;
        }
        if let InputEvent::Gesture(g) = e {
            if g.kind == GestureKind::Long {
                if let Some(id) = cx
                    .hits
                    .at(Point {
                        x: g.end.0,
                        y: g.end.1,
                    })
                    .filter(|id| id.0 >= 200)
                {
                    let i = self.page * self.rows + (id.0 - 200) as usize;
                    if i < self.items.len() {
                        if self.confirm == Some(i) {
                            self.items.remove(i);
                            let _ = cx.app.repo.store.save_bookmarks(&self.key, &self.items);
                            self.confirm = None;
                        } else {
                            self.confirm = Some(i);
                        }
                    }
                    cx.request_redraw(RefreshHint::Ui);
                    return Transition::None;
                }
            }
        }
        match hit(cx, e) {
            Some(1) => return Transition::Back,
            Some(70) => {
                return if self.from_catalog {
                    Transition::Back
                } else {
                    Transition::Push(Box::new(book::BookPage::catalog(self.key.clone(), None)))
                }
            }
            Some(2) => self.page = self.page.saturating_sub(1),
            Some(3) if self.page + 1 < self.items.len().div_ceil(self.rows) => self.page += 1,
            Some(id) if id >= 200 => {
                if let Some((chapter, path, offset)) =
                    self.items.get(self.page * self.rows + (id - 200) as usize)
                {
                    cx.app.reader_jump = Some(chapter.clone());
                    cx.app.reader_anchor = Some((path.clone(), *offset));
                    return Transition::Back;
                }
            }
            _ => {}
        }
        cx.request_redraw(RefreshHint::Ui);
        Transition::None
    }
}
