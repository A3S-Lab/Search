use std::fmt;
use std::io::Read;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

use super::corpus::{LiveCanaryQuery, TierCapability};

const DRIVER_PROTOCOL_VERSION: u32 = 3;
const DRIVER_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_RECEIPT_BYTES: usize = 8 * 1024 * 1024;

pub(super) struct DriverClient {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    pub candidate_identity: String,
    pub driver_identity: String,
}

impl DriverClient {
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn start(
        driver: &Path,
        expected_driver_identity: &str,
        candidate: &Path,
        expected_candidate_identity: &str,
        manifest: &Path,
        evaluated_commit: &str,
        manifest_identity: &str,
        capabilities: &[TierCapability],
        profiles: &[String],
    ) -> Result<Self, DriverError> {
        verify_file_identity(driver, expected_driver_identity)?;
        verify_file_identity(candidate, expected_candidate_identity)?;
        let driver_identity = expected_driver_identity.to_string();
        let candidate_identity = expected_candidate_identity.to_string();
        let mut child = Command::new(driver)
            .arg("--candidate")
            .arg(candidate)
            .arg("--tier-manifest")
            .arg(manifest)
            .arg("--evaluated-commit")
            .arg(evaluated_commit)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .map_err(|_| DriverError::Start)?;
        let stdin = child.stdin.take().ok_or(DriverError::MissingPipe)?;
        let stdout = child.stdout.take().ok_or(DriverError::MissingPipe)?;
        let mut client = Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            candidate_identity,
            driver_identity,
        };
        let ready = tokio::time::timeout(DRIVER_HANDSHAKE_TIMEOUT, client.read_ready())
            .await
            .map_err(|_| DriverError::Timeout)??;
        if ready.schema_version != DRIVER_PROTOCOL_VERSION
            || ready.message_type != "ready"
            || ready.evaluated_commit != evaluated_commit
            || ready.driver_sha256 != client.driver_identity
            || ready.candidate_sha256 != client.candidate_identity
            || ready.manifest_sha256 != manifest_identity
            || ready.capabilities != capabilities
            || ready.profiles != profiles
        {
            return Err(DriverError::HandshakeMismatch);
        }
        Ok(client)
    }

    pub(super) async fn search(
        &mut self,
        attempt_id: u64,
        query: &LiveCanaryQuery,
    ) -> Result<ReceivedAttempt, DriverError> {
        self.write_request(&DriverRequest::Search {
            schema_version: DRIVER_PROTOCOL_VERSION,
            attempt_id,
            query,
        })
        .await?;
        let raw_json = self.read_line().await?;
        Ok(ReceivedAttempt { raw_json })
    }

    pub(super) async fn shutdown(&mut self) -> Result<(), DriverError> {
        if self
            .write_request(&DriverRequest::Shutdown {
                schema_version: DRIVER_PROTOCOL_VERSION,
            })
            .await
            .is_err()
        {
            self.force_stop().await;
            return Err(DriverError::ShutdownWrite);
        }
        match tokio::time::timeout(Duration::from_secs(10), self.child.wait()).await {
            Ok(Ok(status)) if status.success() => Ok(()),
            Ok(Ok(_)) | Ok(Err(_)) => Err(DriverError::ShutdownNonZero),
            _ => {
                self.force_stop().await;
                Err(DriverError::ShutdownTimeout)
            }
        }
    }

    async fn force_stop(&mut self) {
        let _ = self.child.kill().await;
        let _ = self.child.wait().await;
    }

    async fn read_ready(&mut self) -> Result<ReadyReceipt, DriverError> {
        let line = self.read_line().await?;
        serde_json::from_str(&line).map_err(|_| DriverError::InvalidJson)
    }

    async fn read_line(&mut self) -> Result<String, DriverError> {
        read_bounded_line(&mut self.stdout).await
    }

    async fn write_request<T: Serialize>(&mut self, request: &T) -> Result<(), DriverError> {
        let mut encoded = serde_json::to_vec(request).map_err(|_| DriverError::Encode)?;
        encoded.push(b'\n');
        self.stdin
            .write_all(&encoded)
            .await
            .map_err(|_| DriverError::Write)?;
        self.stdin.flush().await.map_err(|_| DriverError::Write)
    }
}

pub(super) struct ReceivedAttempt {
    pub raw_json: String,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum DriverRequest<'a> {
    Search {
        schema_version: u32,
        attempt_id: u64,
        query: &'a LiveCanaryQuery,
    },
    Shutdown {
        schema_version: u32,
    },
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadyReceipt {
    #[serde(rename = "type")]
    message_type: String,
    schema_version: u32,
    evaluated_commit: String,
    driver_sha256: String,
    candidate_sha256: String,
    manifest_sha256: String,
    capabilities: Vec<TierCapability>,
    profiles: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum FailureStage {
    PreExecution,
    Api,
    HttpRss,
    Headless,
}

impl FailureStage {
    pub(super) fn capability(self) -> Option<TierCapability> {
        match self {
            Self::PreExecution => None,
            Self::Api => Some(TierCapability::Api),
            Self::HttpRss => Some(TierCapability::HttpRss),
            Self::Headless => Some(TierCapability::Headless),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AttemptReceipt {
    #[serde(rename = "type")]
    pub message_type: String,
    pub schema_version: u32,
    pub attempt_id: u64,
    pub query_id: String,
    pub evaluated_commit: String,
    pub candidate_sha256: String,
    #[serde(default)]
    pub terminal_error_kind: Option<String>,
    #[serde(default)]
    pub terminal_failure_stage: Option<FailureStage>,
    pub attempt_duration_ms: u64,
    #[serde(default)]
    pub resource_samples: Vec<ProcessTreeResourceSample>,
    pub tiers: Vec<TierReceipt>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProcessTreeResourceSample {
    pub sequence: u64,
    pub campaign_elapsed_ms: u64,
    pub rss_kib: u64,
    pub file_descriptors: usize,
    pub process_count: usize,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TierReceipt {
    pub capability: TierCapability,
    pub profile_sha256: String,
    pub results: a3s_search::SearchResults,
    pub calls: Vec<UpstreamCallReceipt>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct UpstreamCallReceipt {
    pub provider_scope: String,
    pub engine_shortcut: String,
    pub started_offset_ms: u64,
    pub ended_offset_ms: u64,
    #[serde(default)]
    pub is_retry: bool,
    #[serde(default)]
    pub failure_kind: Option<String>,
    #[serde(default)]
    pub retryable: bool,
    #[serde(default)]
    pub retry_after_seconds: Option<u64>,
}

pub(super) fn verify_file_identity(path: &Path, expected: &str) -> Result<(), DriverError> {
    let actual = file_sha256(path)?;
    (actual == expected)
        .then_some(())
        .ok_or(DriverError::ArtifactIdentityMismatch)
}

fn file_sha256(path: &Path) -> Result<String, DriverError> {
    let metadata = std::fs::symlink_metadata(path).map_err(|_| DriverError::ArtifactRead)?;
    if !metadata.file_type().is_file() {
        return Err(DriverError::ArtifactRead);
    }
    let mut file = std::fs::File::open(path).map_err(|_| DriverError::ArtifactRead)?;
    if !file
        .metadata()
        .map_err(|_| DriverError::ArtifactRead)?
        .file_type()
        .is_file()
    {
        return Err(DriverError::ArtifactRead);
    }
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|_| DriverError::ArtifactRead)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    if !std::fs::symlink_metadata(path)
        .map_err(|_| DriverError::ArtifactRead)?
        .file_type()
        .is_file()
    {
        return Err(DriverError::ArtifactRead);
    }
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

#[derive(Debug)]
pub(super) enum DriverError {
    Start,
    MissingPipe,
    ArtifactRead,
    ArtifactIdentityMismatch,
    SealedArtifact(String),
    Encode,
    Write,
    Read,
    InvalidJson,
    InvalidReceipt(String),
    ReceiptTooLarge,
    UnexpectedExit,
    HandshakeMismatch,
    Timeout,
    ShutdownWrite,
    ShutdownNonZero,
    ShutdownTimeout,
}

impl DriverError {
    pub(super) const fn kind(&self) -> &'static str {
        match self {
            Self::Start => "driver_start",
            Self::MissingPipe => "driver_pipe",
            Self::ArtifactRead => "artifact_read",
            Self::ArtifactIdentityMismatch => "artifact_identity_mismatch",
            Self::SealedArtifact(_) => "sealed_artifact_invalid",
            Self::Encode => "driver_encode",
            Self::Write => "driver_write",
            Self::Read => "driver_read",
            Self::InvalidJson => "driver_invalid_json",
            Self::InvalidReceipt(_) => "driver_invalid_receipt",
            Self::ReceiptTooLarge => "driver_receipt_too_large",
            Self::UnexpectedExit => "driver_unexpected_exit",
            Self::HandshakeMismatch => "driver_handshake_mismatch",
            Self::Timeout => "driver_timeout",
            Self::ShutdownWrite => "driver_shutdown_write",
            Self::ShutdownNonZero => "driver_shutdown_nonzero",
            Self::ShutdownTimeout => "driver_shutdown_timeout",
        }
    }
}

impl fmt::Display for DriverError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidReceipt(reason) => write!(formatter, "invalid driver receipt: {reason}"),
            Self::SealedArtifact(reason) => write!(formatter, "invalid sealed artifact: {reason}"),
            _ => formatter.write_str(self.kind()),
        }
    }
}

impl std::error::Error for DriverError {}

async fn read_bounded_line<R>(reader: &mut R) -> Result<String, DriverError>
where
    R: AsyncBufRead + Unpin,
{
    let mut bytes = Vec::with_capacity(8 * 1024);
    loop {
        let available = reader.fill_buf().await.map_err(|_| DriverError::Read)?;
        if available.is_empty() {
            return Err(DriverError::UnexpectedExit);
        }
        if let Some(newline) = available.iter().position(|byte| *byte == b'\n') {
            if bytes.len().saturating_add(newline) > MAX_RECEIPT_BYTES {
                return Err(DriverError::ReceiptTooLarge);
            }
            bytes.extend_from_slice(&available[..newline]);
            reader.consume(newline + 1);
            break;
        }
        if bytes.len().saturating_add(available.len()) > MAX_RECEIPT_BYTES {
            return Err(DriverError::ReceiptTooLarge);
        }
        let consumed = available.len();
        bytes.extend_from_slice(available);
        reader.consume(consumed);
    }
    String::from_utf8(bytes).map_err(|_| DriverError::InvalidJson)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn line_reader_rejects_a_receipt_before_allocating_beyond_the_limit() {
        let mut oversized = vec![b'x'; MAX_RECEIPT_BYTES + 1];
        oversized.push(b'\n');
        let mut reader = BufReader::new(oversized.as_slice());
        assert!(matches!(
            read_bounded_line(&mut reader).await,
            Err(DriverError::ReceiptTooLarge)
        ));
    }

    #[test]
    fn artifact_recheck_rejects_changed_driver_or_candidate_bytes() {
        let artifact = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(artifact.path(), b"sealed artifact").unwrap();
        let expected = file_sha256(artifact.path()).unwrap();
        verify_file_identity(artifact.path(), &expected).unwrap();

        std::fs::write(artifact.path(), b"changed artifact").unwrap();
        assert!(matches!(
            verify_file_identity(artifact.path(), &expected),
            Err(DriverError::ArtifactIdentityMismatch)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn artifact_recheck_rejects_a_replacement_symlink() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("target");
        let link = directory.path().join("candidate");
        std::fs::write(&target, b"sealed artifact").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let expected = file_sha256(&target).unwrap();

        assert!(matches!(
            verify_file_identity(&link, &expected),
            Err(DriverError::ArtifactRead)
        ));
    }
}
