//! 服务器可用性 (断路器): 连续的网络错误 / 超时 / 5xx 把服务器标记为不可用,
//! 之后的自动请求 (章节后台刷新、进度上传、云端书架) 直接走本地缓存, 不再每次等待失败。
//! 连续两次失败后短暂退避 (5 s → 10 s → … → 30 s)，并发失败不延长同一次退避;
//! 任何一次成功 (或服务器明确的业务应答) 立即恢复。

use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::error::NetError;

const FIRST_BACKOFF: Duration = Duration::from_secs(5);
const MAX_BACKOFF: Duration = Duration::from_secs(30);

#[derive(Default)]
struct State {
    /// 连续失败次数; 0 = 可用
    failures: u32,
    /// 不可用时, 下一次允许探测的时间
    probe_at: Option<Instant>,
}

#[derive(Default)]
pub struct ServerHealth(Mutex<State>);

/// 这个错误是否说明 "服务器不可达/故障" (而不是服务器明确拒绝了这次请求)。
pub fn is_outage(err: &NetError) -> bool {
    match err {
        NetError::Network(text) => text != "下载已取消",
        NetError::Timeout => true,
        NetError::Protocol(_) => false,
        // 只认网关/不可用类状态; hub 调用的业务异常也用 500 表示, 那说明服务器在线
        NetError::Api { status, .. } => matches!(status, 502..=504),
        NetError::Auth(_) => false,
    }
}

impl ServerHealth {
    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn record<T>(&self, result: &Result<T, NetError>) {
        self.record_at(result.as_ref().err(), Instant::now());
    }

    fn record_at(&self, err: Option<&NetError>, now: Instant) {
        let mut s = self.lock();
        match err {
            Some(e) if is_outage(e) => {
                if s.probe_at.is_some_and(|until| now < until) {
                    return;
                }
                s.failures = s.failures.saturating_add(1);
                if s.failures >= 2 {
                    let backoff = FIRST_BACKOFF
                        .saturating_mul(1 << (s.failures - 2).min(4))
                        .min(MAX_BACKOFF);
                    s.probe_at = Some(now + backoff);
                }
            }
            // 成功, 或服务器能明确应答 (4xx/业务错误): 说明服务器在线
            _ => {
                if s.failures > 0 {
                    eprintln!("[net] 服务器已恢复");
                }
                *s = State::default();
            }
        }
    }

    /// 服务器被标记为不可用, 且还没到下一次探测时间: 自动请求应直接用缓存。
    pub fn is_down(&self) -> bool {
        self.is_down_at(Instant::now())
    }

    pub fn retry_after(&self) -> Option<Duration> {
        self.lock()
            .probe_at
            .and_then(|at| at.checked_duration_since(Instant::now()))
    }

    fn is_down_at(&self, now: Instant) -> bool {
        let s = self.lock();
        s.failures > 0 && s.probe_at.is_some_and(|t| now < t)
    }

    /// 最近一次请求失败 (不论是否到了探测时间)。
    pub fn degraded(&self) -> bool {
        self.lock().failures > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outage_classification() {
        assert!(is_outage(&NetError::Timeout));
        assert!(is_outage(&NetError::network("x")));
        assert!(is_outage(&NetError::api("bad gateway", 502)));
        assert!(is_outage(&NetError::api("unavailable", 503)));
        assert!(!is_outage(&NetError::api("hub exception", 500)));
        assert!(!is_outage(&NetError::api("not found", 404)));
        assert!(!is_outage(&NetError::api("expired", 401)));
        assert!(!is_outage(&NetError::api("biz", -100)));
    }

    #[test]
    fn failure_marks_down_until_backoff_then_probe() {
        let h = ServerHealth::default();
        let t0 = Instant::now();
        assert!(!h.is_down_at(t0));
        h.record_at(Some(&NetError::api("bad gateway", 502)), t0);
        assert!(!h.is_down_at(t0), "单次失败不应拦截后续请求");
        h.record_at(Some(&NetError::api("bad gateway", 502)), t0);
        assert!(h.is_down_at(t0 + Duration::from_secs(4)));
        assert!(
            !h.is_down_at(t0 + Duration::from_secs(6)),
            "探测时间到了要放行"
        );
        assert!(h.degraded());
    }

    #[test]
    fn backoff_doubles_and_caps() {
        let h = ServerHealth::default();
        let t0 = Instant::now();
        h.record_at(Some(&NetError::Timeout), t0);
        h.record_at(Some(&NetError::Timeout), t0);
        for _ in 0..10 {
            h.record_at(Some(&NetError::Timeout), t0);
        }
        assert!(h.is_down_at(t0 + Duration::from_secs(4)));
        assert!(
            !h.is_down_at(t0 + Duration::from_secs(6)),
            "并发失败不延长退避"
        );
        h.record_at(Some(&NetError::Timeout), t0 + Duration::from_secs(6));
        assert!(h.is_down_at(t0 + Duration::from_secs(15)));
        assert!(!h.is_down_at(t0 + Duration::from_secs(17)));
        h.record_at(Some(&NetError::Timeout), t0 + Duration::from_secs(17));
        h.record_at(Some(&NetError::Timeout), t0 + Duration::from_secs(38));
        assert!(h.is_down_at(t0 + Duration::from_secs(67)));
        assert!(!h.is_down_at(t0 + Duration::from_secs(69)));
    }

    #[test]
    fn success_or_client_error_recovers() {
        let h = ServerHealth::default();
        let t0 = Instant::now();
        h.record_at(Some(&NetError::Timeout), t0);
        h.record_at(Some(&NetError::api("not found", 404)), t0);
        assert!(!h.degraded());
        h.record_at(Some(&NetError::Timeout), t0);
        h.record_at(None, t0);
        assert!(!h.is_down_at(t0));
    }
}
