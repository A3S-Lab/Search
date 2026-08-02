//! Cross-process circuit persistence for short-lived CLI invocations.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use a3s_search::{CircuitBreaker, CircuitSnapshot, CircuitState, EngineOutcomeKind, SearchResults};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const STATE_SCHEMA: &str = "a3s/search-cli-circuit-state/v1";
const STATE_FILE_PREFIX: &str = "circuit-state-v1";
const SCOPE_DOMAIN: &[u8] = b"a3s/search-cli-transport-scope/v1\0";
const MAX_STATE_BYTES: u64 = 1024 * 1024;
const MAX_ENTRIES: usize = 256;
const MAX_KEY_BYTES: usize = 128;
const MAX_OPEN_MILLIS: u64 = 24 * 60 * 60 * 1000;

/// File-backed open-circuit state scoped to one CLI execution.
pub(crate) struct PersistentCircuitState {
    breaker: CircuitBreaker,
    state_path: PathBuf,
    lock_path: PathBuf,
    scope_sha256: String,
    loaded_entries: BTreeMap<String, StateEntry>,
    shortcuts: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StateFile {
    schema: String,
    scope_sha256: String,
    entries: BTreeMap<String, StateEntry>,
}

impl StateFile {
    fn empty(scope_sha256: &str) -> Self {
        Self {
            schema: STATE_SCHEMA.to_string(),
            scope_sha256: scope_sha256.to_string(),
            entries: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StateEntry {
    open_until_unix_ms: u64,
    updated_at_unix_ms: u64,
    ejection_count: u32,
    failure_kind: String,
}

#[derive(Debug)]
enum StateUpdate {
    Open {
        shortcut: String,
        open_until_unix_ms: u64,
        ejection_count: u32,
        failure_kind: String,
    },
    Close {
        shortcut: String,
        loaded_entry: StateEntry,
    },
}

impl PersistentCircuitState {
    /// Loads the platform-native default state file when one is available.
    pub(crate) async fn load_default(
        shortcuts: &[String],
        proxy: Option<&str>,
        browser: &str,
    ) -> io::Result<Option<Self>> {
        let scope_sha256 = transport_scope(proxy, browser);
        let Some(path) = default_state_path(&scope_sha256) else {
            return Ok(None);
        };
        Self::load(path, shortcuts, scope_sha256).await.map(Some)
    }

    async fn load(
        state_path: PathBuf,
        shortcuts: &[String],
        scope_sha256: String,
    ) -> io::Result<Self> {
        let lock_path = lock_path(&state_path)?;
        let read_state_path = state_path.clone();
        let read_lock_path = lock_path.clone();
        let read_scope = scope_sha256.clone();
        let state = tokio::task::spawn_blocking(move || {
            with_locked_state(&read_state_path, &read_lock_path, &read_scope, Ok)
        })
        .await
        .map_err(join_error)??;

        let loaded_at_unix_ms = unix_millis();
        let shortcuts = normalized_shortcuts(shortcuts);
        let loaded_entries = shortcuts
            .iter()
            .filter_map(|shortcut| {
                state
                    .entries
                    .get(shortcut)
                    .cloned()
                    .map(|entry| (shortcut.clone(), entry))
            })
            .collect();
        let breaker = CircuitBreaker::default();
        for shortcut in &shortcuts {
            let Some(entry) = state.entries.get(shortcut) else {
                continue;
            };
            if entry.open_until_unix_ms.saturating_add(MAX_OPEN_MILLIS) < loaded_at_unix_ms {
                continue;
            }
            breaker.restore_open_state(
                shortcut,
                Duration::from_millis(entry.open_until_unix_ms.saturating_sub(loaded_at_unix_ms)),
                entry.ejection_count,
            );
        }

        Ok(Self {
            breaker,
            state_path,
            lock_path,
            scope_sha256,
            loaded_entries,
            shortcuts,
        })
    }

    pub(crate) fn breaker(&self) -> CircuitBreaker {
        self.breaker.clone()
    }

    /// Merges typed outcomes into the shared state without storing query or
    /// result content.
    pub(crate) async fn persist(&self, results: &SearchResults) -> io::Result<()> {
        let now = unix_millis();
        let updates = self.updates(results, now);
        if updates.is_empty() {
            return Ok(());
        }
        let state_path = self.state_path.clone();
        let lock_path = self.lock_path.clone();
        let scope_sha256 = self.scope_sha256.clone();
        tokio::task::spawn_blocking(move || {
            with_locked_state(&state_path, &lock_path, &scope_sha256, |mut state| {
                state.entries.retain(|_, entry| {
                    entry.open_until_unix_ms.saturating_add(MAX_OPEN_MILLIS) >= now
                });
                for update in updates {
                    match update {
                        StateUpdate::Open {
                            shortcut,
                            open_until_unix_ms,
                            ejection_count,
                            failure_kind,
                        } => {
                            let entry = state.entries.entry(shortcut).or_insert(StateEntry {
                                open_until_unix_ms,
                                updated_at_unix_ms: now,
                                ejection_count,
                                failure_kind: failure_kind.clone(),
                            });
                            entry.open_until_unix_ms =
                                entry.open_until_unix_ms.max(open_until_unix_ms);
                            entry.updated_at_unix_ms = now;
                            entry.ejection_count = entry.ejection_count.max(ejection_count);
                            entry.failure_kind = failure_kind;
                        }
                        StateUpdate::Close {
                            shortcut,
                            loaded_entry,
                        } => {
                            if state.entries.get(&shortcut) == Some(&loaded_entry) {
                                state.entries.remove(&shortcut);
                            }
                        }
                    }
                }
                write_state_atomically(&state_path, &state)?;
                Ok(state)
            })
        })
        .await
        .map_err(join_error)??;
        Ok(())
    }

    fn updates(&self, results: &SearchResults, now: u64) -> Vec<StateUpdate> {
        let allowed = self.shortcuts.iter().collect::<BTreeSet<_>>();
        results
            .outcomes()
            .iter()
            .filter(|outcome| allowed.contains(&outcome.shortcut))
            .filter_map(|outcome| {
                let snapshot = self.breaker.snapshot(&outcome.shortcut);
                match (outcome.kind, snapshot.state) {
                    (EngineOutcomeKind::CircuitOpen | EngineOutcomeKind::Rejected, _) => None,
                    (
                        EngineOutcomeKind::Success | EngineOutcomeKind::Empty,
                        CircuitState::Closed,
                    ) => self
                        .loaded_entries
                        .get(&outcome.shortcut)
                        .cloned()
                        .map(|loaded_entry| StateUpdate::Close {
                            shortcut: outcome.shortcut.clone(),
                            loaded_entry,
                        }),
                    (_, CircuitState::Open) => open_update(outcome, snapshot, now),
                    _ => None,
                }
            })
            .collect()
    }
}

fn open_update(
    outcome: &a3s_search::EngineOutcome,
    snapshot: CircuitSnapshot,
    now: u64,
) -> Option<StateUpdate> {
    let retry_after = snapshot.retry_after?;
    let open_millis = u64::try_from(retry_after.as_millis())
        .unwrap_or(u64::MAX)
        .min(MAX_OPEN_MILLIS);
    let failure_kind = outcome.failure.as_ref()?.kind.as_str();
    if failure_kind != "challenge" {
        return None;
    }
    Some(StateUpdate::Open {
        shortcut: outcome.shortcut.clone(),
        open_until_unix_ms: now.saturating_add(open_millis),
        ejection_count: snapshot.ejection_count.max(1),
        failure_kind: failure_kind.to_string(),
    })
}

fn default_state_path(scope_sha256: &str) -> Option<PathBuf> {
    let scope = scope_sha256.strip_prefix("sha256:")?;
    let file_name = format!("{STATE_FILE_PREFIX}-{scope}.json");
    if let Some(directory) = std::env::var_os("A3S_SEARCH_STATE_DIR") {
        let directory = PathBuf::from(directory);
        return directory.is_absolute().then(|| directory.join(&file_name));
    }
    dirs::state_dir()
        .or_else(dirs::data_local_dir)
        .map(|directory| directory.join("a3s-search").join(file_name))
}

fn transport_scope(proxy: Option<&str>, browser: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(SCOPE_DOMAIN);
    digest.update(browser.as_bytes());
    digest.update([0]);
    digest.update(proxy.unwrap_or("direct").as_bytes());
    let digest = digest.finalize();
    let mut encoded = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(encoded, "{byte:02x}");
    }
    format!("sha256:{encoded}")
}

fn normalized_shortcuts(shortcuts: &[String]) -> Vec<String> {
    shortcuts
        .iter()
        .map(|shortcut| shortcut.trim().to_ascii_lowercase())
        .filter(|shortcut| valid_identifier(shortcut, MAX_KEY_BYTES))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn valid_identifier(value: &str, maximum_bytes: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum_bytes
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

fn lock_path(path: &Path) -> io::Result<PathBuf> {
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "state path has no file name")
        })?;
    path.parent()
        .map(|parent| parent.join(format!("{stem}.lock")))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "state path has no parent"))
}

fn with_locked_state<T>(
    state_path: &Path,
    lock_path: &Path,
    scope_sha256: &str,
    operation: impl FnOnce(StateFile) -> io::Result<T>,
) -> io::Result<T> {
    let parent = state_path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "state path has no parent"))?;
    create_private_directory(parent)?;
    let lock = open_private_lock(lock_path)?;
    FileExt::lock_exclusive(&lock)?;
    let state = read_state(state_path, scope_sha256)?;
    let result = operation(state);
    let unlock_result = FileExt::unlock(&lock);
    match (result, unlock_result) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), _) | (Ok(_), Err(error)) => Err(error),
    }
}

fn read_state(path: &Path, scope_sha256: &str) -> io::Result<StateFile> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(StateFile::empty(scope_sha256));
        }
        Err(error) => return Err(error),
    };
    if file.metadata()?.len() > MAX_STATE_BYTES {
        return Ok(StateFile::empty(scope_sha256));
    }
    let mut bytes = Vec::new();
    file.take(MAX_STATE_BYTES + 1).read_to_end(&mut bytes)?;
    let state = match serde_json::from_slice::<StateFile>(&bytes) {
        Ok(state) if valid_state(&state, scope_sha256) => state,
        _ => StateFile::empty(scope_sha256),
    };
    Ok(state)
}

fn valid_state(state: &StateFile, scope_sha256: &str) -> bool {
    state.schema == STATE_SCHEMA
        && state.scope_sha256 == scope_sha256
        && valid_sha256(&state.scope_sha256)
        && state.entries.len() <= MAX_ENTRIES
        && state.entries.iter().all(|(key, entry)| {
            valid_identifier(key, MAX_KEY_BYTES)
                && entry.ejection_count > 0
                && entry.failure_kind == "challenge"
                && entry.open_until_unix_ms >= entry.updated_at_unix_ms
                && entry
                    .open_until_unix_ms
                    .saturating_sub(entry.updated_at_unix_ms)
                    <= MAX_OPEN_MILLIS
        })
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value[7..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn write_state_atomically(path: &Path, state: &StateFile) -> io::Result<()> {
    let mut bytes = serde_json::to_vec(state).map_err(io::Error::other)?;
    bytes.push(b'\n');
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "state path has no parent"))?;
    let mut temporary = None;
    for _ in 0..16 {
        let candidate = parent.join(format!(
            ".{STATE_FILE_PREFIX}.{}.{}.tmp",
            std::process::id(),
            fastrand::u64(..)
        ));
        match open_private_new(&candidate) {
            Ok(file) => {
                temporary = Some((candidate, file));
                break;
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    let (temporary_path, mut file) = temporary.ok_or_else(|| {
        io::Error::new(io::ErrorKind::AlreadyExists, "cannot allocate state file")
    })?;
    let write_result = (|| {
        file.write_all(&bytes)?;
        file.sync_all()?;
        #[cfg(windows)]
        if path.exists() {
            fs::remove_file(path)?;
        }
        fs::rename(&temporary_path, path)?;
        #[cfg(unix)]
        File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if write_result.is_err() {
        let _ = fs::remove_file(&temporary_path);
    }
    write_result
}

fn create_private_directory(path: &Path) -> io::Result<()> {
    fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn open_private_lock(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.create(true).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

fn open_private_new(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

fn unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn join_error(error: tokio::task::JoinError) -> io::Error {
    io::Error::other(format!("circuit-state worker failed: {error}"))
}

#[cfg(test)]
#[path = "circuit_state/tests.rs"]
mod tests;
