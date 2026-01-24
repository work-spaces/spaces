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
        "rev": "df44363711d8bb4ce14d61eb8c6ebf3c5d986f14",
        "checkout": "Revision",
        "clone": "Default",
    },
)
