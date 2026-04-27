#!/usr/bin/env spaces

load(
    "//@star/sdk/star/std/json.star",
    "json_dumps",
)
load(
    "//@star/sdk/star/std/path.star",
    "path_absolute",
    "path_basename",
    "path_components",
    "path_dirname",
    "path_expand_user",
    "path_expand_vars",
    "path_extension",
    "path_is_absolute",
    "path_join",
    "path_normalize",
    "path_parent",
    "path_relative_to",
    "path_separator",
    "path_split",
    "path_stem",
    "path_with_extension",
)

# Path module test results
path_results = {
    "path_joining": {},
    "component_extraction": {},
    "extension_handling": {},
    "path_checking": {},
    "normalization": {},
    "path_expansion": {},
    "path_components": {},
    "parent_navigation": {},
    "utilities": {},
    "edge_cases": {},
}

# ============================================================================
# Path Module Tests
# ============================================================================

# Test path joining
path_results["path_joining"]["join_simple"] = path_join(["src", "components", "app.tsx"])
path_results["path_joining"]["join_multiple"] = path_join(["home", "user", "projects", "myapp"])
path_results["path_joining"]["join_with_empty"] = path_join(["a", "", "b", "c"])
path_results["path_joining"]["join_single"] = path_join(["README.md"])

# Absolute segment resets the path (Python os.path.join behaviour)
path_results["path_joining"]["join_absolute_in_middle"] = path_join(["a/b", "/c", "d"]) == "/c/d"

# Test component extraction
path_results["component_extraction"]["dirname_file"] = path_dirname("/home/user/documents/report.pdf")
path_results["component_extraction"]["dirname_relative"] = path_dirname("src/config/settings.json")
path_results["component_extraction"]["dirname_filename_only"] = path_dirname("README.md")
path_results["component_extraction"]["basename_file"] = path_basename("/home/user/documents/report.pdf")
path_results["component_extraction"]["basename_relative"] = path_basename("src/components/Button.tsx")
path_results["component_extraction"]["basename_root"] = path_basename("README.md")
path_results["component_extraction"]["split_path"] = path_split("/home/user/documents/report.pdf")
path_results["component_extraction"]["split_relative"] = path_split("src/config/settings.json")

# Test extension handling
path_results["extension_handling"]["stem_simple"] = path_stem("/home/user/documents/report.pdf")
path_results["extension_handling"]["stem_multiple_dots"] = path_stem("archive.tar.gz")
path_results["extension_handling"]["stem_no_extension"] = path_stem("Makefile")
path_results["extension_handling"]["extension_simple"] = path_extension("document.pdf")
path_results["extension_handling"]["extension_from_path"] = path_extension("/home/user/config.json")
path_results["extension_handling"]["extension_no_extension"] = path_extension("Makefile")
path_results["extension_handling"]["extension_multiple_dots"] = path_extension("archive.tar.gz")
path_results["extension_handling"]["with_extension_replace"] = path_with_extension("document.txt", "md")
path_results["extension_handling"]["with_extension_add"] = path_with_extension("README", "txt")
path_results["extension_handling"]["with_extension_from_path"] = path_with_extension("/home/user/image.jpg", "png")
path_results["extension_handling"]["with_extension_empty"] = path_with_extension("file.txt", "")

# Dotfiles: the whole name is the stem, extension is empty
path_results["extension_handling"]["stem_dotfile"] = path_stem(".bashrc") == ".bashrc"
path_results["extension_handling"]["extension_dotfile"] = path_extension(".bashrc") == ""
path_results["extension_handling"]["extension_gitignore"] = path_extension(".gitignore") == ""

# Test path type checking
path_results["path_checking"]["is_absolute_unix"] = path_is_absolute("/home/user/file.txt")
path_results["path_checking"]["is_absolute_relative"] = path_is_absolute("./config.json")
path_results["path_checking"]["is_absolute_parent_ref"] = path_is_absolute("../data/file.txt")
path_results["path_checking"]["is_absolute_filename"] = path_is_absolute("README.md")

# Test path normalization
path_results["normalization"]["normalize_double_sep"] = path_normalize("a//b///c")
path_results["normalization"]["normalize_current_dir"] = path_normalize("a/./b")
path_results["normalization"]["normalize_parent_dir"] = path_normalize("a/b/../c")
path_results["normalization"]["normalize_complex"] = path_normalize("./foo//bar/../baz/./file.txt")
path_results["normalization"]["normalize_excess_parent"] = path_normalize("a/b/../../c")
path_results["normalization"]["normalize_absolute"] = path_normalize("/a//b/../c/./d")

# Consecutive leading ".." — Bug regression tests
path_results["normalization"]["normalize_leading_dotdot"] = path_normalize("../../a") == "../../a"
path_results["normalization"]["normalize_pure_dotdots"] = path_normalize("../..") == "../.."
path_results["normalization"]["normalize_single_dotdot"] = path_normalize("..") == ".."

# Absolute path with excess ".." must clamp at root, never go above "/"
path_results["normalization"]["normalize_absolute_excess_parent"] = path_normalize("/a/../../c") == "/c"

# Test absolute path conversion
path_results["normalization"]["absolute_relative"] = path_is_absolute(path_absolute("./config.json"))
path_results["normalization"]["absolute_parent_ref"] = path_is_absolute(path_absolute("../data.txt"))
path_results["normalization"]["absolute_filename"] = path_is_absolute(path_absolute("README.md"))

# Test path expansion - expand_user
path_results["path_expansion"]["expand_user_pattern"] = path_expand_user("~/projects/myapp").endswith("projects/myapp")
path_results["path_expansion"]["expand_user_tilde_only"] = path_expand_user("~").startswith("/") or path_expand_user("~").find(":") >= 0
path_results["path_expansion"]["expand_user_no_tilde"] = path_expand_user("/etc/hosts")

# Test path expansion - expand_vars
path_results["path_expansion"]["expand_vars_HOME"] = len(path_expand_vars("$HOME")) > 0
path_results["path_expansion"]["expand_vars_braces"] = len(path_expand_vars("${HOME}")) > 0
path_results["path_expansion"]["expand_vars_multiple"] = "$" not in path_expand_vars("$HOME/$USER/data")
path_results["path_expansion"]["expand_vars_nonexistent"] = path_expand_vars("$NONEXISTENT_VAR_XYZ/file.txt").startswith("/file.txt")

# Non-ASCII path segments must not be corrupted when no variable is present
path_results["path_expansion"]["expand_vars_nonascii"] = path_expand_vars("/home/tëst/file.txt") == "/home/tëst/file.txt"

# Test path components decomposition
path_results["path_components"]["components_absolute"] = path_components("/home/user/app.py")[0] == "/"
path_results["path_components"]["components_relative"] = path_components("src/app.py")[0] == "src"
path_results["path_components"]["components_single"] = path_components("file.txt") == ["file.txt"]
path_results["path_components"]["components_dot_filtered"] = path_components("a/./b") == ["a", "b"]
path_results["path_components"]["components_dotdot_preserved"] = path_components("a/b/../c") == ["a", "b", "..", "c"]

# Test parent directory navigation
path_results["parent_navigation"]["parent_single"] = path_parent("/home/user/projects/app.py") == "/home/user/projects"
path_results["parent_navigation"]["parent_multiple"] = path_parent("a/b/c/d", levels = 2) == "a/b"
path_results["parent_navigation"]["parent_excess_levels"] = path_parent("file.txt", levels = 5) == ""
path_results["parent_navigation"]["parent_zero_levels"] = path_parent("a/b/c", levels = 0) == "a/b/c"

# Test utility functions
sep = path_separator()
path_results["utilities"]["separator_is_slash_or_backslash"] = sep == "/" or sep == "\\"
path_results["utilities"]["separator_unix"] = sep == "/" or sep == "\\"

# Test path relative_to (basic cases)
path_results["normalization"]["relative_to_same_base"] = path_relative_to("/home/user/file.txt", "/home/user").startswith("file.txt")

# ============================================================================
# Edge Case Tests
# ============================================================================

# Empty path
path_results["edge_cases"]["join_empty_list"] = path_join([]) == ""
path_results["edge_cases"]["dirname_empty"] = path_dirname("") == ""
path_results["edge_cases"]["basename_empty"] = path_basename("") == ""
path_results["edge_cases"]["normalize_empty"] = path_normalize("") == ""
path_results["edge_cases"]["components_empty"] = path_components("") == []
path_results["edge_cases"]["stem_empty"] = path_stem("") == ""
path_results["edge_cases"]["extension_empty"] = path_extension("") == ""

# Root path "/"
path_results["edge_cases"]["dirname_root"] = path_dirname("/") == ""
path_results["edge_cases"]["basename_root"] = path_basename("/") == ""

# Trailing slash: Rust strips it, so "bin" becomes the last component
path_results["edge_cases"]["dirname_trailing_slash"] = path_dirname("/usr/local/bin/") == "/usr/local"
path_results["edge_cases"]["basename_trailing_slash"] = path_basename("/usr/local/bin/") == "bin"

# ============================================================================
# Output Results
# ============================================================================

print("Path Module Test Results:")
print("========================")
print("")
print(json_dumps(path_results, is_pretty = True))
print("")
print("All path functions executed successfully!")
