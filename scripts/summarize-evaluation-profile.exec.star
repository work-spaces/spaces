#!/usr/bin/env spaces
"""
Summarize a Spaces evaluation profile JSON report.

By default this reads:
  .spaces/evaluation-profile.spaces.json

Example:
  spaces spaces/scripts/summarize-evaluation-profile.exec.star
  spaces spaces/scripts/summarize-evaluation-profile.exec.star --top 15
  spaces spaces/scripts/summarize-evaluation-profile.exec.star --profile path/to/profile.json
"""

load(
    "//@star/prelude/exec/args.star",
    "args_opt",
    "args_parse",
    "args_parser",
)
load("//@star/prelude/exec/fs.star", "fs_exists", "fs_read_json")
load("//@star/prelude/exec/log.star", "log_fatal")
load("//@star/prelude/exec/string.star", "string_format_table")

def _safe_num(value):
    if value == None:
        return 0
    return value

def _percent(part, whole):
    if whole <= 0:
        return "0%"
    return "{}%".format((part * 100.0) / whole)

def _insert_top_desc(top_items, item, key, limit):
    value = _safe_num(item.get(key, 0))
    insert_at = len(top_items)

    for index in range(len(top_items)):
        if value > _safe_num(top_items[index].get(key, 0)):
            insert_at = index
            break

    top_items.insert(insert_at, item)
    if len(top_items) > limit:
        top_items.pop()

def _as_ms(value):
    return "{}".format(_safe_num(value))

def _sum_module_durations(modules):
    totals = {
        "queue_wait": 0,
        "load": 0,
        "parse": 0,
        "eval": 0,
        "total": 0,
    }

    for module_entry in modules:
        d = module_entry.get("durations_ms", {})
        totals["queue_wait"] += _safe_num(d.get("queue_wait", 0))
        totals["load"] += _safe_num(d.get("load", 0))
        totals["parse"] += _safe_num(d.get("parse", 0))
        totals["eval"] += _safe_num(d.get("eval", 0))
        totals["total"] += _safe_num(d.get("total", 0))

    return totals

def _summarize_modules(modules, top_n):
    by_module = {}
    top_invocations = []

    for module_entry in modules:
        module_name = module_entry.get("module", "<unknown>")
        durations = module_entry.get("durations_ms", {})

        total_ms = _safe_num(durations.get("total", 0))
        load_ms = _safe_num(durations.get("load", 0))
        parse_ms = _safe_num(durations.get("parse", 0))
        eval_ms = _safe_num(durations.get("eval", 0))
        queue_wait_ms = _safe_num(durations.get("queue_wait", 0))

        builtins = module_entry.get("builtins", [])
        builtin_calls = 0
        builtin_errors = 0
        for b in builtins:
            builtin_calls += b.get("count", 0)
            builtin_errors += b.get("error_count", 0)

        _insert_top_desc(top_invocations, {
            "module": module_name,
            "phase": module_entry.get("phase", ""),
            "cache": module_entry.get("cache_status", ""),
            "total_ms": total_ms,
            "load_ms": load_ms,
            "parse_ms": parse_ms,
            "eval_ms": eval_ms,
            "queue_wait_ms": queue_wait_ms,
        }, "total_ms", top_n)

        if module_name not in by_module:
            by_module[module_name] = {
                "module": module_name,
                "occurrences": 0,
                "total_ms": 0,
                "load_ms": 0,
                "parse_ms": 0,
                "eval_ms": 0,
                "queue_wait_ms": 0,
                "builtin_calls": 0,
                "builtin_errors": 0,
            }

        agg = by_module[module_name]
        agg["occurrences"] += 1
        agg["total_ms"] += total_ms
        agg["load_ms"] += load_ms
        agg["parse_ms"] += parse_ms
        agg["eval_ms"] += eval_ms
        agg["queue_wait_ms"] += queue_wait_ms
        agg["builtin_calls"] += builtin_calls
        agg["builtin_errors"] += builtin_errors

    top_modules = []
    for module_name in by_module:
        _insert_top_desc(top_modules, by_module[module_name], "total_ms", top_n)

    return {
        "top_invocations": top_invocations,
        "top_modules": top_modules,
        "unique_modules": len(by_module),
    }

def _summarize_builtins(profile, modules, top_n):
    builtins_summary = profile.get("builtins_summary", [])

    # If top-level summary is unavailable, aggregate from per-module builtins.
    if len(builtins_summary) == 0:
        pairs = {}
        for module_entry in modules:
            for b in module_entry.get("builtins", []):
                key = "{}.{}".format(b.get("namespace", ""), b.get("function", ""))
                if key not in pairs:
                    pairs[key] = {
                        "namespace": b.get("namespace", ""),
                        "function": b.get("function", ""),
                        "count": 0,
                        "total_ms": 0,
                        "max_ms": 0,
                        "error_count": 0,
                    }

                item = pairs[key]
                item["count"] += b.get("count", 0)
                item["total_ms"] += _safe_num(b.get("total_ms", 0))
                item["max_ms"] = max(item.get("max_ms", 0), _safe_num(b.get("max_ms", 0)))
                item["error_count"] += b.get("error_count", 0)

        builtins_summary = []
        for key in pairs:
            builtins_summary.append(pairs[key])

    totals = {
        "distinct": len(builtins_summary),
        "count": 0,
        "total_ms": 0,
        "error_count": 0,
    }
    top_total = []
    top_avg = []

    for b in builtins_summary:
        count = b.get("count", 0)
        total_ms = _safe_num(b.get("total_ms", 0))
        avg_ms = 0
        if count > 0:
            avg_ms = total_ms / count

        row = {
            "builtin": "{}.{}".format(b.get("namespace", ""), b.get("function", "")),
            "count": count,
            "total_ms": total_ms,
            "avg_ms": avg_ms,
            "max_ms": _safe_num(b.get("max_ms", 0)),
            "error_count": b.get("error_count", 0),
        }

        totals["count"] += count
        totals["total_ms"] += total_ms
        totals["error_count"] += b.get("error_count", 0)

        _insert_top_desc(top_total, row, "total_ms", top_n)
        _insert_top_desc(top_avg, row, "avg_ms", top_n)

    return {
        "totals": totals,
        "top_total": top_total,
        "top_avg": top_avg,
    }

def _cache_rows(cache):
    statuses = ["hit", "miss", "bypass"]
    total = 0
    for status in statuses:
        total += cache.get(status, 0)

    rows = []
    for status in statuses:
        count = cache.get(status, 0)
        rows.append({
            "status": status,
            "count": "{}".format(count),
            "percent": _percent(count, total),
        })

    return rows

def _module_duration_rows(total_duration_ms, module_totals):
    total = _safe_num(module_totals.get("total", 0))
    rows = [
        {
            "metric": "profile total duration",
            "ms": _as_ms(total_duration_ms),
            "percent_of_module_total": _percent(total_duration_ms, total),
        },
        {
            "metric": "module queue wait",
            "ms": _as_ms(module_totals.get("queue_wait", 0)),
            "percent_of_module_total": _percent(module_totals.get("queue_wait", 0), total),
        },
        {
            "metric": "module load",
            "ms": _as_ms(module_totals.get("load", 0)),
            "percent_of_module_total": _percent(module_totals.get("load", 0), total),
        },
        {
            "metric": "module parse",
            "ms": _as_ms(module_totals.get("parse", 0)),
            "percent_of_module_total": _percent(module_totals.get("parse", 0), total),
        },
        {
            "metric": "module eval",
            "ms": _as_ms(module_totals.get("eval", 0)),
            "percent_of_module_total": _percent(module_totals.get("eval", 0), total),
        },
        {
            "metric": "module total (sum)",
            "ms": _as_ms(module_totals.get("total", 0)),
            "percent_of_module_total": _percent(module_totals.get("total", 0), total),
        },
    ]
    return rows

def _render_rows(rows, columns):
    out = []
    for row in rows:
        rendered = {}
        for column in columns:
            rendered[column] = "{}".format(row.get(column, ""))
        out.append(rendered)
    return out

parser = args_parser(
    name = "summarize-evaluation-profile",
    description = "Analyze and summarize a Spaces evaluation profile JSON file",
    options = [
        args_opt(
            "--profile",
            short = "-p",
            help = "Path to evaluation-profile.spaces.json",
            default = ".spaces/evaluation-profile.spaces.json",
        ),
        args_opt(
            "--top",
            short = "-n",
            help = "Number of rows to show in top lists",
            type = "int",
            default = 10,
        ),
    ],
)
args = args_parse(parser)

profile_path = args.get("profile", ".spaces/evaluation-profile.spaces.json")
top_n = args.get("top", 10)
if type(top_n) != "int" or top_n <= 0:
    top_n = 10

if not fs_exists(profile_path):
    log_fatal("Profile file not found: {}".format(profile_path))

profile = fs_read_json(profile_path)
modules = profile.get("modules", [])
if type(modules) != "list":
    log_fatal("Invalid profile format: `modules` must be a list")

module_totals = _sum_module_durations(modules)
module_summary = _summarize_modules(modules, top_n)
builtin_summary = _summarize_builtins(profile, modules, top_n)

print("Evaluation Profile Summary")
print("==========================")
print("profile_path: {}".format(profile_path))
print("schema_version: {}".format(profile.get("schema_version", "")))
print("phase: {}".format(profile.get("phase", "")))
print("workspace_path: {}".format(profile.get("workspace_path", "")))
print("started_at: {}".format(profile.get("started_at", "")))
print("ended_at: {}".format(profile.get("ended_at", "")))
print("total_duration_ms: {}".format(_as_ms(profile.get("total_duration_ms", 0))))
print("modules: {} total / {} unique".format(len(modules), module_summary.get("unique_modules", 0)))

print("")
print("Cache")
print("-----")
print(string_format_table(_cache_rows(profile.get("cache", {}))))

print("")
print("Duration Breakdown")
print("------------------")
print(string_format_table(_module_duration_rows(profile.get("total_duration_ms", 0), module_totals)))

print("")
print("Top Module Invocations by total_ms")
print("----------------------------------")
print(string_format_table(_render_rows(
    module_summary.get("top_invocations", []),
    ["module", "phase", "cache", "total_ms", "load_ms", "parse_ms", "eval_ms", "queue_wait_ms"],
)))

print("")
print("Top Modules (Aggregated by module path)")
print("---------------------------------------")
print(string_format_table(_render_rows(
    module_summary.get("top_modules", []),
    ["module", "occurrences", "total_ms", "load_ms", "parse_ms", "eval_ms", "queue_wait_ms", "builtin_calls", "builtin_errors"],
)))

print("")
print("Builtins Summary")
print("----------------")
print("distinct_builtins: {}".format(builtin_summary.get("totals", {}).get("distinct", 0)))
print("total_builtin_calls: {}".format(builtin_summary.get("totals", {}).get("count", 0)))
print("total_builtin_time_ms: {}".format(_as_ms(builtin_summary.get("totals", {}).get("total_ms", 0))))
print("total_builtin_errors: {}".format(builtin_summary.get("totals", {}).get("error_count", 0)))

print("")
print("Top Builtins by total_ms")
print("------------------------")
print(string_format_table(_render_rows(
    builtin_summary.get("top_total", []),
    ["builtin", "count", "total_ms", "avg_ms", "max_ms", "error_count"],
)))

print("")
print("Top Builtins by avg_ms")
print("----------------------")
print(string_format_table(_render_rows(
    builtin_summary.get("top_avg", []),
    ["builtin", "count", "total_ms", "avg_ms", "max_ms", "error_count"],
)))
