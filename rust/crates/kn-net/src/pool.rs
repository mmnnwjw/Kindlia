//! Bounded HTTP/1.1 idle connection reuse for AutoNovel REST and images.
//! A stream is returned only after a completely framed response (including trailers).
use crate::{
    error::NetError,
    http::{HttpResponse, ServerUrl},
    stream::NetStream,
};
use rustls::ClientConfig;
use std::{
    io::{BufRead, BufReader, Read, Write},
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};

struct Idle {
    origin: String,
    tls: Arc<ClientConfig>,
    reader: BufReader<NetStream>,
    since: Instant,
}
static POOL: OnceLock<Mutex<Vec<Idle>>> = OnceLock::new();
const IDLE: Duration = Duration::from_secs(20);
fn debug(message: &str) {
    if std::env::var_os("KN_DEBUG").is_some() {
        eprintln!("[http-pool] {message}");
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn request(
    server: &str,
    path: &str,
    method: &str,
    headers: &[(&str, String)],
    body: &[u8],
    timeout: Duration,
    tls: Arc<ClientConfig>,
    limit: usize,
    progress: &mut dyn FnMut(usize, Option<usize>) -> bool,
) -> Result<HttpResponse, NetError> {
    if !method.bytes().all(|c| c.is_ascii_uppercase())
        || !path.starts_with('/')
        || path.contains(['\r', '\n', ' '])
        || headers.iter().any(|(k, v)| {
            k.is_empty()
                || !k.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
                || v.contains(['\r', '\n'])
        })
    {
        return Err(NetError::protocol("无效 HTTP 请求"));
    }
    let url = ServerUrl::parse(server)?;
    let origin = format!(
        "{}://{}:{}",
        if url.secure { "https" } else { "http" },
        url.host.to_ascii_lowercase(),
        url.port
    );
    let mut reused = {
        let mut pool = POOL
            .get_or_init(Mutex::default)
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        pool.retain(|c| c.since.elapsed() < IDLE);
        pool.iter()
            .rposition(|c| c.origin == origin && Arc::ptr_eq(&c.tls, &tls))
            .map(|i| pool.swap_remove(i))
    };
    loop {
        let was_reused = reused.is_some();
        let mut reader = if let Some(c) = reused.take() {
            debug("reuse");
            c.reader
        } else {
            debug("connect");
            BufReader::new(NetStream::connect(
                &url.host,
                url.port,
                url.secure,
                timeout,
                tls.clone(),
            )?)
        };
        reader.get_mut().set_read_timeout(Some(timeout)).ok();
        reader.get_mut().set_write_timeout(Some(timeout)).ok();
        let result = (|| {
            let mut head = format!(
                "{method} {path} HTTP/1.1\r\nHost: {}\r\n",
                url.host_header()
            );
            for (k, v) in headers {
                if !["connection", "content-length", "host"]
                    .iter()
                    .any(|h| k.eq_ignore_ascii_case(h))
                {
                    head.push_str(&format!("{k}: {v}\r\n"));
                }
            }
            head.push_str(&format!(
                "Content-Length: {}\r\nConnection: keep-alive\r\n\r\n",
                body.len()
            ));
            reader.get_mut().write_all(head.as_bytes())?;
            reader.get_mut().write_all(body)?;
            reader.get_mut().flush()?;
            response(&mut reader, method, limit, progress)
        })();
        match result {
            Ok((response, keep)) => {
                if keep && reader.buffer().is_empty() {
                    let mut pool = POOL
                        .get_or_init(Mutex::default)
                        .lock()
                        .unwrap_or_else(|e| e.into_inner());
                    if pool
                        .iter()
                        .filter(|c| c.origin == origin && Arc::ptr_eq(&c.tls, &tls))
                        .count()
                        < 2
                    {
                        if pool.len() >= 6 {
                            pool.remove(0);
                        }
                        pool.push(Idle {
                            origin,
                            tls,
                            reader,
                            since: Instant::now(),
                        });
                    }
                }
                return Ok(response);
            }
            // An expired idle socket can fail before a response. Never replay writes.
            Err(NetError::Network(ref text))
                if text != "下载已取消" && was_reused && matches!(method, "GET" | "HEAD") =>
            {
                debug("stale idle; retry safe read once");
            }
            Err(e) => return Err(e),
        }
    }
}

fn line(reader: &mut BufReader<NetStream>, budget: &mut usize) -> Result<String, NetError> {
    let is_start = *budget == 64 * 1024;
    let mut bytes = Vec::new();
    let n = reader
        .take((*budget + 1) as u64)
        .read_until(b'\n', &mut bytes)?;
    if n == 0 && bytes.is_empty() && is_start {
        return Err(NetError::network("对端关闭连接"));
    }
    if n == 0 || n > *budget || !bytes.ends_with(b"\r\n") {
        return Err(NetError::protocol("HTTP 行截断或过长"));
    }
    *budget -= n;
    String::from_utf8(bytes[..n - 2].to_vec())
        .map_err(|_| NetError::protocol("HTTP 响应头编码无效"))
}
fn response(
    reader: &mut BufReader<NetStream>,
    method: &str,
    limit: usize,
    progress: &mut dyn FnMut(usize, Option<usize>) -> bool,
) -> Result<(HttpResponse, bool), NetError> {
    let mut budget = 64 * 1024;
    let (status, headers, mut keep) = loop {
        let status_line = line(reader, &mut budget)?;
        let mut fields = status_line.split_whitespace();
        let version = fields
            .next()
            .ok_or_else(|| NetError::protocol("缺少 HTTP 版本"))?;
        if !matches!(version, "HTTP/1.1" | "HTTP/1.0") {
            return Err(NetError::protocol("无效 HTTP 版本"));
        }
        let status = fields
            .next()
            .and_then(|s| s.parse::<u16>().ok())
            .filter(|s| (100..600).contains(s))
            .ok_or_else(|| NetError::protocol("无效状态码"))?;
        let mut headers = vec![];
        loop {
            let l = line(reader, &mut budget)?;
            if l.is_empty() {
                break;
            }
            let (k, v) = l
                .split_once(':')
                .ok_or_else(|| NetError::protocol("无效 HTTP 响应头"))?;
            headers.push((k.trim().to_owned(), v.trim().to_owned()));
        }
        if (100..200).contains(&status) && status != 101 {
            continue;
        }
        break (status, headers, version == "HTTP/1.1");
    };
    let mut length = None;
    let mut transfer = None;
    for (k, v) in &headers {
        if k.eq_ignore_ascii_case("connection")
            && v.split(',').any(|v| v.trim().eq_ignore_ascii_case("close"))
        {
            keep = false;
        }
        if k.eq_ignore_ascii_case("content-length") {
            let n = v
                .parse::<usize>()
                .map_err(|_| NetError::protocol("无效正文长度"))?;
            if length.is_some_and(|old| old != n) {
                return Err(NetError::protocol("冲突的正文长度"));
            }
            length = Some(n);
        }
        if k.eq_ignore_ascii_case("transfer-encoding") {
            if transfer.is_some() || !v.eq_ignore_ascii_case("chunked") {
                return Err(NetError::protocol("不支持的传输编码"));
            }
            transfer = Some(());
        }
    }
    let mut body = Vec::new();
    if method == "HEAD" || matches!(status, 204 | 304) {
        return Ok((
            HttpResponse {
                status,
                headers,
                body,
            },
            keep,
        ));
    }
    if status == 101 {
        return Err(NetError::protocol("不支持 HTTP 协议升级"));
    }
    let read_exact_body = |reader: &mut BufReader<NetStream>,
                           body: &mut Vec<u8>,
                           count: usize,
                           total: Option<usize>,
                           progress: &mut dyn FnMut(usize, Option<usize>) -> bool|
     -> Result<(), NetError> {
        if count > limit.saturating_sub(body.len()) {
            return Err(NetError::protocol("下载内容超过上限"));
        }
        let target = body.len() + count;
        let mut buf = [0; 8192];
        while body.len() < target {
            let take = (target - body.len()).min(buf.len());
            let n = reader.read(&mut buf[..take])?;
            if n == 0 {
                return Err(NetError::network("响应正文不完整"));
            }
            body.extend_from_slice(&buf[..n]);
            if !progress(body.len(), total) {
                return Err(NetError::network("下载已取消"));
            }
        }
        Ok(())
    };
    if transfer.is_some() {
        // TE wins for decoding, but ambiguous framing is never put back in the pool.
        if length.is_some() {
            keep = false;
        }
        loop {
            let mut chunk_budget = 8192;
            let chunk = line(reader, &mut chunk_budget)?;
            let size = usize::from_str_radix(chunk.split(';').next().unwrap_or("").trim(), 16)
                .map_err(|_| NetError::protocol("无效块长度"))?;
            if size == 0 {
                let mut trailers = 32 * 1024;
                while !line(reader, &mut trailers)?.is_empty() {}
                break;
            }
            read_exact_body(reader, &mut body, size, None, progress)?;
            let mut crlf = [0; 2];
            reader.read_exact(&mut crlf)?;
            if crlf != *b"\r\n" {
                return Err(NetError::protocol("无效块结束符"));
            }
        }
    } else if let Some(n) = length {
        read_exact_body(reader, &mut body, n, Some(n), progress)?;
    } else {
        keep = false;
        let mut buf = [0; 8192];
        loop {
            let n = reader.read(&mut buf)?;
            if n == 0 {
                break;
            }
            if n > limit.saturating_sub(body.len()) {
                return Err(NetError::protocol("下载内容超过上限"));
            }
            body.extend_from_slice(&buf[..n]);
            if !progress(body.len(), None) {
                return Err(NetError::network("下载已取消"));
            }
        }
    }
    Ok((
        HttpResponse {
            status,
            headers,
            body,
        },
        keep,
    ))
}
