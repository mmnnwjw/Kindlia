//! Durable per-account outbox: successful local progress never depends on an HTTP request.
use crate::{
    model::NovelKey,
    repository::Repository,
    store::{self, Store},
    Kindlia,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    sync::{Mutex, OnceLock},
};
#[derive(Clone, Serialize, Deserialize, PartialEq)]
struct Pending {
    key: NovelKey,
    chapter: String,
    updated_at: u64,
}
fn gate() -> &'static Mutex<()> {
    static G: OnceLock<Mutex<()>> = OnceLock::new();
    G.get_or_init(|| Mutex::new(()))
}
fn file(store: &Store, user: &str) -> std::path::PathBuf {
    store.data_path(&format!(
        "history-outbox-{}.json",
        crate::model::digest(&user)
    ))
}
fn load(store: &Store, user: &str) -> Result<BTreeMap<String, Pending>, String> {
    let p = file(store, user);
    if !p.exists() {
        return Ok(BTreeMap::new());
    }
    serde_json::from_slice(&std::fs::read(p).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn save(store: &Store, user: &str, pending: &BTreeMap<String, Pending>) -> Result<(), String> {
    store::atomic_write(
        &file(store, user),
        &serde_json::to_vec(pending).map_err(|e| e.to_string())?,
        true,
    )
    .map_err(|e| e.to_string())
}
pub fn enqueue(
    store: &Store,
    user: &str,
    key: &NovelKey,
    chapter: &str,
    updated_at: u64,
) -> Result<(), String> {
    enqueue_checked(store, user, key, chapter, updated_at, || true)
}
pub(crate) fn enqueue_checked(
    store: &Store, user: &str, key: &NovelKey, chapter: &str, updated_at: u64,
    current: impl FnOnce() -> bool,
) -> Result<(), String> {
    if key.provider_id == "epub" || key.provider_id == "wenku" {
        return Ok(());
    }
    let _lock = gate().lock().map_err(|_| "历史队列锁失效")?;
    if !current() { return Ok(()); }
    let mut pending = load(store, user)?;
    pending.insert(
        key.cache_id(),
        Pending {
            key: key.clone(),
            chapter: chapter.into(),
            updated_at,
        },
    );
    save(store, user, &pending)
}
fn sync_gate() -> &'static Mutex<()> {
    static G: OnceLock<Mutex<()>> = OnceLock::new();
    G.get_or_init(|| Mutex::new(()))
}
fn flush(repo: &Repository, user: &str) -> Result<String, String> {
    let _sync = sync_gate().lock().map_err(|_| "历史同步锁失效")?;
    let pending = {
        let _lock = gate().lock().map_err(|_| "历史队列锁失效")?;
        load(&repo.store, user)?
    };
    if repo.api.get::<bool>("user/read-history/paused")? {
        return Ok("云端阅读历史已暂停；待同步记录保留在本机".into());
    }
    if pending.is_empty() {return Ok("网文阅读记录已同步；文库进度保存在本机".into());}
    for (id, value) in pending {
        repo.api.request(
            "PUT",
            &format!(
                "user/read-history/{}/{}",
                crate::api::encode(&value.key.provider_id),
                crate::api::encode(&value.key.novel_id)
            ),
            value.chapter.as_bytes(),
            "text/plain",
        )?;
        let _lock = gate().lock().map_err(|_| "历史队列锁失效")?;
        let mut latest = load(&repo.store, user)?;
        if latest.get(&id) == Some(&value) {
            latest.remove(&id);
            save(&repo.store, user, &latest)?;
        }
    }
    Ok("网文阅读记录已同步；文库进度保存在本机".into())
}
pub fn start(cx: &mut kn_ui::Cx<Kindlia>) {
    if cx.app.history_syncing || cx.app.repo.offline || !cx.app.repo.api.signed_in() {
        return;
    }
    let mut repo = cx.app.repo.clone();
    repo.api=repo.api.account_snapshot();
    let user=repo.api.username();
    let original=repo.api.session.lock().unwrap_or_else(|e|e.into_inner()).clone();
    cx.app.history_syncing = true;
    let saved = cx.app.progress_writer.fence();
    cx.spawn_app(move || {
        let _ = saved.recv();
        let result = flush(&repo, &user);
        let refreshed=repo.api.session.lock().unwrap_or_else(|e|e.into_inner()).clone();
        Box::new(move |app: &mut Kindlia| {
            app.history_syncing = false;
            if app.repo.api.username()!=user{return;}
            {
                let mut session=app.repo.api.session.lock().unwrap_or_else(|e|e.into_inner());
                if session.revision == original.revision && session.token == original.token
                    && (refreshed.token != original.token || refreshed.cookie != original.cookie) {
                    *session = refreshed;
                    let _ = app.repo.api.persist_session(&session);
                }
            }
            app.history_status = result.unwrap_or_else(|e| {
                eprintln!("[history] {e}");
                format!("同步待重试：{e}")
            });
        })
    });
}

pub fn manage(cx: &mut kn_ui::Cx<Kindlia>, selection: usize) {
    if selection == 0 {
        start(cx);
        return;
    }
    if cx.app.repo.offline || !cx.app.repo.api.signed_in() {
        cx.app.history_status = "请联网并登录后管理云端历史".into();
        return;
    }
    let mut repo = cx.app.repo.clone(); repo.api=repo.api.account_snapshot();
    let user = repo.api.username();
    cx.app.history_syncing = true;
    cx.spawn_app(move || {
        let result = repo
            .api
            .request(
                if selection == 1 { "DELETE" } else { "PUT" },
                "user/read-history/paused",
                b"",
                "text/plain",
            )
            .and_then(|_| {
                if selection == 1 {
                    flush(&repo, &user)
                } else {
                    Ok("云端阅读历史已暂停".into())
                }
            });
        Box::new(move |app: &mut Kindlia| {
            app.history_syncing = false;
            if app.repo.api.username()==user {app.history_status = result.unwrap_or_else(|e| format!("云端历史：{e}"));}
        })
    });
}

pub fn clear_remote(api: &crate::api::ApiClient, store: &Store) -> Result<(), String> {
    let _sync = sync_gate().lock().map_err(|_| "历史同步锁失效")?;
    api.request("DELETE", "user/read-history", b"", "text/plain")?;
    let _lock = gate().lock().map_err(|_| "历史队列锁失效")?;
    save(store, &api.username(), &BTreeMap::new())?;
    store.clear_history()
}
