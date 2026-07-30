use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::Serialize;
use sha2::{Digest, Sha256};

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

pub(super) struct AttemptLog {
    path: PathBuf,
    file: File,
    start_records: usize,
    terminal_records: usize,
    open_attempt: Option<(u64, String)>,
}

impl AttemptLog {
    pub(super) fn create(path: &Path) -> io::Result<Self> {
        if !path.is_absolute() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "attempt log path must be absolute",
            ));
        }
        let mut options = OpenOptions::new();
        options.create_new(true).append(true);
        #[cfg(unix)]
        options.mode(0o600);
        let file = options.open(path)?;
        Ok(Self {
            path: path.to_path_buf(),
            file,
            start_records: 0,
            terminal_records: 0,
            open_attempt: None,
        })
    }

    pub(super) fn append_start(&mut self, attempt_id: u64, query_id: &str) -> io::Result<()> {
        if self.open_attempt.is_some() {
            return Err(invalid_sequence("previous attempt has no terminal record"));
        }
        let record = AttemptStartRecord {
            record_type: "attempt_start",
            schema_version: 1,
            attempt_id,
            query_id,
        };
        self.append_serialized(&record)?;
        self.start_records = self.start_records.saturating_add(1);
        self.open_attempt = Some((attempt_id, query_id.to_string()));
        Ok(())
    }

    pub(super) fn append_driver_terminal(
        &mut self,
        attempt_id: u64,
        query_id: &str,
        raw_json: &str,
    ) -> io::Result<()> {
        self.ensure_open(attempt_id, query_id)?;
        self.append_raw(raw_json)?;
        self.close_attempt();
        Ok(())
    }

    pub(super) fn append_harness_terminal(
        &mut self,
        attempt_id: u64,
        query_id: &str,
        error_kind: &str,
    ) -> io::Result<()> {
        self.ensure_open(attempt_id, query_id)?;
        let record = HarnessTerminalRecord {
            record_type: "attempt_terminal",
            schema_version: 1,
            attempt_id,
            query_id,
            source: "harness",
            error_kind,
        };
        self.append_serialized(&record)?;
        self.close_attempt();
        Ok(())
    }

    pub(super) fn start_records(&self) -> usize {
        self.start_records
    }

    pub(super) fn terminal_records(&self) -> usize {
        self.terminal_records
    }

    pub(super) fn identity(&mut self) -> io::Result<String> {
        self.file.flush()?;
        self.file.sync_data()?;
        let bytes = std::fs::read(&self.path)?;
        Ok(format!("sha256:{:x}", Sha256::digest(bytes)))
    }

    fn ensure_open(&self, attempt_id: u64, query_id: &str) -> io::Result<()> {
        match self.open_attempt.as_ref() {
            Some((open_id, open_query)) if *open_id == attempt_id && open_query == query_id => {
                Ok(())
            }
            _ => Err(invalid_sequence(
                "terminal record does not match the open attempt",
            )),
        }
    }

    fn close_attempt(&mut self) {
        self.terminal_records = self.terminal_records.saturating_add(1);
        self.open_attempt = None;
    }

    fn append_serialized<T: Serialize>(&mut self, record: &T) -> io::Result<()> {
        let encoded = serde_json::to_string(record).map_err(io::Error::other)?;
        self.append_raw(&encoded)
    }

    fn append_raw(&mut self, raw_json: &str) -> io::Result<()> {
        if raw_json.is_empty() || raw_json.contains('\n') || raw_json.contains('\r') {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "attempt record must occupy exactly one JSONL line",
            ));
        }
        self.file.write_all(raw_json.as_bytes())?;
        self.file.write_all(b"\n")?;
        self.file.flush()?;
        self.file.sync_data()
    }
}

#[derive(Serialize)]
struct AttemptStartRecord<'a> {
    #[serde(rename = "type")]
    record_type: &'static str,
    schema_version: u32,
    attempt_id: u64,
    query_id: &'a str,
}

#[derive(Serialize)]
struct HarnessTerminalRecord<'a> {
    #[serde(rename = "type")]
    record_type: &'static str,
    schema_version: u32,
    attempt_id: u64,
    query_id: &'a str,
    source: &'static str,
    error_kind: &'a str,
}

fn invalid_sequence(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_durably_pairs_each_start_with_one_terminal_record() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("attempts.jsonl");
        let mut log = AttemptLog::create(&path).unwrap();
        log.append_start(1, "case-1").unwrap();
        log.append_driver_terminal(1, "case-1", r#"{"type":"attempt","attempt_id":1}"#)
            .unwrap();
        log.append_start(2, "case-2").unwrap();
        log.append_harness_terminal(2, "case-2", "driver_timeout")
            .unwrap();

        let records = std::fs::read_to_string(&path).unwrap();
        assert_eq!(records.lines().count(), 4);
        assert!(records.contains(
            r#"{"type":"attempt_start","schema_version":1,"attempt_id":1,"query_id":"case-1"}"#
        ));
        assert!(records.contains(r#"{"type":"attempt_terminal","schema_version":1,"attempt_id":2,"query_id":"case-2","source":"harness","error_kind":"driver_timeout"}"#));
        assert_eq!(log.start_records(), 2);
        assert_eq!(log.terminal_records(), 2);
        assert!(log.identity().unwrap().starts_with("sha256:"));
        assert!(AttemptLog::create(&path).is_err());
    }

    #[test]
    fn log_rejects_unpaired_or_mismatched_records() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("attempts.jsonl");
        let mut log = AttemptLog::create(&path).unwrap();
        assert!(log
            .append_harness_terminal(1, "case-1", "driver_timeout")
            .is_err());
        log.append_start(1, "case-1").unwrap();
        assert!(log.append_start(2, "case-2").is_err());
        assert!(log
            .append_harness_terminal(1, "case-2", "driver_timeout")
            .is_err());
    }
}
