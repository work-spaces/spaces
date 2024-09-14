"""

"""

spaces_cargo_toml = fs.read_toml("spaces/Cargo.toml")

spaces_version = spaces_cargo_toml["package"]["version"]

spaces_release = "spaces-v{spaces_version}".format()
spaces_repo = "https://github.com/work-spaces/workflows"
repo_arg = "--repo={spaces_repo}".format()
spaces_platform = info.platform_name()
spaces_archive_file = "spaces-v{spaces_version}-{spaces_platform}.zip".format()
spaces_binary = "spaces/target/release/spaces"

run.add_exec(
    name = "build",
    command = "cargo",
    cwd = "spaces",
    args = ["build", "--profile=release"],
    outputs = [spaces_binary]
)

run.add_exec(
    name = "mkdir_deploy",
    command = "mkdir",
    args = ["-p", "build/deploy"]
)

run.add_archive(
    name = "archive_spaces",
    input = spaces_binary,
    version = spaces_version,
    deps = ["build"]
)

run.add_exec(
    name = "check_release",
    command = "gh",
    args = ["release", "view", spaces_release, repo_arg]
)

run.add_exec(
    name = "upload",
    command = "gh",
    args = ["release", "upload", spaces_release, spaces_archive_file, repo_arg],
    deps = ["archive_spaces"]
)

run.add_exec(
    name = "create_release",
    command = "gh",
    args = ["release", "create", spaces_release, "--generate-notes", repo_arg],
    deps = ["check_release", "upload"]
)
