//! Statistics, histogram bins, trend buckets, and ranking.

use super::query::Sample;
use chrono::{Datelike, Duration, Local, LocalResult, NaiveDateTime, TimeZone, Timelike};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, clap::ValueEnum, strum::Display, strum::EnumString,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum Stat {
    Total,
    Mean,
    P50,
    P90,
    P99,
    Max,
    Count,
}

/// Summary statistics of a set of values. Percentiles use the nearest-rank method.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Stats {
    pub count: usize,
    pub total: f64,
    pub mean: f64,
    pub min: f64,
    pub p50: f64,
    pub p90: f64,
    pub p99: f64,
    pub max: f64,
}

/// Nearest-rank percentile of sorted values (`p` in `0..=100`).
pub fn percentile(sorted: &[f64], p: f64) -> Option<f64> {
    if sorted.is_empty() {
        return None;
    }
    let rank = ((p / 100.0) * sorted.len() as f64).ceil() as usize;
    Some(sorted[rank.clamp(1, sorted.len()) - 1])
}

impl Stats {
    pub fn new(values: &[f64]) -> Option<Self> {
        let mut sorted: Vec<f64> = values.iter().copied().filter(|v| v.is_finite()).collect();
        if sorted.is_empty() {
            return None;
        }
        sorted.sort_by(f64::total_cmp);
        let total: f64 = sorted.iter().sum();
        let count = sorted.len();
        Some(Self {
            count,
            total,
            mean: total / count as f64,
            min: sorted[0],
            p50: percentile(&sorted, 50.0)?,
            p90: percentile(&sorted, 90.0)?,
            p99: percentile(&sorted, 99.0)?,
            max: sorted[count - 1],
        })
    }

    pub fn from_samples(samples: &[Sample]) -> Option<Self> {
        let values: Vec<f64> = samples.iter().map(|sample| sample.value).collect();
        Self::new(&values)
    }

    pub fn get(&self, stat: Stat) -> f64 {
        match stat {
            Stat::Total => self.total,
            Stat::Mean => self.mean,
            Stat::P50 => self.p50,
            Stat::P90 => self.p90,
            Stat::P99 => self.p99,
            Stat::Max => self.max,
            Stat::Count => self.count as f64,
        }
    }
}

/// Groups ranked by `stat` (descending, then by key).
pub fn rank(groups: &BTreeMap<String, Vec<Sample>>, stat: Stat) -> Vec<(String, Stats)> {
    let mut ranked: Vec<(String, Stats)> = groups
        .iter()
        .filter_map(|(key, samples)| Stats::from_samples(samples).map(|stats| (key.clone(), stats)))
        .collect();
    ranked.sort_by(|a, b| b.1.get(stat).total_cmp(&a.1.get(stat)).then(a.0.cmp(&b.0)));
    ranked
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, clap::ValueEnum, strum::Display)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum Scale {
    Log,
    Linear,
}

/// Histogram bin covering `[lo, hi)` (the last bin includes `hi`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BinCount {
    pub lo: f64,
    pub hi: f64,
    pub count: u64,
}

/// Bins the values. Log bins span `[log10(min positive), log10(max)]` and count zero
/// (or negative) values in the first bin. Values are clamped to the edges. Falls back to
/// linear bins when there are no positive values.
pub fn make_bins(values: &[f64], bins: usize, scale: Scale) -> Vec<BinCount> {
    let values: Vec<f64> = values.iter().copied().filter(|v| v.is_finite()).collect();
    if values.is_empty() || bins == 0 {
        return Vec::new();
    }
    let max = values.iter().copied().fold(f64::MIN, f64::max);
    let min_positive = values
        .iter()
        .copied()
        .filter(|v| *v > 0.0)
        .fold(f64::MAX, f64::min);
    let is_log = scale == Scale::Log && max > 0.0;

    let (mut lo, mut hi) = if is_log {
        (min_positive.log10(), max.log10())
    } else {
        (values.iter().copied().fold(f64::MAX, f64::min), max)
    };
    if hi <= lo {
        lo -= 0.5;
        hi += 0.5;
    }
    let width = (hi - lo) / bins as f64;
    let edge = |index: usize| {
        let value = lo + width * index as f64;
        if is_log { 10f64.powf(value) } else { value }
    };

    let mut result: Vec<BinCount> = (0..bins)
        .map(|index| BinCount {
            lo: edge(index),
            hi: edge(index + 1),
            count: 0,
        })
        .collect();
    for value in values {
        let position = if is_log {
            if value <= 0.0 {
                0.0
            } else {
                (value.log10() - lo) / width
            }
        } else {
            (value - lo) / width
        };
        let index = (position.floor().max(0.0) as usize).min(bins - 1);
        result[index].count += 1;
    }
    result
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, clap::ValueEnum, strum::Display)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum Bucket {
    /// One point per record
    None,
    Hour,
    Day,
    Week,
}

/// Resolves a local calendar boundary to UTC. Ambiguous boundaries use the earlier
/// instant; nonexistent boundaries advance to the next valid local minute.
fn resolve_local_boundary<Tz: TimeZone>(timezone: &Tz, mut boundary: NaiveDateTime) -> Option<u64> {
    loop {
        match timezone.from_local_datetime(&boundary) {
            LocalResult::Single(date_time) => {
                return Some(date_time.timestamp_millis().max(0) as u64);
            }
            LocalResult::Ambiguous(first, second) => {
                return Some(
                    first
                        .timestamp_millis()
                        .min(second.timestamp_millis())
                        .max(0) as u64,
                );
            }
            LocalResult::None => boundary = boundary.checked_add_signed(Duration::minutes(1))?,
        }
    }
}

/// Start of the local calendar bucket containing `timestamp_ms`. Weeks start on Monday.
fn bucket_start_in_timezone<Tz: TimeZone>(timestamp_ms: u64, bucket: Bucket, timezone: &Tz) -> u64 {
    if bucket == Bucket::None {
        return timestamp_ms;
    }

    let Some(local) = timezone.timestamp_millis_opt(timestamp_ms as i64).single() else {
        return timestamp_ms;
    };
    let local_date_time = local.naive_local();
    let local_date = local_date_time.date();
    let boundary = match bucket {
        Bucket::None => unreachable!(),
        Bucket::Hour => local_date.and_hms_opt(local_date_time.hour(), 0, 0),
        Bucket::Day => local_date.and_hms_opt(0, 0, 0),
        Bucket::Week => local_date
            .checked_sub_signed(Duration::days(i64::from(
                local_date_time.weekday().num_days_from_monday(),
            )))
            .and_then(|date| date.and_hms_opt(0, 0, 0)),
    };
    boundary
        .and_then(|boundary| resolve_local_boundary(timezone, boundary))
        .unwrap_or(timestamp_ms)
}

/// Applies `stat` per bucket (or per record for `Bucket::None`). Points are sorted by time.
pub fn trend(samples: &[Sample], bucket: Bucket, stat: Stat) -> Vec<(u64, f64)> {
    trend_in_timezone(samples, bucket, stat, &Local)
}

fn trend_in_timezone<Tz: TimeZone>(
    samples: &[Sample],
    bucket: Bucket,
    stat: Stat,
    timezone: &Tz,
) -> Vec<(u64, f64)> {
    let mut buckets: BTreeMap<(u64, usize), Vec<f64>> = BTreeMap::new();
    for sample in samples {
        let key = match bucket {
            Bucket::None => (sample.timestamp_ms, sample.record),
            _ => (
                bucket_start_in_timezone(sample.timestamp_ms, bucket, timezone),
                0,
            ),
        };
        buckets.entry(key).or_default().push(sample.value);
    }
    buckets
        .into_iter()
        .filter_map(|((timestamp_ms, _), values)| {
            Stats::new(&values).map(|stats| (timestamp_ms, stats.get(stat)))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::query::DAY_MS;
    use super::*;

    fn sample(timestamp_ms: u64, value: f64, record: usize) -> Sample {
        Sample {
            timestamp_ms,
            value,
            record,
        }
    }

    #[test]
    fn test_percentiles() {
        assert_eq!(percentile(&[], 50.0), None);
        assert_eq!(percentile(&[3.0], 0.0), Some(3.0));
        assert_eq!(percentile(&[3.0], 99.0), Some(3.0));
        // even count
        let even = [1.0, 2.0, 3.0, 4.0];
        assert_eq!(percentile(&even, 50.0), Some(2.0));
        assert_eq!(percentile(&even, 90.0), Some(4.0));
        assert_eq!(percentile(&even, 100.0), Some(4.0));
        // odd count
        let odd = [1.0, 2.0, 3.0, 4.0, 5.0];
        assert_eq!(percentile(&odd, 50.0), Some(3.0));
        assert_eq!(percentile(&odd, 90.0), Some(5.0));
        let hundred: Vec<f64> = (1..=100).map(f64::from).collect();
        assert_eq!(percentile(&hundred, 90.0), Some(90.0));
        assert_eq!(percentile(&hundred, 99.0), Some(99.0));
    }

    #[test]
    fn test_stats() {
        assert_eq!(Stats::new(&[]), None);
        assert_eq!(Stats::new(&[f64::NAN]), None);
        let stats = Stats::new(&[4.0, 1.0, 3.0, 2.0, f64::INFINITY]).unwrap();
        assert_eq!(stats.count, 4);
        assert_eq!(stats.total, 10.0);
        assert_eq!(stats.mean, 2.5);
        assert_eq!(stats.min, 1.0);
        assert_eq!(stats.p50, 2.0);
        assert_eq!(stats.max, 4.0);
        assert_eq!(stats.get(Stat::Count), 4.0);
        assert_eq!(stats.get(Stat::Total), 10.0);
        assert_eq!(stats.get(Stat::P99), 4.0);
    }

    #[test]
    fn test_rank() {
        let groups = BTreeMap::from([
            ("a".to_string(), vec![sample(0, 1.0, 0), sample(0, 1.0, 1)]),
            ("b".to_string(), vec![sample(0, 5.0, 0)]),
            ("c".to_string(), vec![sample(0, 1.0, 0)]),
        ]);
        let keys = |stat| {
            rank(&groups, stat)
                .into_iter()
                .map(|(key, _)| key)
                .collect::<Vec<_>>()
        };
        assert_eq!(keys(Stat::Mean), vec!["b", "a", "c"]);
        assert_eq!(keys(Stat::Count), vec!["a", "b", "c"]);
    }

    #[test]
    fn test_linear_bins() {
        let bins = make_bins(&[0.0, 1.0, 2.0, 3.0, 4.0], 4, Scale::Linear);
        assert_eq!(bins.len(), 4);
        assert_eq!(bins[0].lo, 0.0);
        assert_eq!(bins[3].hi, 4.0);
        let counts: Vec<u64> = bins.iter().map(|b| b.count).collect();
        // the max value is clamped into the last bin
        assert_eq!(counts, vec![1, 1, 1, 2]);
        assert!(make_bins(&[], 4, Scale::Linear).is_empty());
        assert!(make_bins(&[1.0], 0, Scale::Linear).is_empty());
    }

    #[test]
    fn test_log_bins() {
        let bins = make_bins(&[0.0, 0.001, 0.01, 0.1, 1.0], 4, Scale::Log);
        assert_eq!(bins.len(), 4);
        assert!((bins[0].lo - 0.001).abs() < 1e-12);
        assert!((bins[3].hi - 1.0).abs() < 1e-9);
        let counts: Vec<u64> = bins.iter().map(|b| b.count).collect();
        // zero is counted in the first bin
        assert_eq!(counts, vec![2, 1, 1, 1]);
        let total: u64 = counts.iter().sum();
        assert_eq!(total, 5);
    }

    #[test]
    fn test_single_value_and_degenerate_bins() {
        for scale in [Scale::Log, Scale::Linear] {
            let bins = make_bins(&[2.5, 2.5, 2.5], 5, scale);
            assert_eq!(bins.len(), 5);
            assert_eq!(bins.iter().map(|b| b.count).sum::<u64>(), 3);
            assert!(bins.iter().all(|b| b.lo < b.hi));
            assert!(bins.iter().any(|b| b.lo <= 2.5 && 2.5 < b.hi));
        }
        // only zeros: log falls back to linear
        let bins = make_bins(&[0.0, 0.0], 3, Scale::Log);
        assert_eq!(bins.iter().map(|b| b.count).sum::<u64>(), 2);
        assert!(bins[0].lo < 0.0 && bins[2].hi > 0.0);
    }

    #[test]
    fn test_trend() {
        let samples = vec![
            sample(1_000, 1.0, 0),
            sample(1_000, 3.0, 0),
            sample(2_000, 5.0, 1),
            sample(3 * DAY_MS, 7.0, 2),
        ];
        assert_eq!(
            trend(&samples, Bucket::None, Stat::Mean),
            vec![(1_000, 2.0), (2_000, 5.0), (3 * DAY_MS, 7.0)]
        );
        let daily = trend(&samples, Bucket::Day, Stat::Total);
        assert_eq!(daily.len(), 2);
        assert_eq!(daily[0].1, 9.0);
        assert_eq!(daily[1].1, 7.0);
        assert!(daily[0].0 <= 1_000 || daily[0].0 == 0);
        let weekly = trend(&samples, Bucket::Week, Stat::Count);
        assert_eq!(weekly.iter().map(|p| p.1).sum::<f64>(), 4.0);
        for (start, _) in trend(&samples, Bucket::Hour, Stat::Max) {
            assert_eq!(bucket_start_in_timezone(start, Bucket::Hour, &Local), start);
        }
    }

    #[test]
    fn trend_uses_local_calendar_boundaries_across_dst() {
        let timezone = chrono_tz::America::New_York;
        let before_jump = timezone
            .with_ymd_and_hms(2026, 3, 8, 1, 30, 0)
            .single()
            .unwrap()
            .timestamp_millis() as u64;
        let after_jump = timezone
            .with_ymd_and_hms(2026, 3, 8, 3, 30, 0)
            .single()
            .unwrap()
            .timestamp_millis() as u64;
        let samples = [sample(before_jump, 1.0, 0), sample(after_jump, 2.0, 1)];

        let daily_start = timezone
            .with_ymd_and_hms(2026, 3, 8, 0, 0, 0)
            .single()
            .unwrap()
            .timestamp_millis() as u64;
        let weekly_start = timezone
            .with_ymd_and_hms(2026, 3, 2, 0, 0, 0)
            .single()
            .unwrap()
            .timestamp_millis() as u64;

        assert_eq!(
            trend_in_timezone(&samples, Bucket::Day, Stat::Total, &timezone),
            vec![(daily_start, 3.0)]
        );
        assert_eq!(
            trend_in_timezone(&samples, Bucket::Week, Stat::Total, &timezone),
            vec![(weekly_start, 3.0)]
        );
    }

    #[test]
    fn ambiguous_hour_boundary_uses_earlier_instant() {
        let timezone = chrono_tz::America::New_York;
        let first_sample = timezone
            .with_ymd_and_hms(2026, 11, 1, 1, 30, 0)
            .earliest()
            .unwrap()
            .timestamp_millis() as u64;
        let second_sample = timezone
            .with_ymd_and_hms(2026, 11, 1, 1, 30, 0)
            .latest()
            .unwrap()
            .timestamp_millis() as u64;
        let first_boundary = timezone
            .with_ymd_and_hms(2026, 11, 1, 1, 0, 0)
            .earliest()
            .unwrap()
            .timestamp_millis() as u64;
        let samples = [sample(first_sample, 1.0, 0), sample(second_sample, 1.0, 1)];

        assert_eq!(
            trend_in_timezone(&samples, Bucket::Hour, Stat::Count, &timezone),
            vec![(first_boundary, 2.0)]
        );
    }

    #[test]
    fn nonexistent_boundary_advances_to_next_valid_local_minute() {
        let timezone = chrono_tz::America::Sao_Paulo;
        assert!(matches!(
            timezone.with_ymd_and_hms(2018, 11, 4, 0, 0, 0),
            LocalResult::None
        ));
        let sample_time = timezone
            .with_ymd_and_hms(2018, 11, 4, 1, 30, 0)
            .single()
            .unwrap()
            .timestamp_millis() as u64;
        let first_valid_minute = timezone
            .with_ymd_and_hms(2018, 11, 4, 1, 0, 0)
            .single()
            .unwrap()
            .timestamp_millis() as u64;

        assert_eq!(
            bucket_start_in_timezone(sample_time, Bucket::Day, &timezone),
            first_valid_minute
        );
    }
}
