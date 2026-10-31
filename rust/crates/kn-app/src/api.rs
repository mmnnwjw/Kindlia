//! AutoNovel REST 与文库下载，按实际源站协议实现。
use crate::model::{Chapter, Novel, NovelKey, NovelOutline, Page};
use kn_net::{http, rate_limit::RateLimit, stream, NetError};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

pub const DEFAULT_PROVIDERS: &str = "syosetu,kakuyomu,hameln,pixiv,alphapolis,novelup";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListQuery {
    pub page: usize,
    pub page_size: usize,
    pub query: String,
    pub provider: String,
    pub novel_type: u8,
    pub level: u8,
    pub translate: u8,
    pub sort: u8,
}
impl Default for ListQuery {
    fn default() -> Self {
        Self {
            page: 0,
            page_size: 6,
            query: String::new(),
            provider: DEFAULT_PROVIDERS.into(),
            novel_type: 0,
            level: 1,
            translate: 0,
            sort: 0,
        }
    }
}

/// 编码所有非字母数字/下划线/短横线字节，连 . 和 .. 也不成为相对路径。
pub fn encode(value: &str) -> String {
    let mut out = String::new();
    for b in value.bytes() {
        if b.is_ascii_alphanumeric() || b == b'_' || b == b'-' {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

pub fn normalize_base(base: &str) -> Result<String, String> {
    let url = url::Url::parse(base.trim()).map_err(|e| format!("服务器地址无效: {e}"))?;
    if !matches!(url.scheme(), "https" | "http")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("服务器地址须为 http(s)，不带账号、查询或片段".into());
    }
    // 配置写站点根或 /api 均可；未来反代也可放在路径前缀下。
    let clean = url.as_str().trim_end_matches('/').to_owned();
    Ok(clean.strip_suffix("/api").unwrap_or(&clean).to_owned())
}

#[derive(Clone)]
pub struct ApiClient {
    pub(crate) base: String,
    gateway: Option<crate::gateway::Gateway>,
    pub(crate) auth_base: String,
    pub(crate) forum_base: String,
    pub(crate) session: Arc<Mutex<crate::account::Session>>,
    pub(crate) session_file: Option<std::path::PathBuf>,
    timeout: Duration,
    strict_tls: bool,
    limit: Arc<RateLimit>,
    image_limit: Arc<RateLimit>,
}
impl ApiClient {
    pub fn new(base: &str, timeout_secs: u64, strict_tls: bool) -> Result<Self, String> {
        Ok(Self {
            base: normalize_base(base)?,
            gateway: None,
            auth_base: "https://auth.novelia.cc".into(),
            forum_base: "https://forum.novelia.cc".into(),
            session: Arc::new(Mutex::new(Default::default())),
            session_file: None,
            timeout: Duration::from_secs(timeout_secs.clamp(3, 120)),
            strict_tls,
            limit: Arc::new(RateLimit::new(4, Duration::from_secs(1))),
            image_limit: Arc::new(RateLimit::new(2, Duration::from_secs(1))),
        })
    }
    /// Capture the submitting account for background writes; never overwrite a later login's session file.
    pub fn account_snapshot(&self)->Self {
        let mut api=self.clone();
        let session=self.session.lock().unwrap_or_else(|e|e.into_inner()).clone();
        api.session=Arc::new(Mutex::new(session)); api.session_file=None; api
    }
    pub fn configure_gateway(&mut self, enabled: bool, url: &str, key: &str) -> Result<(), String> {
        self.gateway = if enabled {
            Some(crate::gateway::Gateway::new(url, key)?)
        } else {
            None
        };
        Ok(())
    }
    fn transport_url(&self, url: &str, image: bool) -> Result<String, String> {
        let Some(gateway) = &self.gateway else {
            return Ok(url.into());
        };
        let kind = if image {
            "image"
        } else if url.starts_with(&format!("{}/", self.auth_base)) {
            "auth"
        } else if url.starts_with(&format!("{}/", self.forum_base)) {
            "forum"
        } else {
            "main"
        };
        gateway.route(url, kind)
    }
    fn gateway_headers(&self) -> Vec<(&str, String)> {
        self.gateway
            .as_ref()
            .map(|g| vec![("X-Kindlia-Key", g.key.clone())])
            .unwrap_or_default()
    }
    pub fn configure_session(
        &mut self,
        auth: &str,
        forum: &str,
        file: std::path::PathBuf,
    ) -> Result<(), String> {
        self.auth_base = normalize_base(auth)?;
        self.forum_base = normalize_base(forum)?;
        let scoped = file.with_file_name(format!(
            "session-{}.json",
            crate::model::digest(&self.auth_base)
        ));
        let session = std::fs::read(&scoped)
            .ok()
            .and_then(|d| serde_json::from_slice(&d).ok())
            .unwrap_or_default();
        self.session = Arc::new(Mutex::new(session));
        self.session_file = Some(scoped);
        Ok(())
    }
    pub(crate) fn send(
        &self,
        url: &str,
        method: &str,
        body: &[u8],
        content: &str,
        token: Option<&str>,
        cookie: Option<&str>,
    ) -> Result<http::HttpResponse, String> {
        self.limit.wait();
        let url = self.transport_url(url, false)?;
        let (server, path) = http::split_url(&url).map_err(|e| e.to_string())?;
        let mut headers = vec![
            ("Accept", "application/json".into()),
            ("Content-Type", content.into()),
            ("User-Agent", "Kindlia/0.2".into()),
        ];
        headers.extend(self.gateway_headers());
        if let Some(token) = token.filter(|s| !s.is_empty()) {
            headers.push(("Authorization", format!("Bearer {token}")));
        }
        if let Some(cookie) = cookie.filter(|s| !s.is_empty()) {
            headers.push(("Cookie", cookie.into()));
        }
        http::request(
            &server,
            &path,
            method,
            &headers,
            body,
            self.timeout,
            stream::tls_config(self.strict_tls),
        )
        .map_err(|e| e.to_string())
    }
    pub fn request(
        &self,
        method: &str,
        route: &str,
        body: &[u8],
        content: &str,
    ) -> Result<Vec<u8>, String> {
        let url = if let Some(r) = route.strip_prefix("forum:") {
            format!("{}/api/v1/{r}", self.forum_base)
        } else {
            format!("{}/api/{route}", self.base)
        };
        let token = self
            .session
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .token
            .clone();
        let mut r = self.send(&url, method, body, content, Some(&token), None)?;
        if r.status == 401 && !token.is_empty() {
            self.refresh()?;
            let token = self
                .session
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .token
                .clone();
            r = self.send(&url, method, body, content, Some(&token), None)?;
        }
        check_status(&r)?;
        if r.header("Content-Encoding")
            .is_some_and(|e| e.eq_ignore_ascii_case("gzip"))
        {
            kn_net::gzip::gunzip_limited(&r.body, 8 * 1024 * 1024)
                .map_err(|e| format!("gzip: {e:?}"))
        } else {
            Ok(r.body)
        }
    }
    pub fn get<T: DeserializeOwned>(&self, route: &str) -> Result<T, String> {
        serde_json::from_slice(&self.request("GET", route, b"", "application/json")?)
            .map_err(|e| format!("API JSON 无效: {e}"))
    }
    pub fn mutate(
        &self,
        method: &str,
        route: &str,
        json: &serde_json::Value,
    ) -> Result<(), String> {
        self.request(
            method,
            route,
            &serde_json::to_vec(json).map_err(|e| e.to_string())?,
            "application/json",
        )
        .map(|_| ())
    }
    pub fn download(&self, path: &str) -> Result<Vec<u8>, String> {
        let url = if path.starts_with("https://") || path.starts_with("http://") {
            path.to_owned()
        } else {
            format!("{}{}", self.base, path)
        };
        let url = self.transport_url(&url, false)?;
        http::download_resumable_headers(
            &url,
            64 * 1024 * 1024,
            Duration::from_secs(120),
            stream::tls_config(self.strict_tls),
            &self.gateway_headers(),
        )
        .map_err(|e| e.to_string())
    }
    pub fn list(&self, q: &ListQuery) -> Result<Page<NovelOutline>, String> {
        if !(1..=100).contains(&q.page_size) {
            return Err("pageSize 必须在 1..100".into());
        }
        self.get(&format!(
            "novel?page={}&pageSize={}&query={}&provider={}&type={}&level={}&translate={}&sort={}",
            q.page,
            q.page_size,
            encode(&q.query),
            encode(&q.provider),
            q.novel_type,
            q.level,
            q.translate,
            q.sort
        ))
    }
    pub fn novel(&self, key: &NovelKey) -> Result<Novel, String> {
        self.get(&format!(
            "novel/{}/{}",
            encode(&key.provider_id),
            encode(&key.novel_id)
        ))
    }
    pub fn chapter(&self, key: &NovelKey, id: &str) -> Result<Chapter, String> {
        if id.is_empty() {
            return Err("分卷标题不是章节".into());
        }
        self.get(&format!(
            "novel/{}/{}/chapter/{}",
            encode(&key.provider_id),
            encode(&key.novel_id),
            encode(id)
        ))
    }
    pub fn image(&self, url: &str) -> Result<Vec<u8>, String> {
        self.image_limit.wait();
        let url = self.transport_url(url, true)?;
        http::download_resumable_headers(
            &url,
            12 * 1024 * 1024,
            self.timeout,
            stream::tls_config(self.strict_tls),
            &self.gateway_headers(),
        )
        .map_err(|e| e.to_string())
    }
}

pub fn check_status(r: &http::HttpResponse) -> Result<(), String> {
    if (200..300).contains(&r.status) {
        Ok(())
    } else {
        let text = String::from_utf8_lossy(&r.body);
        Err(NetError::api(text.chars().take(240).collect::<String>(), r.status as i32).to_string())
    }
}
