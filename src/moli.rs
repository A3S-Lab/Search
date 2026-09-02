//! Moli-backed rendering for the Search headless tier.
//!
//! Moli is distributed as a standalone executable rather than a small Rust
//! library.  This adapter keeps that process boundary explicit and implements
//! the same [`a3s_use_browser::PageRenderer`] contract used by the other Search
//! integrations.  Each render uses Moli's documented `fetch --dump html`
//! command, while a semaphore bounds the number of concurrent Moli processes.
//!
//! The adapter deliberately does not download a runtime.  A browser executable
//! is an operational dependency and must be installed by the host (or supplied
//! through `A3S_MOLI_EXECUTABLE`).  This keeps library calls offline-safe and
//! makes runtime provenance visible to operators.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use a3s_use_browser::{
    PageRenderer, RenderRequest, RenderedPage, UseError, UseResult, WaitCondition,
};
use async_trait::async_trait;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::{Child, Command};
use tokio::sync::Semaphore;
use tokio::task::JoinHandle;
use tracing::{debug, warn};

/// Canonical upstream project used by the default Search headless backend.
pub const MOLI_REPOSITORY_URL: &str = "https://github.com/lexmount/moli";

const DEFAULT_MAX_TABS: usize = 4;
const STDERR_DIAGNOSTIC_LIMIT: usize = 4 * 1024;
const STDOUT_LIMIT: usize = 32 * 1024 * 1024;
const CLEANUP_TIMEOUT: Duration = Duration::from_secs(2);

/// Configuration for a [`MoliPool`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoliPoolConfig {
    /// An exact Moli executable.  When omitted, [`detect_moli`] is used.
    pub executable: Option<PathBuf>,
    /// Optional proxy passed to Moli's HTTP transport.
    pub proxy_url: Option<String>,
    /// Optional persistent Moli profile directory.
    pub profile_dir: Option<PathBuf>,
    /// Maximum number of concurrent Moli fetch processes.
    pub max_tabs: usize,
}

impl Default for MoliPoolConfig {
    fn default() -> Self {
        Self {
            executable: None,
            proxy_url: None,
            profile_dir: None,
            max_tabs: DEFAULT_MAX_TABS,
        }
    }
}

impl MoliPoolConfig {
    /// Uses one exact executable and leaves all other options at their defaults.
    pub fn with_executable(path: impl Into<PathBuf>) -> Self {
        Self {
            executable: Some(path.into()),
            ..Self::default()
        }
    }

    /// Sets the proxy URL used by subsequent Moli fetches.
    pub fn with_proxy(mut self, proxy_url: impl Into<String>) -> Self {
        self.proxy_url = Some(proxy_url.into());
        self
    }

    /// Sets a persistent profile directory for cookies and browser state.
    pub fn with_profile_dir(mut self, profile_dir: impl Into<PathBuf>) -> Self {
        self.profile_dir = Some(profile_dir.into());
        self
    }

    /// Sets the maximum number of concurrent fetch processes.
    pub fn with_max_tabs(mut self, max_tabs: usize) -> Self {
        self.max_tabs = max_tabs;
        self
    }
}

/// A bounded, cancellation-safe pool of Moli command renders.
///
/// Moli's CLI owns the browser process for one fetch and exits after emitting
/// the document.  The pool therefore owns admission control and cleanup rather
/// than a resident browser handle.
pub struct MoliPool {
    config: MoliPoolConfig,
    closed: Arc<AtomicBool>,
    tab_semaphore: Arc<Semaphore>,
}

impl Default for MoliPool {
    fn default() -> Self {
        Self::new(MoliPoolConfig::default())
    }
}

impl std::fmt::Debug for MoliPool {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MoliPool")
            .field("config", &self.config)
            .field("closed", &self.closed.load(Ordering::Acquire))
            .field("available_tab_permits", &self.available_tab_permits())
            .finish()
    }
}

impl MoliPool {
    /// Creates a pool from typed Moli options.
    pub fn new(mut config: MoliPoolConfig) -> Self {
        config.max_tabs = config.max_tabs.max(1);
        let max_tabs = config.max_tabs;
        Self {
            config,
            closed: Arc::new(AtomicBool::new(false)),
            tab_semaphore: Arc::new(Semaphore::new(max_tabs)),
        }
    }

    /// Creates a pool bound to one exact Moli executable.
    pub fn from_executable(path: impl Into<PathBuf>) -> Self {
        Self::new(MoliPoolConfig::with_executable(path))
    }

    /// Returns the immutable pool configuration.
    pub fn config(&self) -> &MoliPoolConfig {
        &self.config
    }

    /// Returns the number of fetches that can start immediately.
    pub fn available_tab_permits(&self) -> usize {
        self.tab_semaphore.available_permits()
    }

    /// Returns whether this pool has been shut down.
    pub fn is_shutdown(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }

    /// Resolves the configured or discovered executable without starting a
    /// browser process.
    pub fn resolve_executable(&self) -> UseResult<PathBuf> {
        if let Some(path) = self.config.executable.as_deref() {
            return validate_executable(path);
        }
        resolve_moli()
    }

    /// Checks that the selected runtime is available.
    pub fn warm_up(&self) -> UseResult<()> {
        self.ensure_open()?;
        self.resolve_executable().map(|_| ())
    }

    /// Prevents new renders.  In-flight child processes are still cleaned up
    /// by their cancellation-safe guards.
    pub fn shutdown(&self) {
        if !self.closed.swap(true, Ordering::AcqRel) {
            self.tab_semaphore.close();
        }
    }

    fn ensure_open(&self) -> UseResult<()> {
        (!self.is_shutdown()).then_some(()).ok_or_else(|| {
            browser_error(
                "use.browser.closed",
                "Moli rendering pool has already been shut down.",
            )
        })
    }
}

#[async_trait]
impl PageRenderer for MoliPool {
    async fn render(&self, request: RenderRequest) -> UseResult<RenderedPage> {
        validate_request(&request)?;
        self.ensure_open()?;

        if request.timeout_ms == 0 {
            return Err(render_timeout(&request));
        }

        let started = Instant::now();
        let timeout = request.timeout();
        let deadline = tokio::time::Instant::now()
            .checked_add(timeout)
            .ok_or_else(|| {
                browser_error(
                    "use.browser.invalid_timeout",
                    "Moli render timeout exceeds the supported platform range.",
                )
            })?;
        let permit =
            tokio::time::timeout_at(deadline, Arc::clone(&self.tab_semaphore).acquire_owned())
                .await
                .map_err(|_| render_timeout(&request))?
                .map_err(|error| {
                    browser_error(
                        "use.browser.closed",
                        format!("Moli rendering pool is closed: {error}"),
                    )
                })?;
        self.ensure_open()?;

        let executable = self.resolve_executable()?;
        debug!(path = %executable.display(), "Rendering page with Moli");
        let html = run_fetch_command(&executable, &self.config, &request, deadline).await?;
        apply_post_fetch_wait(&request.wait, deadline, &request).await?;
        drop(permit);

        Ok(RenderedPage {
            requested_url: request.url.clone(),
            final_url: request.url,
            status: None,
            content_type: Some("text/html".to_string()),
            html,
            elapsed_ms: duration_millis(started.elapsed()),
            artifacts: Vec::new(),
        })
    }
}

/// Finds an installed Moli executable without downloading anything.
///
/// Detection order is intentionally deterministic: explicit A3S override,
/// generic Moli overrides, PATH, then the locations used by the official
/// installer and common package managers.
pub fn detect_moli() -> Option<PathBuf> {
    for variable in [
        "A3S_MOLI_EXECUTABLE",
        "A3S_SEARCH_MOLI_EXECUTABLE",
        "MOLI_EXECUTABLE",
        "MOLI",
    ] {
        if let Some(path) = std::env::var_os(variable).and_then(|value| resolve_named_path(&value))
        {
            debug!(variable, path = %path.display(), "Found Moli executable");
            return Some(path);
        }
    }

    if let Some(path) = command_in_path("moli") {
        debug!(path = %path.display(), "Found Moli executable in PATH");
        return Some(path);
    }

    let mut candidates = Vec::new();
    if let Some(directory) = std::env::var_os("MOLI_INSTALL_DIR") {
        candidates.push(PathBuf::from(directory).join(executable_name()));
    }
    if let Some(home) = dirs::home_dir() {
        candidates.extend([
            home.join(format!(".local/bin/{}", executable_name())),
            home.join(format!(".cargo/bin/{}", executable_name())),
            home.join(format!(".a3s/bin/{}", executable_name())),
        ]);
    }
    candidates.extend([
        PathBuf::from(format!("/opt/homebrew/bin/{}", executable_name())),
        PathBuf::from(format!("/usr/local/bin/{}", executable_name())),
        PathBuf::from(format!("/usr/bin/{}", executable_name())),
    ]);

    candidates.into_iter().find(|path| is_executable(path))
}

#[cfg(windows)]
fn executable_name() -> &'static str {
    "moli.exe"
}

#[cfg(not(windows))]
fn executable_name() -> &'static str {
    "moli"
}

/// Resolves Moli or returns an actionable, offline-safe runtime error.
pub fn resolve_moli() -> UseResult<PathBuf> {
    detect_moli().ok_or_else(|| {
        browser_error(
            "use.browser.runtime_missing",
            "Moli executable was not found.",
        )
        .with_suggestion(format!(
            "Install Moli from {MOLI_REPOSITORY_URL} (the official installer places it in ~/.local/bin), or set A3S_MOLI_EXECUTABLE=/path/to/moli."
        ))
    })
}

fn validate_executable(path: &Path) -> UseResult<PathBuf> {
    if is_executable(path) {
        Ok(path.to_path_buf())
    } else {
        Err(browser_error(
            "use.browser.runtime_missing",
            format!("Moli executable is missing or not executable: {}", path.display()),
        )
        .with_suggestion(format!(
            "Install Moli from {MOLI_REPOSITORY_URL} or provide an executable path through A3S_MOLI_EXECUTABLE."
        )))
    }
}

fn resolve_named_path(value: &std::ffi::OsStr) -> Option<PathBuf> {
    let path = Path::new(value);
    if path.components().count() > 1 || path.is_absolute() {
        return is_executable(path).then(|| path.to_path_buf());
    }
    command_in_path(value)
}

fn command_in_path(command: impl AsRef<std::ffi::OsStr>) -> Option<PathBuf> {
    let command = command.as_ref();
    let paths = std::env::var_os("PATH")?;
    for directory in std::env::split_paths(&paths) {
        let candidate = directory.join(command);
        if is_executable(&candidate) {
            return Some(candidate);
        }
        #[cfg(windows)]
        {
            for extension in [".exe", ".cmd", ".bat"] {
                let candidate =
                    directory.join(format!("{}{}", command.to_string_lossy(), extension));
                if is_executable(&candidate) {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

fn is_executable(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn validate_request(request: &RenderRequest) -> UseResult<()> {
    if request.screenshot_path.is_some() {
        return Err(browser_error(
            "use.browser.unsupported",
            "Moli HTML rendering does not support screenshot artifacts.",
        ));
    }
    if matches!(
        &request.wait,
        WaitCondition::Selector { css, .. } if css.trim().is_empty()
    ) {
        return Err(browser_error(
            "use.browser.invalid_request",
            "Moli selector waits require a non-empty CSS selector.",
        ));
    }
    Ok(())
}

fn fetch_arguments(config: &MoliPoolConfig, request: &RenderRequest) -> Vec<OsString> {
    let timeout_ms = request.timeout_ms.max(1).to_string();
    let mut args = vec![
        OsString::from("fetch"),
        OsString::from("--dump"),
        OsString::from("html"),
        OsString::from("--wait-until"),
    ];

    match &request.wait {
        WaitCondition::Load => args.push(OsString::from("load")),
        WaitCondition::DomContentLoaded => args.push(OsString::from("domcontentloaded")),
        WaitCondition::NetworkIdle { .. } => args.push(OsString::from("networkidle")),
        WaitCondition::Selector { css, .. } => {
            args.push(OsString::from("load"));
            args.push(OsString::from("--wait-selector"));
            args.push(OsString::from(css));
        }
        WaitCondition::Delay { ms } => {
            args.push(OsString::from("load"));
            args.push(OsString::from("--delay-ms"));
            args.push(OsString::from(ms.to_string()));
        }
    }

    args.extend([
        OsString::from("--http-connect-timeout"),
        OsString::from(&timeout_ms),
        OsString::from("--http-timeout"),
        OsString::from(&timeout_ms),
        OsString::from("--log-level"),
        OsString::from("error"),
    ]);
    if let Some(proxy) = config.proxy_url.as_deref() {
        args.push(OsString::from("--http-proxy"));
        args.push(OsString::from(proxy));
    }
    if let Some(profile_dir) = config.profile_dir.as_deref() {
        args.push(OsString::from("--profile-dir"));
        args.push(profile_dir.as_os_str().to_os_string());
    }
    if let Some(user_agent) = request.user_agent.as_deref() {
        args.push(OsString::from("--user-agent"));
        args.push(OsString::from(user_agent));
    }
    args.push(OsString::from("--timeout"));
    args.push(OsString::from(&timeout_ms));
    args.push(OsString::from(request.url.as_str()));
    args
}

async fn run_fetch_command(
    executable: &Path,
    config: &MoliPoolConfig,
    request: &RenderRequest,
    deadline: tokio::time::Instant,
) -> UseResult<String> {
    let mut command = Command::new(executable);
    command
        .args(fetch_arguments(config, request))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let mut child = command.spawn().map_err(|error| {
        browser_error(
            "use.browser.process",
            format!("Failed to spawn Moli ({}): {error}", executable.display()),
        )
    })?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| browser_error("use.browser.process", "Failed to capture Moli stdout."))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| browser_error("use.browser.process", "Failed to capture Moli stderr."))?;
    let mut process = FetchProcess::new(
        child,
        tokio::spawn(read_limited(stdout, STDOUT_LIMIT)),
        tokio::spawn(read_limited(stderr, STDERR_DIAGNOSTIC_LIMIT)),
    );

    let status = match tokio::time::timeout_at(deadline, process.wait()).await {
        Ok(Ok(status)) => status,
        Ok(Err(error)) => {
            process.terminate().await;
            return Err(browser_error(
                "use.browser.process",
                format!("Failed while waiting for Moli: {error}"),
            ));
        }
        Err(_) => {
            process.terminate().await;
            return Err(render_timeout(request));
        }
    };
    process.mark_reaped();

    let stdout = join_output(process.take_stdout()?, "stdout", deadline, request).await?;
    let stderr = join_output(process.take_stderr()?, "stderr", deadline, request).await?;
    decode_fetch_output(status, stdout, stderr, config.proxy_url.as_deref())
}

struct FetchProcess {
    child: Option<Child>,
    stdout: Option<JoinHandle<std::io::Result<CapturedOutput>>>,
    stderr: Option<JoinHandle<std::io::Result<CapturedOutput>>>,
}

impl FetchProcess {
    fn new(
        child: Child,
        stdout: JoinHandle<std::io::Result<CapturedOutput>>,
        stderr: JoinHandle<std::io::Result<CapturedOutput>>,
    ) -> Self {
        Self {
            child: Some(child),
            stdout: Some(stdout),
            stderr: Some(stderr),
        }
    }

    async fn wait(&mut self) -> std::io::Result<ExitStatus> {
        self.child
            .as_mut()
            .ok_or_else(|| std::io::Error::other("Moli child process state was lost"))?
            .wait()
            .await
    }

    fn mark_reaped(&mut self) {
        self.child.take();
    }

    fn take_stdout(&mut self) -> UseResult<JoinHandle<std::io::Result<CapturedOutput>>> {
        self.stdout.take().ok_or_else(|| {
            browser_error("use.browser.process", "Moli stdout reader state was lost.")
        })
    }

    fn take_stderr(&mut self) -> UseResult<JoinHandle<std::io::Result<CapturedOutput>>> {
        self.stderr.take().ok_or_else(|| {
            browser_error("use.browser.process", "Moli stderr reader state was lost.")
        })
    }

    async fn terminate(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.start_kill();
            let _ = tokio::time::timeout(CLEANUP_TIMEOUT, child.wait()).await;
        }
        self.abort_readers();
    }

    fn abort_readers(&mut self) {
        if let Some(task) = self.stdout.take() {
            task.abort();
        }
        if let Some(task) = self.stderr.take() {
            task.abort();
        }
    }
}

impl Drop for FetchProcess {
    fn drop(&mut self) {
        self.abort_readers();
        let Some(mut child) = self.child.take() else {
            return;
        };
        match tokio::runtime::Handle::try_current() {
            Ok(runtime) => {
                runtime.spawn(async move {
                    let _ = child.start_kill();
                    let _ = tokio::time::timeout(CLEANUP_TIMEOUT, child.wait()).await;
                });
            }
            Err(error) => {
                warn!("Cannot schedule Moli process cleanup: {error}");
                let _ = child.start_kill();
            }
        }
    }
}

#[derive(Debug)]
struct CapturedOutput {
    bytes: Vec<u8>,
    truncated: bool,
}

async fn read_limited(
    mut pipe: impl AsyncRead + Unpin,
    limit: usize,
) -> std::io::Result<CapturedOutput> {
    let mut bytes = Vec::with_capacity(limit.min(8 * 1024));
    let mut buffer = [0_u8; 8 * 1024];
    let mut truncated = false;
    loop {
        let read = pipe.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        let remaining = limit.saturating_sub(bytes.len());
        let retained = remaining.min(read);
        bytes.extend_from_slice(&buffer[..retained]);
        if retained < read {
            truncated = true;
        }
    }
    Ok(CapturedOutput { bytes, truncated })
}

async fn join_output(
    mut task: JoinHandle<std::io::Result<CapturedOutput>>,
    stream: &str,
    deadline: tokio::time::Instant,
    request: &RenderRequest,
) -> UseResult<CapturedOutput> {
    let joined = match tokio::time::timeout_at(deadline, &mut task).await {
        Ok(joined) => joined,
        Err(_) => {
            task.abort();
            return Err(render_timeout(request));
        }
    };
    joined
        .map_err(|error| {
            browser_error(
                "use.browser.process",
                format!("Moli {stream} reader failed: {error}"),
            )
        })?
        .map_err(|error| {
            browser_error(
                "use.browser.process",
                format!("Failed to read Moli {stream}: {error}"),
            )
        })
}

fn decode_fetch_output(
    status: ExitStatus,
    stdout: CapturedOutput,
    stderr: CapturedOutput,
    proxy: Option<&str>,
) -> UseResult<String> {
    if stdout.truncated {
        return Err(browser_error(
            "use.browser.output_too_large",
            format!(
                "Moli returned HTML larger than the {} MiB safety limit.",
                STDOUT_LIMIT / (1024 * 1024)
            ),
        ));
    }
    if !status.success() {
        let diagnostic = bounded_diagnostic(&stderr.bytes, proxy);
        let suffix = if diagnostic.is_empty() {
            String::new()
        } else {
            format!(": {diagnostic}")
        };
        return Err(browser_error(
            "use.browser.process",
            format!("Moli fetch exited with {status}{suffix}"),
        ));
    }

    let html = String::from_utf8(stdout.bytes).map_err(|error| {
        browser_error(
            "use.browser.process",
            format!("Moli returned non-UTF-8 HTML: {error}"),
        )
    })?;
    if html.trim().is_empty() {
        return Err(browser_error(
            "use.browser.empty",
            "Moli returned an empty HTML document.",
        ));
    }
    Ok(html)
}

fn bounded_diagnostic(stderr: &[u8], proxy: Option<&str>) -> String {
    let start = stderr.len().saturating_sub(STDERR_DIAGNOSTIC_LIMIT);
    let mut diagnostic = String::from_utf8_lossy(&stderr[start..]).trim().to_string();
    if let Some(proxy) = proxy {
        diagnostic = diagnostic.replace(proxy, "<redacted-proxy>");
    }
    diagnostic
}

async fn apply_post_fetch_wait(
    wait: &WaitCondition,
    deadline: tokio::time::Instant,
    request: &RenderRequest,
) -> UseResult<()> {
    // Moli owns the lifecycle and selector waits.  Its network-idle threshold
    // is intentionally fixed, so preserve the typed contract's extra quiet
    // period after Moli reports network idle.
    let delay = match wait {
        WaitCondition::NetworkIdle { idle_ms } => Some(*idle_ms),
        WaitCondition::Load
        | WaitCondition::DomContentLoaded
        | WaitCondition::Selector { .. }
        | WaitCondition::Delay { .. } => None,
    };
    if let Some(delay) = delay {
        tokio::time::timeout_at(deadline, tokio::time::sleep(Duration::from_millis(delay)))
            .await
            .map_err(|_| render_timeout(request))?;
    }
    Ok(())
}

fn render_timeout(request: &RenderRequest) -> UseError {
    browser_error(
        "use.browser.timeout",
        format!(
            "Moli rendering exceeded {} ms.",
            request.timeout().as_millis()
        ),
    )
}

fn browser_error(code: impl Into<String>, message: impl Into<String>) -> UseError {
    UseError::new(code, message)
}

fn duration_millis(duration: Duration) -> u64 {
    duration.as_millis().try_into().unwrap_or(u64::MAX)
}

#[cfg(test)]
#[path = "moli_tests.rs"]
mod tests;
