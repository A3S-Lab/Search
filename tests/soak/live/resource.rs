use serde::Serialize;

use super::driver::ProcessTreeResourceSample;

#[derive(Default)]
pub(super) struct DriverResourceTracker {
    next_sequence: u64,
    last_elapsed_ms: Option<u64>,
    samples: Vec<ProcessTreeResourceSample>,
    pub integrity_violations: u64,
}

impl DriverResourceTracker {
    pub(super) fn record(&mut self, samples: &[ProcessTreeResourceSample]) {
        if samples.is_empty() {
            self.integrity_violations = self.integrity_violations.saturating_add(1);
            return;
        }
        for sample in samples {
            let valid = sample.sequence == self.next_sequence
                && self
                    .last_elapsed_ms
                    .is_none_or(|previous| sample.campaign_elapsed_ms > previous)
                && sample.rss_kib > 0
                && sample.process_count > 0;
            if valid {
                self.samples.push(sample.clone());
                self.next_sequence = self.next_sequence.saturating_add(1);
                self.last_elapsed_ms = Some(sample.campaign_elapsed_ms);
            } else {
                self.integrity_violations = self.integrity_violations.saturating_add(1);
            }
        }
    }

    pub(super) fn summarize(&mut self, campaign_elapsed_ms: u64) -> Option<ResourceTimeline> {
        if self
            .samples
            .last()
            .is_some_and(|sample| sample.campaign_elapsed_ms > campaign_elapsed_ms)
        {
            self.integrity_violations = self.integrity_violations.saturating_add(1);
            return None;
        }
        ResourceTimeline::new(&self.samples, campaign_elapsed_ms)
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
pub(super) struct ResourceTimeline {
    pub samples: usize,
    pub campaign_elapsed_ms: u64,
    pub first_sample_elapsed_ms: u64,
    pub last_sample_elapsed_ms: u64,
    pub coverage_ms: u64,
    pub coverage_ratio: f64,
    pub maximum_gap_ms: u64,
    pub first_rss_kib: u64,
    pub final_rss_kib: u64,
    pub max_rss_kib: u64,
    pub rss_growth_kib: i64,
    pub tail_rss_slope_kib_per_minute: f64,
    pub first_fds: usize,
    pub final_fds: usize,
    pub max_fds: usize,
    pub fd_growth: isize,
}

impl ResourceTimeline {
    fn new(samples: &[ProcessTreeResourceSample], campaign_elapsed_ms: u64) -> Option<Self> {
        if samples.len() < 2 || campaign_elapsed_ms == 0 {
            return None;
        }
        let first = samples.first()?;
        let last = samples.last()?;
        let coverage_ms = last
            .campaign_elapsed_ms
            .saturating_sub(first.campaign_elapsed_ms);
        let internal_gap = samples
            .windows(2)
            .map(|pair| {
                pair[1]
                    .campaign_elapsed_ms
                    .saturating_sub(pair[0].campaign_elapsed_ms)
            })
            .max()
            .unwrap_or_default();
        let maximum_gap_ms = first
            .campaign_elapsed_ms
            .max(internal_gap)
            .max(campaign_elapsed_ms.saturating_sub(last.campaign_elapsed_ms));
        Some(Self {
            samples: samples.len(),
            campaign_elapsed_ms,
            first_sample_elapsed_ms: first.campaign_elapsed_ms,
            last_sample_elapsed_ms: last.campaign_elapsed_ms,
            coverage_ms,
            coverage_ratio: coverage_ms as f64 / campaign_elapsed_ms as f64,
            maximum_gap_ms,
            first_rss_kib: first.rss_kib,
            final_rss_kib: last.rss_kib,
            max_rss_kib: samples
                .iter()
                .map(|sample| sample.rss_kib)
                .max()
                .unwrap_or(first.rss_kib),
            rss_growth_kib: last.rss_kib as i64 - first.rss_kib as i64,
            tail_rss_slope_kib_per_minute: tail_rss_slope(samples),
            first_fds: first.file_descriptors,
            final_fds: last.file_descriptors,
            max_fds: samples
                .iter()
                .map(|sample| sample.file_descriptors)
                .max()
                .unwrap_or(first.file_descriptors),
            fd_growth: last.file_descriptors as isize - first.file_descriptors as isize,
        })
    }
}

fn tail_rss_slope(samples: &[ProcessTreeResourceSample]) -> f64 {
    let tail = &samples[samples.len() / 2..];
    if tail.len() < 2 {
        return 0.0;
    }
    let mean_x = tail
        .iter()
        .map(|sample| sample.campaign_elapsed_ms as f64 / 60_000.0)
        .sum::<f64>()
        / tail.len() as f64;
    let mean_y = tail.iter().map(|sample| sample.rss_kib as f64).sum::<f64>() / tail.len() as f64;
    let (covariance, variance) = tail.iter().fold((0.0, 0.0), |state, sample| {
        let x = sample.campaign_elapsed_ms as f64 / 60_000.0 - mean_x;
        let y = sample.rss_kib as f64 - mean_y;
        (state.0 + x * y, state.1 + x * x)
    });
    if variance == 0.0 {
        0.0
    } else {
        covariance / variance
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(sequence: u64, elapsed: u64, rss: u64) -> ProcessTreeResourceSample {
        ProcessTreeResourceSample {
            sequence,
            campaign_elapsed_ms: elapsed,
            rss_kib: rss,
            file_descriptors: 4 + sequence as usize,
            process_count: 1,
        }
    }

    #[test]
    fn summary_uses_real_elapsed_time_and_campaign_edges() {
        let samples = [
            sample(0, 10_000, 1_000),
            sample(1, 70_000, 1_100),
            sample(2, 130_000, 1_200),
        ];
        let summary = ResourceTimeline::new(&samples, 150_000).unwrap();
        assert_eq!(summary.coverage_ms, 120_000);
        assert_eq!(summary.maximum_gap_ms, 60_000);
        let slope_error = (summary.tail_rss_slope_kib_per_minute - 100.0).abs();
        assert!(slope_error <= 100.0 * f64::EPSILON);
    }

    #[test]
    fn tracker_rejects_noncontiguous_or_empty_sample_batches() {
        let mut tracker = DriverResourceTracker::default();
        tracker.record(&[]);
        tracker.record(&[sample(1, 10, 1_000)]);
        assert_eq!(tracker.integrity_violations, 2);
        assert!(tracker.summarize(20).is_none());
    }
}
