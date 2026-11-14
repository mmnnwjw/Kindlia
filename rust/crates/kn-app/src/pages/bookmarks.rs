use super::*;
use crate::{model::NovelKey, Kindlia};
use kn_platform::{GestureKind, InputEvent};
use kn_render::{Bitmap, Point, Rect};
use kn_ui::{Cx, Page, PageId, RefreshHint, Transition};
pub struct BookmarksPage {
    key: NovelKey,
    from_catalog: bool,
    items: Vec<(String, String, usize)>,
    labels: Vec<(String, String)>,
    page: usize,
    rows: usize,
    confirm: Option<(usize, choice::Choice)>,
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
            labels: vec![],
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
        let store = &cx.app.repo.store;
        let novel = store.read_cache::<crate::model::Novel>("novels", &self.key);
        let epub = store.read_cache::<crate::epub::Book>("epub-index", &self.key);
        self.labels = self
            .items
            .iter()
            .enumerate()
            .map(|(i, entry)| {
                store.bookmark_info(&self.key, entry).unwrap_or_else(|| {
                    let title = novel
                        .as_ref()
                        .and_then(|n| {
                            n.toc
                                .iter()
                                .find(|c| c.chapter_id.as_ref() == Some(&entry.0))
                        })
                        .map(|c| c.title().to_owned())
                        .or_else(|| {
                            epub.as_ref().and_then(|b| {
                                b.navigation
                                    .iter()
                                    .find(|c| c.chapter_id == entry.0)
                                    .map(|c| c.title.clone())
                                    .or_else(|| {
                                        entry
                                            .0
                                            .split('#')
                                            .next()
                                            .and_then(|s| s.parse::<usize>().ok())
                                            .and_then(|i| b.chapters.get(i))
                                            .map(|c| c.title.clone())
                                    })
                            })
                        })
                        .unwrap_or_else(|| format!("书签 {}", i + 1));
                    (title, "已保存的阅读位置 · 长按删除".into())
                })
            })
            .collect();
    }
    fn wants_backdrop(&self) -> kn_ui::Backdrop {
        self.confirm
            .as_ref()
            .map(|(_, c)| c.wants_backdrop())
            .unwrap_or(kn_ui::Backdrop::None)
    }
    fn set_backdrop(&mut self, b: Option<Bitmap>) {
        if let Some((_, c)) = &mut self.confirm {
            c.set_backdrop(b);
        }
    }
    fn render(&mut self, cx: &mut Cx<Kindlia>, f: &mut Bitmap) {
        if let Some((_, c)) = &mut self.confirm {
            c.render(cx, f);
            return;
        }
        header(cx, f, "书签", true);
        let m = cx.app.metrics;
        let y = m.header_h() + m.touch;
        self.rows = (cx.height.saturating_sub(y + m.pager_h()) / m.row_h()).max(1) as usize;
        self.page = self
            .page
            .min(self.items.len().div_ceil(self.rows).max(1) - 1);
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
        for (i, _) in self
            .items
            .iter()
            .skip(self.page * self.rows)
            .take(self.rows)
            .enumerate()
        {
            let info = self
                .labels
                .get(self.page * self.rows + i)
                .cloned()
                .unwrap_or_else(|| ("已保存的书签".into(), String::new()));
            row(
                cx,
                f,
                Rect::new(0, (y + i as u32 * m.row_h()) as i32, cx.width, m.row_h()),
                &info.0,
                &info.1,
                "",
                Some(200 + i as u32),
            );
            cx.hits
                .add(
                    HitId(200 + i as u32),
                    Rect::new(0, (y + i as u32 * m.row_h()) as i32, cx.width, m.row_h()),
                )
                .long_press();
        }
        if self.items.is_empty() {
            label(
                cx,
                f,
                Rect::new(m.margin as i32, y as i32, cx.width - 2 * m.margin, m.touch),
                "暂无书签；在阅读菜单中添加",
                true,
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
        if let Some((i, c)) = &mut self.confirm {
            let i = *i;
            match c.input(cx, e) {
                choice::Event::Selected(1) => {
                    self.items.remove(i);
                    if i < self.labels.len() {
                        self.labels.remove(i);
                    }
                    if let Err(error) = cx.app.repo.store.save_bookmarks(&self.key, &self.items) {
                        eprintln!("[bookmark] {error}");
                        self.items = cx.app.repo.store.bookmarks(&self.key);
                        self.labels.clear();
                    }
                    self.confirm = None;
                }
                choice::Event::Selected(_) | choice::Event::Cancel => self.confirm = None,
                _ => {}
            }
            cx.request_redraw(RefreshHint::Ui);
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
                        self.confirm = Some((
                            i,
                            choice::Choice::new("删除这条书签？", ["取消", "删除"], Some(0)),
                        ));
                    }
                    cx.request_redraw(RefreshHint::Ui);
                    return Transition::None;
                }
            }
        }
        match page_hit(cx, e) {
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
