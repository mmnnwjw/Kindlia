use super::*;
use crate::Kindlia;
use kn_platform::InputEvent;
use kn_render::{Bitmap, Rect};
use kn_ui::{Cx, Page, PageId, RefreshHint, Transition};
struct LoginResult(Result<(), String>);
#[derive(Default)]
pub struct MePage {
    busy: bool,
    notice: String,
    confirm: Option<(u8, choice::Choice)>,
}
impl Page<Kindlia> for MePage {
    fn id(&self) -> PageId {
        PageId(130)
    }
    fn render(&mut self, cx: &mut Cx<Kindlia>, f: &mut Bitmap) {
        if let Some((_, c)) = &mut self.confirm {
            c.render(cx, f);
            return;
        }
        header(cx, f, "我的", false);
        let m = cx.app.metrics;
        let w = cx.width;
        let y = m.header_h();
        let signed = cx.app.repo.api.signed_in();
        let username = cx.app.repo.api.username();
        row(
            cx,
            f,
            Rect::new(0, y as i32, w, m.row_h()),
            if signed {
                &username
            } else {
                "登录 Novelia 账号"
            },
            if signed {
                "收藏夹和阅读历史已启用"
            } else {
                "用户名与密码只用于登录，密码不保存"
            },
            "",
            None,
        );
        if !signed {
            button(
                cx,
                f,
                Rect::new(
                    m.margin as i32,
                    (y + m.row_h()) as i32,
                    w - 2 * m.margin,
                    m.touch,
                ),
                "登录",
                10,
                !self.busy,
            );
        } else {
            button(
                cx,
                f,
                Rect::new(
                    m.margin as i32,
                    (y + m.row_h()) as i32,
                    w - 2 * m.margin,
                    m.touch,
                ),
                "退出登录",
                13,
                !self.busy,
            );
        }
        let start = y + m.row_h() + m.touch + m.margin / 2;
        for (i, (t, id)) in [
            ("阅读统计", 24),
            ("检查更新", 25),
            ("设置", 20),
            ("本地书籍", 26),
            ("存储空间", 21),
            ("关于 Kindlia", 22),
            ("退出应用", 23),
        ]
        .iter()
        .enumerate()
        {
            row(
                cx,
                f,
                Rect::new(0, (start + i as u32 * m.row_h()) as i32, w, m.row_h()),
                t,
                "",
                ">",
                Some(*id),
            );
        }
        let status = if self.notice.is_empty() {
            cx.app.history_status.clone()
        } else {
            self.notice.clone()
        };
        label(
            cx,
            f,
            Rect::new(
                0,
                cx.height.saturating_sub(m.tab_bar_h() + m.touch) as i32,
                w,
                m.touch,
            ),
            &status,
            true,
        );
        draw_tabs(cx, f, 3);
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
    fn on_input(&mut self, cx: &mut Cx<Kindlia>, e: &InputEvent) -> Transition<Kindlia> {
        if matches!(e,InputEvent::Gesture(g) if g.kind==kn_platform::GestureKind::Down) {
            return Transition::None;
        }

        if let Some((action, choice)) = &mut self.confirm {
            let action = *action;
            match choice.input(cx, e) {
                choice::Event::Selected(1) => {
                    self.confirm = None;
                    if action == 23 {
                        return Transition::Exit;
                    }
                    let api = cx.app.repo.api.clone();
                    self.busy = true;
                    cx.spawn(move || LoginResult(api.logout()));
                }
                choice::Event::Selected(_) | choice::Event::Cancel => self.confirm = None,
                _ => {}
            }
            cx.request_redraw(RefreshHint::Ui);
            return Transition::None;
        }
        if let Some(t) = tab_input(cx, e, 3) {
            return t;
        }
        match hit(cx, e) {
            Some(10) => return Transition::Push(Box::new(login::LoginPage::new())),
            Some(13) if !self.busy => {
                self.confirm = Some((
                    13,
                    choice::Choice::new("退出当前账号？", ["取消", "退出登录"], Some(0)),
                ))
            }
            Some(23) => {
                self.confirm = Some((
                    23,
                    choice::Choice::new("退出 Kindlia？", ["取消", "退出应用"], Some(0)),
                ))
            }
            Some(20) => return Transition::Push(Box::new(settings::SettingsPage::new(cx.app))),
            Some(21) => return Transition::Push(Box::new(storage::StoragePage::default())),
            Some(22) => {
                return Transition::Push(Box::new(text::TextPage::new(
                    "关于 Kindlia",
                    concat!(
                        "Kindlia ",
                        env!("CARGO_PKG_VERSION"),
                        "\nAutoNovel 原生 Kindle 客户端\n基于 KinNovel，GPL-3.0-or-later"
                    ),
                )))
            }
            Some(24) => return Transition::Push(Box::new(stats::StatsPage::default())),
            Some(25) => return Transition::Push(Box::new(update::UpdatePage::default())),
            Some(26) => return Transition::Push(Box::new(library::LibraryPage::local_books())),
            _ => {}
        }
        cx.request_redraw(RefreshHint::Ui);
        Transition::None
    }
    fn on_message(&mut self, cx: &mut Cx<Kindlia>, m: Box<dyn std::any::Any + Send>) {
        if let Ok(r) = m.downcast::<LoginResult>() {
            self.busy = false;
            self.notice = match r.0 {
                Ok(()) => {
                    cx.toast("账号状态已更新");
                    String::new()
                }
                Err(e) => e,
            };
            cx.request_redraw(RefreshHint::Ui);
        }
    }
}
