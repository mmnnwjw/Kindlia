//! 操作教程弹窗: 第一次用到某个页面时弹出该页的手势说明。两个按钮:
//! 「我知道了」= 不再提示 (记到 `config.tips_seen`); 「感觉会忘记」= 这次先关掉,
//! 下次启动 Kindlia 后再进这个页面时仍会提示 (只记在内存 `tips_later` 里)。
//! 设置里关掉「操作提示」则全部不弹; 重新打开会把记录清空。
//!
//! 页面接入: `render` 最后调 [`show`], `on_input` 最前面调 [`input`]。

use kn_platform::{GestureKind, InputEvent};
use kn_render::{Bitmap, Point};
use kn_ui::widgets::{self, Ink};
use kn_ui::{Cx, HitId, RefreshHint};

use crate::Kindlia;

const HIT_GOT_IT: HitId = HitId(u32::MAX - 1);
const HIT_LATER: HitId = HitId(u32::MAX - 2);

pub struct Tip {
    pub key: &'static str,
    pub title: &'static str,
    pub lines: &'static [&'static str],
}

pub const SHELF: Tip = Tip {
    key: "shelf",
    title: "书架",
    lines: &[
        "顶部可切换「Web收藏」或「文库收藏」；本机书籍在「我的 → 本地书籍」。",
        "点收藏夹查看收藏书籍，长按收藏夹可重命名或删除。",
        "若有最近阅读记录，点顶部卡片可直接继续阅读。",
        "列表上下滑动或点底部分页条翻页，其他列表页也一样。",
    ],
};

pub const SEARCH: Tip = Tip {
    key: "search",
    title: "发现与搜索",
    lines: &[
        "点右上角放大镜进入搜索，左上角返回退出搜索状态。",
        "可按来源、类别、排序与分级筛选，点选选项框在弹窗里切换。",
        "支持切换「Web小说」「文库小说」与各站点榜单。",
    ],
};

pub const READER: Tip = Tip {
    key: "reader",
    title: "阅读",
    lines: &[
        "点屏幕右侧或左滑 / 上滑：下一页；点左侧或右滑：上一页。",
        "点屏幕中部或从顶部下滑：打开菜单，上滑或轻触正文区域收起。",
        "菜单顶栏的书签按钮可添加或取消书签，长按可管理全部书签。",
        "拖动菜单里的进度条：跳到本章任意一页。",
        "点插图全屏查看并放大；点正文里的「注」查看注释。",
    ],
};

pub const KEYBOARD: Tip = Tip {
    key: "keyboard",
    title: "屏幕键盘",
    lines: &[
        "「中 / 英」键切换拼音和英文。",
        "打拼音后点候选词输入；按空格、回车或标点输入第一个候选，候选多时按 ← → 翻看。",
        "「符号」键切换到标点符号，Shift 换一页；在 ， 。 键上向上滑输入 ； ：",
        "长按退格清空输入，长按 Shift 锁定大写。",
    ],
};

pub const BOOKMARKS: Tip = Tip {
    key: "bookmarks",
    title: "目录与书签",
    lines: &[
        "点章节或书签直接跳转阅读。",
        "在「书签」里长按一条书签可以删除它。",
    ],
};

pub const IMAGE: Tip = Tip {
    key: "image",
    title: "查看插图",
    lines: &[
        "用底部的 － ＋ 放大缩小，放大后上下左右滑动查看其它部分。",
        "「原图」下载未压缩的图片。",
        "轻触图片显示或隐藏底部工具栏。",
    ],
};

pub const COMMENTS: Tip = Tip {
    key: "comments",
    title: "评论与回复",
    lines: &[
        "评论全文与缩进回复依次展示，底部分页条显示当前远端页与屏次。",
        "点按具体评论可选择回复；自己的评论支持编辑或删除。",
    ],
};

/// 这个提示现在该不该弹: 设置里开着、没点过「我知道了」、这次运行也没点过「感觉会忘记」。
fn pending(app: &Kindlia, key: &str) -> bool {
    app.config.operation_tips
        && !app.tips_later.contains(key)
        && !app.config.tips_seen.iter().any(|k| k == key)
}

/// 在 `render` 最后调用: `when` 为真且该弹时弹出, 弹出中的这个提示画在最上层。
pub fn show(cx: &mut Cx<Kindlia>, frame: &mut Bitmap, tip: &'static Tip, when: bool) {
    if when && cx.app.tip_open != Some(tip.key) && pending(cx.app, tip.key) {
        cx.app.tip_open = Some(tip.key);
    }
    if cx.app.tip_open == Some(tip.key) {
        let (theme, m) = (cx.theme, cx.app.metrics);
        let chain = cx.app.ui_fonts.clone();
        let mut ink = Ink {
            fonts: &*cx.fonts,
            glyphs: &mut *cx.glyphs,
            chain: &chain,
        };
        widgets::tip_dialog(
            &mut ink,
            frame,
            cx.hits,
            &theme,
            &m,
            tip.title,
            tip.lines,
            HIT_LATER,
            HIT_GOT_IT,
        );
    }
}

/// 在 `on_input` 最前面调用: 这个提示正弹着时处理并吞掉输入 (返回 true)。
pub fn input(cx: &mut Cx<Kindlia>, event: &InputEvent, tip: &'static Tip) -> bool {
    if cx.app.tip_open != Some(tip.key) {
        return false;
    }
    if let InputEvent::Gesture(g) = event {
        if g.kind == GestureKind::Tap {
            match cx.hits.at(Point {
                x: g.start.0,
                y: g.start.1,
            }) {
                Some(HIT_GOT_IT) => {
                    if !cx.app.config.tips_seen.iter().any(|k| k == tip.key) {
                        cx.app.config.tips_seen.push(tip.key.to_string());
                        let _ = cx.app.save_config();
                    }
                    close(cx);
                }
                Some(HIT_LATER) => {
                    cx.app.tips_later.insert(tip.key);
                    close(cx);
                }
                _ => {}
            }
        }
    }
    true
}

fn close(cx: &mut Cx<Kindlia>) {
    cx.app.tip_open = None;
    cx.request_ui_redraw(RefreshHint::Flash);
}
