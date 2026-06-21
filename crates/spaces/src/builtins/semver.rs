use crate::evaluation_profile;
use anyhow::Context;
use anyhow_source_location::format_context;
use starlark::environment::GlobalsBuilder;
use starlark::values::Value;
use starlark::values::list::UnpackList;
use starlark::values::none::NoneType;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Parses a single version string into a [`semver::Version`], producing a
/// contextual error on failure.
fn parse_version(version: &str) -> anyhow::Result<semver::Version> {
    version
        .parse::<semver::Version>()
        .context(format_context!("Failed to parse version `{version}`"))
}

/// Parses a single requirement string into a [`semver::VersionReq`], producing
/// a contextual error on failure.
fn parse_requirement(requirement: &str) -> anyhow::Result<semver::VersionReq> {
    requirement
        .parse::<semver::VersionReq>()
        .context(format_context!(
            "Failed to parse semver requirement `{requirement}`"
        ))
}

/// Parses every entry of `versions` into [`semver::Version`], short-circuiting
/// on the first parse failure.
fn parse_versions(versions: &[String]) -> anyhow::Result<Vec<semver::Version>> {
    versions.iter().map(|v| parse_version(v)).collect()
}

/// Parses every entry of `requirements` into [`semver::VersionReq`],
/// short-circuiting on the first parse failure.
fn parse_requirements(requirements: &[String]) -> anyhow::Result<Vec<semver::VersionReq>> {
    requirements.iter().map(|r| parse_requirement(r)).collect()
}

/// Returns true if `v` satisfies every requirement in `reqs`.
fn version_matches_all(v: &semver::Version, reqs: &[semver::VersionReq]) -> bool {
    reqs.iter().all(|r| r.matches(v))
}

/// Returns versions parsed and sorted in ascending order.
fn sorted_versions_asc(versions: &[String]) -> anyhow::Result<Vec<semver::Version>> {
    let mut parsed = parse_versions(versions)?;
    parsed.sort();
    Ok(parsed)
}

/// Returns the indices of `versions` whose parsed value satisfies every
/// requirement, preserving input order. The parsed [`semver::Version`] for
/// each kept entry is returned alongside the original index so callers can
/// re-use the parse without a second pass.
fn select_matching(
    versions: &[String],
    reqs: &[semver::VersionReq],
) -> anyhow::Result<Vec<(usize, semver::Version)>> {
    let mut out = Vec::new();
    for (i, v_str) in versions.iter().enumerate() {
        let v = parse_version(v_str)?;
        if version_matches_all(&v, reqs) {
            out.push((i, v));
        }
    }
    Ok(out)
}

/// Bumps the major component, resetting minor, patch, pre-release, and build.
fn bump_major_version(mut v: semver::Version) -> semver::Version {
    v.major += 1;
    v.minor = 0;
    v.patch = 0;
    v.pre = semver::Prerelease::EMPTY;
    v.build = semver::BuildMetadata::EMPTY;
    v
}

/// Bumps the minor component, resetting patch, pre-release, and build.
fn bump_minor_version(mut v: semver::Version) -> semver::Version {
    v.minor += 1;
    v.patch = 0;
    v.pre = semver::Prerelease::EMPTY;
    v.build = semver::BuildMetadata::EMPTY;
    v
}

/// Bumps the patch component, resetting pre-release and build.
fn bump_patch_version(mut v: semver::Version) -> semver::Version {
    v.patch += 1;
    v.pre = semver::Prerelease::EMPTY;
    v.build = semver::BuildMetadata::EMPTY;
    v
}

/// Returns the shared regex used to scan an arbitrary string for SemVer
/// version candidates. The regex deliberately allows leading zeros and other
/// minor inconsistencies which the parser will then reject.
fn version_finder_regex() -> &'static regex::Regex {
    use std::sync::OnceLock;
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| {
        regex::Regex::new(
            r"(?x)
            (?P<v>
              \d+\.\d+\.\d+
              (?:-[0-9A-Za-z.-]+)?
              (?:\+[0-9A-Za-z.-]+)?
            )
            ",
        )
        .expect("valid semver regex")
    })
}

/// Attempts to parse a regex-matched candidate into a [`semver::Version`],
/// trimming trailing punctuation (`.`, `-`) that the greedy match may have
/// included as part of pre-release or build identifiers.
fn parse_candidate(candidate: &str) -> Option<semver::Version> {
    let trimmed = candidate.trim_end_matches(['.', '-']);
    if let Ok(v) = trimmed.parse::<semver::Version>() {
        return Some(v);
    }
    candidate.parse::<semver::Version>().ok()
}

/// Scans `input` for the first substring that parses as a valid
/// [`semver::Version`]. Matches `MAJOR.MINOR.PATCH` optionally followed by
/// `-PRE` and/or `+BUILD`. Returns `None` if no valid version is found.
fn find_version_in(input: &str) -> Option<semver::Version> {
    for m in version_finder_regex().captures_iter(input) {
        let candidate = m.name("v")?.as_str();
        if let Some(v) = parse_candidate(candidate) {
            return Some(v);
        }
    }
    None
}

/// Repeatedly strips any of the given suffixes from the end of `input`,
/// continuing until none of the suffixes match. Useful for trimming archive
/// extensions like `.tar.gz` before scanning for a version.
fn strip_suffixes<'a>(mut input: &'a str, suffixes: &[String]) -> &'a str {
    'outer: loop {
        for suffix in suffixes {
            if !suffix.is_empty()
                && let Some(rest) = input.strip_suffix(suffix.as_str())
            {
                input = rest;
                continue 'outer;
            }
        }
        break input;
    }
}

/// Scans `input` for every substring that parses as a valid
/// [`semver::Version`], in the order they appear.
fn find_all_versions_in(input: &str) -> Vec<semver::Version> {
    let mut out = Vec::new();
    for m in version_finder_regex().captures_iter(input) {
        if let Some(cap) = m.name("v")
            && let Some(v) = parse_candidate(cap.as_str())
        {
            out.push(v);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Starlark bindings
// ---------------------------------------------------------------------------

#[starlark_module]
pub fn globals(builder: &mut GlobalsBuilder) {
    /// Validates that the given string is a well-formed semantic version.
    ///
    /// ```python
    /// if semver.is_valid_version("1.2.3"):
    ///     # ...
    /// ```
    ///
    /// # Arguments
    /// * `version`: The version string to validate (e.g., `"1.2.3"`, `"1.2.3-rc.1+build.5"`).
    ///
    /// # Returns
    /// * `bool`: True if the string is a valid semantic version, False otherwise.
    fn is_valid_version(version: &str) -> anyhow::Result<bool> {
        evaluation_profile::profile_builtin_call("semver", "is_valid_version", || {
            Ok(version.parse::<semver::Version>().is_ok())
        })
    }

    /// Validates that the given string is a well-formed semantic version requirement.
    ///
    /// ```python
    /// if semver.is_valid_requirement("^1.2.0"):
    ///     # ...
    /// ```
    ///
    /// # Arguments
    /// * `requirement`: The requirement string to validate (e.g., `"^1.2.0"`, `">=1.0, <2.0"`, `"*"`).
    ///
    /// # Returns
    /// * `bool`: True if the string is a valid semver requirement, False otherwise.
    fn is_valid_requirement(requirement: &str) -> anyhow::Result<bool> {
        evaluation_profile::profile_builtin_call("semver", "is_valid_requirement", || {
            Ok(requirement.parse::<semver::VersionReq>().is_ok())
        })
    }

    /// Parses a semantic version string into its component parts.
    ///
    /// ```python
    /// parts = semver.parse("1.2.3-rc.1+build.5")
    /// # parts == {"major": 1, "minor": 2, "patch": 3, "pre": "rc.1", "build": "build.5"}
    /// ```
    ///
    /// # Arguments
    /// * `version`: The version string to parse.
    ///
    /// # Returns
    /// * `dict`: A dictionary with `major` (`int`), `minor` (`int`), `patch` (`int`), `pre` (`str`), and `build` (`str`).
    fn parse<'v>(
        version: &str,
        eval: &mut starlark::eval::Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Value<'v>> {
        evaluation_profile::profile_builtin_call("semver", "parse", || {
            let v = parse_version(version)?;
            let json = serde_json::json!({
                "major": v.major,
                "minor": v.minor,
                "patch": v.patch,
                "pre": v.pre.as_str(),
                "build": v.build.as_str(),
            });
            Ok(eval.heap().alloc(json))
        })
    }

    /// Returns true if the given version satisfies the given requirement.
    ///
    /// ```python
    /// if semver.matches("1.2.5", "^1.2.0"):
    ///     # ...
    /// ```
    ///
    /// # Arguments
    /// * `version`: The semantic version string.
    /// * `requirement`: The semver requirement string.
    ///
    /// # Returns
    /// * `bool`: True if the version satisfies the requirement, False otherwise.
    fn matches(version: &str, requirement: &str) -> anyhow::Result<bool> {
        evaluation_profile::profile_builtin_call("semver", "matches", || {
            let v = parse_version(version)?;
            let req = parse_requirement(requirement)?;
            Ok(req.matches(&v))
        })
    }

    /// Returns true if the given version satisfies all of the given requirements.
    ///
    /// ```python
    /// if semver.matches_all("1.2.5", ["^1.2.0", ">=1.2.4"]):
    ///     # ...
    /// ```
    ///
    /// # Arguments
    /// * `version`: The semantic version string.
    /// * `requirements`: A list of semver requirement strings, all of which must be satisfied.
    ///
    /// # Returns
    /// * `bool`: True if the version satisfies every requirement, False otherwise.
    fn matches_all(version: &str, requirements: UnpackList<String>) -> anyhow::Result<bool> {
        evaluation_profile::profile_builtin_call("semver", "matches_all", || {
            let v = parse_version(version)?;
            let reqs = parse_requirements(&requirements.items)?;
            Ok(version_matches_all(&v, &reqs))
        })
    }

    /// Compares two semantic versions.
    ///
    /// ```python
    /// order = semver.compare("1.2.3", "1.2.4")
    /// # order == -1
    /// ```
    ///
    /// # Arguments
    /// * `lhs`: The first version string.
    /// * `rhs`: The second version string.
    ///
    /// # Returns
    /// * `int`: -1 if `lhs < rhs`, 0 if equal, 1 if `lhs > rhs`.
    fn compare(lhs: &str, rhs: &str) -> anyhow::Result<i64> {
        evaluation_profile::profile_builtin_call("semver", "compare", || {
            let l = parse_version(lhs)?;
            let r = parse_version(rhs)?;
            Ok(match l.cmp(&r) {
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
            })
        })
    }

    /// Sorts a list of semantic versions in ascending order.
    ///
    /// Invalid versions cause an error.
    ///
    /// ```python
    /// versions = semver.sort(["1.10.0", "1.2.0", "1.2.10"])
    /// # versions == ["1.2.0", "1.2.10", "1.10.0"]
    /// ```
    ///
    /// # Arguments
    /// * `versions`: The list of version strings to sort.
    ///
    /// # Returns
    /// * `list[str]`: The sorted list of versions.
    fn sort(versions: UnpackList<String>) -> anyhow::Result<Vec<String>> {
        evaluation_profile::profile_builtin_call("semver", "sort", || {
            let parsed = sorted_versions_asc(&versions.items)?;
            Ok(parsed.into_iter().map(|v| v.to_string()).collect())
        })
    }

    /// Returns the maximum version from a list of semantic versions.
    ///
    /// ```python
    /// latest = semver.max(["1.2.0", "1.10.0", "1.2.10"])
    /// # latest == "1.10.0"
    /// ```
    ///
    /// # Arguments
    /// * `versions`: A non-empty list of version strings.
    ///
    /// # Returns
    /// * `str`: The greatest version in the list.
    fn max(versions: UnpackList<String>) -> anyhow::Result<String> {
        evaluation_profile::profile_builtin_call("semver", "max", || {
            if versions.items.is_empty() {
                return Err(anyhow::anyhow!("Cannot get max of an empty list"));
            }
            let mut parsed = sorted_versions_asc(&versions.items)?;
            Ok(parsed.pop().unwrap().to_string())
        })
    }

    /// Returns the minimum version from a list of semantic versions.
    ///
    /// ```python
    /// oldest = semver.min(["1.2.0", "1.10.0", "1.2.10"])
    /// # oldest == "1.2.0"
    /// ```
    ///
    /// # Arguments
    /// * `versions`: A non-empty list of version strings.
    ///
    /// # Returns
    /// * `str`: The smallest version in the list.
    fn min(versions: UnpackList<String>) -> anyhow::Result<String> {
        evaluation_profile::profile_builtin_call("semver", "min", || {
            if versions.items.is_empty() {
                return Err(anyhow::anyhow!("Cannot get min of an empty list"));
            }
            let parsed = sorted_versions_asc(&versions.items)?;
            Ok(parsed.into_iter().next().unwrap().to_string())
        })
    }

    /// Filters a list of versions to those that satisfy all of the given requirements.
    ///
    /// The returned versions preserve the order they appear in the input.
    ///
    /// ```python
    /// matching = semver.filter(["1.0.0", "1.2.0", "2.0.0"], ["^1.0"])
    /// # matching == ["1.0.0", "1.2.0"]
    /// ```
    ///
    /// # Arguments
    /// * `versions`: The list of available version strings.
    /// * `requirements`: The list of semver requirement strings; each version must satisfy all of them.
    ///
    /// # Returns
    /// * `list[str]`: The subset of `versions` that satisfy every requirement.
    fn filter(
        versions: UnpackList<String>,
        requirements: UnpackList<String>,
    ) -> anyhow::Result<Vec<String>> {
        evaluation_profile::profile_builtin_call("semver", "filter", || {
            let reqs = parse_requirements(&requirements.items)?;
            let matched = select_matching(&versions.items, &reqs)?;
            Ok(matched
                .into_iter()
                .map(|(i, _)| versions.items[i].clone())
                .collect())
        })
    }

    /// Resolves the highest version from a list of available versions that satisfies all of the given requirements.
    ///
    /// ```python
    /// resolved = semver.resolve(
    ///     ["1.0.0", "1.2.0", "1.2.5", "2.0.0"],
    ///     ["^1.0", ">=1.2"],
    /// )
    /// # resolved == "1.2.5"
    /// ```
    ///
    /// # Arguments
    /// * `versions`: The list of available version strings to choose from.
    /// * `requirements`: The list of semver requirement strings that the chosen version must satisfy.
    ///
    /// # Returns
    /// * `str | None`: The highest matching version, or `None` if no version satisfies the requirements.
    fn resolve<'v>(
        versions: UnpackList<String>,
        requirements: UnpackList<String>,
        eval: &mut starlark::eval::Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Value<'v>> {
        evaluation_profile::profile_builtin_call("semver", "resolve", || {
            let reqs = parse_requirements(&requirements.items)?;
            let matched = select_matching(&versions.items, &reqs)?;

            let best = matched
                .into_iter()
                .max_by(|a, b| a.1.cmp(&b.1))
                .map(|(i, _)| versions.items[i].clone());

            Ok(match best {
                Some(original) => eval.heap().alloc(original),
                None => Value::new_none(),
            })
        })
    }

    /// Returns a list of versions that satisfy all of the given requirements, sorted in descending order (highest first).
    ///
    /// ```python
    /// candidates = semver.resolve_all(
    ///     ["1.0.0", "1.2.0", "1.2.5", "2.0.0"],
    ///     ["^1.0"],
    /// )
    /// # candidates == ["1.2.5", "1.2.0", "1.0.0"]
    /// ```
    ///
    /// # Arguments
    /// * `versions`: The list of available version strings.
    /// * `requirements`: The list of semver requirement strings that returned versions must satisfy.
    ///
    /// # Returns
    /// * `list[str]`: All matching versions, sorted from highest to lowest.
    fn resolve_all(
        versions: UnpackList<String>,
        requirements: UnpackList<String>,
    ) -> anyhow::Result<Vec<String>> {
        evaluation_profile::profile_builtin_call("semver", "resolve_all", || {
            let reqs = parse_requirements(&requirements.items)?;
            let mut matched = select_matching(&versions.items, &reqs)?;
            matched.sort_by(|a, b| b.1.cmp(&a.1));
            Ok(matched
                .into_iter()
                .map(|(i, _)| versions.items[i].clone())
                .collect())
        })
    }

    /// Increments the major component of a version, resetting minor, patch, pre, and build.
    ///
    /// ```python
    /// next_major = semver.bump_major("1.2.3-rc.1")
    /// # next_major == "2.0.0"
    /// ```
    ///
    /// # Arguments
    /// * `version`: The version string to bump.
    ///
    /// # Returns
    /// * `str`: The bumped version.
    fn bump_major(version: &str) -> anyhow::Result<String> {
        evaluation_profile::profile_builtin_call("semver", "bump_major", || {
            Ok(bump_major_version(parse_version(version)?).to_string())
        })
    }

    /// Increments the minor component of a version, resetting patch, pre, and build.
    ///
    /// ```python
    /// next_minor = semver.bump_minor("1.2.3")
    /// # next_minor == "1.3.0"
    /// ```
    ///
    /// # Arguments
    /// * `version`: The version string to bump.
    ///
    /// # Returns
    /// * `str`: The bumped version.
    fn bump_minor(version: &str) -> anyhow::Result<String> {
        evaluation_profile::profile_builtin_call("semver", "bump_minor", || {
            Ok(bump_minor_version(parse_version(version)?).to_string())
        })
    }

    /// Increments the patch component of a version, resetting pre and build.
    ///
    /// ```python
    /// next_patch = semver.bump_patch("1.2.3")
    /// # next_patch == "1.2.4"
    /// ```
    ///
    /// # Arguments
    /// * `version`: The version string to bump.
    ///
    /// # Returns
    /// * `str`: The bumped version.
    fn bump_patch(version: &str) -> anyhow::Result<String> {
        evaluation_profile::profile_builtin_call("semver", "bump_patch", || {
            Ok(bump_patch_version(parse_version(version)?).to_string())
        })
    }

    /// Returns true if the version has a pre-release identifier (e.g., `1.2.3-rc.1`).
    ///
    /// ```python
    /// if semver.is_prerelease("1.2.3-rc.1"):
    ///     # ...
    /// ```
    ///
    /// # Arguments
    /// * `version`: The version string to test.
    ///
    /// # Returns
    /// * `bool`: True if the version is a pre-release, False otherwise.
    fn is_prerelease(version: &str) -> anyhow::Result<bool> {
        evaluation_profile::profile_builtin_call("semver", "is_prerelease", || {
            let v = parse_version(version)?;
            Ok(!v.pre.is_empty())
        })
    }

    /// Extracts the first semantic version found anywhere in the given string.
    ///
    /// Useful for parsing a version out of a package name, archive filename, or tag.
    /// The version may appear as any substring of the input (e.g., `"foo-1.2.3"`,
    /// `"libthing_2.0.0-rc.1.tar.gz"`, `"v1.2.3+build.5"`).
    ///
    /// Optionally accepts a list of suffixes to strip from the end of `name`
    /// before scanning. Suffixes are stripped repeatedly until none match, so
    /// passing e.g. `[".tar.gz"]` reduces `"my-tool-1.2.3-rc.1.tar.gz"` to
    /// `"my-tool-1.2.3-rc.1"` before the regex runs. This is useful when archive
    /// extensions would otherwise be greedily consumed as part of a pre-release
    /// identifier.
    ///
    /// ```python
    /// version = semver.extract_version(
    ///     "my-tool-1.2.3-rc.1.tar.gz",
    ///     suffixes = [".tar.gz"],
    /// )
    /// # version == "1.2.3-rc.1"
    /// ```
    ///
    /// # Arguments
    /// * `name`: The package name (or any string) to scan for a version.
    /// * `suffixes`: Optional list of suffixes to strip from the end of `name`
    ///   before scanning. Stripping is applied repeatedly until no suffix matches.
    ///
    /// # Returns
    /// * `str | None`: The first valid semantic version found, or `None` if none is present.
    fn extract_version<'v>(
        name: &str,
        #[starlark(require = named, default = UnpackList::default())] suffixes: UnpackList<String>,
        eval: &mut starlark::eval::Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Value<'v>> {
        evaluation_profile::profile_builtin_call("semver", "extract_version", || {
            let trimmed = strip_suffixes(name, &suffixes.items);
            Ok(match find_version_in(trimmed) {
                Some(v) => eval.heap().alloc(v.to_string()),
                None => Value::new_none(),
            })
        })
    }

    /// Extracts every semantic version found in the given string, in the order they appear.
    ///
    /// Optionally accepts a list of suffixes to strip from the end of `name`
    /// before scanning. Suffixes are stripped repeatedly until none match, so
    /// passing e.g. `[".gz", ".tar"]` reduces `"foo-1.2.3.tar.gz"` to `"foo-1.2.3"`
    /// before the regex runs. This is useful when archive extensions would
    /// otherwise be greedily consumed as part of a pre-release identifier.
    ///
    /// ```python
    /// versions = semver.extract_all_versions("upgrade 1.2.3 to 2.0.0")
    /// # versions == ["1.2.3", "2.0.0"]
    ///
    /// versions = semver.extract_all_versions(
    ///     "my-tool-1.2.3-rc.1.tar.gz",
    ///     suffixes = [".tar.gz", ".zip"],
    /// )
    /// # versions == ["1.2.3-rc.1"]
    /// ```
    ///
    /// # Arguments
    /// * `name`: The string to scan for versions.
    /// * `suffixes`: Optional list of suffixes to strip from the end of `name`
    ///   before scanning. Stripping is applied repeatedly until no suffix matches.
    ///
    /// # Returns
    /// * `list[str]`: All valid semantic versions found in the input.
    fn extract_all_versions(
        name: &str,
        #[starlark(require = named, default = UnpackList::default())] suffixes: UnpackList<String>,
    ) -> anyhow::Result<Vec<String>> {
        evaluation_profile::profile_builtin_call("semver", "extract_all_versions", || {
            let trimmed = strip_suffixes(name, &suffixes.items);
            Ok(find_all_versions_in(trimmed)
                .into_iter()
                .map(|v| v.to_string())
                .collect())
        })
    }

    /// Validates a list of semver requirements, returning an error for the first invalid entry.
    ///
    /// ```python
    /// semver.validate_requirements(["^1.0", ">=2.0"])
    /// ```
    ///
    /// # Arguments
    /// * `requirements`: The list of semver requirement strings to validate.
    fn validate_requirements(requirements: UnpackList<String>) -> anyhow::Result<NoneType> {
        evaluation_profile::profile_builtin_call("semver", "validate_requirements", || {
            parse_requirements(&requirements.items)?;
            Ok(NoneType)
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_string()).collect()
    }

    fn v(s: &str) -> semver::Version {
        s.parse().unwrap()
    }

    fn r(s: &str) -> semver::VersionReq {
        s.parse().unwrap()
    }

    #[test]
    fn parse_version_accepts_valid() {
        let v = parse_version("1.2.3-rc.1+build.5").unwrap();
        assert_eq!(v.major, 1);
        assert_eq!(v.minor, 2);
        assert_eq!(v.patch, 3);
        assert_eq!(v.pre.as_str(), "rc.1");
        assert_eq!(v.build.as_str(), "build.5");
    }

    #[test]
    fn parse_version_rejects_invalid() {
        let err = parse_version("not-a-version").unwrap_err();
        assert!(format!("{err:#}").contains("not-a-version"));
    }

    #[test]
    fn parse_requirement_accepts_valid() {
        let req = parse_requirement("^1.2.0").unwrap();
        assert!(req.matches(&v("1.2.5")));
        assert!(!req.matches(&v("2.0.0")));
    }

    #[test]
    fn parse_requirement_rejects_invalid() {
        let err = parse_requirement("not a req").unwrap_err();
        assert!(format!("{err:#}").contains("not a req"));
    }

    #[test]
    fn parse_versions_short_circuits_on_error() {
        let err = parse_versions(&s(&["1.0.0", "bad", "2.0.0"])).unwrap_err();
        assert!(format!("{err:#}").contains("bad"));
    }

    #[test]
    fn parse_versions_returns_all_when_valid() {
        let versions = parse_versions(&s(&["1.0.0", "2.0.0"])).unwrap();
        assert_eq!(versions, vec![v("1.0.0"), v("2.0.0")]);
    }

    #[test]
    fn parse_requirements_short_circuits_on_error() {
        let err = parse_requirements(&s(&["^1", "totally invalid"])).unwrap_err();
        assert!(format!("{err:#}").contains("totally invalid"));
    }

    #[test]
    fn version_matches_all_requires_every_req() {
        let reqs = vec![r("^1.0"), r(">=1.2")];
        assert!(version_matches_all(&v("1.2.5"), &reqs));
        assert!(!version_matches_all(&v("1.1.0"), &reqs));
        assert!(!version_matches_all(&v("2.0.0"), &reqs));
    }

    #[test]
    fn version_matches_all_with_empty_reqs_is_true() {
        assert!(version_matches_all(&v("1.2.3"), &[]));
    }

    #[test]
    fn sorted_versions_asc_orders_numerically() {
        let sorted = sorted_versions_asc(&s(&["1.10.0", "1.2.0", "1.2.10"])).unwrap();
        assert_eq!(sorted, vec![v("1.2.0"), v("1.2.10"), v("1.10.0")]);
    }

    #[test]
    fn sorted_versions_asc_propagates_parse_errors() {
        assert!(sorted_versions_asc(&s(&["1.0.0", "junk"])).is_err());
    }

    #[test]
    fn select_matching_preserves_input_order_and_indices() {
        let reqs = vec![r("^1.0")];
        let versions = s(&["2.0.0", "1.0.0", "1.5.0", "0.9.0"]);
        let matched = select_matching(&versions, &reqs).unwrap();
        let indices: Vec<usize> = matched.iter().map(|(i, _)| *i).collect();
        assert_eq!(indices, vec![1, 2]);
        assert_eq!(matched[0].1, v("1.0.0"));
        assert_eq!(matched[1].1, v("1.5.0"));
    }

    #[test]
    fn select_matching_with_no_reqs_keeps_everything() {
        let versions = s(&["1.0.0", "2.0.0"]);
        let matched = select_matching(&versions, &[]).unwrap();
        assert_eq!(matched.len(), 2);
    }

    #[test]
    fn select_matching_propagates_parse_errors() {
        assert!(select_matching(&s(&["bogus"]), &[]).is_err());
    }

    #[test]
    fn bump_major_resets_lower_components() {
        assert_eq!(bump_major_version(v("1.2.3-rc.1+build.5")), v("2.0.0"));
    }

    #[test]
    fn bump_minor_resets_patch_and_metadata() {
        assert_eq!(bump_minor_version(v("1.2.3-rc.1+build.5")), v("1.3.0"));
    }

    #[test]
    fn bump_patch_resets_pre_and_build() {
        assert_eq!(bump_patch_version(v("1.2.3-rc.1+build.5")), v("1.2.4"));
    }

    #[test]
    fn parse_candidate_trims_trailing_punctuation() {
        assert_eq!(parse_candidate("1.2.3.").unwrap(), v("1.2.3"));
        assert_eq!(parse_candidate("1.2.3-").unwrap(), v("1.2.3"));
    }

    #[test]
    fn parse_candidate_returns_full_version() {
        assert_eq!(
            parse_candidate("1.2.3-rc.1+build.5").unwrap(),
            v("1.2.3-rc.1+build.5")
        );
    }

    #[test]
    fn parse_candidate_rejects_garbage() {
        assert!(parse_candidate("not-a-version").is_none());
    }

    #[test]
    fn find_version_in_finds_first_valid() {
        // Whitespace/underscore boundaries terminate the pre-release greedy match.
        assert_eq!(
            find_version_in("my_tool 1.2.3-rc.1 tar.gz").unwrap(),
            v("1.2.3-rc.1")
        );
        assert_eq!(
            find_version_in("v1.2.3+build.5").unwrap(),
            v("1.2.3+build.5")
        );
        // Plain triple is the first match.
        assert_eq!(find_version_in("foo-1.2.3 stuff").unwrap(), v("1.2.3"));
    }

    #[test]
    fn find_version_in_returns_none_when_absent() {
        assert!(find_version_in("no version here").is_none());
        assert!(find_version_in("1.2").is_none());
    }

    #[test]
    fn find_all_versions_in_preserves_order() {
        // Use a whitespace separator so the greedy pre-release match doesn't span both.
        let found = find_all_versions_in("upgrade 1.2.3 to 2.0.0");
        assert_eq!(found, vec![v("1.2.3"), v("2.0.0")]);
    }

    #[test]
    fn find_all_versions_in_returns_empty_when_absent() {
        assert!(find_all_versions_in("nothing here").is_empty());
    }

    #[test]
    fn strip_suffixes_removes_single_suffix() {
        assert_eq!(
            strip_suffixes("foo-1.2.3.tar.gz", &s(&[".tar.gz"])),
            "foo-1.2.3"
        );
    }

    #[test]
    fn strip_suffixes_loops_until_no_match() {
        // `.gz` strips first, then `.tar`, then nothing matches.
        assert_eq!(
            strip_suffixes("foo-1.2.3.tar.gz", &s(&[".gz", ".tar"])),
            "foo-1.2.3"
        );
    }

    #[test]
    fn strip_suffixes_returns_input_when_nothing_matches() {
        assert_eq!(
            strip_suffixes("foo-1.2.3", &s(&[".zip", ".xz"])),
            "foo-1.2.3"
        );
    }

    #[test]
    fn strip_suffixes_with_empty_list_is_identity() {
        assert_eq!(strip_suffixes("foo-1.2.3.tar.gz", &[]), "foo-1.2.3.tar.gz");
    }

    #[test]
    fn strip_suffixes_ignores_empty_suffix_entries() {
        // Empty suffixes would otherwise match infinitely; ensure we skip them.
        assert_eq!(strip_suffixes("foo-1.2.3", &s(&[""])), "foo-1.2.3");
    }

    #[test]
    fn strip_suffixes_then_extract_recovers_clean_version() {
        let trimmed = strip_suffixes("my-tool-1.2.3-rc.1.tar.gz", &s(&[".tar.gz"]));
        assert_eq!(find_all_versions_in(trimmed), vec![v("1.2.3-rc.1")]);
    }
}
