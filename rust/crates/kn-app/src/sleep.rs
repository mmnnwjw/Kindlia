use crate::{model::NovelKey, Kindlia};
use kn_render::Bitmap;
use kn_ui::{Cx, Page, PageId};
pub fn prepare(cx: &mut Cx<Kindlia>, key: NovelKey) {
    if !cx.app.config.sleep_cover || cx.app.sleep_request.as_ref() == Some(&key) {
        return;
    }
    cx.app.sleep_request = Some(key.clone());
    let repo = cx.app.repo.clone();
    let size = (cx.width * 9 / 10, cx.height * 9 / 10);
    cx.spawn_app(move || {
        let image = crate::covers::for_book(&repo, &key, size).ok();
        Box::new(move |app: &mut Kindlia| {
            if app.sleep_request.as_ref() == Some(&key) {
                app.sleep_image = image;
            }
        })
    });
}
pub struct SleepPage(pub Bitmap);
impl Page<Kindlia> for SleepPage {
    fn on_input(
        &mut self,
        _: &mut Cx<Kindlia>,
        _: &kn_platform::InputEvent,
    ) -> kn_ui::Transition<Kindlia> {
        kn_ui::Transition::None
    }
    fn id(&self) -> PageId {
        PageId(59)
    }
    fn render(&mut self, cx: &mut Cx<Kindlia>, frame: &mut Bitmap) {
        frame.fill_rect(frame.bounds(), 255);
        frame.blit(
            &self.0,
            self.0.bounds(),
            (cx.width as i32 - self.0.width() as i32) / 2,
            (cx.height as i32 - self.0.height() as i32) / 2,
        );
    }
}
pub fn prune(root: &std::path::Path, limit_mb: u32) {
    let mut files = vec![];
    let mut total = 0u64;
    for ns in std::fs::read_dir(root.join("cache"))
        .into_iter()
        .flatten()
        .flatten()
    {
        for kind in std::fs::read_dir(ns.path()).into_iter().flatten().flatten() {
            if matches!(
                kind.file_name().to_str(),
                Some("epubs" | "epub-index" | "epub-source")
            ) {
                continue;
            }
            let mut stack = vec![kind.path()];
            while let Some(path) = stack.pop() {
                if let Ok(entries) = std::fs::read_dir(path) {
                    for e in entries.flatten() {
                        if let Ok(t) = e.file_type() {
                            if t.is_dir() {
                                stack.push(e.path());
                            } else if t.is_file() {
                                if let Ok(m) = e.metadata() {
                                    total += m.len();
                                    files.push((
                                        m.modified().unwrap_or(std::time::UNIX_EPOCH),
                                        m.len(),
                                        e.path(),
                                    ));
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    let max = limit_mb.clamp(64, 1024) as u64 * 1024 * 1024;
    files.sort_by_key(|f| f.0);
    for (_, bytes, path) in files {
        if total <= max {
            break;
        }
        if std::fs::remove_file(path).is_ok() {
            total = total.saturating_sub(bytes);
        }
    }
}
