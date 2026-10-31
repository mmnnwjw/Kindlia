//! Comment thread: complete text, indented replies, and actions on the tapped comment.
use super::*;
use kn_platform::{GestureKind, InputEvent};
use kn_render::{Bitmap, Rect};
use kn_ui::{
    keyboard::{EditorEvent, TextEditor},
    Cx, Page, PageId, RefreshHint, Transition,
};
use serde_json::{json, Value};
use std::any::Any;
struct Replies(Result<(Vec<Value>, usize), String>);
struct Changed(Result<(), String>, u64, Option<String>);
pub struct Thread {
    subject: String,
    root: Value,
    replies: Vec<Value>,
    remote: usize,
    remotes: usize,
    page: usize,
    lines: Vec<(crate::comment_format::Line, usize, bool)>,
    expanded: std::collections::HashSet<String>,
    format_hits: Vec<crate::comment_format::Action>,
    busy: bool,
    notice: String,
    picker: Option<(usize, bool, choice::Choice)>,
    editor: TextEditor,
    editing: Option<(usize, bool)>,
}
impl Thread {
    pub fn new(subject: String, root: Value) -> Self {
        let mut editor = TextEditor::new(1000, "");
        editor.set_max_chars(3000);
        Self {
            subject,
            root,
            replies: vec![],
            remote: 0,
            remotes: 1,
            page: 0,
            lines: vec![],
            expanded: Default::default(),
            format_hits: vec![],
            busy: false,
            notice: String::new(),
            picker: None,
            editor,
            editing: None,
        }
    }
    fn comment(&self, n: usize) -> Option<&Value> {
        if n == 0 {
            Some(&self.root)
        } else {
            self.replies.get(n - 1)
        }
    }
    fn load(&mut self, cx: &mut Cx<Kindlia>) {
        self.busy = true;
        self.replies.clear();
        self.lines.clear();
        let repo = cx.app.repo.clone();
        let subject = self.subject.clone();
        let id = self.root["id"].as_u64().unwrap_or(0);
        let page = self.remote;
        cx.spawn(move || {
            Replies((|| {
                let path = format!(
                    "forum:external/comment/novel/{}/{id}/reply?page={}&page_size=20",
                    crate::api::encode(&subject),
                    page + 1
                );
                let cache_key = (&path, repo.api.username());
                let v: Value = if repo.offline {
                    repo.store
                        .read_cache("cloud", &cache_key)
                        .ok_or("没有此评论回复的离线缓存")?
                } else {
                    let value: Value = repo.api.get(&path)?;
                    repo.store.write_cache("cloud", &cache_key, &value)?;
                    value
                };
                Ok((
                    v["items"].as_array().cloned().ok_or("回复格式无效")?,
                    v["total"].as_u64().unwrap_or(0).div_ceil(20).max(1) as usize,
                ))
            })())
        });
    }
    fn change(&mut self, cx: &mut Cx<Kindlia>, target: usize, edit: bool, delete: bool) {
        if self.busy {
            return;
        }
        if cx.app.repo.offline {
            self.notice = "离线状态不能修改评论".into();
            return;
        }
        let Some(c) = self.comment(target) else {
            return;
        };
        let id = c["id"].as_u64().unwrap_or(0);
        let api = cx.app.repo.api.clone();
        let subject = self.subject.clone();
        let root = self.root["id"].clone();
        let text = self.editor.text();
        self.busy = true;
        cx.spawn(move || {
            let (method, path, body) = if delete {
                (
                    "DELETE",
                    format!("forum:external/comment/novel/{id}"),
                    json!({}),
                )
            } else if edit {
                (
                    "PATCH",
                    format!("forum:external/comment/novel/{id}"),
                    json!({"content":text}),
                )
            } else {
                (
                    "POST",
                    format!(
                        "forum:external/comment/novel/{}",
                        crate::api::encode(&subject)
                    ),
                    json!({"content":text,"rootId":root}),
                )
            };
            Changed(
                api.mutate(method, &path, &body),
                if edit || delete { id } else { 0 },
                if delete { None } else { Some(text) },
            )
        });
    }
    fn layout(&mut self, cx: &mut Cx<Kindlia>) {
        if !self.lines.is_empty() {
            return;
        }
        let m = cx.app.metrics;
        let ink = Ink {
            fonts: cx.fonts,
            glyphs: cx.glyphs,
            chain: &cx.app.ui_fonts,
        };
        for n in 0..=self.replies.len() {
            let Some(c) = self.comment(n) else {continue;};
            let author = c["authorUsername"].as_str().unwrap_or("用户").to_owned();
            let body = if c["status"] == 0 || c["status"].is_null() {c["content"].as_str().unwrap_or("")} else {"评论已隐藏或删除"}.to_owned();
            let id = c["id"].to_string();
            let width = (cx.width - 2*m.margin - if n>0 {m.margin} else {0}) as f32;
            self.lines.push((crate::comment_format::Line{runs:vec![crate::comment_format::Run{text:author,..Default::default()}]},n,true));
            for line in crate::comment_format::layout(&body,&id,&self.expanded,&ink,m.body,width) {self.lines.push((line,n,false));}
            self.lines.push((Default::default(),n,false));
        }
    }
    fn rows(cx: &Cx<Kindlia>) -> usize {
        ((cx.height - cx.app.metrics.header_h() - cx.app.metrics.pager_h() - cx.app.metrics.margin)
            / (cx.app.metrics.body * 1.45) as u32)
            .max(1) as usize
    }
}
impl Page<Kindlia> for Thread {
    fn id(&self) -> PageId {
        PageId(58)
    }
    fn enter(&mut self, cx: &mut Cx<Kindlia>, returning: bool) {
        if !returning {self.load(cx);}
    }
    fn wants_backdrop(&self) -> kn_ui::Backdrop {
        self.picker
            .as_ref()
            .map(|(_, _, p)| p)
            .map(|p| p.wants_backdrop())
            .unwrap_or(kn_ui::Backdrop::None)
    }
    fn set_backdrop(&mut self, bg: Option<Bitmap>) {
        if let Some(p) = self.picker.as_mut().map(|(_, _, p)| p) {
            p.set_backdrop(bg);
        }
    }
    fn render(&mut self, cx: &mut Cx<Kindlia>, f: &mut Bitmap) {
        if let Some((_, _, p)) = &mut self.picker {
            p.render(cx, f);
            return;
        }
        header(cx, f, "评论与回复", true);
        let m = cx.app.metrics;
        if self.editing.is_some() {
            button(cx,f,Rect::new(m.margin as i32,(m.header_h()+m.margin/2) as i32,(cx.width-3*m.margin)/2,m.touch),"格式说明",90,true);
            button(cx,f,Rect::new(((cx.width+m.margin)/2) as i32,(m.header_h()+m.margin/2) as i32,(cx.width-3*m.margin)/2,m.touch),"预览草稿",91,true);
            let h = TextEditor::height(&m);
            let mut ink = Ink {
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
                Rect::new(0, (cx.height - h) as i32, cx.width, h),
                "评论内容",
                "提交",
            );
            return;
        }
        self.layout(cx);
        let rows = Self::rows(cx);
        self.page = self.page.min(self.lines.len().div_ceil(rows).max(1) - 1);
        let mut regions: Vec<(usize, Rect)> = Vec::new();
        let mut ink = Ink {
            fonts: cx.fonts,
            glyphs: cx.glyphs,
            chain: &cx.app.ui_fonts,
        };
        let mut format_regions = vec![];
        self.format_hits.clear();
        for (i, (text, n, author)) in self
            .lines
            .iter()
            .skip(self.page * rows)
            .take(rows)
            .enumerate()
        {
            let y = m.header_h() + m.margin + (i as f32 * m.body * 1.45) as u32;
            let x = m.margin + if *n > 0 { m.margin } else { 0 };
            for (rect,action) in crate::comment_format::draw(&mut ink,f,text,x as i32,y as i32,if *author {m.small}else{m.body},if *author{cx.theme.muted}else{cx.theme.foreground},cx.theme.background) {
                let id=500_000+self.format_hits.len() as u32;self.format_hits.push(action);format_regions.push((id,rect));
            }
            let rect = Rect::new(
                x as i32,
                y as i32,
                cx.width - x - m.margin,
                (m.body * 1.45) as u32,
            );
            if let Some((last, area)) = regions.last_mut().filter(|(last, _)| *last == *n) {
                let _ = last;
                area.h = (rect.bottom() - area.y) as u32;
            } else {
                regions.push((*n, rect));
            }
        }
        for (n, rect) in regions {
            cx.hits.add(HitId(300_000 + n as u32), rect);
        }

        for (id,rect) in format_regions {cx.hits.add(HitId(id),rect);}
        if !self.notice.is_empty() {
            let note = ink.fit(&self.notice, m.tiny, (cx.width - 2 * m.margin) as f32);
            ink.text(
                f,
                m.margin as i32,
                (cx.height - m.pager_h() - m.touch / 3) as i32,
                &note,
                m.tiny,
                cx.theme.muted,
            );
        }
        let pages = self.lines.len().div_ceil(rows).max(1);
        widgets::pager_with_label(
            &mut ink,
            f,
            cx.hits,
            &cx.theme,
            &m,
            Rect::new(0, (cx.height - m.pager_h()) as i32, cx.width, m.pager_h()),
            &format!("{} · {}/{}", self.remote + 1, self.page + 1, pages),
            self.page > 0 || self.remote > 0,
            self.page + 1 < pages || self.remote + 1 < self.remotes,
            HitId(2),
            HitId(3),
        );
    }
    fn on_input(&mut self, cx: &mut Cx<Kindlia>, e: &InputEvent) -> Transition<Kindlia> {
        // 按下反馈由运行时负责；等完整手势再修改页面，避免按下/抬起各整页刷新一次。
        if matches!(e, InputEvent::Gesture(g) if g.kind == kn_platform::GestureKind::Down) {
            return Transition::None;
        }
        if let Some((target, confirm, p)) = &mut self.picker {
            match p.input(cx, e) {
                choice::Event::Selected(action) => {
                    let (n, confirm) = (*target, *confirm);
                    self.picker = None;
                    if confirm {
                        if action == 1 {
                            self.change(cx, n, false, true);
                        }
                    } else if !cx.app.repo.api.signed_in() {
                        return Transition::Push(Box::new(login::LoginPage::new()));
                    } else if action == 2 {
                        self.picker = Some((
                            n,
                            true,
                            choice::Choice::new("确认删除这条评论？", ["取消", "删除"], Some(0)),
                        ));
                    } else {
                        let edit = action == 1;
                        let text = if edit {
                            self.comment(n)
                                .and_then(|c| c["content"].as_str())
                                .unwrap_or("")
                                .to_owned()
                        } else if n > 0 {
                            format!(
                                "@{} ",
                                self.comment(n)
                                    .and_then(|c| c["authorUsername"].as_str())
                                    .unwrap_or("")
                            )
                        } else {
                            String::new()
                        };
                        self.editor.set_text(&text);
                        self.editing = Some((n, edit));
                    }
                }
                choice::Event::Cancel => self.picker = None,
                _ => {}
            }
            cx.request_redraw(RefreshHint::Clean);
            return Transition::None;
        }
        if let Some((target, edit)) = self.editing {
            match hit(cx,e){
                Some(90)=>return Transition::Push(Box::new(formatted::FormattedPage::new("评论格式",formatted::HELP))),
                Some(91)=>return Transition::Push(Box::new(formatted::FormattedPage::new("草稿预览",&self.editor.text()))),_=>{}
            }
            if hit(cx, e) == Some(1) {
                self.editing = None;
                cx.request_redraw(RefreshHint::Clean);
                return Transition::None;
            }
            if let InputEvent::Gesture(g) = e {
                if let Some(id) = cx
                    .hits
                    .at(kn_render::Point {
                        x: g.end.0,
                        y: g.end.1,
                    })
                    .filter(|id| self.editor.owns(*id))
                {
                    if self.editor.on_gesture(id, g.kind) == EditorEvent::Submit
                        && !self.editor.text().trim().is_empty()
                    {
                        self.editing = None;
                        self.change(cx, target, edit, false);
                    }
                    cx.request_redraw(RefreshHint::Ui);
                }
            }
            return Transition::None;
        }
        if let Some(id) = hit(cx,e).filter(|id| *id>=500_000) {
            if let Some(action)=self.format_hits.get((id-500_000) as usize).cloned() {
                match action {
                    crate::comment_format::Action::Toggle(key)=>{if !self.expanded.remove(&key){self.expanded.insert(key);}self.lines.clear();},
                    crate::comment_format::Action::Link(url)=>return Transition::Push(Box::new(text::TextPage::new("链接地址",&url))),
                }
                cx.request_redraw(RefreshHint::Ui);
            }
            return Transition::None;
        }
        let pages = self.lines.len().div_ceil(Self::rows(cx)).max(1);
        match hit(cx, e) {
            Some(1) => return Transition::Back,
            Some(2) if self.page > 0 => self.page -= 1,
            Some(2) if self.remote > 0 && !self.busy => {
                self.remote -= 1;
                self.page = 0;
                self.load(cx);
            }
            Some(3) if self.page + 1 < pages => self.page += 1,
            Some(3) if self.remote + 1 < self.remotes && !self.busy => {
                self.remote += 1;
                self.page = 0;
                self.load(cx);
            }
            Some(id) if id >= 300_000 && !self.busy => {
                let n = (id - 300_000) as usize;
                let owner = self.comment(n).and_then(|c| c["authorUsername"].as_str())
                    == Some(cx.app.repo.api.username().as_str());
                self.picker = Some((
                    n,
                    false,
                    choice::Choice::new(
                        "评论操作",
                        if owner {
                            vec!["回复", "编辑", "删除"]
                        } else {
                            vec!["回复"]
                        },
                        None,
                    ),
                ));
            }
            _ => {
                if let InputEvent::Gesture(g) = e {
                    if g.kind == GestureKind::SwipeLeft && self.page + 1 < pages {
                        self.page += 1;
                    } else if g.kind == GestureKind::SwipeRight && self.page > 0 {
                        self.page -= 1;
                    }
                }
            }
        }
        cx.request_redraw(RefreshHint::Ui);
        Transition::None
    }
    fn on_message(&mut self, cx: &mut Cx<Kindlia>, msg: Box<dyn Any + Send>) {
        let msg = match msg.downcast::<Replies>() {
            Ok(r) => {
                self.busy = false;
                match r.0 {
                    Ok((replies, pages)) => {
                        self.replies = replies;
                        self.remotes = pages;
                        self.lines.clear();
                    }
                    Err(e) => self.notice = e,
                }
                cx.request_redraw(RefreshHint::Ui);
                return;
            }
            Err(m) => m,
        };
        if let Ok(r) = msg.downcast::<Changed>() {
            self.busy = false;
            match r.0 {
                Ok(()) => {
                    if r.1 != 0 {
                        for c in std::iter::once(&mut self.root).chain(self.replies.iter_mut()) {
                            if c["id"].as_u64() == Some(r.1) {
                                if let Some(text) = &r.2 {
                                    c["content"] = json!(text);
                                } else {
                                    c["status"] = json!(1);
                                }
                            }
                        }
                    }
                    self.lines.clear();
                    self.load(cx);
                }
                Err(e) => self.notice = e,
            }
            cx.request_redraw(RefreshHint::Ui);
        }
    }
}
