"""
Load the spaces starlark SDK and packages repositories.
"""

checkout.add_repo(
    rule = {"name": "@star/sdk"},
    repo = {
        "url": "https://github.com/work-spaces/sdk",
        "rev": "v0.3.20",
        "checkout": "Revision",
        "clone": "Default",
    },
)

checkout.add_repo(
    rule = {"name": "@star/packages"},
    repo = {
        "url": "https://github.com/work-spaces/packages",
        # Provides rust_add() zed configuration
        "rev": "9d88c2db7b5577d4762054ba929ff2e92089ef94",
        "checkout": "Revision",
        "clone": "Default",
    },
)
