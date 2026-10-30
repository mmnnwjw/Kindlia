pub mod account;
pub mod api;
pub mod conversion;
pub mod covers;
pub mod epub;
pub mod fonts;
pub mod gateway;
pub mod history;
pub mod migration;
pub mod model;
pub mod pages;
pub mod paragraphs;
pub mod progress;
pub mod repository;
pub mod sleep;
pub mod stats;
pub mod store;
pub mod tz;
#[cfg(test)]
mod tests;

use kn_render::FontId;
use kn_ui::{widgets::Metrics, App, Page, Theme};
use repository::Repository;
use store::{Config, Store};

pub struct Kindlia {
    pub progress_writer: progress::Writer,
    pub config: Config,
    pub repo: Repository,
    pub ui_fonts: Vec<FontId>,
    pub reader_font_cache: Option<(String, FontId)>,
    pub metrics: Metrics,
    pub dpi: u32,
    pub light: kn_platform::Frontlight,
    pub reader_jump: Option<String>,
    pub discover_query: Option<api::ListQuery>,
    pub reader_anchor: Option<(String, usize)>,
    pub history_syncing: bool,
    pub history_status: String,
    pub unlocked: bool,
    pub lock_pending: bool,
    pub lock_showing: bool,
    pub sleep_request: Option<model::NovelKey>,
    pub sleep_image: Option<kn_render::Bitmap>,
}
impl Kindlia {
    pub fn new(
        root: std::path::PathBuf,
        config: Config,
        ui_fonts: Vec<FontId>,
        width: u32,
        height: u32,
        dpi: u32,
        light: kn_platform::Frontlight,
    ) -> Result<Self, String> {
        let mut api =
            api::ApiClient::new(&config.api_base_url, config.timeout_secs, config.strict_tls)?;
        let store = Store::new(root, &api::normalize_base(&config.api_base_url)?);
        api.configure_session(
            &config.auth_base_url,
            &config.forum_base_url,
            store.data_path("session.json"),
        )?;
        api.configure_gateway(
            config.gateway_enabled,
            &config.gateway_url,
            &config.gateway_key,
        )?;
        let repo = Repository {
            api,
            store,
            offline: config.offline,
        };
        Ok(Self {
            progress_writer: progress::Writer::new()?,
            config,
            repo,
            ui_fonts,
            reader_font_cache: None,
            metrics: Metrics::for_screen(width, height),
            dpi,
            light,
            reader_jump: None,
            discover_query: None,
            reader_anchor: None,
            history_syncing: false,
            history_status: String::new(),
            unlocked: false,
            lock_pending: false,
            lock_showing: false,
            sleep_request: None,
            sleep_image: None,
        })
    }
    pub fn unlock(&mut self) {
        self.unlocked = true;
        self.lock_pending = false;
        self.lock_showing = false;
    }
    pub fn save_config(&self) -> Result<(), String> {
        self.config.save(&self.repo.store.root)
    }
    /// 设置页替换仓储，已排队任务仍使用原快照；新请求和缓存切换到新端点。
    pub fn apply_config(&mut self, config: Config) -> Result<(), String> {
        let mut api =
            api::ApiClient::new(&config.api_base_url, config.timeout_secs, config.strict_tls)?;
        api.configure_gateway(
            config.gateway_enabled,
            &config.gateway_url,
            &config.gateway_key,
        )?;
        config.save(&self.repo.store.root)?;
        let store = if api.base == self.repo.api.base {
            self.repo.store.clone()
        } else {
            Store::new(self.repo.store.root.clone(), &api::normalize_base(&config.api_base_url)?)
        };
        api.configure_session(
            &config.auth_base_url,
            &config.forum_base_url,
            store.data_path("session.json"),
        )?;
        if api.base == self.repo.api.base && api.auth_base == self.repo.api.auth_base {
            api.session = self.repo.api.session.clone();
            api.session_file = self.repo.api.session_file.clone();
        }
        self.repo = Repository {
            api,
            store,
            offline: config.offline,
        };
        self.config = config;
        Ok(())
    }
}
impl App for Kindlia {
    fn on_suspend(&mut self) {
        self.progress_writer.flush();
    }
    fn ui_refresh_mode(&self) -> kn_ui::UiRefreshMode {
        if self.config.ui_refresh_mode == "classic" {
            kn_ui::UiRefreshMode::Classic
        } else {
            kn_ui::UiRefreshMode::Smooth
        }
    }
    fn on_start(cx: &mut kn_ui::Cx<Self>) {
        history::start(cx);
        let root = cx.app.repo.store.root.clone();
        let limit = cx.app.config.cache_limit_mb;
        // 与 KN 一样独立执行维护，不能占住唯一的本地章节/字体读取队列。
        if let Err(error) = std::thread::Builder::new().name("cache-prune".into()).spawn(move || sleep::prune(&root, limit)) {
            eprintln!("[cache-prune] {error}");
        }
    }
    fn sleep_screen(&mut self) -> Option<Box<dyn Page<Self>>> {
        if self.config.sleep_cover && std::env::var("KINDLIA_SLEEP_COVER").as_deref() == Ok("1") {
            return self
                .sleep_image
                .clone()
                .map(|image| Box::new(sleep::SleepPage(image)) as Box<dyn Page<Self>>);
        }
        None
    }
    fn on_resume(&mut self) {
        if pages::lock::pin_set(&self.config) && !self.lock_showing {
            self.unlocked = false;
            self.lock_pending = true;
        }
    }
    fn take_transition(&mut self, _top: kn_ui::PageId) -> kn_ui::Transition<Self> {
        if self.lock_pending {
            self.lock_pending = false;
            self.lock_showing = true;
            return kn_ui::Transition::Push(Box::new(pages::lock::LockPage::resume()));
        }
        if self.config.operation_tips && !self.config.tips_seen.contains(&_top.0) {
            let text = match _top.0 {
                30 => Some(
                    "左右边缘点按或滑动翻页。
点按中部或从顶部下滑呼出阅读菜单。
目录返回保留当前阅读页；长按书签按钮管理书签。",
                ),
                11 => Some(
                    "右上角搜索图标进入搜索。
左上角返回图标退出搜索；筛选按钮弹出选项列表。",
                ),
                58 => Some(
                    "评论全文和缩进回复依次显示。
点按具体评论选择回复；自己的评论可编辑或删除。",
                ),
                _ => None,
            };
            if let Some(text) = text {
                self.config.tips_seen.push(_top.0);
                let _ = self.save_config();
                return kn_ui::Transition::Push(Box::new(pages::text::TextPage::new(
                    "操作提示",
                    text,
                )));
            }
        }
        kn_ui::Transition::None
    }
    fn home_page(&mut self) -> Box<dyn Page<Self>> {
        if pages::lock::pin_set(&self.config) && !self.unlocked {
            return Box::new(pages::lock::LockPage::startup());
        }
        if !self.config.welcome_skipped && !self.repo.api.signed_in() && !self.repo.offline {
            return Box::new(pages::login::LoginPage::welcome());
        }
        Box::new(pages::library::LibraryPage::shelf())
    }
    fn theme(&self) -> Theme {
        Theme::new(self.config.night)
    }
}
