//! KinNovel-style byte-budgeted LRU for already decoded cover bitmaps.
use std::{
    cell::Cell,
    collections::HashMap,
    time::{Duration, Instant},
};
pub trait Weight {
    fn bytes(&self) -> usize;
}
impl Weight for Option<kn_render::Bitmap> {
    fn bytes(&self) -> usize {
        self.as_ref().map_or(0, |b| b.data().len())
    }
}
impl Weight for Option<(crate::covers::Metadata, kn_render::Bitmap)> {
    fn bytes(&self) -> usize {
        self.as_ref().map_or(0, |(_, b)| b.data().len())
    }
}
struct Entry<V> {
    value: V,
    used: Cell<u64>,
    failed: Option<Instant>,
}
pub struct Cache<V> {
    map: HashMap<String, Entry<V>>,
    tick: Cell<u64>,
}
impl<V> Default for Cache<V> {
    fn default() -> Self {
        Self {
            map: HashMap::new(),
            tick: Cell::new(0),
        }
    }
}
impl<V: Weight> Cache<V> {
    pub fn contains_key(&mut self, key: &str) -> bool {
        if self.map.get(key).is_some_and(|e| {
            e.failed
                .is_some_and(|at| at.elapsed() >= Duration::from_secs(20))
        }) {
            self.map.remove(key);
        }
        self.map.contains_key(key)
    }
    pub fn get(&self, key: &str) -> Option<&V> {
        let e = self.map.get(key)?;
        self.tick.set(self.tick.get() + 1);
        e.used.set(self.tick.get());
        Some(&e.value)
    }
    pub fn insert(&mut self, key: String, value: V) {
        self.put(key, value, false);
    }
    pub fn finish(&mut self, key: String, value: V, failed: bool) {
        self.put(key, value, failed);
    }
    fn put(&mut self, key: String, value: V, failed: bool) {
        self.tick.set(self.tick.get() + 1);
        self.map.insert(
            key,
            Entry {
                value,
                used: Cell::new(self.tick.get()),
                failed: failed.then(Instant::now),
            },
        );
        while self.map.len() > 64
            || self.map.values().map(|e| e.value.bytes()).sum::<usize>() > 8 * 1024 * 1024
        {
            let Some(old) = self
                .map
                .iter()
                .min_by_key(|(_, e)| e.used.get())
                .map(|(k, _)| k.clone())
            else {
                break;
            };
            self.map.remove(&old);
        }
    }
}
