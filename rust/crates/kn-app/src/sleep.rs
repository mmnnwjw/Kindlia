use crate::{model::NovelKey, Kindlia};
use kn_render::Bitmap;
use kn_ui::{Cx, Page, PageId};
pub fn restore(cx: &mut Cx<Kindlia>) {
    if let Ok(bytes) = std::fs::read(cx.app.repo.store.data_path("sleep_cover.json")) {
        if let Ok(key) = serde_json::from_slice(&bytes) {
            prepare(cx, key);
        }
    }
}
pub fn prepare(cx: &mut Cx<Kindlia>, key: NovelKey) {
    if !cx.app.config.sleep_cover || cx.app.sleep_request.as_ref() == Some(&key) {
        return;
    }
    cx.app.sleep_request = Some(key.clone());
    let repo = cx.app.repo.clone();
    let size = (cx.width * 9 / 10, cx.height * 9 / 10);
    cx.spawn_app(move || {
        let _ = crate::store::atomic_write(
            &repo.store.data_path("sleep_cover.json"),
            &serde_json::to_vec(&key).unwrap_or_default(),
            true,
        );
        let image = crate::covers::for_book(&repo, &key, size)
            .ok()
            .map(|b| b.dithered());
        Box::new(move |app: &mut Kindlia| {
            if app.sleep_request.as_ref() == Some(&key) {
                app.sleep_image = image;
            }
        })
    });
}
pub struct SleepPage(pub Option<Bitmap>);
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
        frame.fill_rect(frame.bounds(), cx.theme.background);
        let m = cx.app.metrics;
        let footer = kn_render::Rect::new(
            m.margin as i32,
            (cx.height - 2 * m.touch - m.margin) as i32,
            cx.width - 2 * m.margin,
            2 * m.touch,
        );
        if let Some(image) = &self.0 {
            let image = image.fit_within(cx.width - 2 * m.margin, footer.y.max(1) as u32);
            frame.blit(
                &image,
                image.bounds(),
                (cx.width as i32 - image.width() as i32) / 2,
                (footer.y - image.height() as i32) / 2,
            );
        } else {
            kn_ui::widgets::card(
                frame,
                &cx.theme,
                &m,
                kn_render::Rect::new(
                    m.margin as i32,
                    (cx.height / 4) as i32,
                    cx.width - 2 * m.margin,
                    cx.height / 3,
                ),
            );
        }
        let last = cx
            .app
            .repo
            .store
            .shelf()
            .ok()
            .and_then(|v| v.into_iter().find(|b| b.progress.is_some()));
        let title = last.as_ref().map(|b| b.title.as_str()).unwrap_or("Kindlia");
        let status = last
            .as_ref()
            .and_then(|b| b.progress.as_ref())
            .map(|p| {
                let page = if p.pages > 0 {
                    format!("{}/{}", p.page, p.pages)
                } else {
                    p.page.to_string()
                };
                if p.chapter_title.is_empty() {
                    format!("{page} · 休眠中")
                } else {
                    format!("{} · {page} · 休眠中", p.chapter_title)
                }
            })
            .unwrap_or_else(|| "休眠中".into());
        let mut ink = kn_ui::widgets::Ink {
            fonts: cx.fonts,
            glyphs: cx.glyphs,
            chain: &cx.app.ui_fonts,
        };
        let title = ink.fit(&title, m.title, footer.w as f32);
        ink.text_centered(
            frame,
            kn_render::Rect::new(footer.x, footer.y, footer.w, m.touch),
            &title,
            m.title,
            cx.theme.foreground,
        );
        ink.text_centered(
            frame,
            kn_render::Rect::new(footer.x, footer.y + m.touch as i32, footer.w, m.touch),
            &ink.fit(&status, m.small, footer.w as f32),
            m.small,
            cx.theme.muted,
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
                                    if e.file_name().to_string_lossy().contains(".tmp-kindlia-")
                                        && m.modified()
                                            .ok()
                                            .and_then(|t| t.elapsed().ok())
                                            .is_some_and(|age| age.as_secs() > 86400)
                                    {
                                        let _ = std::fs::remove_file(e.path());
                                        continue;
                                    }
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
