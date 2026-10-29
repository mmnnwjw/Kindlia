//! DTO 对照 server/api/RouteWebNovel.kt、api/model/WebNovel.kt 和 web/model/WebNovel.ts。
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NovelKey {
    pub provider_id: String,
    pub novel_id: String,
}

pub fn digest(value: &impl Serialize) -> String {
    format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(value).expect("serializable cache key"))
    )
}

impl NovelKey {
    pub fn cache_id(&self) -> String {
        format!("web_{}", digest(self))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Page<T> {
    pub items: Vec<T>,
    /// 总页数，绝不是当前页码。请求 page 从 0 开始。
    pub page_number: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NovelOutline {
    pub provider_id: String,
    pub novel_id: String,
    pub title_jp: String,
    pub title_zh: Option<String>,
    #[serde(rename = "type")]
    pub novel_type: Option<String>,
    pub attentions: Vec<String>,
    pub keywords: Vec<String>,
    pub extra: Option<String>,
    pub favored: Option<String>,
    pub last_read_at: Option<i64>,
    pub total: u64,
    pub jp: u64,
    #[serde(default)]
    pub baidu: u64,
    pub youdao: u64,
    pub gpt: u64,
    pub sakura: u64,
    pub update_at: Option<i64>,
}

pub fn preferred<'a>(zh: Option<&'a str>, jp: &'a str) -> &'a str {
    zh.filter(|s| !s.trim().is_empty()).unwrap_or(jp)
}

impl NovelOutline {
    pub fn key(&self) -> NovelKey {
        NovelKey {
            provider_id: self.provider_id.clone(),
            novel_id: self.novel_id.clone(),
        }
    }
    pub fn title(&self) -> &str {
        preferred(self.title_zh.as_deref(), &self.title_jp)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Author {
    pub name: String,
    pub link: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TocItem {
    pub title_jp: String,
    pub title_zh: Option<String>,
    /// null 是分卷标题，保留在目录中，但不可请求。
    pub chapter_id: Option<String>,
    pub create_at: Option<i64>,
}
impl TocItem {
    pub fn title(&self) -> &str {
        preferred(self.title_zh.as_deref(), &self.title_jp)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Novel {
    pub wenku_id: Option<String>,
    pub title_jp: String,
    pub title_zh: Option<String>,
    pub authors: Vec<Author>,
    #[serde(rename = "type")]
    pub novel_type: Option<String>,
    pub attentions: Vec<String>,
    pub keywords: Vec<String>,
    pub points: Option<i64>,
    pub total_characters: Option<u64>,
    pub introduction_jp: String,
    pub introduction_zh: Option<String>,
    pub glossary: std::collections::HashMap<String, String>,
    pub toc: Vec<TocItem>,
    pub visited: u64,
    pub sync_at: i64,
    pub favored: Option<String>,
    pub last_read_chapter_id: Option<String>,
    pub jp: u64,
    #[serde(default)]
    pub baidu: u64,
    pub youdao: u64,
    pub gpt: u64,
    pub sakura: u64,
}
impl Novel {
    pub fn title(&self) -> &str {
        preferred(self.title_zh.as_deref(), &self.title_jp)
    }
    pub fn adjacent(&self, chapter: &str, forward: bool) -> Option<String> {
        let ids: Vec<&str> = self
            .toc
            .iter()
            .filter_map(|i| i.chapter_id.as_deref())
            .collect();
        let i = ids.iter().position(|id| *id == chapter)?;
        let next = if forward {
            i.checked_add(1)?
        } else {
            i.checked_sub(1)?
        };
        ids.get(next).map(|id| (*id).to_owned())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Chapter {
    pub title_jp: String,
    pub title_zh: Option<String>,
    pub novel_title_jp: Option<String>,
    pub novel_title_zh: Option<String>,
    pub prev_id: Option<String>,
    pub next_id: Option<String>,
    pub paragraphs: Vec<String>,
    pub baidu_paragraphs: Option<Vec<String>>,
    pub youdao_paragraphs: Option<Vec<String>>,
    pub gpt_paragraphs: Option<Vec<String>>,
    pub sakura_paragraphs: Option<Vec<String>>,
}
impl Chapter {
    pub fn title(&self) -> &str {
        preferred(self.title_zh.as_deref(), &self.title_jp)
    }
}
