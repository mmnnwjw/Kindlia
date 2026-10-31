//! Kindlia update feed: public GitHub releases and an optional manually maintained R2 mirror.
use crate::{
    store::{Config, Store},
    Kindlia,
};
use kn_net::{http, stream};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::mpsc;
use std::time::Duration;
pub const SIGNING_PUBLIC_KEY: &str =
    "5add9eb805ce8d3faaffaeb1b2d027dd771e2f86178b1a39e4ea8bbafa442f2a";

const GITHUB: &str = "https://api.github.com/repos/mmnnwjw/Kindlia/releases?per_page=20";
#[derive(Clone, Debug)]
pub struct Release {
    pub version: String,
    pub changes: Vec<VersionNotes>,
    pub source: String,
    pub urls: Vec<String>,
    pub sha256: String,
}
/// Source history is retained independently of downloadable assets.
#[derive(Clone, Debug)]
pub struct VersionNotes {
    pub version: String,
    pub date: String,
    pub notes: String,
}
impl Release {
    /// Newest first, excluding the installed version. If up to date, show latest notes.
    pub fn changes_since(&self, current: &str) -> Vec<&VersionNotes> {
        let mut changes: Vec<_> = self
            .changes
            .iter()
            .filter(|n| {
                version(&n.version)
                    .zip(version(current))
                    .is_some_and(|(v, c)| v > c)
            })
            .collect();
        if changes.is_empty() {
            changes.extend(self.changes.iter().find(|n| n.version == self.version));
        }
        changes
    }
}
struct Feed {
    releases: Vec<Release>,
    changes: Vec<VersionNotes>,
}
fn version(s: &str) -> Option<(u32, u32, u32)> {
    let v: Vec<_> = s.trim_start_matches('v').split('.').collect();
    if v.len() != 3 {
        return None;
    }
    Some((v[0].parse().ok()?, v[1].parse().ok()?, v[2].parse().ok()?))
}
pub fn newer(s: &str) -> bool {
    version(s)
        .zip(version(env!("CARGO_PKG_VERSION")))
        .is_some_and(|(a, b)| a > b)
}
pub fn https(s: &str) -> Result<String, String> {
    let u = url::Url::parse(s.trim()).map_err(|_| "请输入 HTTPS 地址")?;
    if u.scheme() != "https"
        || u.host_str().is_none()
        || !u.username().is_empty()
        || u.password().is_some()
        || u.query().is_some()
        || u.fragment().is_some()
    {
        return Err("更新地址须为不带账号、查询或片段的 HTTPS 地址".into());
    }
    Ok(s.trim().trim_end_matches('/').into())
}
fn get(url: &str, limit: usize) -> Result<Vec<u8>, String> {
    http::download(
        url,
        limit,
        Duration::from_secs(20),
        stream::tls_config(true),
    )
    .map_err(|e| e.to_string())
}
fn feed(url: &str, source: &str, r2: bool) -> Result<Feed, String> {
    let v: Value =
        serde_json::from_slice(&get(url, 2 * 1024 * 1024)?).map_err(|e| e.to_string())?;
    let list = v.as_array().ok_or("更新列表格式无效")?;
    let mut result = vec![];
    let mut changes = vec![];
    for r in list.iter().take(30) {
        if r["draft"].as_bool() == Some(true) || r["prerelease"].as_bool() == Some(true) {
            continue;
        }
        let Some(v) = r["tag_name"].as_str().filter(|s| version(s).is_some()) else {
            continue;
        };
        let v = v.trim_start_matches('v');
        changes.push(VersionNotes {
            version: v.into(),
            date: r["published_at"]
                .as_str()
                .or_else(|| r["created_at"].as_str())
                .unwrap_or("")
                .chars()
                .take(10)
                .collect(),
            notes: r["body"]
                .as_str()
                .unwrap_or("")
                .chars()
                .take(32000)
                .collect(),
        });
        let package = format!("kindlia-{v}.zip");
        let Some(assets) = r["assets"].as_array() else {
            continue;
        };
        let Some(asset) = assets.iter().find(|a| a["name"].as_str() == Some(&package)) else {
            continue;
        };
        let sha = asset["digest"]
            .as_str()
            .unwrap_or("")
            .strip_prefix("sha256:")
            .unwrap_or("");
        if sha.len() != 64 || !sha.bytes().all(|b| b.is_ascii_hexdigit()) {
            continue;
        }
        let download = if r2 {
            let base = url
                .strip_suffix("/releases.json")
                .ok_or("R2 列表路径无效")?;
            format!("{base}/{package}")
        } else {
            asset["browser_download_url"].as_str().unwrap_or("").into()
        };
        https(&download)?;
        result.push(Release {
            version: v.into(),
            changes: vec![],
            source: source.into(),
            urls: vec![download],
            sha256: sha.to_ascii_lowercase(),
        });
    }
    Ok(Feed {
        releases: result,
        changes,
    })
}
pub fn check(config: &Config, store: &Store) -> Result<Release, String> {
    if config.offline {
        return Err("离线模式无法检查更新".into());
    }
    let mut sources = vec![(GITHUB.to_owned(), "GitHub", false)];
    if !config.update_r2_url.trim().is_empty() {
        sources.push((
            format!("{}/releases.json", https(&config.update_r2_url)?),
            "R2",
            true,
        ));
    }
    let count = sources.len();
    let (tx, rx) = mpsc::channel();
    for (url, label, r2) in sources {
        let tx = tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(feed(&url, label, r2));
        });
    }
    drop(tx);
    let mut deadline = std::time::Instant::now() + Duration::from_secs(23);
    let mut releases = vec![];
    let mut history = std::collections::BTreeMap::new();
    let mut errors: Vec<String> = vec![];
    for _ in 0..count {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        match rx.recv_timeout(left) {
            Ok(Ok(mut v)) => {
                if !v.releases.is_empty() {
                    deadline = deadline.min(std::time::Instant::now() + Duration::from_secs(5));
                }
                releases.append(&mut v.releases);
                for n in v.changes {
                    if let Some(key) = version(&n.version) {
                        let old = history.entry(key).or_insert_with(|| n.clone());
                        if old.notes.trim().is_empty() && !n.notes.trim().is_empty() {
                            *old = n;
                        }
                    }
                }
            }
            Ok(Err(e)) => errors.push(e),
            Err(_) => break,
        }
    }
    releases.sort_by_key(|r| std::cmp::Reverse(version(&r.version)));
    let mut chosen = releases
        .first()
        .cloned()
        .ok_or_else(|| format!("没有可用的公开发行版：{}", errors.join("；")))?;
    for r in &releases {
        if r.version == chosen.version && r.sha256 == chosen.sha256 {
            for u in &r.urls {
                if !chosen.urls.contains(u) {
                    chosen.urls.push(u.clone());
                }
            }
        }
    }
    chosen.changes = history
        .into_iter()
        .rev()
        .filter(|(v, _)| Some(*v) <= version(&chosen.version))
        .map(|(_, n)| n)
        .collect();
    chosen
        .urls
        .sort_by_key(|u| u.starts_with("https://github.com/"));
    let _ = store.write_cache(
        "update",
        &"latest",
        &json!({"version":chosen.version,"checked":crate::stats::unix_now()}),
    );
    Ok(chosen)
}
fn hex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    s.as_bytes()
        .chunks(2)
        .map(|p| {
            std::str::from_utf8(p)
                .ok()
                .and_then(|s| u8::from_str_radix(s, 16).ok())
        })
        .collect()
}
pub fn download(
    release: &Release,
    progress: impl Fn(usize, Option<usize>) + Send + Sync,
) -> Result<Vec<u8>, String> {
    let name = format!("kindlia-{}.zip", release.version);
    let mut errors: Vec<String> = vec![];
    for url in &release.urls {
        let attempt = (|| {
            let started = std::time::Instant::now();
            let mut callback = |n, t| {
                progress(n, t);
                started.elapsed() < Duration::from_secs(240)
            };
            let package_url = format!("{url}?sha256={}", release.sha256);
            let bytes = http::download_with(
                &package_url,
                32 * 1024 * 1024,
                Duration::from_secs(60),
                stream::tls_config(true),
                &mut callback,
            )
            .map_err(|e| e.to_string())?;
            let sha = format!("{:x}", Sha256::digest(&bytes));
            if sha != release.sha256 {
                return Err("更新包 SHA256 不一致".into());
            }
            let sig = String::from_utf8(get(&format!("{url}.sig?sha256={sha}"), 1024)?)
                .map_err(|_| "签名格式无效")?;
            let key = hex(SIGNING_PUBLIC_KEY).ok_or("内置公钥无效")?;
            let sig = hex(sig.trim()).ok_or("签名格式无效")?;
            let message = format!("kindlia-update-v1\n{name}\n{sha}\n");
            ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, &key)
                .verify(message.as_bytes(), &sig)
                .map_err(|_| "更新包签名校验失败")?;
            Ok(bytes)
        })();
        match attempt {
            Ok(b) => return Ok(b),
            Err(e) => errors.push(e),
        }
    }
    Err(errors.join("；"))
}
fn state(store: &Store) -> Value {
    std::fs::read(store.root.join("data/update.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or(json!({}))
}
fn save_state(store: &Store, checked: i64, prompted: Value) {
    let _ = crate::store::atomic_write(
        &store.root.join("data/update.json"),
        &serde_json::to_vec(&json!({"checked":checked,"prompted":prompted})).unwrap(),
        true,
    );
}
pub fn start(cx: &kn_ui::Cx<Kindlia>) {
    if !cx.app.config.update_auto || cx.app.repo.offline {
        return;
    }
    let store = cx.app.repo.store.clone();
    let state = state(&store);
    if crate::stats::unix_now() - state["checked"].as_i64().unwrap_or(0) < 86400 {
        return;
    }
    save_state(&store, crate::stats::unix_now(), state["prompted"].clone());
    let config = cx.app.config.clone();
    let poster = cx.app_poster();
    cx.spawn_local_droppable(move || {
        // Submit into the maintenance lane, never behind or in front of foreground API work.
        // The checker uses independently bounded HTTP requests and posts only a small result.
        let _ = std::thread::Builder::new()
            .name("kindlia-update-check".into())
            .spawn(move || {
                if let Ok(release) = check(&config, &store) {
                    if newer(&release.version)
                        && state["prompted"].as_str() != Some(&release.version)
                    {
                        save_state(&store, crate::stats::unix_now(), json!(release.version));
                        poster.post(kn_ui::Delivery::App(Box::new(move |app: &mut Kindlia| {
                            app.pending_update = Some(release)
                        })));
                    }
                }
            });
    });
}
