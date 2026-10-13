//! HTTP front for the native search service.

use std::net::SocketAddr;
use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use super::{parse_search_body, reply_for_results, NativeWebSearch, SearchReply};
use crate::SearchError;

const MAX_BODY_BYTES: usize = 64 * 1024;

pub(crate) fn parse_listen_addr(listen: &str) -> std::result::Result<SocketAddr, String> {
    listen
        .parse()
        .map_err(|_| format!("listen address must be host:port, got {listen}"))
}

pub(crate) fn public_bind_requires_key(
    addr: SocketAddr,
    api_key: Option<&str>,
) -> std::result::Result<(), String> {
    if addr.ip().is_loopback() || api_key.is_some_and(|key| !key.is_empty()) {
        Ok(())
    } else {
        Err("a non-loopback search listener requires A3S_SEARCH_API_KEY".to_string())
    }
}

pub(crate) fn deployment_api_key() -> Option<String> {
    std::env::var("A3S_SEARCH_API_KEY")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub(crate) async fn serve_native(listen: &str) -> crate::Result<()> {
    let addr = parse_listen_addr(listen).map_err(SearchError::Other)?;
    let api_key = deployment_api_key();
    public_bind_requires_key(addr, api_key.as_deref()).map_err(SearchError::Other)?;
    let listener = TcpListener::bind(addr)
        .await
        .map_err(|error| SearchError::Network(error.to_string()))?;
    let bound = listener
        .local_addr()
        .map_err(|error| SearchError::Network(error.to_string()))?;
    tracing::info!(%bound, "native search service listening");
    serve_on(
        listener,
        Arc::new(super::EngineWebSearch::new(super::DEFAULT_TIMEOUT)),
        api_key,
    )
    .await
    .map_err(|error| SearchError::Network(error.to_string()))
}

pub(crate) async fn serve_on<S: NativeWebSearch + 'static>(
    listener: TcpListener,
    search: Arc<S>,
    api_key: Option<String>,
) -> std::io::Result<()> {
    loop {
        let (stream, _) = listener.accept().await?;
        let search = Arc::clone(&search);
        let api_key = api_key.clone();
        tokio::spawn(async move {
            if let Err(error) = handle_connection(stream, search, api_key).await {
                tracing::debug!(%error, "search connection ended");
            }
        });
    }
}

async fn handle_connection<S: NativeWebSearch>(
    mut stream: TcpStream,
    search: Arc<S>,
    api_key: Option<String>,
) -> std::io::Result<()> {
    let received = match read_http(&mut stream).await? {
        Some(received) => received,
        None => return Ok(()),
    };
    let reply = dispatch(&received, search.as_ref(), api_key.as_deref()).await;
    write_json(&mut stream, &reply).await
}

struct Received {
    method: String,
    path: String,
    headers: String,
    body: Vec<u8>,
}

async fn dispatch<S: NativeWebSearch>(
    received: &Received,
    search: &S,
    api_key: Option<&str>,
) -> SearchReply {
    match (received.method.as_str(), received.path.as_str()) {
        ("GET", "/health") => SearchReply::Json {
            status: 200,
            body: serde_json::json!({"status": "ok"}),
        },
        ("POST", "/v1/search") => {
            if let Some(expected) = api_key {
                if !presented_key(&received.headers)
                    .is_some_and(|presented| keys_match(expected, &presented))
                {
                    return super::reply(401, "UNAUTHORIZED", "search credential was rejected");
                }
            }
            match parse_search_body(&received.body) {
                Ok(request) => match search.search(&request).await {
                    Ok(results) => reply_for_results(&request, &results),
                    Err(_) => super::reply(503, "UNAVAILABLE", "search failed"),
                },
                Err(reply) => reply,
            }
        }
        _ => super::reply(404, "NOT_FOUND", "search route was not found"),
    }
}

fn presented_key(headers: &str) -> Option<String> {
    let mut authorization = None;
    let mut api_key = None;
    for line in headers.lines() {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.eq_ignore_ascii_case("authorization") {
            authorization = bearer(value.trim());
        } else if name.eq_ignore_ascii_case("x-api-key") {
            api_key = Some(value.trim().to_string());
        }
    }
    authorization.or(api_key)
}

fn bearer(value: &str) -> Option<String> {
    let (scheme, token) = value.split_once(' ')?;
    scheme
        .eq_ignore_ascii_case("bearer")
        .then(|| token.trim().to_string())
        .filter(|token| !token.is_empty())
}

fn keys_match(expected: &str, presented: &str) -> bool {
    let left = expected.as_bytes();
    let right = presented.as_bytes();
    let mut diff = left.len() ^ right.len();
    let width = left.len().max(right.len());
    for index in 0..width {
        let a = left.get(index).copied().unwrap_or(0);
        let b = right.get(index).copied().unwrap_or(0);
        diff |= usize::from(a ^ b);
    }
    diff == 0
}

async fn read_http(stream: &mut TcpStream) -> std::io::Result<Option<Received>> {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 4096];
    let header_end = loop {
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            return Ok(None);
        }
        buffer.extend_from_slice(&chunk[..read]);
        if buffer.len() > MAX_BODY_BYTES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "search request exceeds 64KiB",
            ));
        }
        if let Some(offset) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            break offset + 4;
        }
    };
    let header_text = String::from_utf8_lossy(&buffer[..header_end]).into_owned();
    let mut lines = header_text.lines();
    let Some(request_line) = lines.next() else {
        return Ok(None);
    };
    let mut parts = request_line.split_whitespace();
    let Some(method) = parts.next() else {
        return Ok(None);
    };
    let Some(path) = parts.next() else {
        return Ok(None);
    };
    let content_length = header_text.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.eq_ignore_ascii_case("content-length")
            .then(|| value.trim().parse::<usize>().ok())
            .flatten()
    });
    let length = content_length.unwrap_or(0);
    if length > MAX_BODY_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "search body exceeds 64KiB",
        ));
    }
    while buffer.len() < header_end + length {
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
    }
    let body_end = (header_end + length).min(buffer.len());
    Ok(Some(Received {
        method: method.to_string(),
        path: path.to_string(),
        headers: header_text,
        body: buffer[header_end..body_end].to_vec(),
    }))
}

async fn write_json(stream: &mut TcpStream, reply: &SearchReply) -> std::io::Result<()> {
    let SearchReply::Json { status, body } = reply;
    let payload = serde_json::to_vec(body).unwrap_or_else(|_| {
        br#"{"success":false,"error":"search response could not be encoded","code":"UNAVAILABLE"}"#
            .to_vec()
    });
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        _ => "Service Unavailable",
    };
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        payload.len()
    );
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(&payload).await?;
    Ok(())
}
