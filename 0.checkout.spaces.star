"""
Load the spaces starlark SDK and packages repositories.
"""

checkout.add_repo(
    rule = {"name": "@star/sdk"},
    repo = {
        "url": "https://github.com/work-spaces/sdk",
        "rev": "v0.3.22",
        "checkout": "Revision",
        "clone": "Default",
    },
)

checkout.add_repo(
    rule = {"name": "@star/packages"},
    repo = {
        "url": "https://github.com/work-spaces/packages",
        "rev": "9d18141113a3f2af4b4fe04a220cde3f7053a672",
        "checkout": "Revision",
        "clone": "Default",
    },
)
