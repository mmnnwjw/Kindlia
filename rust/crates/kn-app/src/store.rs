//! 沿用 KinNovel 的临时文件 → sync → rename 策略。缓存与用户数据严格分开。
use crate::{
    api::normalize_base,
    model::{digest, NovelKey},
    paragraphs::ReadingMode,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
};

pub fn atomic_write(path: &Path, bytes: &[u8], durable: bool) -> io::Result<()> {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("缺少父目录"))?;
    fs::create_dir_all(parent)?;
    let temp = path.with_extension(format!(
        "tmp-kindlia-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.write_all(bytes)?;
        if durable {
            file.sync_all()?;
        }
        drop(file);
        fs::rename(&temp, path)?;
        #[cfg(unix)]
        if durable {
            if let Ok(dir) = fs::File::open(parent) {
                let _ = dir.sync_all();
            }
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}

pub fn root_dir() -> PathBuf {
    std::env::var_os("KINDLIA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            if cfg!(target_os = "linux") {
                PathBuf::from("/mnt/us/extensions/kindlia")
            } else {
                PathBuf::from("../local-data")
            }
        })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub api_base_url: String,
    pub gateway_enabled: bool,
    pub gateway_url: String,
    pub gateway_key: String,
    pub timeout_secs: u64,
    pub strict_tls: bool,
    pub offline: bool,
    pub font_size: u32,
    pub line_spacing: f64,
    pub margin: u32,
    pub reading_mode: ReadingMode,
    pub landscape: bool,
    pub night: bool,
    /// 可放入 NotoSansCJKjp-Regular.otf 等日文字体；相对路径基于 KINDLIA_HOME。
    pub font_paths: Vec<String>,
    pub translation: String,
    pub show_original: bool,
    pub show_translation: bool,
    pub reader_font: String,
    pub auth_base_url: String,
    pub forum_base_url: String,
    pub paragraph_spacing: f64,
    pub first_line_indent: bool,
    pub welcome_skipped: bool,
    pub page_turn_animation: bool,
    pub swipe_steps: u8,
    pub full_refresh_every: u8,
    pub prefetch_chapters: bool,
    pub original_size: u16,
    pub original_gray: u8,
    pub ui_refresh_mode: String,
    pub page_flash: bool,
    pub landscape_side: String,
    pub sleep_cover: bool,
    pub operation_tips: bool,
    pub tips_seen: Vec<u16>,
    pub ignore_japanese: bool,
    pub ignore_ai: bool,
    pub cache_limit_mb: u32,
    pub lock_pin: String,
    pub convert: String,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            api_base_url: "https://n.novelia.cc".into(),
            gateway_enabled: false,
            gateway_url: String::new(),
            gateway_key: String::new(),
            timeout_secs: 30,
            strict_tls: true,
            offline: false,
            font_size: 48,
            line_spacing: 1.42,
            margin: 34,
            reading_mode: ReadingMode::Auto,
            landscape: false,
            night: false,
            font_paths: vec![],
            translation: "auto".into(),
            show_original: false,
            show_translation: true,
            reader_font: String::new(),
            auth_base_url: "https://auth.novelia.cc".into(),
            forum_base_url: "https://forum.novelia.cc".into(),
            paragraph_spacing: 0.22,
            first_line_indent: false,
            welcome_skipped: false,
            page_turn_animation: true,
            swipe_steps: 12,
            full_refresh_every: 6,
            prefetch_chapters: false,
            original_size: 85,
            original_gray: 80,
            ui_refresh_mode: "smooth".into(),
            page_flash: false,
            landscape_side: "left".into(),
            sleep_cover: false,
            operation_tips: true,
            tips_seen: vec![],
            ignore_japanese: false,
            ignore_ai: false,
            cache_limit_mb: 192,
            lock_pin: String::new(),
            convert: String::new(),
        }
    }
}
impl Config {
    pub fn load(root: &Path) -> Result<Self, String> {
        let path = root.join("config.toml");
        if !path.exists() {
            let config = Self::default();
            config.save(root)?;
            return Ok(config);
        }
        let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
        let mut config: Self = toml::from_str(&text).map_err(|e| format!("config.toml: {e}"))?;
        if !text.contains("show_original") {
            config.show_original = matches!(
                config.reading_mode,
                ReadingMode::Japanese | ReadingMode::Bilingual
            );
            config.show_translation = config.reading_mode != ReadingMode::Japanese;
            config.translation = match config.reading_mode {
                ReadingMode::Sakura => "sakura",
                ReadingMode::Gpt => "gpt",
                ReadingMode::Youdao => "youdao",
                ReadingMode::Baidu => "baidu",
                _ => "auto",
            }
            .into();
        }
        config.api_base_url = normalize_base(&config.api_base_url)?;
        config.font_size = config.font_size.clamp(24, 96);
        config.line_spacing = config.line_spacing.clamp(1.0, 2.5);
        config.margin = config.margin.clamp(8, 120);
        Ok(config)
    }
    pub fn save(&self, root: &Path) -> Result<(), String> {
        atomic_write(
            &root.join("config.toml"),
            toml::to_string_pretty(self)
                .map_err(|e| e.to_string())?
                .as_bytes(),
            true,
        )
        .map_err(|e| e.to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Progress {
    pub chapter_id: String,
    pub path: String,
    pub offset: usize,
    pub updated_at: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShelfEntry {
    pub key: NovelKey,
    pub title: String,
    pub favorite: bool,
    pub progress: Option<Progress>,
}

#[derive(Clone)]
pub struct Store {
    pub root: PathBuf,
    namespace: String,
    gate: Arc<Mutex<()>>,
    pending_progress: Arc<Mutex<std::collections::HashMap<NovelKey, PendingProgress>>>,
    cached_shelf: Arc<Mutex<Option<Vec<ShelfEntry>>>>,
}
#[derive(Clone)]
pub(crate) struct PendingProgress {
    pub sequence: u64,
    pub title: String,
    pub user: String,
    pub progress: Progress,
}
impl Store {
    pub fn new(root: PathBuf, base: &str) -> Self {
        Self {
            root,
            namespace: digest(&base),
            gate: Arc::new(Mutex::new(())),
            pending_progress: Arc::new(Mutex::new(std::collections::HashMap::new())),
            cached_shelf: Arc::new(Mutex::new(None)),
        }
    }
    pub fn cache_path(&self, kind: &str, key: &impl Serialize) -> PathBuf {
        self.root
            .join("cache")
            .join(&self.namespace)
            .join(kind)
            .join(format!("{}.json", digest(key)))
    }
    pub fn data_path(&self, name: &str) -> PathBuf {
        self.root.join("data").join(&self.namespace).join(name)
    }
    pub fn bookmarks(&self, key: &NovelKey) -> Vec<(String, String, usize)> {
        std::fs::read(self.data_path(&format!("bookmarks-{}.json", digest(key))))
            .ok()
            .and_then(|d| serde_json::from_slice(&d).ok())
            .unwrap_or_default()
    }
    pub fn save_bookmarks(
        &self,
        key: &NovelKey,
        value: &[(String, String, usize)],
    ) -> Result<(), String> {
        atomic_write(
            &self.data_path(&format!("bookmarks-{}.json", digest(key))),
            &serde_json::to_vec(value).map_err(|e| e.to_string())?,
            true,
        )
        .map_err(|e| e.to_string())
    }
    pub fn read_cache<T: DeserializeOwned>(&self, kind: &str, key: &impl Serialize) -> Option<T> {
        let path = self.cache_path(kind, key);
        let bytes = fs::read(&path)
            .or_else(|e| {
                if kind == "epub-source" {
                    fs::read(self.data_path(&format!("epub-source-{}.json", digest(key))))
                } else {
                    Err(e)
                }
            })
            .ok()?;
        match serde_json::from_slice(&bytes) {
            Ok(value) => Some(value),
            Err(_) => {
                let _ = fs::remove_file(path);
                None
            }
        }
    }
    pub fn write_cache(
        &self,
        kind: &str,
        key: &impl Serialize,
        value: &impl Serialize,
    ) -> Result<(), String> {
        let path = if kind == "epub-source" {
            self.data_path(&format!("epub-source-{}.json", digest(key)))
        } else {
            self.cache_path(kind, key)
        };
        atomic_write(
            &path,
            &serde_json::to_vec(value).map_err(|e| e.to_string())?,
            false,
        )
        .map_err(|e| e.to_string())
    }
    fn shelf_path(&self) -> PathBuf {
        self.root
            .join("data")
            .join(&self.namespace)
            .join("shelf.json")
    }
    fn disk_shelf(&self) -> Result<Vec<ShelfEntry>, String> {
        let path = self.shelf_path();
        match fs::read(path) {
            Ok(data) => {
                serde_json::from_slice(&data).map_err(|e| format!("书架损坏，未覆盖原文件: {e}"))
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(vec![]),
            Err(e) => Err(e.to_string()),
        }
    }
    pub fn shelf(&self) -> Result<Vec<ShelfEntry>, String> {
        let cached = self.cached_shelf.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let mut entries = if let Some(entries) = cached { entries } else {
            let entries = self.disk_shelf()?;
            self.cached_shelf.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert(entries).clone()
        };
        let pending = self.pending_progress.lock().unwrap_or_else(|e| e.into_inner());
        for (key, value) in pending.iter() {
            if let Some(entry) = entries.iter_mut().find(|e| e.key == *key) {
                entry.title = value.title.clone();
                entry.progress = Some(value.progress.clone());
            } else {
                entries.push(ShelfEntry { key: key.clone(), title: value.title.clone(), favorite: false, progress: Some(value.progress.clone()) });
            }
        }
        entries.sort_by_key(|e| std::cmp::Reverse(e.progress.as_ref().map_or(0, |p| p.updated_at)));
        Ok(entries)
    }
    pub(crate) fn stage_progress(&self, key: NovelKey, title: String, progress: Progress, user: String) {
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        self.pending_progress.lock().unwrap_or_else(|e| e.into_inner()).insert(key, PendingProgress {
            sequence: SEQUENCE.fetch_add(1, Ordering::Relaxed), title, user, progress,
        });
    }
    pub(crate) fn progress_is_current(&self, key: &NovelKey, sequence: u64) -> bool {
        self.pending_progress.lock().unwrap_or_else(|e| e.into_inner()).get(key).is_some_and(|p| p.sequence == sequence)
    }
    pub(crate) fn finish_progress(&self, key: &NovelKey, sequence: u64) {
        let mut pending = self.pending_progress.lock().unwrap_or_else(|e| e.into_inner());
        if pending.get(key).is_some_and(|p| p.sequence == sequence) { pending.remove(key); }
    }
    pub(crate) fn persist_progress(&self, key: &NovelKey) -> Result<Option<PendingProgress>, String> {
        let _lock = self.gate.lock().unwrap_or_else(|e| e.into_inner());
        let Some(value) = self.pending_progress.lock().unwrap_or_else(|e| e.into_inner()).get(key).cloned() else { return Ok(None); };
        let mut entries = self.disk_shelf()?;
        if let Some(entry) = entries.iter_mut().find(|e| e.key == *key) {
            entry.title = value.title.clone();
            entry.progress = Some(value.progress.clone());
        } else {
            entries.push(ShelfEntry { key: key.clone(), title: value.title.clone(), favorite: false, progress: Some(value.progress.clone()) });
        }
        entries.sort_by_key(|e| std::cmp::Reverse(e.progress.as_ref().map_or(0, |p| p.updated_at)));
        atomic_write(&self.shelf_path(), &serde_json::to_vec_pretty(&entries).map_err(|e| e.to_string())?, true).map_err(|e| e.to_string())?;
        *self.cached_shelf.lock().unwrap_or_else(|e| e.into_inner()) = Some(entries);
        Ok(Some(value))
    }
    pub fn update_entry(
        &self,
        key: &NovelKey,
        title: &str,
        change: impl FnOnce(&mut ShelfEntry),
    ) -> Result<(), String> {
        let _lock = self.gate.lock().unwrap_or_else(|e| e.into_inner());
        let mut entries = self.disk_shelf()?;
        let i = entries
            .iter()
            .position(|e| &e.key == key)
            .unwrap_or_else(|| {
                entries.push(ShelfEntry {
                    key: key.clone(),
                    title: title.into(),
                    favorite: false,
                    progress: None,
                });
                entries.len() - 1
            });
        entries[i].title = title.into();
        change(&mut entries[i]);
        entries.sort_by_key(|e| std::cmp::Reverse(e.progress.as_ref().map_or(0, |p| p.updated_at)));
        atomic_write(
            &self.shelf_path(),
            &serde_json::to_vec_pretty(&entries).map_err(|e| e.to_string())?,
            true,
        )
        .map_err(|e| e.to_string())?;
        *self.cached_shelf.lock().unwrap_or_else(|e| e.into_inner()) = Some(entries);
        Ok(())
    }
    pub fn clear_history(&self) -> Result<(), String> {
        let _lock = self.gate.lock().unwrap_or_else(|e| e.into_inner());
        let mut entries = self.disk_shelf()?;
        self.pending_progress.lock().unwrap_or_else(|e| e.into_inner()).clear();
        for e in &mut entries {
            e.progress = None;
        }
        entries.retain(|e| e.favorite);
        atomic_write(
            &self.shelf_path(),
            &serde_json::to_vec_pretty(&entries).map_err(|e| e.to_string())?,
            true,
        )
        .map_err(|e| e.to_string())?;
        *self.cached_shelf.lock().unwrap_or_else(|e| e.into_inner()) = Some(entries);
        Ok(())
    }
    pub fn progress(&self, key: &NovelKey) -> Option<Progress> {
        self.shelf()
            .ok()?
            .into_iter()
            .find(|e| &e.key == key)?
            .progress
    }
}
