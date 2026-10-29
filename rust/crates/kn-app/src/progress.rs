//! Ordered durable progress writes. UI reads staged progress immediately; shutdown drains the queue.
use crate::{model::NovelKey, store::{Progress, Store}};
use std::sync::{mpsc, Arc, Mutex};

enum Job {
    Save(Store, NovelKey),
    Fence(mpsc::Sender<()>),
}
pub struct Writer {
    sender: Option<mpsc::Sender<Job>>,
    thread: Option<std::thread::JoinHandle<()>>,
    error: Arc<Mutex<Option<String>>>,
}
impl Writer {
    pub fn new() -> Result<Self, String> {
        let (sender, receiver) = mpsc::channel();
        let error = Arc::new(Mutex::new(None));
        let errors = error.clone();
        let thread = std::thread::Builder::new().name("progress-save".into()).spawn(move || {
            while let Ok(job) = receiver.recv() {
                match job {
                    Job::Save(store, key) => {
                        let at = std::time::Instant::now();
                        let result = (|| {
                            let Some(value) = store.persist_progress(&key)? else { return Ok::<_, String>(()); };
                            if !value.user.is_empty() {
                                crate::history::enqueue_checked(&store, &value.user, &key, &value.progress.chapter_id, value.progress.updated_at,
                                    || store.progress_is_current(&key, value.sequence))?;
                            }
                            store.finish_progress(&key, value.sequence);
                            Ok(())
                        })();
                        if let Err(e) = result {
                            eprintln!("[progress-save] {e}");
                            *errors.lock().unwrap_or_else(|e| e.into_inner()) = Some(e);
                        }
                        if std::env::var_os("KN_DEBUG").is_some() {
                            eprintln!("[progress-save] worker {:.1} ms", at.elapsed().as_secs_f64()*1000.0);
                        }
                    }
                    Job::Fence(done) => { let _ = done.send(()); }
                }
            }
        }).map_err(|e| e.to_string())?;
        Ok(Self { sender: Some(sender), thread: Some(thread), error })
    }
    pub fn save(&self, store: Store, key: NovelKey, title: String, progress: Progress, user: String) -> Result<(), String> {
        store.stage_progress(key.clone(), title, progress, user);
        self.sender.as_ref().ok_or("进度写入队列已关闭")?.send(Job::Save(store, key)).map_err(|_| "进度写入线程已停止".into())
    }
    /// Enqueued without blocking the UI; network sync waits on it in its own worker.
    pub fn fence(&self) -> mpsc::Receiver<()> {
        let (done, receiver) = mpsc::channel();
        if let Some(sender) = &self.sender { let _ = sender.send(Job::Fence(done)); }
        receiver
    }
    pub fn flush(&self) { let _ = self.fence().recv(); }
    pub fn take_error(&self) -> Option<String> { self.error.lock().unwrap_or_else(|e| e.into_inner()).take() }
}
impl Drop for Writer {
    fn drop(&mut self) {
        self.sender.take();
        if let Some(thread) = self.thread.take() { let _ = thread.join(); }
    }
}
