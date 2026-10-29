//! Novelia auth 的用户名登录、refresh-token Cookie 和访问令牌；从不保存密码。
use serde::{Deserialize, Serialize};
#[derive(Default, Clone, Serialize, Deserialize)]
pub struct Session {
    pub username: String,
    pub token: String,
    pub cookie: String,
    /// 仅用于进程内的账号操作排序，不写入令牌文件。
    #[serde(skip)]
    pub(crate) revision: u64,
}

impl crate::api::ApiClient {
    pub fn login(&self, username: &str, password: &str) -> Result<(), String> {
        let revision = {
            let mut s = self.session.lock().unwrap_or_else(|e| e.into_inner());
            s.revision = s.revision.wrapping_add(1);
            s.revision
        };
        let body = serde_json::to_vec(
            &serde_json::json!({"username":username,"password":password,"app":"n"}),
        )
        .map_err(|e| e.to_string())?;
        let response = self.send(
            &format!("{}/api/v1/auth/login", self.auth_base),
            "POST",
            &body,
            "application/json",
            None,
            None,
        )?;
        crate::api::check_status(&response)?;
        let token = String::from_utf8(response.body.clone()).map_err(|e| e.to_string())?;
        if token.trim().is_empty() {
            return Err("登录未返回令牌".into());
        }
        let cookie = response
            .headers
            .iter()
            .find(|(k, v)| k.eq_ignore_ascii_case("set-cookie") && v.starts_with("refresh-token="))
            .map(|(_, v)| v.split(';').next().unwrap_or("").to_owned())
            .unwrap_or_default();
        let mut s = self.session.lock().unwrap_or_else(|e| e.into_inner());
        if s.revision != revision {
            return Err("账号操作已被更新的操作替代".into());
        }
        *s = Session {
            username: username.into(),
            token: token.trim().into(),
            cookie,
            revision: revision.wrapping_add(1),
        };
        self.persist_session(&s)
    }
    pub fn refresh(&self) -> Result<(), String> {
        let snapshot = self.session.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if snapshot.cookie.is_empty() {
            return Err("登录过期，请重新登录".into());
        }
        let r = self.send(
            &format!("{}/api/v1/auth/refresh?app=n", self.auth_base),
            "POST",
            b"",
            "application/json",
            None,
            Some(&snapshot.cookie),
        )?;
        let mut s = self.session.lock().unwrap_or_else(|e| e.into_inner());
        if s.revision != snapshot.revision {
            return Ok(());
        }
        s.revision = s.revision.wrapping_add(1);
        if r.status == 401 {
            s.token.clear();
            s.cookie.clear();
            self.persist_session(&s)?;
            return Err("登录已失效，请重新登录".into());
        }
        crate::api::check_status(&r)?;
        s.token = String::from_utf8(r.body)
            .map_err(|e| e.to_string())?
            .trim()
            .to_owned();
        self.persist_session(&s)
    }
    pub fn logout(&self) -> Result<(), String> {
        // UI 读账号状态只短暂取锁；请求期间不持锁，并立刻使旧登录/刷新结果失效。
        let snapshot = {
            let mut s = self.session.lock().unwrap_or_else(|e| e.into_inner());
            let old = s.clone();
            *s = Session { revision: s.revision.wrapping_add(1), ..Session::default() };
            self.persist_session(&s)?;
            old
        };
        let result = if snapshot.cookie.is_empty() {
            Ok(())
        } else {
            self.send(
                &format!("{}/api/v1/auth/logout", self.auth_base),
                "POST",
                b"",
                "application/json",
                None,
                Some(&snapshot.cookie),
            )
            .and_then(|r| crate::api::check_status(&r))
        };
        result
    }
    pub fn username(&self) -> String {
        self.session
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .username
            .clone()
    }
    pub fn signed_in(&self) -> bool {
        let s = self.session.lock().unwrap_or_else(|e| e.into_inner());
        !s.token.is_empty() || !s.cookie.is_empty()
    }
    pub(crate) fn persist_session(&self, s: &Session) -> Result<(), String> {
        if let Some(path) = &self.session_file {
            crate::store::atomic_write(
                path,
                &serde_json::to_vec(s).map_err(|e| e.to_string())?,
                true,
            )
            .map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}
