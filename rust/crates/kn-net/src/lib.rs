//! Kindlia 使用的 KinNovel HTTP/TLS 基础设施，不包含轻书架 SignalR/会话。
pub mod dns;
pub mod error;
pub mod gzip;
pub mod health;
pub mod http;
pub mod rate_limit;
pub mod stream;
pub mod turn;
pub use error::NetError;
