use super::*;
use crate::{api::ListQuery, Kindlia};
use kn_platform::InputEvent;
use kn_render::{Bitmap, Rect};
use kn_ui::{Cx, Page, PageId, RefreshHint, Transition};
pub struct FilterPage {
    query: ListQuery,
    providers: u8,
    picker: Option<(usize, super::choice::Choice)>,
}
impl FilterPage {
    pub fn new(query: ListQuery) -> Self {
        let providers = [
            "syosetu",
            "kakuyomu",
            "hameln",
            "pixiv",
            "alphapolis",
            "novelup",
        ]
        .iter()
        .enumerate()
        .fold(0, |v, (i, p)| {
            if query.provider.split(',').any(|s| s == *p) {
                v | (1 << i)
            } else {
                v
            }
        });
        Self {
            query,
            providers,
            picker: None,
        }
    }
}
impl Page<Kindlia> for FilterPage {
    fn id(&self) -> PageId {
        PageId(54)
    }
    fn wants_backdrop(&self) -> kn_ui::Backdrop {
        self.picker.as_ref().map(|(_,p)| p).map(|p| p.wants_backdrop()).unwrap_or(kn_ui::Backdrop::None)
    }
    fn set_backdrop(&mut self, bg: Option<Bitmap>) {
        if let Some(p) = self.picker.as_mut().map(|(_,p)| p) { p.set_backdrop(bg); }
    }
    fn render(&mut self, cx: &mut Cx<Kindlia>, f: &mut Bitmap) {
        if let Some((_, picker)) = &mut self.picker {
            picker.render(cx, f);
            return;
        }
        header(cx, f, "搜索与筛选", true);
        let m = cx.app.metrics;
        let w = cx.width;
        let y = m.header_h();
        for (i, name) in [
            "syosetu",
            "kakuyomu",
            "hameln",
            "pixiv",
            "alphapolis",
            "novelup",
        ]
        .iter()
        .enumerate()
        {
            selected_button(
                cx,
                f,
                Rect::new(
                    (i as u32 % 2 * w / 2) as i32,
                    (y + i as u32 / 2 * m.touch) as i32,
                    w / 2,
                    m.touch,
                ),
                &format!(
                    "{} {name}",
                    if self.providers & (1 << i) != 0 {
                        "✓"
                    } else {
                        "□"
                    }
                ),
                20 + i as u32,
                self.providers & (1 << i) != 0,
            );
        }
        let labels = [
            format!(
                "类型：{}",
                ["全部", "连载中", "已完结", "短篇"][self.query.novel_type as usize]
            ),
            format!(
                "分级：{}",
                ["全部", "一般向", "R18"][self.query.level.min(2) as usize]
            ),
            format!(
                "翻译：{}",
                ["全部", "GPT", "Sakura"][self.query.translate as usize]
            ),
            format!(
                "排序：{}",
                ["更新", "点击", "相关"][self.query.sort as usize]
            ),
        ];
        for (i, t) in labels.iter().enumerate() {
            row(
                cx,
                f,
                Rect::new(
                    0,
                    (y + m.touch * 3 + i as u32 * m.row_h()) as i32,
                    w,
                    m.row_h(),
                ),
                t,
                "点按选择",
                "",
                Some(40 + i as u32),
            );
        }
        button(
            cx,
            f,
            Rect::new(0, cx.height.saturating_sub(m.touch) as i32, w, m.touch),
            "应用筛选",
            10,
            true,
        );
    }
    fn on_input(&mut self, cx: &mut Cx<Kindlia>, e: &InputEvent) -> Transition<Kindlia> {
        // 按下反馈由运行时负责；等完整手势再修改页面，避免按下/抬起各整页刷新一次。
        if matches!(e, InputEvent::Gesture(g) if g.kind == kn_platform::GestureKind::Down) {
            return Transition::None;
        }
        if let Some((field, picker)) = &mut self.picker {
            match picker.input(cx, e) {
                super::choice::Event::Selected(n) => {
                    match *field {
                        0 => self.query.novel_type = n as u8,
                        1 => self.query.level = n as u8,
                        2 => self.query.translate = n as u8,
                        _ => self.query.sort = n as u8,
                    }
                    self.picker = None;
                    cx.request_redraw(RefreshHint::Clean);
                }
                super::choice::Event::Cancel => {
                    self.picker = None;
                    cx.request_redraw(RefreshHint::Clean);
                }
                _ => {}
            }
            return Transition::None;
        }
        match hit(cx, e) {
            Some(1) => return Transition::Back,
            Some(10) => {
                self.query.provider = [
                    "syosetu",
                    "kakuyomu",
                    "hameln",
                    "pixiv",
                    "alphapolis",
                    "novelup",
                ]
                .iter()
                .enumerate()
                .filter(|(i, _)| self.providers & (1 << i) != 0)
                .map(|(_, p)| *p)
                .collect::<Vec<_>>()
                .join(",");
                cx.app.discover_query = Some(self.query.clone());
                return Transition::Back;
            }
            Some(id) if (20..26).contains(&id) => self.providers ^= 1 << (id - 20),
            Some(id) if (40..44).contains(&id) => {
                let field = (id - 40) as usize;
                let (name, options, selected) = options(field + 1, &self.query);
                self.picker = Some((
                    field,
                    super::choice::Choice::new(&name, options, Some(selected)),
                ));
            }
            _ => {}
        }
        cx.request_redraw(RefreshHint::Ui);
        Transition::None
    }
}

pub fn options(field: usize, query: &ListQuery) -> (String, Vec<String>, usize) {
    let (name, labels, selected): (&str, &[&str], usize) = match field {
        0 => (
            "来源",
            &[
                "全部站点",
                "syosetu",
                "kakuyomu",
                "hameln",
                "pixiv",
                "alphapolis",
                "novelup",
                "自定义多个站点",
            ],
            [
                "",
                "syosetu",
                "kakuyomu",
                "hameln",
                "pixiv",
                "alphapolis",
                "novelup",
            ]
            .iter()
            .position(|s| *s == query.provider)
            .unwrap_or(0),
        ),
        1 => (
            "类型",
            &["全部", "连载中", "已完结", "短篇"],
            query.novel_type as usize,
        ),
        2 => ("分级", &["全部", "一般向", "R18"], query.level as usize),
        3 => ("翻译", &["全部", "GPT", "Sakura"], query.translate as usize),
        _ => ("排序", &["更新", "点击", "相关"], query.sort as usize),
    };
    (
        name.into(),
        labels.iter().map(|s| s.to_string()).collect(),
        selected.min(labels.len() - 1),
    )
}
pub fn apply(field: usize, selected: usize, query: &mut ListQuery) {
    match field {
        0 => {
            query.provider = [
                crate::api::DEFAULT_PROVIDERS,
                "syosetu",
                "kakuyomu",
                "hameln",
                "pixiv",
                "alphapolis",
                "novelup",
            ][selected.min(6)]
            .into()
        }
        1 => query.novel_type = selected as u8,
        2 => query.level = selected as u8,
        3 => query.translate = selected as u8,
        _ => query.sort = selected as u8,
    }
}
