//! 复用 KinNovel 发现分类的列表行；分类和条件来自源站 option.ts。
use super::*;
use kn_platform::InputEvent;
use kn_render::{Bitmap, Rect};
use kn_ui::{Cx, Page, PageId, Transition};
pub struct RankCategories;
const CATEGORIES: [(&str, &str, usize); 4] = [
    ("成为小说家：流派", "syosetu", 1),
    ("成为小说家：综合", "syosetu", 0),
    ("成为小说家：异世界转移/转生", "syosetu", 2),
    ("Kakuyomu：流派", "kakuyomu", 0),
];
impl Page<Kindlia> for RankCategories {
    fn id(&self) -> PageId {
        PageId(13)
    }
    fn render(&mut self, cx: &mut Cx<Kindlia>, f: &mut Bitmap) {
        search_header(cx, f, "发现", false, 20);
        let m = cx.app.metrics;
        let mut ink = kn_ui::widgets::Ink {
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
            &["Web小说", "文库小说", "排行"],
            2,
            50,
        );
        let m = cx.app.metrics;
        for (i, (name, _, _)) in CATEGORIES.iter().enumerate() {
            row(
                cx,
                f,
                Rect::new(
                    0,
                    (m.header_h() + m.touch + i as u32 * m.row_h()) as i32,
                    cx.width,
                    m.row_h(),
                ),
                name,
                "",
                ">",
                Some(100 + i as u32),
            );
        }
        draw_tabs(cx, f, 2);
    }
    fn on_input(&mut self, cx: &mut Cx<Kindlia>, e: &InputEvent) -> Transition<Kindlia> {
        // 按下反馈由运行时负责；等完整手势再修改页面，避免按下/抬起各整页刷新一次。
        if matches!(e, InputEvent::Gesture(g) if g.kind == kn_platform::GestureKind::Down) {
            return Transition::None;
        }
        if let Some(t) = tab_input(cx, e, 2) {
            return t;
        }
        match hit(cx, e) {
            Some(20) => Transition::Push(Box::new(library::LibraryPage::search(String::new()))),
            Some(50) => Transition::Root(Box::new(library::LibraryPage::discover())),
            Some(51) => Transition::Root(Box::new(cloud::CloudPage::root(cloud::Mode::Wenku(
                String::new(),
                0,
            )))),
            Some(52) => Transition::None,
            Some(id) if (100..104).contains(&id) => {
                let (_, p, k) = CATEGORIES[(id - 100) as usize];
                Transition::Push(Box::new(cloud::CloudPage::rank(p, k)))
            }
            _ => Transition::None,
        }
    }
}
