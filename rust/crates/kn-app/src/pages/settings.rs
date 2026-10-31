//! 沿用 KinNovel 设置页：顶部分段、紧凑行、左右步进、右侧开关，即时保存。
use super::*;
use crate::{store::Config, Kindlia};
use kn_platform::InputEvent;
use kn_render::{Bitmap, Point, Rect};
use kn_ui::{
    keyboard::{EditorEvent, TextEditor},
    Cx, Page, PageId, RefreshHint, Transition,
};

#[derive(Clone, Copy)]
enum Field {
    FontSize,
    Line,
    Paragraph,
    Margin,
    Indent,
    Landscape,
    Night,
    Translation,
    Original,
    Translated,
    Font,
    Scan,
    Api,
    Auth,
    Forum,
    Offline,
    Gateway,
    GatewayUrl,
    GatewayKey,
    UpdateAuto,
    UpdateR2,
    Animation,
    Steps,
    Refresh,
    Prefetch,
    OriginalSize,
    OriginalGray,
    UiRefresh,
    Flash,
    Side,
    SleepCover,
    Tips,
    IgnoreJapanese,
    IgnoreAi,
    CacheLimit,
    Lock,
    ChangeLock,
    Convert,
    History,
}
fn fields(group: usize) -> &'static [(Field, &'static str)] {
    use Field::*;
    match group {
        0 => &[
            (FontSize, "正文字号"),
            (Line, "行高"),
            (Paragraph, "段间距"),
            (Margin, "页边距"),
            (Indent, "首行缩进"),
            (Font, "阅读字体"),
            (Scan, "扫描字体目录"),
            (Prefetch, "章节预加载"),
            (Convert, "简繁转换"),
        ],
        1 => &[
            (Night, "夜间模式"),
            (Landscape, "屏幕方向"),
            (Animation, "翻页动画"),
            (Steps, "动画阶数"),
            (Refresh, "清理间隔（屏）"),
            (UiRefresh, "UI 刷新模式"),
            (Flash, "翻页全屏刷新"),
            (Side, "横屏握持"),
            (SleepCover, "休眠显示封面"),
            (Tips, "操作提示"),
        ],
        2 => &[
            (Translation, "翻译偏好"),
            (Translated, "显示译文"),
            (Original, "显示原文"),
            (OriginalSize, "日语字号（%）"),
            (OriginalGray, "日语灰度"),
            (IgnoreJapanese, "隐藏未翻译Web"),
            (IgnoreAi, "隐藏纯 AI 译文"),
            (CacheLimit, "网络缓存上限（MB）"),
            (History, "云端阅读历史"),
        ],
        4 => &[(Lock, "锁屏密码"), (ChangeLock, "修改锁屏密码")],
        _ => &[
            (Gateway, "反代开关"),
            (GatewayUrl, "反代地址"),
            (GatewayKey, "反代密钥"),
            (Api, "API 地址"),
            (Auth, "登录服务"),
            (Forum, "书籍评论服务"),
            (Offline, "离线模式"),
            (UpdateAuto, "自动检查更新"),
            (UpdateR2, "R2 更新地址"),
        ],
    }
}
pub struct SettingsPage {
    draft: Config,
    group: usize,
    editor: TextEditor,
    editing: Option<Field>,
    notice: String,
    fonts: Vec<String>,
    picker: Option<(Field, choice::Choice)>,
}
impl SettingsPage {
    pub fn new(app: &Kindlia) -> Self {
        let mut editor = TextEditor::new(1000, "");
        editor.set_chinese(false);
        editor.set_max_chars(512);
        Self {
            draft: app.config.clone(),
            group: 0,
            editor,
            editing: None,
            notice: String::new(),
            fonts: crate::fonts::list(&app.repo.store.root),
            picker: None,
        }
    }
    fn value(&self, field: Field) -> String {
        use Field::*;
        match field {
            FontSize => self.draft.font_size.to_string(),
            Line => format!("{:.2}", self.draft.line_spacing),
            Paragraph => format!("{:.2}", self.draft.paragraph_spacing),
            Margin => self.draft.margin.to_string(),
            Indent => on(self.draft.first_line_indent),
            Night => on(self.draft.night),
            Original => on(self.draft.show_original),
            Translated => on(self.draft.show_translation),
            Offline => on(self.draft.offline),
            Gateway => on(self.draft.gateway_enabled),
            UpdateAuto => on(self.draft.update_auto),
            UpdateR2 => if self.draft.update_r2_url.is_empty(){"未配置".into()}else{self.draft.update_r2_url.clone()},
            GatewayKey => if self.draft.gateway_key.is_empty() {
                "未设置"
            } else {
                "已设置"
            }
            .into(),
            Animation => on(self.draft.page_turn_animation),
            Prefetch => on(self.draft.prefetch_chapters),
            Steps => self.draft.swipe_steps.to_string(),
            Refresh => self.draft.full_refresh_every.to_string(),
            OriginalSize => self.draft.original_size.to_string(),
            OriginalGray => self.draft.original_gray.to_string(),
            Landscape => if self.draft.landscape {
                "横屏"
            } else {
                "竖屏"
            }
            .into(),
            Translation => match self.draft.translation.as_str() {
                "auto" => "自动优选",
                "youdao" => "有道",
                "gpt" => "GPT",
                "sakura" => "Sakura",
                _ => "百度",
            }
            .into(),
            Font => {
                if self.draft.reader_font.is_empty() {
                    "系统字体".into()
                } else {
                    self.draft.reader_font.clone()
                }
            }
            Scan => format!("{} 款", self.fonts.len()),
            UiRefresh => if self.draft.ui_refresh_mode == "classic" {
                "经典"
            } else {
                "流畅"
            }
            .into(),
            Flash => on(self.draft.page_flash),
            SleepCover => on(self.draft.sleep_cover),
            Tips => on(self.draft.operation_tips),
            IgnoreJapanese => on(self.draft.ignore_japanese),
            IgnoreAi => on(self.draft.ignore_ai),
            CacheLimit => self.draft.cache_limit_mb.to_string(),
            Side => if self.draft.landscape_side == "right" {
                "向右转"
            } else {
                "向左转"
            }
            .into(),
            Convert => match self.draft.convert.as_str() {
                "t2s" => "繁转简",
                "s2t" => "简转繁",
                _ => "关闭",
            }
            .into(),
            Lock => on(!self.draft.lock_pin.is_empty()),
            History => "管理".into(),
            _ => "编辑".into(),
        }
    }
    fn step(&self, f: Field) -> Option<(f64, f64, f64, f64)> {
        use Field::*;
        Some(match f {
            CacheLimit => (self.draft.cache_limit_mb as f64, 64., 1024., 64.),
            FontSize => (self.draft.font_size as f64, 28., 80., 4.),
            Line => (self.draft.line_spacing, 1.0, 2.0, 0.05),
            Paragraph => (self.draft.paragraph_spacing, 0., 2.0, 0.05),
            Margin => (self.draft.margin as f64, 16., 96., 8.),
            Steps => (self.draft.swipe_steps as f64, 6., 60., 6.),
            Refresh => (self.draft.full_refresh_every as f64, 2., 12., 1.),
            OriginalSize => (self.draft.original_size as f64, 60., 120., 5.),
            OriginalGray => (self.draft.original_gray as f64, 0., 200., 10.),
            _ => return None,
        })
    }
    fn save(&mut self, cx: &mut Cx<Kindlia>) -> bool {
        if !self.draft.show_original && !self.draft.show_translation {
            self.notice = "至少保留原文或译文之一".into();
            self.draft = cx.app.config.clone();
            return false;
        }
        match cx.app.apply_config(self.draft.clone()) {
            Ok(()) => {
                self.notice.clear();
                true
            }
            Err(e) => {
                self.notice = e;
                self.draft = cx.app.config.clone();
                false
            }
        }
    }
}
fn on(v: bool) -> String {
    if v { "开" } else { "关" }.into()
}
impl Page<Kindlia> for SettingsPage {
    fn id(&self) -> PageId {
        PageId(40)
    }
    fn enter(&mut self, cx: &mut Cx<Kindlia>, _: bool) {
        self.fonts = crate::fonts::list(&cx.app.repo.store.root);
        self.draft = cx.app.config.clone();
    }
    fn wants_backdrop(&self) -> kn_ui::Backdrop {
        self.picker.as_ref().map(|(_,p)| p).map(|p| p.wants_backdrop()).unwrap_or(kn_ui::Backdrop::None)
    }
    fn set_backdrop(&mut self, bg: Option<Bitmap>) {
        if let Some(p) = self.picker.as_mut().map(|(_,p)| p) { p.set_backdrop(bg); }
    }
    fn render(&mut self, cx: &mut Cx<Kindlia>, frame: &mut Bitmap) {
        if let Some((_, picker)) = &mut self.picker {
            picker.render(cx, frame);
            return;
        }
        header(cx, frame, "设置", true);
        let m = cx.app.metrics;
        let theme = cx.theme;
        let side = m.margin as i32;
        let chain = cx.app.ui_fonts.clone();
        let mut ink = Ink {
            fonts: cx.fonts,
            glyphs: cx.glyphs,
            chain: &chain,
        };
        let seg = Rect::new(
            side,
            m.header_h() as i32 + side / 3,
            cx.width - 2 * m.margin,
            m.touch,
        );
        widgets::segmented(
            &mut ink,
            frame,
            cx.hits,
            &theme,
            &m,
            seg,
            &["阅读", "显示", "内容", "网络", "隐私"],
            self.group,
            10,
        );
        let mut y = seg.bottom() + side / 2;
        let row_h = (m.touch as f32 * 1.15) as i32;
        let btn = m.touch as i32;
        for (i, (field, title)) in fields(self.group).iter().enumerate() {
            let r = Rect::new(0, y, cx.width, row_h as u32);
            ink.text(
                frame,
                side,
                y + (row_h - (m.body * 1.25) as i32) / 2,
                title,
                m.body,
                theme.foreground,
            );
            let id = |k| HitId(100 + i as u32 * 4 + k);
            if let Some((v, min, max, _)) = self.step(*field) {
                let plus = Rect::new(
                    cx.width as i32 - side - btn,
                    y + (row_h - btn) / 2,
                    btn as u32,
                    btn as u32,
                );
                let value_w = (m.body * 2.6) as i32;
                let value_r = Rect::new(plus.x - value_w, y, value_w as u32, row_h as u32);
                let minus = Rect::new(value_r.x - btn, plus.y, btn as u32, btn as u32);
                for (rect, label, enabled, k) in [
                    (minus, "－", v > min + 0.0001, 0),
                    (plus, "＋", v < max - 0.0001, 1),
                ] {
                    widgets::button(
                        &mut ink,
                        frame,
                        &theme,
                        &m,
                        rect,
                        label,
                        if enabled {
                            ButtonStyle::Secondary
                        } else {
                            ButtonStyle::Disabled
                        },
                    );
                    cx.hits.add(id(k), rect).rounded(m.radius).enabled(enabled);
                }
                ink.text_centered(
                    frame,
                    value_r,
                    &self.value(*field),
                    m.body,
                    theme.foreground,
                );
            } else {
                let sw = Rect::new(
                    cx.width as i32 - side - btn * 2,
                    y + (row_h - btn * 2 / 3) / 2,
                    (btn * 2) as u32,
                    (btn * 2 / 3) as u32,
                );
                let value = self.value(*field);
                let on = value == "开"
                    || matches!(field, Field::GatewayKey) && !self.draft.gateway_key.is_empty()
                    || matches!(field, Field::Font) && !self.draft.reader_font.is_empty()
                    || matches!(field, Field::Landscape) && self.draft.landscape
                    || matches!(field, Field::Translation) && self.draft.translation != "auto";
                let text = ink.fit(&value, m.small, (sw.w - 2 * m.margin / 3) as f32);
                widgets::button(
                    &mut ink,
                    frame,
                    &theme,
                    &m,
                    sw,
                    &text,
                    if on {
                        ButtonStyle::Primary
                    } else {
                        ButtonStyle::Secondary
                    },
                );
                cx.hits.add(id(2), r).enabled(!matches!(field,Field::ChangeLock)||lock::pin_set(&self.draft));
            }
            frame.fill_rect(
                Rect::new(side, r.bottom() - 1, cx.width - 2 * m.margin, 1),
                theme.mid,
            );
            y += row_h;
        }
        let note = if !self.notice.is_empty() {
            self.notice.clone()
        } else {
            match self.group {
                0 => format!(
                    "修改即时保存\n字体目录：/mnt/us/extensions/kindlia/fonts\n当前字体：{}",
                    self.value(Field::Font)
                ),
                1 => "UI 模式即时生效；全屏翻页/清理间隔/休眠封面重启生效".into(),
                2 => "日语字号相对正文字号；灰度 0 最深\n参考原文独立设置，不影响译文".into(),
                4 => "六位数字密码；恢复应用时重新验证".into(),
                _ => format!(
                    "反代：{}\n先设置地址与密钥，再打开反代开关",
                    self.draft.gateway_url
                ),
            }
        };
        for line in note.lines() {
            if y + (m.small * 1.5) as i32 > cx.height as i32 {
                break;
            }
            let text = ink.fit(line, m.small, (cx.width - 2 * m.margin) as f32);
            ink.text(frame, side, y + side / 2, &text, m.small, theme.muted);
            y += (m.small * 1.5) as i32;
        }
        if self.editing.is_some() {
            let h = TextEditor::height(&m);
            self.editor.render(
                &mut ink,
                frame,
                cx.hits,
                &theme,
                &m,
                Rect::new(0, cx.height.saturating_sub(h) as i32, cx.width, h),
                "https://…",
                "确定",
            );
        }
    }
    fn on_input(&mut self, cx: &mut Cx<Kindlia>, event: &InputEvent) -> Transition<Kindlia> {
        // 按下反馈由运行时负责；等完整手势再修改页面，避免按下/抬起各整页刷新一次。
        if matches!(event, InputEvent::Gesture(g) if g.kind == kn_platform::GestureKind::Down) {
            return Transition::None;
        }
        if let Some((field, picker)) = &mut self.picker {
            match picker.input(cx, event) {
                choice::Event::Selected(n) => {
                    match *field {
                        Field::Font => {
                            self.draft.reader_font = if n == 0 {
                                String::new()
                            } else {
                                self.fonts.get(n - 1).cloned().unwrap_or_default()
                            }
                        }
                        Field::Translation => {
                            self.draft.translation =
                                ["auto", "youdao", "gpt", "sakura", "baidu"][n].into()
                        }
                        Field::Landscape => self.draft.landscape = n == 1,
                        Field::UiRefresh => {
                            self.draft.ui_refresh_mode = ["smooth", "classic"][n].into()
                        }
                        Field::Side => self.draft.landscape_side = ["left", "right"][n].into(),
                        Field::History => {
                            self.picker = None;
                            crate::history::manage(cx, n);
                            cx.request_redraw(RefreshHint::Clean);
                            return Transition::None;
                        }
                        Field::Convert => self.draft.convert = ["", "t2s", "s2t"][n].into(),
                        _ => {}
                    }
                    self.picker = None;
                    self.save(cx);
                    cx.request_redraw(RefreshHint::Clean);
                }
                choice::Event::Cancel => {
                    self.picker = None;
                    cx.request_redraw(RefreshHint::Clean);
                }
                _ => {}
            }
            return Transition::None;
        }
        if let Some(field) = self.editing {
            if hit(cx, event) == Some(1) {
                self.editing = None;
                cx.request_redraw(RefreshHint::Ui);
                return Transition::None;
            }
            if let InputEvent::Gesture(g) = event {
                if let Some(id) = cx
                    .hits
                    .at(Point {
                        x: g.end.0,
                        y: g.end.1,
                    })
                    .filter(|id| self.editor.owns(*id))
                {
                    if self.editor.on_gesture(id, g.kind) == EditorEvent::Submit {
                        let text = self.editor.text();
                        let validated = if matches!(field, Field::GatewayKey) {
                            Ok(text)
                        } else if matches!(field, Field::UpdateR2) {
                            if text.trim().is_empty(){Ok(String::new())}else{crate::update::https(&text)}
                        } else if matches!(field, Field::GatewayUrl) {
                            crate::gateway::Gateway::new(&text, "preview-key-123456")
                                .map(|_| text.trim().trim_end_matches('/').to_owned())
                        } else {
                            crate::api::normalize_base(&text)
                        };
                        match validated {
                            Ok(v) => {
                                let changed =
                                    matches!(field, Field::Api) && v != self.draft.api_base_url;
                                match field {
                                    Field::Api => self.draft.api_base_url = v,
                                    Field::Auth => self.draft.auth_base_url = v,
                                    Field::GatewayUrl => self.draft.gateway_url = v,
                                    Field::GatewayKey => self.draft.gateway_key = v,
                                    Field::UpdateR2 => self.draft.update_r2_url = v,
                                    _ => self.draft.forum_base_url = v,
                                };
                                if self.save(cx) {
                                    self.editing = None;
                                    if changed {
                                        return Transition::Home;
                                    }
                                }
                            }
                            Err(e) => self.notice = e,
                        }
                    }
                }
            }
            cx.request_redraw(RefreshHint::Ui);
            return Transition::None;
        }
        match hit(cx, event) {
            Some(1) => return Transition::Back,
            Some(id) if (10..15).contains(&id) => {
                self.group = (id - 10) as usize;
                self.notice.clear();
            }
            Some(id) if id >= 100 => {
                if let Some((field, _)) = fields(self.group).get(((id - 100) / 4) as usize) {
                    use Field::*;
                    let field = *field;
                    if let Some((v, min, max, step)) = self.step(field) {
                        let value =
                            (v + if (id - 100) % 4 == 0 { -step } else { step }).clamp(min, max);
                        match field {
                            FontSize => self.draft.font_size = value.round() as u32,
                            Line => self.draft.line_spacing = (value * 100.0).round() / 100.0,
                            Paragraph => self.draft.paragraph_spacing = (value * 100.0).round() / 100.0,
                            Margin => self.draft.margin = value.round() as u32,
                            Steps => self.draft.swipe_steps = value.round() as u8,
                            Refresh => self.draft.full_refresh_every = value.round() as u8,
                            OriginalSize => self.draft.original_size = value.round() as u16,
                            OriginalGray => self.draft.original_gray = value.round() as u8,
                            CacheLimit => self.draft.cache_limit_mb = value.round() as u32,
                            _ => {}
                        }
                    } else {
                        match field {
                            Indent => self.draft.first_line_indent = !self.draft.first_line_indent,
                            UpdateAuto => self.draft.update_auto = !self.draft.update_auto,
                            Animation => {
                                self.draft.page_turn_animation = !self.draft.page_turn_animation
                            }
                            Prefetch => {
                                self.draft.prefetch_chapters = !self.draft.prefetch_chapters
                            }
                            Night => self.draft.night = !self.draft.night,
                            Landscape => {
                                self.picker = Some((
                                    field,
                                    choice::Choice::new(
                                        "屏幕方向",
                                        ["竖屏", "横屏"],
                                        Some(self.draft.landscape as usize),
                                    ),
                                ))
                            }
                            UiRefresh => {
                                self.picker = Some((
                                    field,
                                    choice::Choice::new(
                                        "UI 刷新模式",
                                        ["流畅", "经典"],
                                        Some((self.draft.ui_refresh_mode == "classic") as usize),
                                    ),
                                ))
                            }
                            Side => {
                                self.picker = Some((
                                    field,
                                    choice::Choice::new(
                                        "横屏握持",
                                        ["向左转", "向右转"],
                                        Some((self.draft.landscape_side == "right") as usize),
                                    ),
                                ))
                            }
                            Convert => {
                                self.picker = Some((
                                    field,
                                    choice::Choice::new(
                                        "简繁转换",
                                        ["关闭", "繁转简", "简转繁"],
                                        Some(
                                            ["", "t2s", "s2t"]
                                                .iter()
                                                .position(|s| *s == self.draft.convert)
                                                .unwrap_or(0),
                                        ),
                                    ),
                                ))
                            }
                            History => {
                                self.picker = Some((
                                    field,
                                    choice::Choice::new(
                                        "云端阅读历史（仅Web小说）",
                                        ["立即同步", "恢复云端记录", "暂停云端记录"],
                                        None,
                                    ),
                                ))
                            }
                            Lock => {
                                return Transition::Push(Box::new(
                                    if lock::pin_set(&cx.app.config) {
                                        lock::LockPage::verify(lock::After::Disable)
                                    } else {
                                        lock::LockPage::enable()
                                    },
                                ))
                            }
                            ChangeLock => {
                                return Transition::Push(Box::new(
                                    if lock::pin_set(&cx.app.config) {
                                        lock::LockPage::verify(lock::After::Change)
                                    } else {
                                        lock::LockPage::enable()
                                    },
                                ))
                            }
                            Flash => self.draft.page_flash = !self.draft.page_flash,
                            SleepCover => self.draft.sleep_cover = !self.draft.sleep_cover,
                            Tips => {
                                self.draft.operation_tips = !self.draft.operation_tips;
                                if self.draft.operation_tips {
                                    self.draft.tips_seen.clear();
                                }
                            }
                            IgnoreJapanese => {
                                self.draft.ignore_japanese = !self.draft.ignore_japanese
                            }
                            IgnoreAi => self.draft.ignore_ai = !self.draft.ignore_ai,
                            Original => self.draft.show_original = !self.draft.show_original,
                            Translated => {
                                self.draft.show_translation = !self.draft.show_translation
                            }
                            Offline => self.draft.offline = !self.draft.offline,
                            Gateway => self.draft.gateway_enabled = !self.draft.gateway_enabled,
                            Translation => {
                                self.picker = Some((
                                    field,
                                    choice::Choice::new(
                                        "翻译偏好",
                                        ["自动优选", "有道", "GPT", "Sakura", "百度"],
                                        Some(
                                            ["auto", "youdao", "gpt", "sakura", "baidu"]
                                                .iter()
                                                .position(|s| *s == self.draft.translation)
                                                .unwrap_or(0),
                                        ),
                                    ),
                                ))
                            }
                            Font => {
                                let mut options = vec!["系统字体".to_string()];
                                options.extend(self.fonts.clone());
                                let selected = self
                                    .fonts
                                    .iter()
                                    .position(|s| *s == self.draft.reader_font)
                                    .map_or(0, |i| i + 1);
                                self.picker = Some((
                                    field,
                                    choice::Choice::new("阅读字体", options, Some(selected)),
                                ));
                            }
                            Scan => {
                                self.fonts = crate::fonts::list(&cx.app.repo.store.root);
                            }
                            Api | Auth | Forum | GatewayUrl | GatewayKey | UpdateR2 => {
                                self.editor.set_masked(matches!(field, GatewayKey));
                                self.editor.set_text(match field {
                                    Api => &self.draft.api_base_url,
                                    Auth => &self.draft.auth_base_url,
                                    GatewayUrl => &self.draft.gateway_url,
                                    GatewayKey => &self.draft.gateway_key,
                                    UpdateR2 => &self.draft.update_r2_url,
                                    _ => &self.draft.forum_base_url,
                                });
                                self.editing = Some(field);
                            }
                            _ => {}
                        }
                    }
                    if self.editing.is_none() && self.picker.is_none() {
                        self.save(cx);
                    }
                }
            }
            _ => {}
        }
        cx.request_redraw(RefreshHint::Ui);
        Transition::None
    }
}
