use super::*;
use crate::{api::encode, Kindlia};
use kn_platform::InputEvent;
use kn_render::{Bitmap, Point, Rect};
use kn_ui::{
    keyboard::{EditorEvent, TextEditor},
    Cx, Page, PageId, RefreshHint, Transition,
};
pub struct ManagePage {
    path: String,
    title: String,
    editor: TextEditor,
    editing: bool,
    notice: String,
    confirm: Option<choice::Choice>,
    busy: bool,
    back: bool,
}
struct ResultMessage(Result<(), String>);
impl ManagePage {
    pub fn folder(wenku: bool, id: String, title: String) -> Self {
        Self {
            path: format!(
                "user/favored-{}/{}",
                if wenku { "wenku" } else { "web" },
                encode(&id)
            ),
            editor: TextEditor::new(1000, &title),
            title,
            editing: false,
            notice: String::new(),
            confirm: None,
            busy: false,
            back: false,
        }
    }
    pub fn remove(path: String) -> Self {
        Self {
            path,
            title: "移除所选项目".into(),
            editor: TextEditor::new(1000, ""),
            editing: false,
            notice: String::new(),
            confirm: None,
            busy: false,
            back: false,
        }
    }
    fn apply(&mut self, cx: &mut Cx<Kindlia>, delete: bool) {
        if cx.app.repo.offline {
            self.notice = "离线模式不可修改".into();
            return;
        }
        let api = cx.app.repo.api.clone();
        let path = self.path.clone();
        let name = self.editor.text();
        self.busy = true;
        cx.spawn(move || {
            ResultMessage(api.mutate(
                if delete { "DELETE" } else { "PUT" },
                &path,
                &serde_json::json!({"title":name}),
            ))
        });
    }
}
impl Page<Kindlia> for ManagePage {
    fn id(&self) -> PageId {
        PageId(52)
    }
    fn wants_backdrop(&self) -> kn_ui::Backdrop {
        self.confirm
            .as_ref()
            .map(|c| c.wants_backdrop())
            .unwrap_or(kn_ui::Backdrop::None)
    }
    fn set_backdrop(&mut self, b: Option<Bitmap>) {
        if let Some(c) = &mut self.confirm {
            c.set_backdrop(b);
        }
    }
    fn render(&mut self, cx: &mut Cx<Kindlia>, f: &mut Bitmap) {
        if let Some(c) = &mut self.confirm {
            c.render(cx, f);
            return;
        }
        header(cx, f, &self.title, true);
        let m = cx.app.metrics;
        let y = m.header_h();
        button(
            cx,
            f,
            Rect::new(0, y as i32, cx.width, m.touch),
            "重命名收藏夹",
            10,
            !self.busy && self.title != "移除所选项目",
        );
        button(
            cx,
            f,
            Rect::new(0, (y + m.touch) as i32, cx.width, m.touch),
            "删除 / 移除",
            11,
            !self.busy,
        );
        label(
            cx,
            f,
            Rect::new(0, (y + m.touch * 2) as i32, cx.width, m.touch),
            &self.notice,
            true,
        );
        if self.editing {
            let h = TextEditor::height(&m);
            let mut ink = kn_ui::widgets::Ink {
                fonts: cx.fonts,
                glyphs: cx.glyphs,
                chain: &cx.app.ui_fonts,
            };
            self.editor.render(
                &mut ink,
                f,
                cx.hits,
                &cx.theme,
                &m,
                Rect::new(0, cx.height.saturating_sub(h) as i32, cx.width, h),
                "名称",
                "保存",
            );
        }
    }
    fn on_input(&mut self, cx: &mut Cx<Kindlia>, e: &InputEvent) -> Transition<Kindlia> {
        // 按下反馈由运行时负责；等完整手势再修改页面，避免按下/抬起各整页刷新一次。
        if matches!(e, InputEvent::Gesture(g) if g.kind == kn_platform::GestureKind::Down) {
            return Transition::None;
        }
        if let Some(c) = &mut self.confirm {
            match c.input(cx, e) {
                choice::Event::Selected(1) => {
                    self.confirm = None;
                    self.apply(cx, true);
                }
                choice::Event::Selected(_) | choice::Event::Cancel => self.confirm = None,
                _ => {}
            }
            cx.request_redraw(RefreshHint::Ui);
            return Transition::None;
        }
        if self.busy {
            return Transition::None;
        }
        if self.editing {
            if let InputEvent::Gesture(g) = e {
                if let Some(id) = cx
                    .hits
                    .at(Point {
                        x: g.end.0,
                        y: g.end.1,
                    })
                    .filter(|id| self.editor.owns(*id))
                {
                    if self.editor.on_gesture(id, g.kind) == EditorEvent::Submit {
                        self.editing = false;
                        self.apply(cx, false);
                    }
                    cx.request_redraw(RefreshHint::Ui);
                    return Transition::None;
                }
            }
        }
        match hit(cx, e) {
            Some(1) => return Transition::Back,
            Some(10) => self.editing = true,
            Some(11) => {
                self.confirm = Some(choice::Choice::new(
                    "确认删除 / 移除？将修改云端数据",
                    ["取消", "删除"],
                    Some(0),
                ))
            }
            _ => {}
        }
        cx.request_redraw(RefreshHint::Ui);
        Transition::None
    }
    fn on_message(&mut self, cx: &mut Cx<Kindlia>, m: Box<dyn std::any::Any + Send>) {
        if let Ok(r) = m.downcast::<ResultMessage>() {
            self.busy = false;
            match r.0 {
                Ok(()) => {
                    self.back = true;
                    cx.toast("操作已完成");
                }
                Err(e) => self.notice = e,
            }
            cx.request_redraw(RefreshHint::Ui);
        }
    }
    fn take_transition(&mut self) -> Transition<Kindlia> {
        if self.back {
            self.back = false;
            Transition::Back
        } else {
            Transition::None
        }
    }
}
