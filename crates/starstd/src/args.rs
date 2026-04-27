use anyhow::Context;
use anyhow_source_location::format_context;
use clap::error::ErrorKind as ClapErrorKind;
use clap::{Arg, ArgAction, Command, builder::PossibleValuesParser};
use serde::{Deserialize, Serialize};
use starlark::environment::GlobalsBuilder;
use starlark::eval::Evaluator;
use starlark::values::Value;
use starlark::values::none::NoneType;
use std::collections::{BTreeMap, HashSet};
use std::vec;

use crate::is_lsp_mode;
use crate::script;

// ---------------------------------------------------------------------------
// Public spec types (wire-compatible with @std/args.star)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParserSpec {
    pub name: Option<String>,
    pub description: Option<String>,
    pub options: Option<Vec<OptionSpec>>,
    pub positional: Option<Vec<PositionalSpec>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParserOptions {
    pub name: Option<String>,
    pub description: Option<String>,
    pub options: Option<Vec<OptionSpec>>,
    pub positional: Option<Vec<PositionalSpec>>,
}

impl From<ParserOptions> for ParserSpec {
    fn from(opts: ParserOptions) -> Self {
        ParserSpec {
            name: opts.name,
            description: opts.description,
            options: opts.options,
            positional: opts.positional,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OptionSpec {
    pub kind: String, // "flag" | "opt" | "list"
    pub long: String,
    pub short: Option<String>,
    pub help: Option<String>,
    pub default: Option<serde_json::Value>,
    pub choices: Option<Vec<String>>,
    #[serde(rename = "type")]
    pub value_type: Option<String>, // "str" (default) | "int" | "bool"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PositionalSpec {
    pub name: String,
    pub required: Option<bool>,
    pub variadic: Option<bool>,
}

/// Parse request - encapsulates both the parser spec and argv for parsing.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParseRequest {
    pub spec: ParserSpec,
    pub argv: Vec<String>,
}

// ---------------------------------------------------------------------------
// Internal types: mapping spec -> clap and back to JSON
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ValueType {
    Str,
    Int,
    Bool,
}

impl ValueType {
    fn parse(s: Option<&str>) -> anyhow::Result<Self> {
        Ok(match s.unwrap_or("str") {
            "str" => ValueType::Str,
            "int" => ValueType::Int,
            "bool" => ValueType::Bool,
            other => anyhow::bail!("`type` must be one of: str, int, bool (got `{other}`)"),
        })
    }
}

#[derive(Debug, Clone)]
enum ArgKind {
    Flag,
    Opt {
        value_type: ValueType,
        default: serde_json::Value,
    },
    List {
        value_type: ValueType,
    },
    Positional {
        variadic: bool,
    },
}

#[derive(Debug, Clone)]
struct ArgMeta {
    /// Normalized snake_case key — used as both clap `Arg::id` and JSON key.
    id: String,
    /// Original `--long` or positional name, for error messages.
    display: String,
    kind: ArgKind,
}

/// Outcome of `run_parse`. Lets us keep `std::process::exit` out of the core
/// logic so it remains testable.
#[derive(Debug)]
pub enum ParseOutcome {
    Parsed(serde_json::Value),
    Help(String),
    Error(String),
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn normalize_key(raw: &str) -> String {
    let trimmed = raw.trim_start_matches('-');
    let mut out = String::with_capacity(trimmed.len());
    for ch in trimmed.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push('_');
        }
    }
    while out.contains("__") {
        out = out.replace("__", "_");
    }
    out.trim_matches('_').to_string()
}

fn json_int(n: i64) -> serde_json::Value {
    serde_json::Value::Number(serde_json::Number::from(n))
}

fn default_for_opt(
    value_type: ValueType,
    user_default: Option<&serde_json::Value>,
    display: &str,
) -> Result<serde_json::Value, String> {
    if let Some(v) = user_default {
        let type_ok = match (value_type, v) {
            (ValueType::Int, serde_json::Value::Number(_))
            | (ValueType::Bool, serde_json::Value::Bool(_))
            | (ValueType::Str, serde_json::Value::String(_)) => true,
            _ => false,
        };
        if !type_ok {
            return Err(format!(
                "Default value for `{display}` does not match its declared type `{}`",
                match value_type {
                    ValueType::Int => "int",
                    ValueType::Bool => "bool",
                    ValueType::Str => "str",
                }
            ));
        }
        return Ok(v.clone());
    }
    Ok(match value_type {
        ValueType::Int => json_int(0),
        ValueType::Bool => serde_json::Value::Bool(false),
        ValueType::Str => serde_json::Value::String(String::new()),
    })
}

fn convert_typed(
    raw: &str,
    value_type: ValueType,
    display: &str,
) -> Result<serde_json::Value, String> {
    match value_type {
        ValueType::Str => Ok(serde_json::Value::String(raw.to_string())),
        ValueType::Int => raw
            .parse::<i64>()
            .map(json_int)
            .map_err(|_| format!("Expected integer value for `{display}`, got `{raw}`")),
        ValueType::Bool => match raw {
            "true" | "1" | "yes" | "on" => Ok(serde_json::Value::Bool(true)),
            "false" | "0" | "no" | "off" => Ok(serde_json::Value::Bool(false)),
            _ => Err(format!(
                "Expected boolean value for `{display}`, got `{raw}`"
            )),
        },
    }
}

// ---------------------------------------------------------------------------
// Build clap Command from ParserSpec
// ---------------------------------------------------------------------------

fn build_command(spec: &ParserSpec) -> anyhow::Result<(Command, Vec<ArgMeta>)> {
    let program_name = spec.name.clone().unwrap_or_else(|| "program".to_string());

    let mut cmd = Command::new(program_name)
        .no_binary_name(true)
        .disable_help_flag(false);
    if let Some(desc) = &spec.description
        && !desc.trim().is_empty()
    {
        cmd = cmd.about(desc.clone());
    }

    let mut metas = Vec::<ArgMeta>::new();
    let mut seen_keys = HashSet::<String>::new();
    let mut seen_longs = HashSet::<String>::new();
    let mut seen_shorts = HashSet::<char>::new();

    // --- Options -----------------------------------------------------------
    for opt in spec.options.clone().unwrap_or_default() {
        let kind_str = opt.kind.trim().to_ascii_lowercase();
        if kind_str != "flag" && kind_str != "opt" && kind_str != "list" {
            anyhow::bail!("Unknown option kind `{}`", opt.kind);
        }

        let key = normalize_key(&opt.long);
        if key.is_empty() {
            anyhow::bail!("Could not derive key from option `{}`", opt.long);
        }
        if !seen_keys.insert(key.clone()) {
            anyhow::bail!("Duplicate normalized option key `{key}`");
        }

        let long_stripped = opt.long.trim_start_matches('-').to_string();
        if !seen_longs.insert(long_stripped.clone()) {
            anyhow::bail!("Duplicate option `{}`", opt.long);
        }

        let short_char = if let Some(ref short) = opt.short {
            let c = short
                .chars()
                .nth(1)
                .ok_or_else(|| anyhow::anyhow!("Short option must be like `-x`, got `{short}`"))?;
            if !seen_shorts.insert(c) {
                anyhow::bail!("Duplicate short option `-{c}`");
            }
            Some(c)
        } else {
            None
        };

        let mut arg = Arg::new(key.clone()).long(long_stripped);
        if let Some(c) = short_char {
            arg = arg.short(c);
        }
        if let Some(help) = opt.help.clone() {
            arg = arg.help(help);
        }

        let display = opt.long.clone();
        let meta_kind = match kind_str.as_str() {
            "flag" => {
                arg = arg.action(ArgAction::SetTrue);
                ArgKind::Flag
            }
            "opt" => {
                let value_type = ValueType::parse(opt.value_type.as_deref())?;
                if opt.choices.is_some() && value_type != ValueType::Str {
                    anyhow::bail!("Choices require string values for `{}`", opt.long);
                }
                arg = arg.action(ArgAction::Set).num_args(1);
                if value_type == ValueType::Str
                    && let Some(choices) = &opt.choices
                    && !choices.is_empty()
                {
                    arg = arg.value_parser(PossibleValuesParser::new(choices));
                }
                let default = default_for_opt(value_type, opt.default.as_ref(), &opt.long)
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                ArgKind::Opt {
                    value_type,
                    default,
                }
            }
            "list" => {
                let value_type = ValueType::parse(opt.value_type.as_deref())?;
                if opt.choices.is_some() && value_type != ValueType::Str {
                    anyhow::bail!("Choices require string values for `{}`", opt.long);
                }
                arg = arg.action(ArgAction::Append).num_args(1);
                if value_type == ValueType::Str
                    && let Some(choices) = &opt.choices
                    && !choices.is_empty()
                {
                    arg = arg.value_parser(PossibleValuesParser::new(choices));
                }
                ArgKind::List { value_type }
            }
            _ => unreachable!("kind validated above"),
        };

        cmd = cmd.arg(arg);
        metas.push(ArgMeta {
            id: key,
            display,
            kind: meta_kind,
        });
    }

    // --- Positionals -------------------------------------------------------
    let positional_specs = spec.positional.clone().unwrap_or_default();
    let positional_count = positional_specs.len();
    let mut saw_variadic = false;

    for (idx, pos) in positional_specs.into_iter().enumerate() {
        if pos.name.trim().is_empty() {
            anyhow::bail!("Positional at index {idx} has empty name");
        }
        let key = normalize_key(&pos.name);
        if key.is_empty() {
            anyhow::bail!("Invalid positional name `{}`", pos.name);
        }
        if !seen_keys.insert(key.clone()) {
            anyhow::bail!("Duplicate positional `{}`", pos.name);
        }

        let variadic = pos.variadic.unwrap_or(false);
        let required = pos.required.unwrap_or(false);

        if variadic && idx != positional_count - 1 {
            anyhow::bail!(
                "Variadic positional `{}` must be the last positional",
                pos.name
            );
        }
        if saw_variadic {
            anyhow::bail!("Only one variadic positional is allowed");
        }
        saw_variadic = variadic;

        let mut arg = Arg::new(key.clone()).index(idx + 1).required(required);
        if variadic {
            arg = arg
                .action(ArgAction::Append)
                .num_args(if required { 1.. } else { 0.. });
        } else {
            arg = arg.action(ArgAction::Set).num_args(1);
        }

        cmd = cmd.arg(arg);
        metas.push(ArgMeta {
            id: key,
            display: pos.name,
            kind: ArgKind::Positional { variadic },
        });
    }

    Ok((cmd, metas))
}

// ---------------------------------------------------------------------------
// Convert clap matches -> JSON object using ArgMeta
// ---------------------------------------------------------------------------

fn matches_to_json(
    matches: &clap::ArgMatches,
    metas: &[ArgMeta],
) -> Result<serde_json::Value, String> {
    let mut out = BTreeMap::<String, serde_json::Value>::new();

    for meta in metas {
        match &meta.kind {
            ArgKind::Flag => {
                let v = matches.get_flag(&meta.id);
                out.insert(meta.id.clone(), serde_json::Value::Bool(v));
            }
            ArgKind::Opt {
                value_type,
                default,
            } => {
                if let Some(raw) = matches.get_one::<String>(&meta.id) {
                    let typed = convert_typed(raw, *value_type, &meta.display)?;
                    out.insert(meta.id.clone(), typed);
                } else {
                    out.insert(meta.id.clone(), default.clone());
                }
            }
            ArgKind::List { value_type } => {
                let mut arr = Vec::<serde_json::Value>::new();
                if let Some(values) = matches.get_many::<String>(&meta.id) {
                    for raw in values {
                        arr.push(convert_typed(raw, *value_type, &meta.display)?);
                    }
                }
                out.insert(meta.id.clone(), serde_json::Value::Array(arr));
            }
            ArgKind::Positional { variadic } => {
                if *variadic {
                    let mut arr = Vec::<serde_json::Value>::new();
                    if let Some(values) = matches.get_many::<String>(&meta.id) {
                        for raw in values {
                            arr.push(serde_json::Value::String(raw.clone()));
                        }
                    }
                    out.insert(meta.id.clone(), serde_json::Value::Array(arr));
                } else if let Some(raw) = matches.get_one::<String>(&meta.id) {
                    out.insert(meta.id.clone(), serde_json::Value::String(raw.clone()));
                } else {
                    out.insert(meta.id.clone(), serde_json::Value::Null);
                }
            }
        }
    }

    Ok(serde_json::Value::Object(out.into_iter().collect()))
}

// ---------------------------------------------------------------------------
// Core entry point used by both Starlark builtin and tests
// ---------------------------------------------------------------------------

pub fn run_parse(req: ParseRequest) -> ParseOutcome {
    let (cmd, metas) = match build_command(&req.spec) {
        Ok(pair) => pair,
        Err(e) => return ParseOutcome::Error(format!("{e}")),
    };

    match cmd.try_get_matches_from(req.argv) {
        Ok(matches) => match matches_to_json(&matches, &metas) {
            Ok(value) => ParseOutcome::Parsed(value),
            Err(msg) => ParseOutcome::Error(msg),
        },
        Err(err) => match err.kind() {
            ClapErrorKind::DisplayHelp | ClapErrorKind::DisplayVersion => {
                ParseOutcome::Help(err.to_string())
            }
            _ => ParseOutcome::Error(err.to_string()),
        },
    }
}

// ---------------------------------------------------------------------------
// Starlark globals
// ---------------------------------------------------------------------------

#[starlark_module]
pub fn globals(builder: &mut GlobalsBuilder) {
    /// Returns the full argv list passed to the script.
    ///
    /// ```python
    /// values = args.argv()
    /// ```
    fn argv() -> anyhow::Result<Vec<String>> {
        if is_lsp_mode() {
            return Ok(vec![]);
        }
        Ok(script::get_args_vec())
    }

    /// Returns argv[0] (program/script name), or empty string if absent.
    ///
    /// ```python
    /// name = args.program()
    /// ```
    fn program() -> anyhow::Result<String> {
        if is_lsp_mode() {
            return Ok("".to_string());
        }
        let args = script::get_args_vec();
        Ok(args.first().cloned().unwrap_or_default())
    }

    /// Creates a parser specification.
    ///
    /// ```python
    /// spec = args.parser(name="deploy", options=[...], positional=[...])
    /// ```
    fn parser<'v>(
        options: starlark::values::Value,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Value<'v>> {
        let opts: ParserOptions = serde_json::from_value(options.to_json_value()?)
            .context(format_context!("Invalid parser options"))?;
        let spec: ParserSpec = opts.into();
        let json_value = serde_json::to_value(&spec)
            .context(format_context!("Failed to serialize parser spec"))?;
        Ok(eval.heap().alloc(json_value))
    }

    /// Parses argv according to a parser spec.
    ///
    /// On `--help` or `-h`, prints usage and exits 0.
    /// On bad input, prints usage + error and exits 2.
    ///
    /// ```python
    /// parsed = args.parse(spec)
    /// ```
    fn parse<'v>(spec: Value, eval: &mut Evaluator<'v, '_, '_>) -> anyhow::Result<Value<'v>> {
        let parser_spec: ParserSpec = serde_json::from_value(spec.to_json_value()?)
            .context(format_context!("Invalid parser spec"))?;

        let argv = script::get_args_vec();
        let slice: Vec<String> = if argv.is_empty() {
            Vec::new()
        } else {
            argv[1..].to_vec()
        };

        let req = ParseRequest {
            spec: parser_spec,
            argv: slice,
        };

        match run_parse(req) {
            ParseOutcome::Parsed(value) => Ok(eval.heap().alloc(value)),
            ParseOutcome::Help(text) => {
                if !is_lsp_mode() {
                    print!("{text}");
                    std::process::exit(0);
                }
                Ok(eval.heap().alloc(serde_json::json!({})))
            }
            ParseOutcome::Error(text) => {
                if !is_lsp_mode() {
                    eprint!("{text}");
                    std::process::exit(2);
                }
                Ok(eval.heap().alloc(serde_json::json!({})))
            }
        }
    }

    /// Internal no-op hook to mirror module shape.
    fn _args_module_loaded() -> anyhow::Result<NoneType> {
        Ok(NoneType)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_spec(json: serde_json::Value) -> ParserSpec {
        serde_json::from_value(json).expect("valid spec json")
    }

    fn parse(spec: ParserSpec, argv: &[&str]) -> ParseOutcome {
        run_parse(ParseRequest {
            spec,
            argv: argv.iter().map(|s| s.to_string()).collect(),
        })
    }

    fn parsed(out: ParseOutcome) -> serde_json::Value {
        match out {
            ParseOutcome::Parsed(v) => v,
            ParseOutcome::Help(t) => panic!("expected parsed, got help: {t}"),
            ParseOutcome::Error(t) => panic!("expected parsed, got error: {t}"),
        }
    }

    #[test]
    fn flag_default_false_and_set_true() {
        let spec = make_spec(serde_json::json!({
            "name": "p",
            "options": [{"kind": "flag", "long": "--dry-run"}],
        }));
        let v = parsed(parse(spec.clone(), &[]));
        assert_eq!(v["dry_run"], serde_json::Value::Bool(false));

        let v = parsed(parse(spec, &["--dry-run"]));
        assert_eq!(v["dry_run"], serde_json::Value::Bool(true));
    }

    #[test]
    fn opt_with_default_and_choices() {
        let spec = make_spec(serde_json::json!({
            "name": "p",
            "options": [{
                "kind": "opt",
                "long": "--env",
                "short": "-e",
                "default": "dev",
                "choices": ["dev", "stg", "prod"],
                "type": "str",
            }],
        }));
        let v = parsed(parse(spec.clone(), &[]));
        assert_eq!(v["env"], serde_json::Value::String("dev".into()));

        let v = parsed(parse(spec.clone(), &["-e", "prod"]));
        assert_eq!(v["env"], serde_json::Value::String("prod".into()));

        let v = parsed(parse(spec.clone(), &["--env=stg"]));
        assert_eq!(v["env"], serde_json::Value::String("stg".into()));

        let bad = parse(spec, &["--env", "bogus"]);
        assert!(matches!(bad, ParseOutcome::Error(_)));
    }

    #[test]
    fn opt_int_type() {
        let spec = make_spec(serde_json::json!({
            "name": "p",
            "options": [{"kind": "opt", "long": "--count", "type": "int", "default": 1}],
        }));
        let v = parsed(parse(spec.clone(), &[]));
        assert_eq!(v["count"], json_int(1));

        let v = parsed(parse(spec.clone(), &["--count", "42"]));
        assert_eq!(v["count"], json_int(42));

        let bad = parse(spec, &["--count", "nope"]);
        assert!(matches!(bad, ParseOutcome::Error(_)));
    }

    #[test]
    fn list_appends() {
        let spec = make_spec(serde_json::json!({
            "name": "p",
            "options": [{"kind": "list", "long": "--tag", "short": "-t"}],
        }));
        let v = parsed(parse(spec, &["-t", "a", "--tag", "b", "--tag=c"]));
        assert_eq!(v["tag"], serde_json::json!(["a", "b", "c"]));
    }

    #[test]
    fn positionals_required_and_variadic() {
        let spec = make_spec(serde_json::json!({
            "name": "p",
            "positional": [
                {"name": "service", "required": true},
                {"name": "targets", "variadic": true},
            ],
        }));
        let v = parsed(parse(spec.clone(), &["web", "a", "b"]));
        assert_eq!(v["service"], serde_json::Value::String("web".into()));
        assert_eq!(v["targets"], serde_json::json!(["a", "b"]));

        let v = parsed(parse(spec.clone(), &["web"]));
        assert_eq!(v["targets"], serde_json::json!([]));

        let bad = parse(spec, &[]);
        assert!(matches!(bad, ParseOutcome::Error(_)));
    }

    #[test]
    fn help_returns_help_outcome() {
        let spec = make_spec(serde_json::json!({
            "name": "p",
            "options": [{"kind": "flag", "long": "--dry-run"}],
        }));
        let out = parse(spec, &["--help"]);
        assert!(matches!(out, ParseOutcome::Help(_)));
    }

    #[test]
    fn unknown_option_errors() {
        let spec = make_spec(serde_json::json!({"name": "p"}));
        let out = parse(spec, &["--nope"]);
        assert!(matches!(out, ParseOutcome::Error(_)));
    }

    #[test]
    fn double_dash_terminates_options() {
        let spec = make_spec(serde_json::json!({
            "name": "p",
            "positional": [{"name": "rest", "variadic": true}],
        }));
        let v = parsed(parse(spec, &["--", "--not-a-flag", "x"]));
        assert_eq!(v["rest"], serde_json::json!(["--not-a-flag", "x"]));
    }

    #[test]
    fn short_opt_bare_dash_errors() {
        // A short option of just "-" (no letter) must not panic; it should error.
        let spec = make_spec(serde_json::json!({
            "name": "p",
            "options": [{"kind": "flag", "long": "--foo", "short": "-"}],
        }));
        let out = parse(spec, &[]);
        assert!(matches!(out, ParseOutcome::Error(_)));
    }

    #[test]
    fn duplicate_normalized_key_errors() {
        // --dry-run and --dry_run both normalize to `dry_run`
        let spec = make_spec(serde_json::json!({
            "name": "p",
            "options": [
                {"kind": "flag", "long": "--dry-run"},
                {"kind": "flag", "long": "--dry_run"},
            ],
        }));
        let out = parse(spec, &[]);
        assert!(matches!(out, ParseOutcome::Error(_)));
    }

    #[test]
    fn bool_type_values() {
        let spec = make_spec(serde_json::json!({
            "name": "p",
            "options": [{"kind": "opt", "long": "--flag", "type": "bool", "default": false}],
        }));
        for truthy in ["true", "1", "yes", "on"] {
            let v = parsed(parse(spec.clone(), &["--flag", truthy]));
            assert_eq!(
                v["flag"],
                serde_json::Value::Bool(true),
                "expected true for `{truthy}`"
            );
        }
        for falsy in ["false", "0", "no", "off"] {
            let v = parsed(parse(spec.clone(), &["--flag", falsy]));
            assert_eq!(
                v["flag"],
                serde_json::Value::Bool(false),
                "expected false for `{falsy}`"
            );
        }
        let bad = parse(spec, &["--flag", "maybe"]);
        assert!(matches!(bad, ParseOutcome::Error(_)));
    }

    #[test]
    fn optional_positional_returns_null() {
        let spec = make_spec(serde_json::json!({
            "name": "p",
            "positional": [{"name": "output"}],
        }));
        let v = parsed(parse(spec.clone(), &[]));
        assert_eq!(v["output"], serde_json::Value::Null);

        let v = parsed(parse(spec, &["file.txt"]));
        assert_eq!(v["output"], serde_json::Value::String("file.txt".into()));
    }

    #[test]
    fn invalid_option_kind_errors() {
        let spec = make_spec(serde_json::json!({
            "name": "p",
            "options": [{"kind": "foobar", "long": "--foo"}],
        }));
        let out = parse(spec, &[]);
        assert!(matches!(out, ParseOutcome::Error(_)));
    }

    #[test]
    fn choices_on_list_with_non_str_type_errors() {
        let spec = make_spec(serde_json::json!({
            "name": "p",
            "options": [{
                "kind": "list",
                "long": "--level",
                "type": "int",
                "choices": ["1", "2"],
            }],
        }));
        let out = parse(spec, &[]);
        assert!(matches!(out, ParseOutcome::Error(_)));
    }

    #[test]
    fn mismatched_default_type_errors() {
        // type="int" but default is a string — should produce an error, not silently use wrong type.
        let spec = make_spec(serde_json::json!({
            "name": "p",
            "options": [{"kind": "opt", "long": "--count", "type": "int", "default": "oops"}],
        }));
        let out = parse(spec, &[]);
        assert!(matches!(out, ParseOutcome::Error(_)));
    }
}
