//! Short-lived GET coalescing, isolated by session revision and mutation epoch.
use std::{
    collections::HashMap,
    sync::{Arc, Condvar, Mutex},
    time::{Duration, Instant},
};
#[derive(Default)]
pub struct Cache {
    flights: Mutex<HashMap<String, Arc<Flight>>>,
}
struct Flight {
    result: Mutex<Option<Result<Vec<u8>, String>>>,
    ready: Condvar,
    created: Instant,
}
impl Cache {
    pub fn get(
        &self,
        key: String,
        read: impl FnOnce() -> Result<Vec<u8>, String>,
    ) -> Result<Vec<u8>, String> {
        let (flight, leader) = {
            let mut flights = self.flights.lock().unwrap_or_else(|e| e.into_inner());
            flights.retain(|_, f| {
                f.created.elapsed() < Duration::from_secs(2)
                    || f.result.lock().unwrap_or_else(|e| e.into_inner()).is_none()
            });
            let bytes: usize = flights
                .values()
                .map(|f| {
                    f.result
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .as_ref()
                        .and_then(|r| r.as_ref().ok())
                        .map_or(0, Vec::len)
                })
                .sum();
            if bytes > 8 * 1024 * 1024 || flights.len() > 32 {
                flights.retain(|_, f| f.result.lock().unwrap_or_else(|e| e.into_inner()).is_none());
            }
            if let Some(f) = flights.get(&key) {
                (f.clone(), false)
            } else {
                let f = Arc::new(Flight {
                    result: Mutex::new(None),
                    ready: Condvar::new(),
                    created: Instant::now(),
                });
                flights.insert(key, f.clone());
                (f, true)
            }
        };
        if leader {
            // Always notify followers, including when an unexpected worker panic unwinds.
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(read))
                .unwrap_or_else(|_| Err("网络任务异常终止".into()));
            *flight.result.lock().unwrap_or_else(|e| e.into_inner()) = Some(result.clone());
            flight.ready.notify_all();
            result
        } else {
            let mut result = flight.result.lock().unwrap_or_else(|e| e.into_inner());
            while result.is_none() {
                result = flight.ready.wait(result).unwrap_or_else(|e| e.into_inner());
            }
            result.as_ref().unwrap().clone()
        }
    }
}
