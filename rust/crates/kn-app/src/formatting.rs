//! Source timestamps are Unix seconds, displayed in the device timezone.
pub fn date(at: Option<i64>) -> String {
    let Some(t) = at.filter(|t| *t > 0) else {
        return String::new();
    };
    let local = t + crate::tz::local_utc_offset_secs(t);
    let (y, m, d) = crate::stats::civil_from_days(local.div_euclid(86400));
    let sec = local.rem_euclid(86400);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}",
        sec / 3600,
        sec % 3600 / 60
    )
}
pub fn updated(at: Option<i64>) -> String {
    let Some(t) = at.filter(|t| *t > 0) else {
        return "更新时间未知".into();
    };
    let secs = crate::stats::unix_now().saturating_sub(t).max(0);
    let age = if secs < 60 {
        "刚刚".into()
    } else if secs < 3600 {
        format!("{} 分钟前", secs / 60)
    } else if secs < 86400 {
        format!("{} 小时前", secs / 3600)
    } else {
        format!("{} 天前", secs / 86400)
    };
    format!("{age}更新")
}
pub fn characters(n: Option<u64>) -> String {
    match n {
        Some(n) if n >= 10000 => format!("{:.1} 万字", n as f64 / 10000.),
        Some(n) => format!("{n} 字"),
        None => "字数未知".into(),
    }
}

/// Accept both the Wenku string array and Web-style author objects, without extra requests.
pub fn authors(value: &serde_json::Value) -> Vec<String> {
    value
        .get("authors")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .filter_map(|v| {
            v.as_str()
                .or_else(|| v.get("name").and_then(|v| v.as_str()))
        })
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}
pub fn deserialize_authors<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<Vec<String>, D::Error> {
    use serde::Deserialize;
    let v = serde_json::Value::deserialize(d)?;
    Ok(authors(&serde_json::json!({"authors":v})))
}
