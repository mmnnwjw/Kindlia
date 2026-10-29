//! 单入口反代只改变传输地址，源站 DTO 和本地缓存命名保持原样。
#[derive(Clone)]
pub struct Gateway {
    root: String,
    pub key: String,
}
impl Gateway {
    pub fn new(root: &str, key: &str) -> Result<Self, String> {
        let url = url::Url::parse(root.trim()).map_err(|_| "反代地址须为 HTTPS 域名")?;
        let local = matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"));
        if !(url.scheme() == "https" || url.scheme() == "http" && local)
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || url.path() != "/"
        {
            return Err("反代地址须为 HTTPS 根地址，不带路径、账号或查询参数".into());
        }
        if !(16..=256).contains(&key.len())
            || !key
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"-_.~".contains(&c))
        {
            return Err("反代密钥须为 16～256 位字母、数字或 -_.~".into());
        }
        Ok(Self {
            root: url.as_str().trim_end_matches('/').into(),
            key: key.into(),
        })
    }
    pub fn route(&self, original: &str, kind: &str) -> Result<String, String> {
        let url = url::Url::parse(original).map_err(|_| "无效源站地址")?;
        if !matches!(url.scheme(), "http" | "https")
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err("源站地址不是有效 HTTP(S) URL".into());
        }
        if kind == "image" {
            return Ok(format!(
                "{}/image?url={}",
                self.root,
                crate::api::encode(url.as_str())
            ));
        }
        if !matches!(kind, "main" | "auth" | "forum") {
            return Err("未知反代服务".into());
        }
        Ok(format!(
            "{}/{kind}{}{}",
            self.root,
            url.path(),
            url.query().map(|q| format!("?{q}")).unwrap_or_default()
        ))
    }
}
