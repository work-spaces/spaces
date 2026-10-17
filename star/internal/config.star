"""
Spaces checkout configuration
"""

load(
    "//@star/sdk/star/checkout-config.star",
    "checkout_config_register_optin",
    "checkout_config_register_optout",
    "checkout_config_register_value",
)

GITHUB_REPOSITORY_CONFIG_KEY = "GITHUB_REPOSITORY"
SPACES_CHECKOUT_INSTALL_SPACES_CONFIG_KEY = "SPACES_CHECKOUT_INSTALL_SPACES"
SPACES_DEV_SKILL_CONFIG_KEY = "SPACES_DEV_SKILL"
SPACES_ENABLE_SANDBOX_CONFIG_KEY = "SPACES_ENABLE_SANDBOX"
SPACES_RELEASE_TAG_CONFIG_KEY = "SPACES_RELEASE_TAG"

def config_register_checkout() -> None:
    """
    Registers Spaces checkout configuration.

    Args:
        None.

    Returns:
        None.
    """
    checkout_config_register_value(
        GITHUB_REPOSITORY_CONFIG_KEY,
        help = "GitHub repository used for release publishing",
    )
    checkout_config_register_optin(
        SPACES_CHECKOUT_INSTALL_SPACES_CONFIG_KEY,
        help = "Check out install-spaces for testing spaces sync",
    )
    checkout_config_register_optout(
        SPACES_DEV_SKILL_CONFIG_KEY,
        help = "Enable the spaces dev skill in the workspace",
    )
    checkout_config_register_optin(
        SPACES_ENABLE_SANDBOX_CONFIG_KEY,
        help = "Enable sandbox-related build dependencies",
    )
    checkout_config_register_value(
        SPACES_RELEASE_TAG_CONFIG_KEY,
        help = "GitHub release tag to build and publish",
    )
