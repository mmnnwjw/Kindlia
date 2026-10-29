//! AutoNovel 的数组输入直接转为 kn-text 块，不经过 HTML，正文中的标签不会被解释。
use crate::model::Chapter;
use kn_text::{Block, BlockKind};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadingMode {
    #[default]
    Auto,
    Sakura,
    Gpt,
    Youdao,
    Baidu,
    Japanese,
    Bilingual,
}
impl ReadingMode {
    pub const ALL: [Self; 7] = [
        Self::Auto,
        Self::Sakura,
        Self::Gpt,
        Self::Youdao,
        Self::Baidu,
        Self::Japanese,
        Self::Bilingual,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "中文优选",
            Self::Sakura => "Sakura",
            Self::Gpt => "GPT",
            Self::Youdao => "有道",
            Self::Baidu => "百度",
            Self::Japanese => "日语原文",
            Self::Bilingual => "中日对照",
        }
    }
    pub fn next(self) -> Self {
        Self::ALL[(Self::ALL.iter().position(|i| *i == self).unwrap_or(0) + 1) % Self::ALL.len()]
    }
}

pub struct BuiltChapter {
    pub blocks: Vec<Block>,
    pub notice: Option<String>,
}

pub fn build(chapter: &Chapter, mode: ReadingMode) -> BuiltChapter {
    build_preferences(
        chapter,
        match mode {
            ReadingMode::Sakura => "sakura",
            ReadingMode::Gpt => "gpt",
            ReadingMode::Youdao => "youdao",
            ReadingMode::Baidu => "baidu",
            _ => "auto",
        },
        matches!(mode, ReadingMode::Japanese | ReadingMode::Bilingual),
        mode != ReadingMode::Japanese,
    )
}
pub fn build_preferences(
    chapter: &Chapter,
    preference: &str,
    original: bool,
    translated: bool,
) -> BuiltChapter {
    let mode = if !translated {
        ReadingMode::Japanese
    } else {
        match preference {
            "sakura" => ReadingMode::Sakura,
            "gpt" => ReadingMode::Gpt,
            "youdao" => ReadingMode::Youdao,
            "baidu" => ReadingMode::Baidu,
            _ => ReadingMode::Auto,
        }
    };
    // 原文决定空行和插图；译文只按同索引读，图片不在双语模式重复。
    let candidates = [
        (&chapter.sakura_paragraphs, "Sakura"),
        (&chapter.gpt_paragraphs, "GPT"),
        (&chapter.youdao_paragraphs, "有道"),
        (&chapter.baidu_paragraphs, "百度"),
    ];
    let wanted = match mode {
        ReadingMode::Sakura => Some(0),
        ReadingMode::Gpt => Some(1),
        ReadingMode::Youdao => Some(2),
        ReadingMode::Baidu => Some(3),
        _ => None,
    };
    let selected = if mode == ReadingMode::Japanese {
        None
    } else if let Some(i) = wanted {
        candidates[i].0.as_ref().map(|p| (p, candidates[i].1))
    } else {
        candidates
            .iter()
            .find_map(|(p, name)| p.as_ref().filter(|p| !p.is_empty()).map(|p| (p, *name)))
    };
    let mut notice = if selected.is_none() && mode != ReadingMode::Japanese {
        Some("所选译文不存在，显示日语原文".to_owned())
    } else {
        None
    };
    let mut blocks = Vec::new();
    for (i, jp) in chapter.paragraphs.iter().enumerate() {
        // 路径以原文段落索引固定；换译文、字号、双语模式时沿用锚点。
        let path = format!("/p/{i}");
        let mut block = Block {
            path: path.clone(),
            ..Block::default()
        };
        if jp.trim().is_empty() {
            block.text = "　".into();
            blocks.push(block);
            continue;
        }
        if let Some(url) = jp.strip_prefix("<图片>") {
            let url = url.trim();
            if url.starts_with("https://") || url.starts_with("http://") {
                block.kind = BlockKind::Image;
                block.source_url = url.to_owned();
            } else {
                block.text = "[插图地址无效]".into();
            }
            blocks.push(block);
            continue;
        }
        let zh = selected.and_then(|(p, _)| p.get(i));
        if selected.is_some() && zh.is_none() {
            notice = Some("译文段落数量不足，缺失段落显示原文".into());
        }
        block.text = zh.unwrap_or(jp).clone();
        blocks.push(block);
        if original && translated && zh.is_some() {
            blocks.push(Block {
                text: jp.clone(),
                path: format!("/jp/{i}"),
                spans: vec![kn_text::Span {
                    start: 0,
                    end: jp.chars().count(),
                    style: kn_text::InlineStyle {
                        muted: true,
                        ..Default::default()
                    },
                }],
                ..Block::default()
            });
        }
    }
    BuiltChapter { blocks, notice }
}
