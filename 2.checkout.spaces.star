"""
Finalize checkout.
"""

load("//@star/packages/star/rust.star", "rust_checkout_update")
load("//@star/sdk/star/checkout-config.star", "checkout_config_add_markdown")

checkout_config_add_markdown("checkout_config")

rust_checkout_update(
    "rustup_init",
    projects = ["."],
)
