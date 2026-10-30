use super::*;
use crate::Kindlia;
use kn_platform::InputEvent;
use kn_render::{Bitmap, Point, Rect};
use kn_ui::{
    keyboard::{EditorEvent, TextEditor},
    Cx, Page, PageId, RefreshHint, Transition,
};
struct LoginResult(Result<(), String>);
pub struct MePage {
    username: String,
    password: TextEditor,
    editor: TextEditor,
    field: u8,
    busy: bool,
    notice: String,
}
impl Default for MePage {
    fn default() -> Self {
        let mut password = TextEditor::new(1200, "");
        password.set_chinese(false);
        password.set_masked(true);
        let mut editor = TextEditor::new(1000, "");
        editor.set_chinese(false);
        Self {
            username: String::new(),
            password,
            editor,
            field: 0,
            busy: false,
            notice: String::new(),
        }
    }
}
impl Page<Kindlia> for MePage {
    fn id(&self) -> PageId {
        PageId(130)
    }
    fn render(&mut self, cx: &mut Cx<Kindlia>, f: &mut Bitmap) {
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
        for (i, (t, id)) in [("阅读统计", 24), ("设置", 20), ("存储空间", 21), ("关于 Kindlia", 22), ("退出应用", 23)]
            .iter()
            .enumerate()
        {
            row(
                cx,
                f,
                Rect::new(0, (start + i as u32 * m.row_h()) as i32, w, m.row_h()),
                t,
                "",
                "",
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
        if self.field != 0 {
            let h = TextEditor::height(&m);
            let ed = if self.field == 1 {
                &mut self.editor
            } else {
                &mut self.password
            };
            let mut ink = kn_ui::widgets::Ink {
                fonts: cx.fonts,
                glyphs: cx.glyphs,
                chain: &cx.app.ui_fonts,
            };
            ed.render(
                &mut ink,
                f,
                cx.hits,
                &cx.theme,
                &m,
                Rect::new(0, cx.height.saturating_sub(h) as i32, w, h),
                "输入",
                "确定",
            );
        } else {
            draw_tabs(cx, f, 3);
        }
    }
    fn on_input(&mut self, cx: &mut Cx<Kindlia>, e: &InputEvent) -> Transition<Kindlia> {
        // 按下反馈由运行时负责；等完整手势再修改页面，避免按下/抬起各整页刷新一次。
        if matches!(e, InputEvent::Gesture(g) if g.kind == kn_platform::GestureKind::Down) {
            return Transition::None;
        }
        if self.field != 0 {
            if let InputEvent::Gesture(g) = e {
                let ed = if self.field == 1 {
                    &mut self.editor
                } else {
                    &mut self.password
                };
                if let Some(id) = cx
                    .hits
                    .at(Point {
                        x: g.end.0,
                        y: g.end.1,
                    })
                    .filter(|id| ed.owns(*id))
                {
                    if ed.on_gesture(id, g.kind) == EditorEvent::Submit {
                        if self.field == 1 {
                            self.username = self.editor.text();
                        }
                        self.field = 0;
                    }
                    cx.request_redraw(RefreshHint::Ui);
                    return Transition::None;
                }
            }
        }
        if let Some(t) = tab_input(cx, e, 3) {
            return t;
        }
        match hit(cx,e){Some(10)=>return Transition::Push(Box::new(login::LoginPage::new())),Some(11)=>return Transition::Push(Box::new(login::LoginPage::new())),Some(12)if !self.busy=>{let username=self.username.clone();let password=self.password.text();self.password.set_text("");let api=cx.app.repo.api.clone();self.busy=true;cx.spawn(move||LoginResult(api.login(&username,&password)));},Some(13)if !self.busy=>{let api=cx.app.repo.api.clone();self.busy=true;cx.spawn(move||LoginResult(api.logout()));},Some(20)=>return Transition::Push(Box::new(settings::SettingsPage::new(cx.app))),Some(21)=>return Transition::Push(Box::new(storage::StoragePage::default())),Some(22)=>return Transition::Push(Box::new(text::TextPage::new("关于 Kindlia",concat!("Kindlia ",env!("CARGO_PKG_VERSION"),"\nAutoNovel 原生 Kindle 客户端\n账号注册、找回密码请使用 auth.novelia.cc\n自定义字体放入 /mnt/us/extensions/kindlia/fonts\nGPL-3.0-or-later\n基于 KinNovel，保留原作者许可。")))),Some(24)=>return Transition::Push(Box::new(stats::StatsPage::default())),Some(23)=>return Transition::Exit,_=>{}}
        cx.request_redraw(RefreshHint::Ui);
        Transition::None
    }
    fn on_message(&mut self, cx: &mut Cx<Kindlia>, m: Box<dyn std::any::Any + Send>) {
        if let Ok(r) = m.downcast::<LoginResult>() {
            self.busy = false;
            self.notice = match r.0 {
                Ok(()) => "账号状态已更新".into(),
                Err(e) => e,
            };
            cx.request_redraw(RefreshHint::Ui);
        }
    }
}
