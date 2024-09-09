"""

"""

cargo_toml = fs.read_toml("spaces/test_data/spaces_cargo.toml")

print(cargo_toml["package"]["version"])