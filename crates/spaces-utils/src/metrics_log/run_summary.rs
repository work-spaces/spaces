//! The run summary shown at the end of a successful `spaces run`, checkout, or `sync`
//! when the `enable-run-metrics` feature is enabled: a bar chart of the longest running
//! operations recorded during the command (rules, starlark module evaluation, and
//! internal steps).

use super::command::{default_plot_options, default_plot_width, format_seconds};
use super::{HistogramRecord, PERF_COMMAND_DURATION, PERF_PREFIX, get_metric_info, get_short_name};
use console::bootstrap;
use console::plot::BarChart;

const RUN_SUMMARY_LIMIT: usize = 20;
const CACHE_LABEL: &str = "cache";
const OVERLAP_NOTE: &str = "Times are inclusive: enclosing tasks (for example `evaluate` and `step graph.execute`) include the rules and modules they ran.";

#[derive(Debug, Clone, PartialEq)]
struct Entry {
    label: String,
    total_seconds: f64,
}

/// `<metric> (<cache>, x<count>) <label values>`, for example
/// `rule (executed) //spaces:check` or `step (x3) graph.sort`. The label values come last
/// because the chart truncates long labels from the start.
fn get_entry_label(histogram: &HistogramRecord) -> String {
    let short_name = get_short_name(&histogram.name);
    let mut label = short_name
        .strip_prefix("perf.")
        .unwrap_or(short_name)
        .to_string();

    let mut notes = Vec::new();
    if let Some(cache) = histogram.labels.get(CACHE_LABEL) {
        notes.push(cache.clone());
    }
    if histogram.samples.len() > 1 {
        notes.push(format!("x{}", histogram.samples.len()));
    }
    if !notes.is_empty() {
        label.push_str(&format!(" ({})", notes.join(", ")));
    }

    let keys: Vec<&str> = match get_metric_info(&histogram.name) {
        Some(info) => info.labels.to_vec(),
        None => histogram.labels.keys().map(String::as_str).collect(),
    };
    for key in keys.into_iter().filter(|key| *key != CACHE_LABEL) {
        if let Some(value) = histogram.labels.get(key) {
            label.push(' ');
            label.push_str(value);
        }
    }
    label
}

/// All `perf.*` timings except the command itself, longest first.
fn get_entries(histograms: &[HistogramRecord]) -> Vec<Entry> {
    let mut entries: Vec<Entry> = histograms
        .iter()
        .filter(|histogram| {
            histogram.name.starts_with(PERF_PREFIX)
                && histogram.name != PERF_COMMAND_DURATION
                && !histogram.samples.is_empty()
        })
        .map(|histogram| Entry {
            label: get_entry_label(histogram),
            total_seconds: histogram.samples.iter().sum(),
        })
        .collect();
    entries.sort_by(|a, b| {
        b.total_seconds
            .total_cmp(&a.total_seconds)
            .then_with(|| a.label.cmp(&b.label))
    });
    entries
}

/// Emits a bar chart of the longest running tasks recorded during the command.
pub fn show_run_summary(
    console: &console::Console,
    histograms: &[HistogramRecord],
    elapsed: std::time::Duration,
    is_tty: bool,
) {
    let entries = get_entries(histograms);
    if entries.is_empty() {
        return;
    }

    let options = default_plot_options(is_tty);
    let ascii = options.ascii;
    let shown = entries.len().min(RUN_SUMMARY_LIMIT);
    let title = format!(
        "Longest running tasks (top {shown} of {}, run took {})",
        entries.len(),
        format_seconds(elapsed.as_secs_f64(), ascii)
    );
    let bars = entries
        .into_iter()
        .take(RUN_SUMMARY_LIMIT)
        .map(|entry| (entry.label, entry.total_seconds))
        .collect();
    let chart = BarChart::new(bars)
        .title(title)
        .footer(OVERLAP_NOTE)
        .value_formatter(move |value| format_seconds(value, ascii))
        .width(default_plot_width(is_tty))
        .options(options);

    let mut container = bootstrap::Container::new();
    container.add(bootstrap::VerticalSpacer::new(1));
    container.add(chart);
    console.emit_container(&container);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics_log::tests::histogram;
    use crate::metrics_log::{
        PERF_MODULE_DURATION, PERF_RULE_DURATION, PERF_STEP_DURATION, USAGE_COMMAND,
    };

    #[test]
    fn test_entry_labels() {
        assert_eq!(
            get_entry_label(&histogram(
                PERF_RULE_DURATION,
                &[("rule", "//spaces:check"), ("cache", "executed")],
                &[1.0],
            )),
            "rule (executed) //spaces:check"
        );
        assert_eq!(
            get_entry_label(&histogram(
                PERF_STEP_DURATION,
                &[("step", "graph.sort")],
                &[1.0, 2.0, 3.0],
            )),
            "step (x3) graph.sort"
        );
        assert_eq!(
            get_entry_label(&histogram(
                PERF_MODULE_DURATION,
                &[("module", "spaces/spaces.star"), ("cache", "miss")],
                &[1.0],
            )),
            "module (miss) spaces/spaces.star"
        );
        assert_eq!(
            get_entry_label(&histogram(
                "spaces.perf.new_thing.duration_seconds",
                &[("b", "2"), ("a", "1")],
                &[1.0],
            )),
            "new_thing 1 2"
        );
    }

    #[test]
    fn test_entries_are_sorted_and_filtered() {
        let histograms = vec![
            histogram(PERF_COMMAND_DURATION, &[("command", "run")], &[100.0]),
            histogram(PERF_STEP_DURATION, &[("step", "graph.sort")], &[1.0, 2.0]),
            histogram(
                PERF_RULE_DURATION,
                &[("rule", "//:a"), ("cache", "executed")],
                &[5.0],
            ),
            histogram(
                PERF_RULE_DURATION,
                &[("rule", "//:b"), ("cache", "restored")],
                &[],
            ),
            histogram(USAGE_COMMAND, &[("command", "run")], &[50.0]),
        ];
        let entries = get_entries(&histograms);
        assert_eq!(
            entries,
            vec![
                Entry {
                    label: "rule (executed) //:a".to_string(),
                    total_seconds: 5.0,
                },
                Entry {
                    label: "step (x2) graph.sort".to_string(),
                    total_seconds: 3.0,
                },
            ]
        );
    }
}
