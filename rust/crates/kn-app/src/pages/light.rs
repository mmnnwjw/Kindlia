//! 阅读菜单里的快捷亮度/色温面板 (小说、漫画阅读页共用): 挂在菜单顶栏下方,
//! 每行 "名称 值 | − | 滑条 | ＋"。点滑条直接跳到该位置, 在滑条上横向滑动则取松手位置;
//! 改动只刷新面板这一小块 (`RefreshHint::Ui`)。机型不支持的项不显示 (都不支持则整个面板不画)。

use kn_platform::{Frontlight, Gesture, LightProp};
use kn_render::{Bitmap, Point, Rect};
use kn_ui::widgets::{self, Ink, Metrics};
use kn_ui::{Cx, HitId, Hits, RefreshHint, Theme};

use crate::Kindlia;

const HIT_PANEL: HitId = HitId(100);
const HIT_BRIGHT_DOWN: HitId = HitId(101);
const HIT_BRIGHT_SLIDER: HitId = HitId(102);
const HIT_BRIGHT_UP: HitId = HitId(103);
const HIT_WARM_DOWN: HitId = HitId(104);
const HIT_WARM_SLIDER: HitId = HitId(105);
const HIT_WARM_UP: HitId = HitId(106);

const ROWS: [(LightProp, &str, [HitId; 3]); 2] = [
    (
        LightProp::Brightness,
        "亮度",
        [HIT_BRIGHT_DOWN, HIT_BRIGHT_SLIDER, HIT_BRIGHT_UP],
    ),
    (
        LightProp::Warmth,
        "色温",
        [HIT_WARM_DOWN, HIT_WARM_SLIDER, HIT_WARM_UP],
    ),
];

/// 在 `top` 处画面板, 返回面板下沿 (什么都不支持时原样返回 `top`)。
#[allow(clippy::too_many_arguments)]
pub fn draw(
    ink: &mut Ink,
    frame: &mut Bitmap,
    hits: &mut Hits,
    theme: &Theme,
    m: &Metrics,
    light: &Frontlight,
    w: u32,
    top: i32,
) -> i32 {
    let rows: Vec<_> = ROWS
        .iter()
        .filter_map(|&(prop, name, ids)| Some((light.get(prop)?, name, ids)))
        .collect();
    if rows.is_empty() {
        return top;
    }
    let (theme, m) = (*theme, *m);
    let pad = m.margin as i32 / 2;
    let gap = (m.margin as f32 * 0.5) as i32;
    let btn = m.touch as i32;
    let side = m.margin as i32;
    let height = pad + rows.len() as i32 * btn + (rows.len() as i32 - 1) * gap + pad;
    let panel = Rect::new(0, top, w, height as u32);
    frame.fill_rect(panel, theme.background);
    frame.fill_rect(Rect::new(0, panel.bottom() - 2, w, 2), theme.foreground);
    hits.add(HIT_PANEL, panel).no_feedback();

    let label_w = ink.width("色温 24", m.small).ceil() as i32 + gap;
    let mut y = top + pad;
    for (level, name, [down, slider, up]) in rows {
        let value = level.value.to_string();
        widgets::slider_row(
            ink,
            frame,
            hits,
            &theme,
            &m,
            Rect::new(side, y, w - 2 * side as u32, btn as u32),
            name,
            &value,
            label_w,
            level.value,
            level.max,
            down,
            slider,
            up,
        );
        y += btn + gap;
    }
    panel.bottom()
}

fn row_of(id: HitId) -> Option<(LightProp, usize)> {
    ROWS.iter()
        .find_map(|(prop, _, ids)| ids.iter().position(|i| *i == id).map(|k| (*prop, k)))
}

fn value_at(cx: &Cx<Kindlia>, prop: LightProp, slider: HitId, x: i32) -> Option<i32> {
    let max = cx.app.light.get(prop)?.max;
    let knob = (cx.app.metrics.touch as f32 * 0.42) as i32;
    let track = widgets::track_rect(cx.hits.rect_of(slider)?, knob);
    let t = (x - track.x).clamp(0, track.w as i32) as f32 / track.w.max(1) as f32;
    Some((t * max as f32).round() as i32)
}

fn apply(cx: &mut Cx<Kindlia>, prop: LightProp, value: i32) {
    let before = cx.app.light.get(prop).map(|l| l.value);
    if cx.app.light.set(prop, value) != before {
        cx.request_redraw(RefreshHint::Ui);
    }
}

/// 处理面板内的点击; 不是面板的命中返回 false。
pub fn on_tap(cx: &mut Cx<Kindlia>, id: HitId, p: Point) -> bool {
    if id == HIT_PANEL {
        return true;
    }
    let Some((prop, k)) = row_of(id) else {
        return false;
    };
    let Some(current) = cx.app.light.get(prop) else {
        return true;
    };
    let value = match k {
        0 => Some(current.value - 1),
        1 => value_at(cx, prop, id, p.x),
        _ => Some(current.value + 1),
    };
    if let Some(v) = value {
        apply(cx, prop, v);
    }
    true
}

/// 起点在面板里的滑动: 起点在滑条上则取松手位置的值; 其余吞掉 (不翻页)。
pub fn on_swipe(cx: &mut Cx<Kindlia>, g: &Gesture) -> bool {
    let start = Point {
        x: g.start.0,
        y: g.start.1,
    };
    let Some(id) = cx.hits.at(start) else {
        return false;
    };
    if id == HIT_PANEL {
        return true;
    }
    let Some((prop, 1)) = row_of(id) else {
        return row_of(id).is_some();
    };
    if let Some(v) = value_at(cx, prop, id, g.end.0) {
        apply(cx, prop, v);
    }
    true
}
