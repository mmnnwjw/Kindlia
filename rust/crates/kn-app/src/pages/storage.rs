//! KN-style asynchronous cache accounting; user settings and progress are never cleared.
use super::*;
use kn_platform::InputEvent;
use kn_render::{Bitmap, Rect};
use kn_ui::{Cx, Page, PageId, RefreshHint, Transition};
use std::{
    any::Any,
    path::{Path, PathBuf},
};
const WEB: &[&str] = &[
    "chapters",
    "novels",
    "lists",
    "cloud",
    "images",
    "book-meta",
];
const WENKU: &[&str] = &["epubs", "epub-index"];
struct ResultMessage(Result<[u64; 2], String>);
#[derive(Default)]
pub struct StoragePage {
    sizes: Option<[u64; 2]>,
    busy: bool,
    notice: String,
    confirm: Option<(Option<usize>, choice::Choice)>,
}
fn dirs(root: &Path, kind: usize) -> Vec<PathBuf> {
    let mut result = vec![];
    for ns in std::fs::read_dir(root.join("cache"))
        .into_iter()
        .flatten()
        .flatten()
    {
        if !ns.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        for name in if kind == 0 { WEB } else { WENKU } {
            result.push(ns.path().join(name));
        }
    }
    result
}
fn size(path: &Path) -> u64 {
    let mut stack = vec![path.to_owned()];
    let mut total = 0;
    while let Some(p) = stack.pop() {
        if let Ok(entries) = std::fs::read_dir(p) {
            for e in entries.flatten() {
                if let Ok(t) = e.file_type() {
                    if t.is_dir() {
                        stack.push(e.path());
                    } else if t.is_file() {
                        total += e.metadata().map(|m| m.len()).unwrap_or(0);
                    }
                }
            }
        }
    }
    total
}
fn measure(root: &Path) -> [u64; 2] {
    [0, 1].map(|i| dirs(root, i).iter().map(|p| size(p)).sum())
}
fn human(n: u64) -> String {
    format!("{:.1} MB", n as f64 / 1024. / 1024.)
}
impl StoragePage {
    fn update(&mut self, cx: &mut Cx<Kindlia>, clear: Option<Option<usize>>) {
        self.busy = true;
        let root = cx.app.repo.store.root.clone();
        cx.spawn_local(move || {
            ResultMessage((|| {
                if let Some(which) = clear {
                    // Preserve origins of existing downloads for later re-downloads, outside cache/.
                    for ns in std::fs::read_dir(root.join("cache"))
                        .into_iter()
                        .flatten()
                        .flatten()
                    {
                        for e in std::fs::read_dir(ns.path().join("epub-source"))
                            .into_iter()
                            .flatten()
                            .flatten()
                        {
                            if e.file_type().is_ok_and(|t| t.is_file()) {
                                let destination = root.join("data").join(ns.file_name()).join(
                                    format!("epub-source-{}", e.file_name().to_string_lossy()),
                                );
                                crate::store::atomic_write(
                                    &destination,
                                    &std::fs::read(e.path()).map_err(|e| e.to_string())?,
                                    true,
                                )
                                .map_err(|e| e.to_string())?;
                            }
                        }
                    }
                    for k in 0..2 {
                        if which.is_none() || which == Some(k) {
                            for path in dirs(&root, k) {
                                if path.exists() {
                                    std::fs::remove_dir_all(&path).map_err(|e| e.to_string())?;
                                }
                            }
                        }
                    }
                }
                Ok(measure(&root))
            })())
        });
    }
}
impl Page<Kindlia> for StoragePage {
    fn id(&self) -> PageId {
        PageId(53)
    }
    fn enter(&mut self, cx: &mut Cx<Kindlia>, _: bool) {
        self.update(cx, None);
    }
    fn wants_backdrop(&self) -> kn_ui::Backdrop {
        self.confirm.as_ref().map(|(_,p)| p).map(|p| p.wants_backdrop()).unwrap_or(kn_ui::Backdrop::None)
    }
    fn set_backdrop(&mut self, bg: Option<Bitmap>) {
        if let Some(p) = self.confirm.as_mut().map(|(_,p)| p) { p.set_backdrop(bg); }
    }
    fn render(&mut self, cx: &mut Cx<Kindlia>, f: &mut Bitmap) {
        if let Some((_, c)) = &mut self.confirm {
            c.render(cx, f);
            return;
        }
        header(cx, f, "存储空间", true);
        let m = cx.app.metrics;
        for (i, title) in ["Web小说及公共缓存", "文库小说下载"].iter().enumerate() {
            let y = m.header_h() + i as u32 * (m.row_h() + m.touch);
            row(
                cx,
                f,
                Rect::new(0, y as i32, cx.width, m.row_h()),
                title,
                if i == 0 {
                    "章节、列表、封面与插图"
                } else {
                    "已下载 EPUB 和本地索引"
                },
                &self
                    .sizes
                    .map(|s| human(s[i]))
                    .unwrap_or_else(|| "统计中…".into()),
                None,
            );
            button(
                cx,
                f,
                Rect::new(
                    m.margin as i32,
                    (y + m.row_h()) as i32,
                    cx.width - 2 * m.margin,
                    m.touch,
                ),
                "清除此类缓存",
                10 + i as u32,
                !self.busy,
            );
        }
        let y = m.header_h() + 2 * (m.row_h() + m.touch) + m.margin;
        button(
            cx,
            f,
            Rect::new(m.margin as i32, y as i32, cx.width - 2 * m.margin, m.touch),
            "全部清除",
            12,
            !self.busy,
        );
        label(
            cx,
            f,
            Rect::new(0, (y + m.touch) as i32, cx.width, m.touch),
            if self.notice.is_empty() {
                "保留配置、账号、字体、书架和阅读进度"
            } else {
                &self.notice
            },
            true,
        );
    }
    fn on_input(&mut self, cx: &mut Cx<Kindlia>, e: &InputEvent) -> Transition<Kindlia> {
        // 按下反馈由运行时负责；等完整手势再修改页面，避免按下/抬起各整页刷新一次。
        if matches!(e, InputEvent::Gesture(g) if g.kind == kn_platform::GestureKind::Down) {
            return Transition::None;
        }
        if let Some((which, c)) = &mut self.confirm {
            match c.input(cx, e) {
                choice::Event::Selected(1) => {
                    let which = *which;
                    self.confirm = None;
                    self.update(cx, Some(which));
                }
                choice::Event::Selected(_) | choice::Event::Cancel => self.confirm = None,
                _ => {}
            }
            cx.request_redraw(RefreshHint::Clean);
            return Transition::None;
        }
        match hit(cx, e) {
            Some(1) => return Transition::Back,
            Some(id @ 10..=12) if !self.busy => {
                self.confirm = Some((
                    if id == 12 {
                        None
                    } else {
                        Some((id - 10) as usize)
                    },
                    choice::Choice::new(
                        "清理后离线无法读取，确认清除？",
                        ["取消", "确认清除"],
                        Some(0),
                    ),
                ))
            }
            _ => {}
        }
        cx.request_redraw(RefreshHint::Ui);
        Transition::None
    }
    fn on_message(&mut self, cx: &mut Cx<Kindlia>, msg: Box<dyn Any + Send>) {
        if let Ok(r) = msg.downcast::<ResultMessage>() {
            self.busy = false;
            match r.0 {
                Ok(s) => {
                    self.sizes = Some(s);
                    self.notice.clear();
                }
                Err(e) => self.notice = e,
            }
            cx.request_redraw(RefreshHint::Ui);
        }
    }
}
