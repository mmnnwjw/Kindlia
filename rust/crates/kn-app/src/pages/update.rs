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
        header(cx, f, "应用更新", true);
        let m = cx.app.metrics;
        let mut y = m.header_h() + m.margin;
        label(
            cx,
            f,
            Rect::new(m.margin as i32, y as i32, cx.width - 2 * m.margin, m.touch),
            &format!("当前版本 {}", env!("CARGO_PKG_VERSION")),
            false,
        );
        y += m.touch;
        if let Some(r) = &self.release {
            label(
                cx,
                f,
                Rect::new(m.margin as i32, y as i32, cx.width - 2 * m.margin, m.touch),
                &format!("最新版本 {} · {}", r.version, r.source),
                false,
            );
            y += m.touch;
            let lines = r
                .notes
                .lines()
                .filter(|l| !l.trim().is_empty())
                .take(7)
                .map(|l| l.chars().take(36).collect::<String>())
                .collect::<Vec<_>>();
            for line in lines {
                label(
                    cx,
                    f,
                    Rect::new(
                        m.margin as i32,
                        y as i32,
                        cx.width - 2 * m.margin,
                        (m.body * 1.5) as u32,
                    ),
                    &line,
                    false,
                );
                y += (m.body * 1.5) as u32;
            }
        }
        if self.release.as_ref().is_some_and(|r| !r.notes.is_empty()) {
            button(
                cx,
                f,
                Rect::new(
                    m.margin as i32,
                    (cx.height - 4 * m.touch - m.margin) as i32,
                    cx.width - 2 * m.margin,
                    m.touch,
                ),
                "完整更新说明",
                4,
                !self.installing,
            );
        }
        let note = if self.notice.is_empty() {
            if self
                .release
                .as_ref()
                .is_some_and(|r| crate::update::newer(&r.version))
            {
                "发现新版本，安装将保留配置和阅读数据"
            } else {
                "已是最新版本"
            }
        } else {
            &self.notice
        };
        label(
            cx,
            f,
            Rect::new(
                m.margin as i32,
                (cx.height - 3 * m.touch - m.margin) as i32,
                cx.width - 2 * m.margin,
                m.touch,
            ),
            note,
            false,
        );
        button(
            cx,
            f,
            Rect::new(
                m.margin as i32,
                (cx.height - 2 * m.touch - m.margin) as i32,
                cx.width - 2 * m.margin,
                m.touch,
            ),
            if self.installed {
                "完成，退出应用"
            } else {
                "下载并安装"
            },
            2,
            !self.busy
                && (self.installed
                    || self
                        .release
                        .as_ref()
                        .is_some_and(|r| crate::update::newer(&r.version))),
        );
        button(
            cx,
            f,
            Rect::new(
                m.margin as i32,
                (cx.height - m.touch - m.margin / 2) as i32,
                cx.width - 2 * m.margin,
                m.touch,
            ),
            "重新检查",
            3,
            !self.busy && !self.installed,
        );
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
            Some(4) => {
                if let Some(r) = &self.release {
                    return Transition::Push(Box::new(text::TextPage::new("更新说明", &r.notes)));
                }
            }
            _ => {}
        }
        cx.request_redraw(RefreshHint::Ui);
        Transition::None
    }
    fn on_message(&mut self, cx: &mut Cx<Kindlia>, msg: Box<dyn Any + Send>) {
        let msg = match msg.downcast::<Checked>() {
            Ok(r) => {
                self.busy = false;
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
