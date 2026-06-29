#!/usr/bin/env spaces
"""
Generate Icons

Rasterize the Spaces logo SVGs into PNGs (and friends) using Inkscape.

For every variant (black, gray, white, transparent) this produces:
  - a macOS `.iconset` directory (iconutil-friendly) plus an assembled `.icns`
  - a web-friendly `favicon.ico` (16/32/48 px PNG-encoded entries)
  - a plain web PNG for places where SVG can't be used
  - a Slack-emoji-sized PNG (128 px, under Slack's 128 KB limit)

Run directly to test with defaults:
  spaces spaces/scripts/icons.exec.star

Or customize:
  spaces spaces/scripts/icons.exec.star --variant black --variant white
  spaces spaces/scripts/icons.exec.star --icons-dir icons-v0.17.0 --output build/icons
"""

load(
    "//@star/prelude/exec/args.star",
    "args_list",
    "args_opt",
    "args_parse",
    "args_parser",
)
load("//@star/prelude/exec/env.star", "env_which")
load(
    "//@star/prelude/exec/fs.star",
    "fs_exists",
    "fs_mkdir",
    "fs_read_bytes",
    "fs_remove",
    "fs_write_bytes",
)
load("//@star/prelude/exec/log.star", "log_error", "log_fatal", "log_info")
load("//@star/prelude/exec/path.star", "path_join")
load(
    "//@star/prelude/exec/process.star",
    "process_options",
    "process_run",
    "process_stderr_capture",
    "process_stdout_capture",
)
load("//@star/prelude/exec/sys.star", "sys_exit")

VARIANTS = ["black", "gray", "white", "transparent"]

# macOS iconutil expects these logical sizes; @2x doubles the pixels.
ICONSET_SIZES = [16, 32, 128, 256, 512]

FAVICON_SIZES = [16, 32, 48]

# Inkscape candidates to probe when one is not supplied on the command line.
INKSCAPE_CANDIDATES = [
    "/Applications/Inkscape.app/Contents/MacOS/inkscape",
    "inkscape",
]

def _resolve_inkscape(explicit):
    if explicit:
        return explicit
    for candidate in INKSCAPE_CANDIDATES:
        found = env_which(candidate)
        if found:
            return found
    log_fatal("Inkscape not found; pass --inkscape with the executable path")
    return ""

def render_png(inkscape, svg, out_png, size):
    """Rasterize a single SVG to a square PNG at the given pixel size."""
    options = process_options(
        command = inkscape,
        args = [
            "--export-type=png",
            "--export-filename={}".format(out_png),
            "--export-width={}".format(size),
            "--export-height={}".format(size),
            svg,
        ],
        stdout = process_stdout_capture(),
        stderr = process_stderr_capture(),
    )
    result = process_run(options)
    if result.get("status", 1) != 0:
        log_error(result.get("stderr", ""))
        log_fatal("inkscape failed rendering {} @ {}px".format(svg, size))
    if not fs_exists(out_png):
        log_fatal("inkscape produced no output for {} @ {}px".format(svg, size))

def _le16(value):
    return [value % 256, (value // 256) % 256]

def _le32(value):
    return [
        value % 256,
        (value // 256) % 256,
        (value // 65536) % 256,
        (value // 16777216) % 256,
    ]

def build_ico(png_paths, sizes, out_ico):
    """Assemble PNG-encoded entries into a browser-friendly favicon.ico."""
    count = len(png_paths)
    header = [0, 0, 1, 0] + _le16(count)
    dir_size = 16 * count
    offset = len(header) + dir_size

    entries = []
    payload = []
    for index in range(count):
        data = fs_read_bytes(png_paths[index])
        size = sizes[index]
        width = 0 if size >= 256 else size
        height = width
        entries += [width, height, 0, 0] + _le16(1) + _le16(32) + _le32(len(data)) + _le32(offset)
        payload += data
        offset += len(data)

    fs_write_bytes(out_ico, header + entries + payload)

def make_iconset(inkscape, svg, variant, output):
    """Produce a `.iconset` dir and convert it to `.icns` via iconutil."""
    iconset_dir = path_join([output, "spaces-logo-{}.iconset".format(variant)])
    fs_mkdir(iconset_dir, parents = True, exist_ok = True)

    for size in ICONSET_SIZES:
        render_png(inkscape, svg, path_join([iconset_dir, "icon_{}x{}.png".format(size, size)]), size)
        render_png(inkscape, svg, path_join([iconset_dir, "icon_{}x{}@2x.png".format(size, size)]), size * 2)

    icns = path_join([output, "spaces-logo-{}.icns".format(variant)])
    result = process_run(process_options(
        command = "iconutil",
        args = ["--convert", "icns", "--output", icns, iconset_dir],
        stdout = process_stdout_capture(),
        stderr = process_stderr_capture(),
    ))
    if result.get("status", 1) != 0:
        log_error(result.get("stderr", ""))
        log_fatal("iconutil failed for {}".format(variant))
    fs_remove(iconset_dir, recursive = True)
    log_info("  icns:    {}".format(icns))

def make_favicon(inkscape, svg, variant, output):
    fs_mkdir(output, parents = True, exist_ok = True)
    pngs = []
    for size in FAVICON_SIZES:
        png = path_join([output, "favicon-{}-{}.png".format(variant, size)])
        render_png(inkscape, svg, png, size)
        pngs.append(png)
    ico = path_join([output, "favicon-{}.ico".format(variant)])
    build_ico(pngs, FAVICON_SIZES, ico)
    for png in pngs:
        fs_remove(png)
    log_info("  favicon: {}".format(ico))

def make_web(inkscape, svg, variant, output, size):
    fs_mkdir(output, parents = True, exist_ok = True)
    png = path_join([output, "spaces-logo-{}-web.png".format(variant)])
    render_png(inkscape, svg, png, size)
    log_info("  web:     {}".format(png))

def make_emoji(inkscape, svg, variant, output, size):
    fs_mkdir(output, parents = True, exist_ok = True)
    png = path_join([output, "spaces-logo-{}-emoji.png".format(variant)])
    render_png(inkscape, svg, png, size)
    log_info("  emoji:   {}".format(png))

def make_svg(svg, variant, output):
    fs_mkdir(output, parents = True, exist_ok = True)
    out_svg = path_join([output, "spaces-logo-{}.svg".format(variant)])
    fs_write_bytes(out_svg, fs_read_bytes(svg))
    log_info("  svg:     {}".format(out_svg))

parser = args_parser(
    name = "icons",
    description = "Rasterize Spaces logo SVGs into icon/web/emoji assets with Inkscape",
    options = [
        args_opt("--icons-dir", short = "-i", default = "icons-v0.17.0", help = "Directory with spaces-logo-*.svg"),
        args_opt("--output", short = "-o", default = "build/icons", help = "Output directory"),
        args_opt("--inkscape", help = "Path to the inkscape executable"),
        args_opt("--web-size", type = "int", default = 1024, help = "Web PNG size in px"),
        args_opt("--emoji-size", type = "int", default = 128, help = "Slack emoji size in px"),
        args_list("--variant", short = "-v", choices = VARIANTS, help = "Variant(s) to build (default: all)"),
    ],
)
args = args_parse(parser)

icons_dir = args.get("icons_dir", "<not provided>")
icons_dir = args.get("icons_dir", "icons-v0.17.0")
output = args.get("output", "build/icons")
inkscape = _resolve_inkscape(args.get("inkscape"))
variants = args.get("variant") or VARIANTS
web_size = args.get("web_size", 512)
emoji_size = args.get("emoji_size", 128)

log_info("Using inkscape: {}".format(inkscape))
log_info("Building variants: {}".format(", ".join(variants)))

for variant in variants:
    svg = path_join([icons_dir, "spaces-logo-{}.svg".format(variant)])
    if not fs_exists(svg):
        log_fatal("SVG not found: {}".format(svg))
    log_info("{} ({})".format(variant, svg))
    make_iconset(inkscape, svg, variant, output)
    make_favicon(inkscape, svg, variant, output)
    make_web(inkscape, svg, variant, output, web_size)
    make_emoji(inkscape, svg, variant, output, emoji_size)
    make_svg(svg, variant, output)

log_info("Done. Output in {}".format(output))
sys_exit(0)
