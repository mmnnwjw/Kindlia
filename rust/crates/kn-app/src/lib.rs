pub mod account;
pub mod api;
pub mod comment_format;
pub mod conversion;
mod cover_cache;
pub mod covers;
pub mod crash;
pub mod epub;
pub mod fonts;
pub mod formatting;
pub mod gateway;
pub mod history;
pub mod images;
pub mod migration;
pub mod model;
mod net_cache;
pub mod pages;
pub mod paragraphs;
pub mod progress;
pub mod repository;
pub mod sleep;
pub mod splash;
pub mod stats;
pub mod store;
#[cfg(test)]
mod tests;
pub mod tz;
pub mod update;
pub mod update_install;
pub mod update_notes;

use kn_render::FontId;
use kn_ui::{widgets::Metrics, App, Page, Theme};
use repository::Repository;
use store::{Config, Store};

pub struct Kindlia {
    pub startup_notice: Option<String>,
    pub pending_update: Option<update::Release>,
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
    pub tips_later: std::collections::HashSet<&'static str>,
    pub tip_open: Option<&'static str>,
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
            startup_notice: None,
            pending_update: None,
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
            tips_later: Default::default(),
            tip_open: None,
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
            Store::new(
                self.repo.store.root.clone(),
                &api::normalize_base(&config.api_base_url)?,
            )
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
    fn startup_hold(&self) -> std::time::Duration {
        std::time::Duration::from_secs(2)
    }
    fn draw_toast(
        &self,
        frame: &mut kn_render::Bitmap,
        fonts: &kn_render::FontStore,
        glyphs: &mut kn_render::GlyphCache,
        text: &str,
    ) {
        let bottom = frame.height() as i32 - self.metrics.margin as i32;
        let mut ink = kn_ui::widgets::Ink {
            fonts,
            glyphs,
            chain: &self.ui_fonts,
        };
        kn_ui::widgets::toast(&mut ink, frame, &self.theme(), &self.metrics, text, bottom);
    }

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
        sleep::restore(cx);
        update::start(cx);
        let root = cx.app.repo.store.root.clone();
        let limit = cx.app.config.cache_limit_mb;
        // 与 KN 一样独立执行维护，不能占住唯一的本地章节/字体读取队列。
        if let Err(error) = std::thread::Builder::new()
            .name("cache-prune".into())
            .spawn(move || sleep::prune(&root, limit))
        {
            eprintln!("[cache-prune] {error}");
        }
    }
    fn sleep_screen(&mut self) -> Option<Box<dyn Page<Self>>> {
        if self.config.sleep_cover && std::env::var("KINDLIA_SLEEP_COVER").as_deref() == Ok("1") {
            return Some(Box::new(sleep::SleepPage(self.sleep_image.clone())));
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
        if !self.lock_pending && pages::is_root_page(_top) {
            if let Some(text) = self.startup_notice.take() {
                return kn_ui::Transition::Push(Box::new(pages::text::TextPage::new(
                    "恢复提示",
                    &text,
                )));
            }
            if let Some(release) = self.pending_update.take() {
                update::mark_prompted(&self.repo.store, &release.version);
                return kn_ui::Transition::Push(Box::new(pages::update::UpdatePage::with_release(
                    release,
                )));
            }
        }
        if self.lock_pending {
            self.lock_pending = false;
            self.lock_showing = true;
            return kn_ui::Transition::Push(Box::new(pages::lock::LockPage::resume()));
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
