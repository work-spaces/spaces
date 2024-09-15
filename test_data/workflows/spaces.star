"""

"""

current_path = info.current_workspace_path()

print("Current path is (workflows) {}".format(current_path))
debug("Current path is (workflows) {}".format(current_path))

run.add_exec(
    rule = {"name": "pre-configure" },
    exec = {
        "command": "sleep",
        "args": ["2"],
    }
)

run.add_exec(
    rule = {"name": "pre-configure2", "deps": ["pre-configure"] },
    exec = {
        "command": "sleep",
        "args": ["2"],
    }
)