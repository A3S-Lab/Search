use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize)]
pub(super) struct ResourceSummary {
    pub samples: usize,
    pub first_rss_kib: u64,
    pub final_rss_kib: u64,
    pub max_rss_kib: u64,
    pub rss_growth_kib: i64,
    pub rss_25_percent_kib: u64,
    pub rss_50_percent_kib: u64,
    pub rss_75_percent_kib: u64,
    pub tail_rss_slope_kib_per_minute: f64,
    pub first_fds: usize,
    pub final_fds: usize,
    pub max_fds: usize,
    pub fd_growth: isize,
}

pub(super) async fn sample_resources(
    running: Arc<AtomicBool>,
    samples: Arc<Mutex<Vec<(u64, usize)>>>,
    warmup: Duration,
) {
    tokio::time::sleep(warmup).await;
    while running.load(Ordering::Acquire) {
        if let (Some(rss), Some(fds)) = (rss_kib(), fd_count()) {
            samples.lock().unwrap().push((rss, fds));
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    if let (Some(rss), Some(fds)) = (rss_kib(), fd_count()) {
        samples.lock().unwrap().push((rss, fds));
    }
}

pub(super) fn summarize_resources(samples: &[(u64, usize)]) -> ResourceSummary {
    assert!(
        samples.len() >= 2,
        "resource sampler produced too few samples"
    );
    let first = samples[0];
    let last = samples[samples.len() - 1];
    ResourceSummary {
        samples: samples.len(),
        first_rss_kib: first.0,
        final_rss_kib: last.0,
        max_rss_kib: samples
            .iter()
            .map(|sample| sample.0)
            .max()
            .unwrap_or(first.0),
        rss_growth_kib: last.0 as i64 - first.0 as i64,
        rss_25_percent_kib: samples[samples.len() / 4].0,
        rss_50_percent_kib: samples[samples.len() / 2].0,
        rss_75_percent_kib: samples[samples.len() * 3 / 4].0,
        tail_rss_slope_kib_per_minute: tail_rss_slope(samples),
        first_fds: first.1,
        final_fds: last.1,
        max_fds: samples
            .iter()
            .map(|sample| sample.1)
            .max()
            .unwrap_or(first.1),
        fd_growth: last.1 as isize - first.1 as isize,
    }
}

pub(super) fn resource_snapshot() -> Option<(u64, usize)> {
    Some((rss_kib()?, fd_count()?))
}

fn tail_rss_slope(samples: &[(u64, usize)]) -> f64 {
    let tail = &samples[samples.len() / 2..];
    if tail.len() < 2 {
        return 0.0;
    }
    let mean_x = (tail.len() - 1) as f64 / 2.0;
    let mean_y = tail.iter().map(|sample| sample.0 as f64).sum::<f64>() / tail.len() as f64;
    let (covariance, variance) =
        tail.iter()
            .enumerate()
            .fold((0.0, 0.0), |(covariance, variance), (index, sample)| {
                let x = index as f64 - mean_x;
                let y = sample.0 as f64 - mean_y;
                (covariance + x * y, variance + x * x)
            });
    if variance == 0.0 {
        0.0
    } else {
        covariance / variance * 60.0
    }
}

#[cfg(target_os = "linux")]
fn rss_kib() -> Option<u64> {
    std::fs::read_to_string("/proc/self/status")
        .ok()?
        .lines()
        .find_map(|line| line.strip_prefix("VmRSS:"))?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

#[cfg(not(target_os = "linux"))]
fn rss_kib() -> Option<u64> {
    let output = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .ok()?;
    String::from_utf8(output.stdout).ok()?.trim().parse().ok()
}

fn fd_count() -> Option<usize> {
    let directory = if cfg!(target_os = "linux") {
        "/proc/self/fd"
    } else {
        "/dev/fd"
    };
    Some(std::fs::read_dir(directory).ok()?.count())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tail_slope_ignores_warmup_and_detects_sustained_growth() {
        let plateau = [(1_000, 4), (2_000, 4), (2_000, 4), (2_000, 4)];
        assert_eq!(tail_rss_slope(&plateau), 0.0);

        let growing = [(1_000, 4), (2_000, 4), (2_100, 4), (2_200, 4)];
        assert_eq!(tail_rss_slope(&growing), 6_000.0);
    }
}
