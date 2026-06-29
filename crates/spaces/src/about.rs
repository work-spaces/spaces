use crate::singleton;
use anyhow::Context;
use anyhow_source_location::format_context;
use console::bootstrap::{
    Banner, Container, DescriptionList, Divider, DividerStyle, Link, Paragraph, Variant, Width,
    align_center,
    typography::{TypographyMode, typography_mode},
};

/// ASCII rendering of the spaces honeycomb logo (7-hex flower).
const LOGO_ASCII: &[&str] = &[
    r"    __    __    ",
    r"   /  \__/  \   ",
    r"   \__/  \__/   ",
    r"   /  \__/  \   ",
    r"   \__/  \__/   ",
    r"      \__/      ",
];

/// Unicode rendering of the spaces honeycomb logo (hexagonal cluster).
const LOGO_UNICODE: &[&str] = &[
    "  ⬢ ⬢ ⬢  ",
    " ⬢ ⬡ ⬡ ⬢ ",
    "⬢ ⬡ ⬡ ⬡ ⬢",
    " ⬢ ⬡ ⬡ ⬢ ",
    "  ⬢ ⬢ ⬢  ",
];

pub fn show(console: console::Console) -> anyhow::Result<()> {
    let version = singleton::get_spaces_version()
        .context(format_context!("Failed to determine spaces version"))?;

    let logo = match typography_mode() {
        TypographyMode::Ascii => LOGO_ASCII,
        TypographyMode::Unicode | TypographyMode::NerdFonts => LOGO_UNICODE,
    };

    let mut container = Container::new();
    container.add(console::bootstrap::VerticalSpacer::new(1));
    container.add(Banner::new("About Spaces").width(Width::Medium));
    for line in logo {
        container.add(align_center(*line, Width::Medium).variant(Variant::Warning));
    }
    container.add(console::bootstrap::VerticalSpacer::new(1));
    container.add(
        Paragraph::new("Reproducible PolyRepo Workspaces with Starlark Build Rules")
            .variant(Variant::Light),
    );

    let details = DescriptionList::new()
        .compact(true)
        .item("version", version.to_string())
        .item(
            "github",
            Link::new("https://github.com/work-spaces/spaces")
                .url("https://github.com/work-spaces/spaces")
                .render(),
        )
        .item(
            "docs",
            Link::new("https://work-spaces.github.io")
                .url("https://work-spaces.github.io/")
                .render(),
        );

    container.add(details);
    container.add(
        Divider::new()
            .style(DividerStyle::Double)
            .width(Width::Medium),
    );
    container.add(console::bootstrap::VerticalSpacer::new(1));

    console.emit_container(&container);
    Ok(())
}
