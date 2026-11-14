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

pub fn relative_time(at: u64) -> String {
    let age = crate::stats::unix_now().max(0) as u64;
    let age = age.saturating_sub(at);
    if age < 60 {
        "刚刚".into()
    } else if age < 3600 {
        format!("{} 分钟前", age / 60)
    } else if age < 86400 {
        format!("{} 小时前", age / 3600)
    } else {
        format!("{} 天前", age / 86400)
    }
}

pub fn web_badge(kind: Option<&str>, attentions: &[String]) -> String {
    if attentions.iter().any(|s| s.eq_ignore_ascii_case("r18g")) {
        return "R18G".into();
    }
    if attentions.iter().any(|s| s.eq_ignore_ascii_case("r18")) {
        return "R18".into();
    }
    match kind.unwrap_or("") {
        "连载中" | "已完结" | "短篇" => kind.unwrap().into(),
        _ => "Web".into(),
    }
}

/// Use only fields already returned by the list endpoint; no per-book lookup.
pub fn book_badge(v: &serde_json::Value) -> String {
    if !v["providerId"].is_string() {
        return v["level"].as_str().unwrap_or("文库").into();
    }
    let attentions = v["attentions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| a.as_str().map(str::to_owned))
        .collect::<Vec<_>>();
    web_badge(v["type"].as_str(), &attentions)
}
