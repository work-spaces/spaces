"""
Load the spaces starlark SDK and packages repositories.
"""

checkout.add_repo(
    rule = {"name": "@star/sdk"},
    repo = {
        "url": "https://github.com/work-spaces/sdk",
        "rev": "a602188a5028eafe22edda18c82811b6f62fb04f",
        "checkout": "Revision",
        "clone": "Default",
    },
)

checkout.add_repo(
    rule = {"name": "@star/packages"},
    repo = {
        "url": "https://github.com/work-spaces/packages",
        "rev": "e65b1eb90e2c547e993474cc6ec8f99d43f6b6fd",
        "checkout": "Revision",
        "clone": "Default",
    },
)
