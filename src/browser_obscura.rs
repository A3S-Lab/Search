//! Obscura headless browser integration.
//!
//! This module implements a headless browser backend using obscura,
//! a Rust-native browser built for AI agents and web scraping.
//!
//! ## Architecture
//!
//! - [`CdpClient`]: Low-level CDP WebSocket client. Sends JSON-RPC 2.0 commands
//!   and routes responses/events back to callers.
//! - [`ObscuraPool`]: Manages the obscura subprocess lifecycle (spawn, connect,
//!   shutdown). Provides a shared `CdpClient` to all tabs.
//! - [`ObscuraFetcher`]: Implements [`PageFetcher`]. Opens a new tab for each
//!   fetch, navigates, waits, extracts HTML, and closes the tab.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use tokio::{
    process::Command,
    sync::{broadcast, mpsc, oneshot, Mutex, Semaphore},
    time::{sleep, Duration},
};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{debug, warn};

use crate::browser_setup_obs::ensure_obscura;
use crate::error::SearchError;
use crate::fetcher::{PageFetcher, WaitStrategy};
use crate::Result;

// ---------------------------------------------------------------------------
// CDP types
// ---------------------------------------------------------------------------

/// A raw CDP message — either a command response or an event.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(untagged)]
enum CdpMessage {
    Response(CdpResponse),
    Event(CdpEvent),
}

/// CDP command response.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct CdpResponse {
    pub id: i64,
    #[serde(default)]
    pub result: Option<serde_json::Value>,
    #[serde(default)]
    pub error: Option<CdpError>,
}

/// CDP error object.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct CdpError {
    pub code: i64,
    pub message: String,
}

/// CDP event notification.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct CdpEvent {
    pub method: String,
    pub params: Option<serde_json::Value>,
    #[serde(default)]
    pub session_id: Option<String>,
}

/// Result of Target.createTarget.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct CreateTargetResult {
    pub target_id: String,
}

/// Result of Target.attachToTarget.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct AttachTargetResult {
    pub session_id: String,
}

/// Result of Page.navigate.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct NavigateResult {
    pub frame_id: String,
    pub loader_id: Option<String>,
    #[serde(default)]
    pub error_text: Option<String>,
}

/// A CDP remote object returned by Runtime.evaluate.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct RemoteObject {
    #[serde(rename = "type")]
    pub obj_type: String,
    pub value: Option<serde_json::Value>,
    pub object_id: Option<String>,
}

/// Result of Runtime.evaluate.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct EvaluateResult {
    pub result: RemoteObject,
}

/// Result of DOM.getDocument.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct GetDocumentResult {
    pub root: DomNode,
}

/// A DOM node.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct DomNode {
    #[serde(rename = "nodeId")]
    pub node_id: u64,
}

/// Result of DOM.querySelector.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct QuerySelectorResult {
    #[serde(rename = "nodeId")]
    pub node_id: u64,
}

/// Result of DOM.getOuterHTML.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct GetOuterHtmlResult {
    #[serde(rename = "outerHTML")]
    pub outer_html: String,
}

// ---------------------------------------------------------------------------
// CDP client
// ---------------------------------------------------------------------------

/// Low-level CDP WebSocket client.
///
/// Send commands with [`CdpClient::send_command`] and receive events via
/// [`CdpClient::events()`].
pub struct CdpClient {
    pending: Arc<tokio::sync::Mutex<HashMap<i64, oneshot::Sender<Result<CdpResponse>>>>>,
    event_tx: broadcast::Sender<CdpEvent>,
    write_tx: mpsc::Sender<Message>,
    _reader_handle: tokio::task::AbortHandle,
}

impl CdpClient {
    /// Connect to the CDP WebSocket server at the given URL.
    pub async fn connect(ws_url: &str) -> Result<Self> {
        let (ws_stream, _) = connect_async(ws_url)
            .await
            .map_err(|e| SearchError::Browser(format!("Failed to connect to CDP: {}", e)))?;

        let (ws_write, ws_read) = tokio_tungstenite::WebSocketStream::split(ws_stream);

        let pending: Arc<tokio::sync::Mutex<HashMap<i64, oneshot::Sender<Result<CdpResponse>>>>> =
            Arc::new(tokio::sync::Mutex::new(HashMap::new()));
        let (event_tx, _) = broadcast::channel(64);
        let (write_tx, mut write_rx) = mpsc::channel::<Message>(32);

        let event_tx_clone = event_tx.clone();
        let pending_clone = pending.clone();

        // Spawn writer task
        let writer_handle = tokio::spawn(async move {
            let mut ws_write = ws_write;
            while let Some(msg) = write_rx.recv().await {
                if ws_write.send(msg).await.is_err() {
                    break;
                }
            }
        });

        // Spawn reader task — reads from WebSocket, routes responses to waiting
        // callers, broadcasts events to all subscribers.
        let reader_handle = tokio::spawn(async move {
            let mut ws_read = ws_read;
            while let Some(msg) = ws_read.next().await {
                match msg {
                    Ok(Message::Text(text)) => match serde_json::from_str::<CdpMessage>(&text) {
                        Ok(CdpMessage::Response(resp)) => {
                            let id = resp.id;
                            let tx = {
                                let mut guard = pending_clone.lock().await;
                                guard.remove(&id)
                            };
                            if let Some(tx) = tx {
                                let _ = tx.send(Ok(resp));
                            }
                        }
                        Ok(CdpMessage::Event(evt)) => {
                            let _ = event_tx_clone.send(evt);
                        }
                        Err(e) => {
                            debug!("Failed to parse CDP message: {}", e);
                        }
                    },
                    Ok(Message::Close(..)) | Err(..) => {
                        break;
                    }
                    _ => {}
                }
            }
            let _ = writer_handle.abort();
        });

        Ok(Self {
            pending,
            event_tx,
            write_tx,
            _reader_handle: reader_handle.abort_handle(),
        })
    }

    /// Send a CDP command and wait for the response.
    pub async fn send_command<R: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<R> {
        static NEXT_ID: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(1);

        let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let cmd = serde_json::json!({
            "id": id,
            "method": method,
            "params": params.unwrap_or(serde_json::Value::Null),
        });

        let msg = Message::Text(
            serde_json::to_string(&cmd)
                .map_err(|e| {
                    SearchError::Browser(format!("Failed to serialize CDP command: {}", e))
                })?
                .into(),
        );

        let (tx, rx) = oneshot::channel();

        self.pending.lock().await.insert(id, tx);
        self.write_tx
            .send(msg)
            .await
            .map_err(|e| SearchError::Browser(format!("Failed to send CDP command: {}", e)))?;

        let resp = rx
            .await
            .map_err(|e| SearchError::Browser(format!("CDP request {} dropped: {}", id, e)))?
            .map_err(|e| SearchError::Browser(format!("CDP error: {}", e)))?;

        if resp.error.is_some() {
            let err = resp.error.unwrap();
            return Err(SearchError::Browser(format!(
                "CDP error {}: {}",
                err.code, err.message
            )));
        }

        serde_json::from_value(resp.result.unwrap_or(serde_json::Value::Null))
            .map_err(|e| SearchError::Browser(format!("Failed to parse CDP response: {}", e)))
    }

    /// Subscribe to CDP events.
    pub fn events(&self) -> broadcast::Receiver<CdpEvent> {
        self.event_tx.subscribe()
    }
}

// ---------------------------------------------------------------------------
// ObscuraPool
// ---------------------------------------------------------------------------

/// Configuration for the obscura pool.
#[derive(Debug, Clone)]
pub struct ObscuraPoolConfig {
    /// Maximum number of concurrent browser tabs.
    pub max_tabs: usize,
    /// Path to the obscura executable. If `None`, auto-detected.
    pub obscura_path: Option<String>,
    /// Proxy URL for the browser to use.
    pub proxy_url: Option<String>,
}

impl Default for ObscuraPoolConfig {
    fn default() -> Self {
        Self {
            max_tabs: 4,
            obscura_path: None,
            proxy_url: None,
        }
    }
}

/// A shared pool managing a single obscura subprocess with tab concurrency control.
///
/// The subprocess is lazily spawned on the first `acquire_client()` call.
pub struct ObscuraPool {
    config: ObscuraPoolConfig,
    child: Mutex<Option<tokio::process::Child>>,
    client: Mutex<Option<Arc<CdpClient>>>,
    tab_semaphore: Arc<Semaphore>,
}

impl ObscuraPool {
    /// Creates a new pool with the given configuration.
    pub fn new(config: ObscuraPoolConfig) -> Self {
        let max_tabs = config.max_tabs;
        Self {
            config,
            child: Mutex::new(None),
            client: Mutex::new(None),
            tab_semaphore: Arc::new(Semaphore::new(max_tabs)),
        }
    }

    /// Returns the tab semaphore for acquiring permits before opening tabs.
    pub fn tab_semaphore(&self) -> &Arc<Semaphore> {
        &self.tab_semaphore
    }

    /// Lazily spawns obscura and connects, returning a shared client.
    pub async fn acquire_client(&self) -> Result<Arc<CdpClient>> {
        // Fast path: already connected
        if let Ok(guard) = self.client.try_lock() {
            if let Some(ref client) = *guard {
                return Ok(Arc::clone(client));
            }
        }

        // Slow path: spawn and connect
        let mut child_guard = self.child.lock().await;
        let mut client_guard = self.client.lock().await;

        // Double-check after acquiring locks
        if let Some(ref client) = *client_guard {
            return Ok(Arc::clone(client));
        }

        // Resolve obscura binary
        let obscura_path = if let Some(ref path) = self.config.obscura_path {
            PathBuf::from(path)
        } else {
            ensure_obscura().await?
        };

        // Find a free port
        let port = find_free_port()?;

        // Build spawn arguments
        let mut args = vec![
            "serve".to_string(),
            "--host".to_string(),
            "127.0.0.1".to_string(),
            "--port".to_string(),
            port.to_string(),
        ];

        if let Some(ref proxy) = self.config.proxy_url {
            args.push("--proxy".to_string());
            args.push(proxy.clone());
        }

        debug!("Spawning obscura at {}", obscura_path.display());

        let child = Command::new(&obscura_path)
            .args(&args)
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| {
                SearchError::Browser(format!(
                    "Failed to spawn obscura ({}): {}",
                    obscura_path.display(),
                    e
                ))
            })?;

        *child_guard = Some(child);

        // Wait for CDP server to be ready
        wait_for_cdp_ready("127.0.0.1", port, Duration::from_secs(10)).await?;

        let ws_url = format!("ws://127.0.0.1:{}", port);
        debug!("Connecting to obscura CDP at {}", ws_url);

        let client = CdpClient::connect(&ws_url).await?;
        let client = Arc::new(client);
        *client_guard = Some(Arc::clone(&client));

        Ok(client)
    }

    /// Shuts down the obscura subprocess.
    pub async fn shutdown(&self) {
        let mut child_guard = self.child.lock().await;
        if let Some(mut child) = child_guard.take() {
            if let Err(e) = child.kill().await {
                warn!("Failed to kill obscura subprocess: {}", e);
            } else {
                debug!("Obscura subprocess killed");
            }
        }

        let mut client_guard = self.client.lock().await;
        client_guard.take();
    }
}

/// Find a free TCP port.
fn find_free_port() -> Result<u16> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")
        .map_err(|e| SearchError::Browser(format!("Failed to find a free port: {}", e)))?;
    let port = listener
        .local_addr()
        .map_err(|e| SearchError::Browser(format!("Failed to read assigned port: {}", e)))?
        .port();
    Ok(port)
}

/// Poll until the CDP server at `host:port` accepts TCP connections.
async fn wait_for_cdp_ready(host: &str, port: u16, timeout: Duration) -> Result<()> {
    let addr = format!("{}:{}", host, port);
    let deadline = tokio::time::Instant::now() + timeout;

    loop {
        if tokio::time::Instant::now() >= deadline {
            return Err(SearchError::Browser(format!(
                "Timed out waiting for CDP server at {} to become ready",
                addr
            )));
        }

        if tokio::net::TcpStream::connect(&addr).await.is_ok() {
            debug!("CDP server at {} is ready", addr);
            return Ok(());
        }

        sleep(Duration::from_millis(100)).await;
    }
}

// ---------------------------------------------------------------------------
// ObscuraFetcher
// ---------------------------------------------------------------------------

/// A [`PageFetcher`] that uses obscura to render JavaScript-heavy pages.
pub struct ObscuraFetcher {
    pool: Arc<ObscuraPool>,
    wait: WaitStrategy,
    user_agent: Option<String>,
}

impl ObscuraFetcher {
    /// Creates a new fetcher with default wait strategy (`Load`).
    pub fn new(pool: Arc<ObscuraPool>) -> Self {
        Self {
            pool,
            wait: WaitStrategy::default(),
            user_agent: None,
        }
    }

    /// Sets the wait strategy for page rendering.
    pub fn with_wait(mut self, wait: WaitStrategy) -> Self {
        self.wait = wait;
        self
    }

    /// Sets a custom user agent.
    pub fn with_user_agent(mut self, user_agent: impl Into<String>) -> Self {
        self.user_agent = Some(user_agent.into());
        self
    }

    /// Execute a single fetch operation: create tab, navigate, wait, extract HTML, close tab.
    async fn do_fetch(&self, url: &str) -> Result<String> {
        // Acquire tab permit
        let _permit = self
            .pool
            .tab_semaphore()
            .acquire()
            .await
            .map_err(|e| SearchError::Browser(format!("Tab semaphore closed: {}", e)))?;

        let client = self.pool.acquire_client().await?;

        // Create a new blank tab
        let create_result: CreateTargetResult = client
            .send_command(
                "Target.createTarget",
                Some(serde_json::json!({ "url": "about:blank" })),
            )
            .await
            .map_err(|e| SearchError::Browser(format!("Failed to create target: {}", e)))?;

        let target_id = &create_result.target_id;

        // Attach to the target to get a session ID
        let attach_result: AttachTargetResult = client
            .send_command(
                "Target.attachToTarget",
                Some(serde_json::json!({ "targetId": target_id, "flatten": true })),
            )
            .await
            .map_err(|e| SearchError::Browser(format!("Failed to attach to target: {}", e)))?;

        let session_id = &attach_result.session_id;

        // Helper to send session-tagged commands
        async fn send_session_cmd<R: serde::de::DeserializeOwned>(
            client: &CdpClient,
            session_id: &str,
            method: &str,
            params: Option<serde_json::Value>,
        ) -> Result<R> {
            let mut p = params.unwrap_or(serde_json::Value::Null);
            if let Some(obj) = p.as_object_mut() {
                obj.insert("sessionId".to_string(), serde_json::json!(session_id));
            } else {
                p = serde_json::json!({ "sessionId": session_id });
            }
            client.send_command(method, Some(p)).await
        }

        // Enable required domains
        let _: serde_json::Value = send_session_cmd(&*client, session_id, "Page.enable", None)
            .await
            .map_err(|e| SearchError::Browser(format!("Page.enable failed: {}", e)))?;
        let _: serde_json::Value = send_session_cmd(&*client, session_id, "Runtime.enable", None)
            .await
            .map_err(|e| SearchError::Browser(format!("Runtime.enable failed: {}", e)))?;
        let _: serde_json::Value = send_session_cmd(&*client, session_id, "Network.enable", None)
            .await
            .map_err(|e| SearchError::Browser(format!("Network.enable failed: {}", e)))?;

        // Set user agent if configured
        if let Some(ref ua) = self.user_agent {
            let _: serde_json::Value = send_session_cmd(
                &*client,
                session_id,
                "Network.setUserAgentOverride",
                Some(serde_json::json!({ "userAgent": ua })),
            )
            .await
            .map_err(|e| SearchError::Browser(format!("Failed to set user agent: {}", e)))?;
        }

        // Subscribe to page events for wait strategy
        let events = client.events();
        let (load_tx, mut load_rx) = tokio::sync::mpsc::channel::<()>(1);
        let load_tx_clone = load_tx.clone();
        let session_id_clone = session_id.clone();

        // Spawn event listener
        let events_handler = tokio::spawn(async move {
            let mut events = events;
            while let Ok(evt) = events.recv().await {
                let matches_session = evt.session_id.as_ref() == Some(&session_id_clone);
                if !matches_session {
                    continue;
                }

                match evt.method.as_str() {
                    "Page.loadEventFired" | "Page.navigatedWithinDocument" => {
                        let _ = load_tx_clone.try_send(());
                    }
                    _ => {}
                }
            }
        });

        // Navigate to the target URL
        let _: NavigateResult = send_session_cmd(
            &*client,
            session_id,
            "Page.navigate",
            Some(serde_json::json!({ "url": url })),
        )
        .await
        .map_err(|e| SearchError::Browser(format!("Navigation failed: {}", e)))?;

        // Wait according to strategy
        match &self.wait {
            WaitStrategy::Load => {
                // Wait for load event
                let _ = tokio::time::timeout(Duration::from_secs(30), load_rx.recv()).await;
            }
            WaitStrategy::NetworkIdle { idle_ms } => {
                let _ = tokio::time::timeout(Duration::from_secs(30), load_rx.recv()).await;
                sleep(Duration::from_millis(*idle_ms)).await;
            }
            WaitStrategy::Selector { css, timeout_ms } => {
                let _ = tokio::time::timeout(Duration::from_secs(30), load_rx.recv()).await;
                // Poll for selector
                let deadline = tokio::time::Instant::now() + Duration::from_millis(*timeout_ms);
                let mut found = false;
                while tokio::time::Instant::now() < deadline {
                    let doc: GetDocumentResult =
                        send_session_cmd(&*client, session_id, "DOM.getDocument", None)
                            .await
                            .map_err(|e| {
                                SearchError::Browser(format!("DOM.getDocument failed: {}", e))
                            })?;

                    let selector_result: Option<QuerySelectorResult> = send_session_cmd(
                        &*client,
                        session_id,
                        "DOM.querySelector",
                        Some(serde_json::json!({
                            "nodeId": doc.root.node_id,
                            "selector": css
                        })),
                    )
                    .await
                    .ok();

                    if selector_result.is_some() {
                        found = true;
                        break;
                    }
                    sleep(Duration::from_millis(200)).await;
                }
                if !found {
                    debug!(
                        "Selector '{}' not found within {}ms, proceeding",
                        css, timeout_ms
                    );
                }
            }
            WaitStrategy::Delay { ms } => {
                let _ = tokio::time::timeout(Duration::from_secs(30), load_rx.recv()).await;
                sleep(Duration::from_millis(*ms)).await;
            }
        }

        // Stop the events handler
        let _ = events_handler.abort();

        // Extract HTML via Runtime.evaluate with document.documentElement.outerHTML
        let eval_result: EvaluateResult = send_session_cmd(
            &*client,
            session_id,
            "Runtime.evaluate",
            Some(serde_json::json!({
                "expression": "document.documentElement.outerHTML",
                "returnByValue": true
            })),
        )
        .await
        .map_err(|e| SearchError::Browser(format!("Failed to evaluate: {}", e)))?;

        let html = eval_result
            .result
            .value
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .ok_or_else(|| {
                SearchError::Browser("Runtime.evaluate returned no value".to_string())
            })?;

        // Close the tab
        let _: serde_json::Value = client
            .send_command(
                "Target.closeTarget",
                Some(serde_json::json!({ "targetId": target_id })),
            )
            .await
            .map_err(|e| SearchError::Browser(format!("Failed to close target: {}", e)))?;

        Ok(html)
    }
}

#[async_trait]
impl PageFetcher for ObscuraFetcher {
    async fn fetch(&self, url: &str) -> Result<String> {
        self.do_fetch(url).await
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_obscura_pool_config_default() {
        let config = ObscuraPoolConfig::default();
        assert_eq!(config.max_tabs, 4);
        assert!(config.obscura_path.is_none());
        assert!(config.proxy_url.is_none());
    }

    #[test]
    fn test_obscura_pool_new() {
        let pool = ObscuraPool::new(ObscuraPoolConfig::default());
        assert_eq!(pool.tab_semaphore().available_permits(), 4);
    }

    #[test]
    fn test_obscura_pool_custom_tabs() {
        let config = ObscuraPoolConfig {
            max_tabs: 8,
            ..Default::default()
        };
        let pool = ObscuraPool::new(config);
        assert_eq!(pool.tab_semaphore().available_permits(), 8);
    }

    #[test]
    fn test_obscura_fetcher_new() {
        let pool = Arc::new(ObscuraPool::new(ObscuraPoolConfig::default()));
        let fetcher = ObscuraFetcher::new(pool);
        assert!(matches!(fetcher.wait, WaitStrategy::Load));
        assert!(fetcher.user_agent.is_none());
    }

    #[test]
    fn test_obscura_fetcher_with_wait() {
        let pool = Arc::new(ObscuraPool::new(ObscuraPoolConfig::default()));
        let fetcher = ObscuraFetcher::new(pool).with_wait(WaitStrategy::Selector {
            css: "div.g".to_string(),
            timeout_ms: 5000,
        });
        assert!(matches!(fetcher.wait, WaitStrategy::Selector { .. }));
    }

    #[test]
    fn test_obscura_fetcher_with_user_agent() {
        let pool = Arc::new(ObscuraPool::new(ObscuraPoolConfig::default()));
        let fetcher = ObscuraFetcher::new(pool).with_user_agent("CustomBot/1.0");
        assert_eq!(fetcher.user_agent.as_deref(), Some("CustomBot/1.0"));
    }

    #[tokio::test]
    async fn test_obscura_pool_shutdown_no_process() {
        let pool = ObscuraPool::new(ObscuraPoolConfig::default());
        // Shutdown without ever launching should not panic
        pool.shutdown().await;
    }

    #[test]
    fn test_obscura_pool_config_clone() {
        let config = ObscuraPoolConfig {
            max_tabs: 8,
            obscura_path: Some("/usr/bin/obscura".to_string()),
            proxy_url: Some("http://localhost:8080".to_string()),
        };
        let cloned = config.clone();
        assert_eq!(cloned.max_tabs, 8);
        assert_eq!(cloned.obscura_path.as_deref(), Some("/usr/bin/obscura"));
        assert_eq!(cloned.proxy_url.as_deref(), Some("http://localhost:8080"));
    }

    #[test]
    fn test_find_free_port() {
        let port = find_free_port().expect("Should find a free port");
        assert!(port > 0, "Port should be non-zero");
    }

    // -------------------------------------------------------------------------
    // CDP type deserialization tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_cdp_response_deserialization() {
        let json = r#"{"id":1,"result":{"targetId":"tab1"}}"#;
        let resp: CdpResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.id, 1);
        let result = resp.result.unwrap();
        assert_eq!(result.get("targetId").unwrap().as_str().unwrap(), "tab1");
    }

    #[test]
    fn test_cdp_response_with_error() {
        let json = r#"{"id":2,"error":{"code":-32600,"message":"Invalid request"}}"#;
        let resp: CdpResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.id, 2);
        assert!(resp.error.is_some());
        let err = resp.error.unwrap();
        assert_eq!(err.code, -32600);
        assert_eq!(err.message, "Invalid request");
    }

    #[test]
    fn test_cdp_event_deserialization() {
        let json = r#"{"method":"Page.loadEventFired","params":{}}"#;
        let evt: CdpEvent = serde_json::from_str(json).unwrap();
        assert_eq!(evt.method, "Page.loadEventFired");
        assert!(evt.params.is_some());
        assert!(evt.session_id.is_none());
    }

    #[test]
    fn test_cdp_event_with_session_id() {
        // Note: CdpEvent.session_id has #[serde(default)], so missing is ok
        let json = r#"{"method":"Page.navigatedWithinDocument","params":{"frameId":"f1"}}"#;
        let evt: CdpEvent = serde_json::from_str(json).unwrap();
        assert_eq!(evt.method, "Page.navigatedWithinDocument");
        assert!(evt.session_id.is_none());
    }

    #[test]
    fn test_create_target_result_deserialization() {
        // Struct field is target_id (snake_case, no rename attr), so JSON key must match
        let json = r#"{"target_id":"target-abc-123"}"#;
        let result: CreateTargetResult = serde_json::from_str(json).unwrap();
        assert_eq!(result.target_id, "target-abc-123");
    }

    #[test]
    fn test_attach_target_result_deserialization() {
        // Struct field is session_id (no rename), so JSON key must be session_id
        let json = r#"{"session_id":"session-xyz"}"#;
        let result: AttachTargetResult = serde_json::from_str(json).unwrap();
        assert_eq!(result.session_id, "session-xyz");
    }

    #[test]
    fn test_navigate_result_deserialization() {
        // Struct fields are frame_id, loader_id (no rename), so JSON must use snake_case
        let json = r#"{"frame_id":"frame-1","loader_id":"loader-1"}"#;
        let result: NavigateResult = serde_json::from_str(json).unwrap();
        assert_eq!(result.frame_id, "frame-1");
        assert_eq!(result.loader_id.as_deref(), Some("loader-1"));
        assert!(result.error_text.is_none());
    }

    #[test]
    fn test_navigate_result_with_error() {
        let json = r#"{"frame_id":"frame-1","loader_id":null,"error_text":"Navigation failed"}"#;
        let result: NavigateResult = serde_json::from_str(json).unwrap();
        assert_eq!(result.frame_id, "frame-1");
        assert!(result.error_text.is_some());
        assert_eq!(result.error_text.as_deref(), Some("Navigation failed"));
    }

    #[test]
    fn test_remote_object_with_value() {
        // obj_type has #[serde(rename = "type")], so JSON key must be "type"
        let json = r#"{"type":"string","value":"hello"}"#;
        let obj: RemoteObject = serde_json::from_str(json).unwrap();
        assert_eq!(obj.obj_type, "string");
        assert!(obj.value.is_some());
        assert_eq!(obj.value.unwrap().as_str().unwrap(), "hello");
        assert!(obj.object_id.is_none());
    }

    #[test]
    fn test_remote_object_with_object_id() {
        // object_id has no rename, so JSON key must be object_id (snake_case)
        let json = r#"{"type":"object","value":null,"object_id":"obj-123"}"#;
        let obj: RemoteObject = serde_json::from_str(json).unwrap();
        assert_eq!(obj.obj_type, "object");
        assert!(obj.value.is_none());
        assert_eq!(obj.object_id.as_deref(), Some("obj-123"));
    }

    #[test]
    fn test_evaluate_result_deserialization() {
        let json = r#"{"result":{"type":"string","value":"<html></html>"}}"#;
        let result: EvaluateResult = serde_json::from_str(json).unwrap();
        assert_eq!(result.result.obj_type, "string");
        assert_eq!(
            result.result.value.as_ref().unwrap().as_str().unwrap(),
            "<html></html>"
        );
    }

    #[test]
    fn test_get_document_result_deserialization() {
        // node_id has #[serde(rename = "nodeId")], so JSON key must be "nodeId"
        let json = r#"{"root":{"nodeId":42}}"#;
        let result: GetDocumentResult = serde_json::from_str(json).unwrap();
        assert_eq!(result.root.node_id, 42);
    }

    #[test]
    fn test_query_selector_result_deserialization() {
        // node_id has #[serde(rename = "nodeId")]
        let json = r#"{"nodeId":99}"#;
        let result: QuerySelectorResult = serde_json::from_str(json).unwrap();
        assert_eq!(result.node_id, 99);
    }

    #[test]
    fn test_get_outer_html_result_deserialization() {
        // outer_html has #[serde(rename = "outerHTML")]
        let json = r#"{"outerHTML":"<div>Hello</div>"}"#;
        let result: GetOuterHtmlResult = serde_json::from_str(json).unwrap();
        assert_eq!(result.outer_html, "<div>Hello</div>");
    }

    // -------------------------------------------------------------------------
    // ObscuraPoolConfig tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_obscura_pool_config_all_fields() {
        let config = ObscuraPoolConfig {
            max_tabs: 16,
            obscura_path: Some("/custom/path/obscura".to_string()),
            proxy_url: Some("socks5://localhost:1080".to_string()),
        };
        assert_eq!(config.max_tabs, 16);
        assert_eq!(config.obscura_path.as_deref(), Some("/custom/path/obscura"));
        assert_eq!(config.proxy_url.as_deref(), Some("socks5://localhost:1080"));
    }

    #[test]
    fn test_obscura_pool_config_partial_clone() {
        let config = ObscuraPoolConfig {
            max_tabs: 4,
            obscura_path: None,
            proxy_url: Some("http://proxy:8080".to_string()),
        };
        let cloned = config.clone();
        assert_eq!(cloned.max_tabs, 4);
        assert!(cloned.obscura_path.is_none());
        assert_eq!(cloned.proxy_url.as_deref(), Some("http://proxy:8080"));
    }

    // -------------------------------------------------------------------------
    // ObscuraFetcher builder method tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_obscura_fetcher_builder_chain() {
        let pool = Arc::new(ObscuraPool::new(ObscuraPoolConfig::default()));
        let fetcher = ObscuraFetcher::new(pool)
            .with_wait(WaitStrategy::NetworkIdle { idle_ms: 500 })
            .with_user_agent("TestBot/1.0");
        assert!(matches!(
            fetcher.wait,
            WaitStrategy::NetworkIdle { idle_ms: 500 }
        ));
        assert_eq!(fetcher.user_agent.as_deref(), Some("TestBot/1.0"));
    }

    #[test]
    fn test_obscura_fetcher_with_delay_strategy() {
        let pool = Arc::new(ObscuraPool::new(ObscuraPoolConfig::default()));
        let fetcher = ObscuraFetcher::new(pool).with_wait(WaitStrategy::Delay { ms: 3000 });
        assert!(matches!(fetcher.wait, WaitStrategy::Delay { ms: 3000 }));
    }

    // -------------------------------------------------------------------------
    // Semaphore and port tests
    // -------------------------------------------------------------------------

    #[tokio::test]
    async fn test_obscura_pool_semaphore_acquire_multiple() {
        let config = ObscuraPoolConfig {
            max_tabs: 2,
            ..Default::default()
        };
        let pool = ObscuraPool::new(config);

        // Acquire all 2 permits
        let permit1 = pool.tab_semaphore().acquire().await.unwrap();
        let permit2 = pool.tab_semaphore().acquire().await.unwrap();
        assert_eq!(pool.tab_semaphore().available_permits(), 0);

        // Release them
        drop(permit1);
        drop(permit2);
        assert_eq!(pool.tab_semaphore().available_permits(), 2);
    }

    #[test]
    fn test_find_free_port_uniqueness() {
        let ports: Vec<u16> = (0..10).map(|_| find_free_port().unwrap()).collect();
        // All ports should be unique
        let mut sorted = ports.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(
            ports.len(),
            sorted.len(),
            "Ports should be unique: {:?}",
            ports
        );
    }

    #[test]
    fn test_find_free_port_valid_range() {
        for _ in 0..100 {
            let port = find_free_port().unwrap();
            assert!(port > 0, "Port must be non-zero");
            assert!(port >= 1, "Port must be >= 1");
        }
    }

    // -------------------------------------------------------------------------
    // wait_for_cdp_ready timeout behavior
    // -------------------------------------------------------------------------

    #[tokio::test]
    async fn test_wait_for_cdp_ready_timeout() {
        // Connecting to a non-routable IP should eventually timeout
        let result = wait_for_cdp_ready("127.0.0.1", 1, Duration::from_millis(50)).await;
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("Timed out"),
            "Error should mention timeout: {}",
            err_msg
        );
    }

    // -------------------------------------------------------------------------
    // Debug and clone on CDP types
    // -------------------------------------------------------------------------

    #[test]
    fn test_cdp_error_debug() {
        let err = CdpError {
            code: -32601,
            message: "Method not found".to_string(),
        };
        let debug_str = format!("{:?}", err);
        assert!(debug_str.contains("-32601"));
        assert!(debug_str.contains("Method not found"));
    }

    #[test]
    fn test_cdp_response_clone() {
        let json = r#"{"id":1,"result":{"targetId":"tab1"}}"#;
        let resp: CdpResponse = serde_json::from_str(json).unwrap();
        let cloned = resp.clone();
        assert_eq!(cloned.id, resp.id);
    }

    #[test]
    fn test_cdp_event_clone() {
        let json = r#"{"method":"Page.loadEventFired","params":{},"sessionId":"s1"}"#;
        let evt: CdpEvent = serde_json::from_str(json).unwrap();
        let cloned = evt.clone();
        assert_eq!(cloned.method, evt.method);
        assert_eq!(cloned.session_id, evt.session_id);
    }
}
