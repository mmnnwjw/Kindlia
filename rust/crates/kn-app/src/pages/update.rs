//! In-app checker and confirmed installation; no KUAL menu entry is added.
use super::*;
use kn_platform::InputEvent;
use kn_render::{Bitmap, Rect};
use kn_ui::{Cx, Page, PageId, RefreshHint, Transition};
use std::any::Any;
struct Checked(Result<crate::update::Release, String>);
struct Installed(Result<usize, String>);
struct Progress(usize, Option<usize>);
#[derive(Default)]
pub struct UpdatePage {
    release: Option<crate::update::Release>,
    busy: bool,
    notice: String,
    confirm: Option<choice::Choice>,
    installed: bool,
    installing: bool,
    notes_page: usize,
    notes_layout: Option<(u32, u32, Vec<(String, u8)>)>,
}
impl UpdatePage {
    pub fn with_release(release: crate::update::Release) -> Self {
        Self {
            release: Some(release),
            ..Default::default()
        }
    }
    fn check(&mut self, cx: &Cx<Kindlia>) {
        self.busy = true;
        self.notes_page = 0;
        self.notes_layout = None;
        self.notice = "正在检查 GitHub / R2…".into();
        let config = cx.app.config.clone();
        let store = cx.app.repo.store.clone();
        if let Some(s) = cx.spawner() {
            s.spawn_on(kn_ui::Lane::Background, true, move || {
                Checked(crate::update::check(&config, &store))
            });
        }
    }
}
impl Page<Kindlia> for UpdatePage {
    fn id(&self) -> PageId {
        PageId(137)
    }
    fn enter(&mut self, cx: &mut Cx<Kindlia>, _: bool) {
        if self.release.is_none() && !self.busy {
            self.check(cx);
        }
    }
    fn wants_backdrop(&self) -> kn_ui::Backdrop {
        self.confirm
            .as_ref()
            .map(|c| c.wants_backdrop())
            .unwrap_or(kn_ui::Backdrop::None)
    }
    fn set_backdrop(&mut self, bg: Option<Bitmap>) {
        if let Some(c) = &mut self.confirm {
            c.set_backdrop(bg);
        }
    }
    fn render(&mut self, cx: &mut Cx<Kindlia>, f: &mut Bitmap) {
        if let Some(c) = &mut self.confirm {
            c.render(cx, f);
            return;
        }
        header(cx, f, "检查更新", true);
        let m = cx.app.metrics;
        let theme = cx.theme.clone();
        let side = m.margin as i32;
        let width = cx.width - 2 * m.margin;
        let mut ink = kn_ui::widgets::Ink {
            fonts: cx.fonts,
            glyphs: cx.glyphs,
            chain: &cx.app.ui_fonts,
        };
        let row_h = (m.body * 1.7) as i32;
        let pad = m.margin as i32 / 2;
        let card = Rect::new(
            side,
            (m.header_h() + m.margin) as i32,
            width,
            (2 * row_h + pad) as u32,
        );
        kn_ui::widgets::card(f, &theme, &m, card);
        for (i, (key, value)) in [
            ("当前版本", env!("CARGO_PKG_VERSION").to_owned()),
            (
                "最新版本",
                if self.busy && !self.installing {
                    "正在检查…".into()
                } else if let Some(r) = &self.release {
                    format!("{} · 来自 {}", r.version, r.source)
                } else {
                    "检查失败".into()
                },
            ),
        ]
        .iter()
        .enumerate()
        {
            let y = card.y + pad / 2 + i as i32 * row_h + (row_h - (m.body * 1.25) as i32) / 2;
            ink.text(f, card.x + pad, y, key, m.body, theme.muted);
            let value = ink.fit(value, m.body, width as f32 * 0.68);
            let w = ink.width(&value, m.body).ceil() as i32;
            ink.text(
                f,
                card.right() - pad - w,
                y,
                &value,
                m.body,
                theme.foreground,
            );
        }
        let newer = self
            .release
            .as_ref()
            .is_some_and(|r| crate::update::newer(&r.version));
        use kn_ui::widgets::ButtonStyle as B;
        let buttons: Vec<(&str, u32, B)> = if self.installed {
            vec![("完成，退出应用", 2, B::Primary)]
        } else if self.installing {
            vec![("正在下载并安装…", 0, B::Disabled)]
        } else if self.busy {
            vec![("返回", 1, B::Secondary)]
        } else if newer {
            vec![("以后再说", 1, B::Secondary), ("下载并安装", 2, B::Primary)]
        } else {
            vec![("返回", 1, B::Primary), ("重新检查", 3, B::Secondary)]
        };
        let button_y = (cx.height - m.margin - m.touch) as i32;
        let gap = m.margin / 2;
        let count = buttons.len() as u32;
        let bw = (width - gap * (count - 1)) / count;
        for (i, (text, id, style)) in buttons.iter().enumerate() {
            let rect = Rect::new(side + (i as u32 * (bw + gap)) as i32, button_y, bw, m.touch);
            kn_ui::widgets::button(&mut ink, f, &theme, &m, rect, text, *style);
            if *id != 0 {
                cx.hits.add(kn_ui::HitId(*id), rect).rounded(m.radius);
            }
        }
        let mut y = card.bottom() + m.margin as i32;
        let bottom = button_y - pad;
        let line_h = (m.small * 1.6) as i32;
        let status = if self.busy || !self.notice.is_empty() {
            self.notice.as_str()
        } else if newer {
            "发现新版本；安装将保留配置和阅读数据"
        } else {
            "已是最新版本"
        };
        for line in wrap(status, &ink, m.small, width as f32)
            .into_iter()
            .take(3)
        {
            ink.text(f, side, y, &line, m.small, theme.muted);
            y += line_h;
        }
        if self.busy || self.installed {
            return;
        }
        let Some(r) = &self.release else {
            return;
        };
        if self
            .notes_layout
            .as_ref()
            .is_none_or(|(w, size, _)| *w != width || *size != m.small.to_bits())
        {
            let changes = r.changes_since(env!("CARGO_PKG_VERSION"));
            let mut lines = vec![];
            if newer && changes.len() > 1 {
                for l in wrap(
                    &format!(
                        "从 {} 更新到 {}，包含 {} 个版本的改动",
                        env!("CARGO_PKG_VERSION"),
                        r.version,
                        changes.len()
                    ),
                    &ink,
                    m.small,
                    width as f32,
                ) {
                    lines.push((l, 2));
                }
            }
            for change in changes {
                let notes = crate::update_notes::plain_notes(&change.notes);
                if notes.is_empty() {
                    continue;
                }
                if !lines.is_empty() {
                    lines.push((String::new(), 0));
                }
                lines.push((
                    if change.date.is_empty() {
                        format!("v{}", change.version)
                    } else {
                        format!("v{} · {}", change.version, change.date)
                    },
                    1,
                ));
                for (text, heading) in notes {
                    for line in wrap(&text, &ink, m.small, width as f32) {
                        lines.push((line, if heading { 2 } else { 0 }));
                    }
                }
            }
            self.notes_layout = Some((width, m.small.to_bits(), lines));
        }
        y += pad;
        let lines = &self.notes_layout.as_ref().unwrap().2;
        if lines.is_empty() {
            ink.text(f, side, y, "此版本未提供功能变更说明", m.small, theme.muted);
            return;
        }
        let all_rows = ((bottom - y) / line_h).max(1) as usize;
        let needs_pager = lines.len() > all_rows;
        let rows = if needs_pager {
            ((bottom - y - m.touch as i32) / line_h).max(1) as usize
        } else {
            all_rows
        };
        let pages = lines.len().div_ceil(rows).max(1);
        self.notes_page = self.notes_page.min(pages - 1);
        for (line, style) in lines.iter().skip(self.notes_page * rows).take(rows) {
            ink.text(
                f,
                side,
                y,
                line,
                if *style == 1 { m.body } else { m.small },
                if *style == 2 {
                    theme.muted
                } else {
                    theme.foreground
                },
            );
            y += line_h;
        }
        if pages > 1 {
            kn_ui::widgets::pager(
                &mut ink,
                f,
                cx.hits,
                &theme,
                &m,
                Rect::new(side, bottom - m.touch as i32, width, m.touch),
                self.notes_page,
                pages,
                kn_ui::HitId(6),
                kn_ui::HitId(7),
            );
        }
    }
    fn on_input(&mut self, cx: &mut Cx<Kindlia>, e: &InputEvent) -> Transition<Kindlia> {
        if let Some(c) = &mut self.confirm {
            match c.input(cx, e) {
                choice::Event::Selected(1) => {
                    self.confirm = None;
                    self.busy = true;
                    self.installing = true;
                    self.notice = "正在下载并校验签名…".into();
                    let r = self.release.clone().unwrap();
                    let root = cx.app.repo.store.root.clone();
                    let poster = cx.page_poster();
                    if let Some(s) = cx.spawner() {
                        s.spawn_on(kn_ui::Lane::Background, false, move || {
                            let last = std::sync::Mutex::new(std::time::Instant::now());
                            let downloaded = crate::update::download(&r, |n, t| {
                                let mut at = last.lock().unwrap();
                                if at.elapsed() >= std::time::Duration::from_millis(500) {
                                    *at = std::time::Instant::now();
                                    poster.post(Progress(n, t));
                                }
                            });
                            Installed(downloaded.and_then(|bytes| {
                                crate::update_install::install(&bytes, &r.version, &root)
                            }))
                        });
                    }
                }
                choice::Event::Cancel | choice::Event::Selected(_) => self.confirm = None,
                _ => {}
            }
            cx.request_redraw(RefreshHint::Ui);
            return Transition::None;
        }
        match hit(cx, e) {
            Some(1) if !self.installing => return Transition::Back,
            Some(2) if self.installed => return Transition::Exit,
            Some(2)
                if !self.busy
                    && self
                        .release
                        .as_ref()
                        .is_some_and(|r| crate::update::newer(&r.version)) =>
            {
                self.confirm = Some(choice::Choice::new(
                    "安装更新？配置与数据将保留",
                    ["取消", "安装"],
                    Some(0),
                ))
            }
            Some(3) if !self.busy => self.check(cx),
            Some(6) => self.notes_page = self.notes_page.saturating_sub(1),
            Some(7) => self.notes_page = self.notes_page.saturating_add(1),
            _ => {
                if !self.busy {
                    if let InputEvent::Gesture(g) = e {
                        match g.kind {
                            kn_platform::GestureKind::SwipeLeft => {
                                self.notes_page = self.notes_page.saturating_add(1)
                            }
                            kn_platform::GestureKind::SwipeRight => {
                                self.notes_page = self.notes_page.saturating_sub(1)
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        cx.request_redraw(RefreshHint::Ui);
        Transition::None
    }
    fn on_message(&mut self, cx: &mut Cx<Kindlia>, msg: Box<dyn Any + Send>) {
        let msg = match msg.downcast::<Checked>() {
            Ok(r) => {
                self.busy = false;
                self.notes_page = 0;
                self.notes_layout = None;
                match r.0 {
                    Ok(release) => {
                        self.release = Some(release);
                        self.notice.clear();
                    }
                    Err(e) => self.notice = e,
                };
                cx.request_redraw(RefreshHint::Ui);
                return;
            }
            Err(m) => m,
        };
        let msg = match msg.downcast::<Progress>() {
            Ok(p) => {
                self.notice = match p.1 {
                    Some(t) if t > 0 => format!("正在下载 {}%", p.0 * 100 / t),
                    _ => format!("正在下载 {} KB", p.0 / 1024),
                };
                cx.request_redraw(RefreshHint::Ui);
                return;
            }
            Err(m) => m,
        };
        if let Ok(r) = msg.downcast::<Installed>() {
            self.busy = false;
            self.installing = false;
            match r.0 {
                Ok(_) => {
                    self.installed = true;
                    self.notice = "安装完成；退出后从 KUAL 重新打开 Kindlia".into();
                }
                Err(e) => self.notice = format!("更新失败：{e}"),
            };
            cx.request_redraw(RefreshHint::Ui);
        }
    }
}

/// KinNovel announcement/update wrapping, including hanging Chinese punctuation.
fn wrap(text: &str, ink: &kn_ui::widgets::Ink, size: f32, width: f32) -> Vec<String> {
    const NO_LINE_START: &str = "，。、；：！？）」』】》〉”’…—,.;:!?)";
    let mut lines = vec![];
    for segment in text.split('\n') {
        let mut line = String::new();
        for c in segment.chars() {
            let mut candidate = line.clone();
            candidate.push(c);
            if !line.is_empty() && ink.width(&candidate, size) > width && !NO_LINE_START.contains(c)
            {
                lines.push(std::mem::take(&mut line));
            }
            line.push(c);
        }
        lines.push(line);
    }
    lines
}
