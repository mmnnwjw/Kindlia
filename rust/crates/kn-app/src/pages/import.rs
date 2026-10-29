//! 源站“本地小说”在 Kindle 上对应指定目录导入 EPUB/TXT。
use super::*;
use crate::{model::NovelKey, Kindlia};
use kn_platform::InputEvent;
use kn_render::{Bitmap, Rect};
use kn_ui::{Cx, Page, PageId, RefreshHint, Transition};
pub struct ImportPage {
    files: Vec<std::path::PathBuf>,
    page: usize,
    rows: usize,
    busy: bool,
    notice: String,
    pending: Option<Transition<Kindlia>>,
}
struct Imported(Result<NovelKey, String>);
impl Default for ImportPage {
    fn default() -> Self {
        Self {
            files: vec![],
            page: 0,
            rows: 6,
            busy: false,
            notice: "请把 EPUB/TXT 放到 /mnt/us/extensions/kindlia/books".into(),
            pending: None,
        }
    }
}
impl Page<Kindlia> for ImportPage {
    fn id(&self) -> PageId {
        PageId(56)
    }
    fn enter(&mut self, cx: &mut Cx<Kindlia>, _: bool) {
        let dir = cx.app.repo.store.root.join("books");
        let _ = std::fs::create_dir_all(&dir);
        self.files = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.is_file()
                    && p.extension().is_some_and(|x| {
                        matches!(
                            x.to_string_lossy().to_ascii_lowercase().as_str(),
                            "epub" | "txt"
                        )
                    })
            })
            .collect();
        self.files.sort();
    }
    fn render(&mut self, cx: &mut Cx<Kindlia>, f: &mut Bitmap) {
        header(cx, f, "导入本地小说", true);
        let m = cx.app.metrics;
        let y = m.header_h() + m.touch;
        self.rows = (cx.height.saturating_sub(y + m.pager_h()) / m.row_h()).max(1) as usize;
        label(
            cx,
            f,
            Rect::new(0, m.header_h() as i32, cx.width, m.touch),
            &self.notice,
            true,
        );
        for (i, p) in self
            .files
            .iter()
            .skip(self.page * self.rows)
            .take(self.rows)
            .enumerate()
        {
            row(
                cx,
                f,
                Rect::new(0, (y + i as u32 * m.row_h()) as i32, cx.width, m.row_h()),
                &p.file_name().unwrap_or_default().to_string_lossy(),
                "点按导入书架并阅读",
                "",
                Some(200 + i as u32),
            );
        }
        pager(
            cx,
            f,
            self.page,
            self.files.len().div_ceil(self.rows).max(1),
            0,
        );
    }
    fn on_input(&mut self, cx: &mut Cx<Kindlia>, e: &InputEvent) -> Transition<Kindlia> {
        // 按下反馈由运行时负责；等完整手势再修改页面，避免按下/抬起各整页刷新一次。
        if matches!(e, InputEvent::Gesture(g) if g.kind == kn_platform::GestureKind::Down) {
            return Transition::None;
        }
        if self.busy {
            return Transition::None;
        }
        match hit(cx, e) {
            Some(1) => return Transition::Back,
            Some(2) => self.page = self.page.saturating_sub(1),
            Some(3) if self.page + 1 < self.files.len().div_ceil(self.rows) => self.page += 1,
            Some(id) if id >= 200 => {
                if let Some(path) = self
                    .files
                    .get(self.page * self.rows + (id - 200) as usize)
                    .cloned()
                {
                    self.busy = true;
                    self.notice = "正在解析本地小说…".into();
                    let store = cx.app.repo.store.clone();
                    cx.spawn_local(move || {
                        Imported((|| {
                            if std::fs::metadata(&path).map_err(|e| e.to_string())?.len()
                                > 64 * 1024 * 1024
                            {
                                return Err("文件超过64 MiB".into());
                            }
                            let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
                            let name = path.file_name().unwrap_or_default().to_string_lossy();
                            use sha2::Digest;
                            let identity = format!("local:{:x}", sha2::Sha256::digest(&bytes));
                            let key = crate::epub::import(&store, &identity, &bytes, &name)?;
                            let book = crate::epub::index(&store, &key)?;
                            store.update_entry(&key, &book.title, |e| e.favorite = true)?;
                            Ok(key)
                        })())
                    });
                }
            }
            _ => {}
        }
        cx.request_redraw(RefreshHint::Ui);
        Transition::None
    }
    fn on_message(&mut self, cx: &mut Cx<Kindlia>, m: Box<dyn std::any::Any + Send>) {
        if let Ok(r) = m.downcast::<Imported>() {
            self.busy = false;
            match r.0 {
                Ok(key) => {
                    self.pending = Some(Transition::Push(Box::new(reader::ReaderPage::new(
                        key,
                        "0".into(),
                    ))))
                }
                Err(e) => self.notice = e,
            }
            cx.request_redraw(RefreshHint::Ui);
        }
    }
    fn take_transition(&mut self) -> Transition<Kindlia> {
        self.pending.take().unwrap_or(Transition::None)
    }
}
