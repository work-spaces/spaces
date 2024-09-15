"""

"""

current_path = info.current_workspace_path()

print("Current path is{}".format(current_path))

run.add_exec(
    rule = {"name": "build", "deps": ["configure1", "configure2", "configure4"]},
    exec = {
        "command": "sleep",
        "args": ["3"],
    },
)

run.add_exec(
    rule = {"name": "env_test", "deps": ["configure1", "configure2", "configure4"]},
    exec = {
        "command": "cat",
        "args": ["spaces.star"],
        "redirect_stdout": "env_test.txt",
        "env": {"SALTR_VAR": "HELLO WORLD!"},
    },
)

run.add_exec(
    rule = {"name": "pre-configure", "deps": ["workflows:pre-configure"] },
    exec = {
        "command": "sleep",
        "args": ["5"],
    }
)

run.add_exec(
    rule = {"name": "post-build", "deps": ["build"]},
    exec = {
        "command": "sleep",
        "args": ["4"],
    }
)

run.add_exec(
    rule = {"name": "configure1", "deps": ["pre-configure"]},
    exec = {
        "command": "sleep",
        "args": ["2"],
    }
)

run.add_exec(
    rule = {"name": "configure2", "deps": ["pre-configure"]},
    exec = {
        "command": "sleep",
        "args": ["4"],
    }
)

run.add_exec(
    rule = {"name": "configure3", "deps": ["pre-configure"]},
    exec = {
        "command": "sleep",
        "args": ["3"],
    }
)

run.add_exec(
    rule = {"name": "configure4", "deps": ["configure3"]},
    exec = {
        "command": "sleep",
        "args": ["2"],
    }
)
