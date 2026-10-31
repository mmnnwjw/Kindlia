pub mod book;
pub mod bookmarks;
pub mod choice;
pub mod cloud;
pub mod comments;
pub mod detail;
pub mod filters;
pub mod formatted;
pub mod import;
pub mod library;
pub mod light;
pub mod lock;
pub mod login;
pub mod manage;
pub mod me;
pub mod ranks;
pub mod reader;
pub mod settings;
pub mod storage;
pub mod stats;
pub mod text;
pub mod update;

use crate::Kindlia;
use kn_platform::{GestureKind, InputEvent};
use kn_render::{Bitmap, Rect};
use kn_ui::{
    widgets::{self, ButtonStyle, Ink},
    Cx, HitId,
};

pub fn hit(cx: &Cx<Kindlia>, event: &InputEvent) -> Option<u32> {
    if let InputEvent::Gesture(g) = event {
        if g.kind == GestureKind::Tap {
            return cx
                .hits
                .at(kn_render::Point {
                    x: g.end.0,
                    y: g.end.1,
                })
                .map(|id| id.0);
        }
    }
    None
}
pub fn header(cx: &mut Cx<Kindlia>, frame: &mut Bitmap, title: &str, back: bool) {
    let m = cx.app.metrics;
    let mut ink = Ink {
        fonts: cx.fonts,
        glyphs: cx.glyphs,
        chain: &cx.app.ui_fonts,
    };
    widgets::header(
        &mut ink,
        frame,
        &cx.theme,
        &m,
        title,
        &status_text(),
        back,
        false,
    );
    if back {
        cx.hits
            .add(HitId(1), Rect::new(0, 0, m.header_h(), m.header_h()));
    }
}
pub const TAB_BASE: u32 = 900_000;
pub fn draw_tabs(cx: &mut Cx<Kindlia>, frame: &mut Bitmap, active: usize) {
    let mut ink = Ink {
        fonts: cx.fonts,
        glyphs: cx.glyphs,
        chain: &cx.app.ui_fonts,
    };
    widgets::tab_bar(
        &mut ink,
        frame,
        cx.hits,
        &cx.theme,
        &cx.app.metrics,
        &["书架", "历史", "发现", "我的"],
        active,
        TAB_BASE,
    );
}
pub fn tab_input(
    cx: &Cx<Kindlia>,
    event: &InputEvent,
    active: usize,
) -> Option<kn_ui::Transition<Kindlia>> {
    let id = hit(cx, event)?;
    let i = id.checked_sub(TAB_BASE)? as usize;
    if i > 3 {
        return None;
    }
    Some(if i == active {
        kn_ui::Transition::None
    } else {
        kn_ui::Transition::Root(match i {
            0 => Box::new(library::LibraryPage::shelf()),
            1 if cx.app.repo.api.signed_in() => {
                Box::new(cloud::CloudPage::new(cloud::Mode::History))
            }
            1 => Box::new(library::LibraryPage::history()),
            2 => Box::new(library::LibraryPage::discover()),
            _ => Box::new(me::MePage::default()),
        })
    })
}
pub fn status_text() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let minute = (now + crate::tz::local_utc_offset_secs(now)).rem_euclid(86400) / 60;
    let mut s = format!("{:02}:{:02}", minute / 60, minute % 60);
    if let Some(level) = battery_level() {
        s.push_str(&format!(" · {level}%"));
    }
    s
}
// KinNovel 的电源节点遍历：Kindle 不保证节点名为 battery。
fn battery_level() -> Option<u32> {
    use std::{
        sync::{Mutex, OnceLock},
        time::{Duration, Instant},
    };
    static CACHE: OnceLock<Mutex<Option<(Instant, Option<u32>)>>> = OnceLock::new();
    let mut cached = CACHE.get_or_init(|| Mutex::new(None)).lock().ok()?;
    if let Some((at, level)) = *cached {
        if at.elapsed() < Duration::from_secs(60) {
            return level;
        }
    }
    let level = read_battery_level();
    *cached = Some((Instant::now(), level));
    level
}
fn read_battery_level() -> Option<u32> {
    for entry in std::fs::read_dir("/sys/class/power_supply").ok()?.flatten() {
        if let Ok(text) = std::fs::read_to_string(entry.path().join("capacity")) {
            if let Ok(v) = text.trim().parse::<u32>() {
                if v <= 100 {
                    return Some(v);
                }
            }
        }
    }
    None
}
pub fn search_header(cx: &mut Cx<Kindlia>, frame: &mut Bitmap, title: &str, back: bool, id: u32) {
    let m = cx.app.metrics;
    let mut ink = Ink {
        fonts: cx.fonts,
        glyphs: cx.glyphs,
        chain: &cx.app.ui_fonts,
    };
    widgets::header_with_icon(
        &mut ink,
        frame,
        &cx.theme,
        &m,
        title,
        &status_text(),
        back,
        Some(widgets::HeaderIcon::Search),
    );
    cx.hits.add(
        HitId(id),
        Rect::new(
            (cx.width - m.header_h()) as i32,
            0,
            m.header_h(),
            m.header_h(),
        ),
    );
    if back {
        cx.hits
            .add(HitId(1), Rect::new(0, 0, m.header_h(), m.header_h()));
    }
}
pub fn selected_button(
    cx: &mut Cx<Kindlia>,
    frame: &mut Bitmap,
    rect: Rect,
    text: &str,
    id: u32,
    selected: bool,
) {
    let m = cx.app.metrics;
    let mut ink = Ink {
        fonts: cx.fonts,
        glyphs: cx.glyphs,
        chain: &cx.app.ui_fonts,
    };
    widgets::button(
        &mut ink,
        frame,
        &cx.theme,
        &m,
        rect.inflate(-5),
        text,
        if selected {
            ButtonStyle::Primary
        } else {
            ButtonStyle::Secondary
        },
    );
    cx.hits.add(HitId(id), rect).rounded(m.radius);
}
pub fn label(cx: &mut Cx<Kindlia>, frame: &mut Bitmap, rect: Rect, text: &str, muted: bool) {
    let mut ink = Ink {
        fonts: cx.fonts,
        glyphs: cx.glyphs,
        chain: &cx.app.ui_fonts,
    };
    ink.text_centered(
        frame,
        rect,
        text,
        cx.app.metrics.small,
        if muted {
            cx.theme.muted
        } else {
            cx.theme.foreground
        },
    );
}
pub fn option_button(
    cx: &mut Cx<Kindlia>,
    frame: &mut Bitmap,
    rect: Rect,
    title: &str,
    value: &str,
    id: u32,
) {
    let m = cx.app.metrics;
    let r = rect.inflate(-5);
    frame.rounded_rect(
        r,
        m.radius,
        Some(cx.theme.background),
        Some(cx.theme.foreground),
        2,
    );
    let mut ink = Ink {
        fonts: cx.fonts,
        glyphs: cx.glyphs,
        chain: &cx.app.ui_fonts,
    };
    let title = ink.fit(title, m.body, (r.w.saturating_sub(12)) as f32);
    let value = ink.fit(value, m.small, (r.w.saturating_sub(12)) as f32);
    ink.text_centered(
        frame,
        Rect::new(r.x, r.y, r.w, r.h * 3 / 5),
        &title,
        m.body,
        cx.theme.foreground,
    );
    ink.text_centered(
        frame,
        Rect::new(r.x, r.y + (r.h * 3 / 5) as i32, r.w, r.h * 2 / 5),
        &value,
        m.small,
        cx.theme.muted,
    );
    cx.hits.add(HitId(id), rect).rounded(m.radius);
}
pub fn button(
    cx: &mut Cx<Kindlia>,
    frame: &mut Bitmap,
    rect: Rect,
    text: &str,
    id: u32,
    enabled: bool,
) {
    let m = cx.app.metrics;
    let mut ink = Ink {
        fonts: cx.fonts,
        glyphs: cx.glyphs,
        chain: &cx.app.ui_fonts,
    };
    widgets::button(
        &mut ink,
        frame,
        &cx.theme,
        &m,
        rect.inflate(-5),
        text,
        if enabled {
            ButtonStyle::Secondary
        } else {
            ButtonStyle::Disabled
        },
    );
    cx.hits
        .add(HitId(id), rect)
        .enabled(enabled)
        .rounded(m.radius);
}
pub fn row(
    cx: &mut Cx<Kindlia>,
    frame: &mut Bitmap,
    rect: Rect,
    title: &str,
    subtitle: &str,
    meta: &str,
    id: Option<u32>,
) {
    let mut ink = Ink {
        fonts: cx.fonts,
        glyphs: cx.glyphs,
        chain: &cx.app.ui_fonts,
    };
    widgets::list_row(
        &mut ink,
        frame,
        &cx.theme,
        &cx.app.metrics,
        rect,
        title,
        subtitle,
        meta,
    );
    if let Some(id) = id {
        cx.hits.add(HitId(id), rect).long_press();
    }
}
pub fn pager(cx: &mut Cx<Kindlia>, frame: &mut Bitmap, page: usize, pages: usize, bottom: u32) {
    let m = cx.app.metrics;
    let rect = Rect::new(
        0,
        cx.height.saturating_sub(bottom + m.pager_h()) as i32,
        cx.width,
        m.pager_h(),
    );
    let mut ink = Ink {
        fonts: cx.fonts,
        glyphs: cx.glyphs,
        chain: &cx.app.ui_fonts,
    };
    widgets::pager(
        &mut ink,
        frame,
        cx.hits,
        &cx.theme,
        &m,
        rect,
        page,
        pages.max(1),
        HitId(2),
        HitId(3),
    );
}

pub mod image;
