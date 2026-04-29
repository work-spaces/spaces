#!/usr/bin/env spaces

# Demo script showing comprehensive usage of @star/sdk/star/std/args.star
#
# Example invocations:
#   spaces spaces/test/args.exec.star --help
#   spaces spaces/test/args.exec.star -v deploy myapp prod staging
#   spaces spaces/test/args.exec.star deploy myapp --dry-run -e staging --tag important
#   spaces spaces/test/args.exec.star deploy myapp -t v1.0 -t hotfix --config custom.yaml

load(
    "//@star/sdk/star/std/args.star",
    "args_argv",
    "args_flag",
    "args_list",
    "args_opt",
    "args_parse",
    "args_parser",
    "args_pos",
    "args_program",
)

"""Demo deployment tool using the args module."""

# ===== DEFINE FLAGS (boolean options) =====
verbose_flag = args_flag(
    "--verbose",
    short = "-v",
    help = "Enable verbose output",
)

dry_run_flag = args_flag(
    "--dry-run",
    short = "-d",
    help = "Preview changes without applying them",
)

force_flag = args_flag(
    "--force",
    short = "-f",
    help = "Force deployment even if validation fails",
)

# ===== DEFINE OPTIONS (single-value arguments) =====
env_opt = args_opt(
    "--env",
    short = "-e",
    default = "dev",
    choices = ["dev", "staging", "prod"],
    help = "Target environment (dev, staging, prod)",
)

config_opt = args_opt(
    "--config",
    short = "-c",
    default = "deploy.yaml",
    help = "Path to deployment configuration file",
)

timeout_opt = args_opt(
    "--timeout",
    type = "int",
    default = 300,
    help = "Deployment timeout in seconds",
)

max_retries_opt = args_opt(
    "--max-retries",
    short = "-r",
    type = "int",
    default = 3,
    help = "Maximum number of retries on failure",
)

# ===== DEFINE LIST OPTIONS (repeatable arguments) =====
tag_list = args_list(
    "--tag",
    short = "-t",
    help = "Add a deployment tag (can be used multiple times)",
)

exclude_list = args_list(
    "--exclude",
    short = "-x",
    help = "Exclude a service from deployment (can be used multiple times)",
)

notify_list = args_list(
    "--notify",
    short = "-n",
    help = "Notify a user after deployment (can be used multiple times)",
)

# ===== DEFINE POSITIONAL ARGUMENTS =====
# Required positional: the service to deploy
service_pos = args_pos("service", required = True)

# Variadic positional: optional target environments/regions
targets_pos = args_pos("targets", variadic = True)

# ===== CREATE PARSER SPECIFICATION =====
parser_spec = args_parser(
    name = "deploy",
    description = "Deploy a service to one or more target environments with flexible configuration",
    options = [
        verbose_flag,
        dry_run_flag,
        force_flag,
        env_opt,
        config_opt,
        timeout_opt,
        max_retries_opt,
        tag_list,
        exclude_list,
        notify_list,
    ],
    positional = [service_pos, targets_pos],
)

# ===== PARSE COMMAND-LINE ARGUMENTS =====
# args_parse automatically handles --help and -h
# It also handles errors by printing usage and exiting with code 2
parsed = args_parse(parser_spec)

# ===== DISPLAY PARSED RESULTS =====
print("=== Deployment Tool - Args Module Demo ===")
print("")

print("Program Information:")
print("  Program Name: {}".format(args_program()))
print("  Full Argv: {}".format(args_argv()))
print("")

print("Positional Arguments:")

print("  Service: {}".format(parsed.get("service")))
print("  Targets: {}".format(parsed.get("targets")))
print("")

print("Boolean Flags:")
print("  Verbose: {}".format(parsed.get("verbose")))
print("  Dry Run: {}".format(parsed.get("dry_run")))
print("  Force: {}".format(parsed.get("force")))
print("")

print("Single-Value Options:")
print("  Environment: {}".format(parsed.get("env")))
print("  Config File: {}".format(parsed.get("config")))
print("  Timeout: {} seconds".format(parsed.get("timeout")))
print("  Max Retries: {}".format(parsed.get("max_retries")))
print("")

print("List Options (repeatable):")
print("  Tags: {}".format(parsed.get("tag")))
print("  Excluded Services: {}".format(parsed.get("exclude")))
print("  Users to Notify: {}".format(parsed.get("notify")))
print("")

# ===== DEMONSTRATE CONDITIONAL LOGIC =====
print("=== Processing Logic ===")
print("")

if parsed.get("verbose"):
    print("✓ VERBOSE mode enabled - showing detailed output")

if parsed.get("dry_run"):
    print("⚠ DRY RUN mode - no actual changes will be applied")

if parsed.get("force"):
    print("⚠ FORCE mode - skipping validation checks")

if parsed.get("env") == "prod":
    print("🔴 PRODUCTION deployment detected")

# Process tags
if parsed.get("tag"):
    print("📋 Deployment tags ({}):".format(len(parsed["tag"])))
    for tag in parsed["tag"]:
        print("   - {}".format(tag))

# Process excluded services
if parsed.get("exclude"):
    print("🚫 Excluding {} services:".format(len(parsed["exclude"])))
    for svc in parsed["exclude"]:
        print("   - {}".format(svc))

# Process notifications
if parsed.get("notify"):
    print("📧 Will notify {} users:".format(len(parsed["notify"])))
    for user in parsed["notify"]:
        print("   - {}".format(user))

# Process targets
if parsed.get("targets"):
    print("🎯 Deploying to {} targets:".format(len(parsed.get("targets"))))
    for target in parsed.get("targets"):
        print("   - {}".format(target))
else:
    print("🎯 No specific targets - using default for {} environment".format(parsed.get("env")))

print("")
print("✅ Demo complete!")
