use super::{components, span};
use crossterm::style::{Attribute, ContentStyle};
use markdown::{ParseOptions, mdast};
use superconsole::Line;

pub struct Container<'a> {
    pub(crate) components: Vec<Box<dyn components::Component + 'a>>,
}

impl<'a> std::fmt::Debug for Container<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Container").finish()
    }
}

impl<'a> Default for Container<'a> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> Container<'a> {
    pub fn new() -> Self {
        Self {
            components: Vec::new(),
        }
    }

    pub fn add<ComponentType>(&mut self, component: ComponentType)
    where
        ComponentType: components::Component + 'a,
    {
        self.components.push(Box::new(component));
    }

    pub fn extend(&mut self, other: Self) {
        self.components.extend(other.components);
    }

    pub fn render(&self) -> Vec<superconsole::Line> {
        let mut lines = Vec::new();
        for component in &self.components {
            lines.extend(component.render());
        }
        lines
    }

    pub fn add_markdown(&mut self, markdown: &str) {
        match markdown::to_mdast(markdown, &ParseOptions::default()) {
            Ok(root) => self.add_markdown_node(&root),
            Err(_) => self.add(components::Paragraph::new(markdown.to_string())),
        }
    }

    fn add_markdown_node(&mut self, node: &mdast::Node) {
        match node {
            mdast::Node::Root(root) => {
                for child in &root.children {
                    self.add_markdown_node(child);
                }
            }
            mdast::Node::Heading(heading) => {
                let text = md_inline_text(&heading.children);
                let header = match heading.depth {
                    1 => components::Header::h1(text),
                    2 => components::Header::h2(text),
                    _ => components::Header::h3(text),
                };
                self.add(header);
            }
            mdast::Node::Paragraph(paragraph) => {
                self.add(components::Paragraph::from_line(md_inline_line(
                    &paragraph.children,
                )));
            }
            mdast::Node::List(list) => {
                self.add(md_list_component(list));
            }
            mdast::Node::Blockquote(quote) => {
                self.add(md_blockquote_component(quote));
            }
            mdast::Node::Code(code) => {
                let mut quote =
                    components::Blockquote::new().variant(components::Variant::Secondary);
                for line in md_code_lines(code) {
                    quote.push_line(line);
                }
                self.add(quote);
            }
            mdast::Node::Table(table) => {
                self.add(md_table_component(table));
            }
            mdast::Node::ThematicBreak(_) => {
                self.add(components::Divider::new());
            }
            _ => {
                let text = md_block_text(node);
                if !text.trim().is_empty() {
                    self.add(components::Paragraph::new(text));
                }
            }
        }
    }
}

fn md_table_component(table: &mdast::Table) -> components::Table {
    let mut headers = Vec::new();
    let mut rows = Vec::new();

    for (index, row_node) in table.children.iter().enumerate() {
        let mdast::Node::TableRow(row) = row_node else {
            continue;
        };

        let cells: Vec<String> = row
            .children
            .iter()
            .filter_map(|cell_node| {
                if let mdast::Node::TableCell(cell) = cell_node {
                    Some(md_inline_text(&cell.children))
                } else {
                    None
                }
            })
            .collect();

        if index == 0 {
            headers = cells;
        } else {
            rows.push(cells);
        }
    }

    let alignments = table
        .align
        .iter()
        .map(|align| match align {
            mdast::AlignKind::Left => components::Align::Left,
            mdast::AlignKind::Right => components::Align::Right,
            mdast::AlignKind::Center => components::Align::Center,
            mdast::AlignKind::None => components::Align::Left,
        })
        .collect::<Vec<_>>();

    components::Table::new()
        .headers(headers)
        .alignments(alignments)
        .rows(rows)
}

fn md_code_lines(code: &mdast::Code) -> Vec<Line> {
    if code.value.is_empty() {
        return vec![Line::default()];
    }

    code.value
        .lines()
        .map(|text| {
            let mut line = Line::default();
            line.push(span::code(text.to_string()));
            line
        })
        .collect()
}

fn md_blockquote_component(quote: &mdast::Blockquote) -> components::Blockquote {
    let mut blockquote = components::Blockquote::new();

    for child in &quote.children {
        for line in md_block_lines(child) {
            blockquote.push_line(line);
        }
    }

    blockquote
}

fn md_block_lines(node: &mdast::Node) -> Vec<Line> {
    match node {
        mdast::Node::Paragraph(paragraph) => vec![md_inline_line(&paragraph.children)],
        mdast::Node::Heading(heading) => vec![md_inline_line(&heading.children)],
        mdast::Node::Code(code) => md_code_lines(code),
        mdast::Node::List(list) => md_list_component(list).render(),
        mdast::Node::Table(table) => md_table_component(table).render(),
        mdast::Node::ThematicBreak(_) => vec![components::Divider::new().render()],
        _ => {
            let text = md_block_text(node);
            if text.trim().is_empty() {
                Vec::new()
            } else {
                vec![components::Paragraph::new(text).render()]
            }
        }
    }
}

fn md_list_component(list: &mdast::List) -> components::List {
    let mut rendered = if list.ordered {
        components::List::ordered()
    } else {
        components::List::unordered()
    };

    for item_node in &list.children {
        let mdast::Node::ListItem(item) = item_node else {
            continue;
        };

        let mut line = Line::default();
        let mut has_content = false;
        let mut nested_lists = Vec::new();

        if let Some(is_checked) = item.checked {
            let marker = if is_checked { "[x] " } else { "[ ] " };
            line.push(span::plain_text(marker.to_string()));
            has_content = true;
        }

        for child in &item.children {
            match child {
                mdast::Node::Paragraph(paragraph) => {
                    if has_content {
                        line.push(span::plain_text(" ".to_string()));
                    }
                    line.extend(md_inline_line(&paragraph.children).iter().cloned());
                    has_content = true;
                }
                mdast::Node::List(nested) => {
                    nested_lists.push(md_list_component(nested));
                }
                mdast::Node::Code(code) => {
                    if has_content {
                        line.push(span::plain_text(" ".to_string()));
                    }
                    line.push(span::code(code.value.clone()));
                    has_content = true;
                }
                _ => {
                    let text = md_block_text(child);
                    if !text.is_empty() {
                        if has_content {
                            line.push(span::plain_text(" ".to_string()));
                        }
                        line.push(span::plain_text(text));
                        has_content = true;
                    }
                }
            }
        }

        if has_content {
            rendered = rendered.item(line);
        } else {
            rendered = rendered.item("");
        }

        for nested in nested_lists {
            rendered = rendered.nested(nested);
        }
    }

    rendered
}

fn md_block_text(node: &mdast::Node) -> String {
    match node {
        mdast::Node::Text(text) => text.value.clone(),
        mdast::Node::Paragraph(paragraph) => md_inline_text(&paragraph.children),
        mdast::Node::Heading(heading) => md_inline_text(&heading.children),
        mdast::Node::Strong(strong) => md_inline_text(&strong.children),
        mdast::Node::Emphasis(emphasis) => md_inline_text(&emphasis.children),
        mdast::Node::Delete(delete) => md_inline_text(&delete.children),
        mdast::Node::InlineCode(code) => code.value.clone(),
        mdast::Node::InlineMath(math) => math.value.clone(),
        mdast::Node::Code(code) => code.value.clone(),
        mdast::Node::Math(math) => math.value.clone(),
        mdast::Node::Html(html) => html.value.clone(),
        mdast::Node::Link(link) => {
            let label = md_inline_text(&link.children);
            if label.is_empty() {
                link.url.clone()
            } else {
                label
            }
        }
        mdast::Node::Break(_) => " ".to_string(),
        _ => node.to_string(),
    }
}

fn md_inline_text(nodes: &[mdast::Node]) -> String {
    nodes.iter().map(md_block_text).collect::<Vec<_>>().join("")
}

fn md_style_with_attribute(style: ContentStyle, attribute: Attribute) -> ContentStyle {
    ContentStyle {
        foreground_color: style.foreground_color,
        background_color: style.background_color,
        underline_color: style.underline_color,
        attributes: style.attributes.with(attribute),
    }
}

fn md_inline_line(nodes: &[mdast::Node]) -> Line {
    let mut line = Line::default();
    md_push_inline_nodes(&mut line, nodes, components::default_style());
    line
}

fn md_push_inline_nodes(line: &mut Line, nodes: &[mdast::Node], base_style: ContentStyle) {
    for node in nodes {
        match node {
            mdast::Node::Text(text) => {
                line.push(span::styled_span(base_style, text.value.clone()));
            }
            mdast::Node::InlineCode(code) => {
                line.push(span::code(code.value.clone()));
            }
            mdast::Node::Strong(strong) => {
                md_push_inline_nodes(
                    line,
                    &strong.children,
                    md_style_with_attribute(base_style, Attribute::Bold),
                );
            }
            mdast::Node::Emphasis(emphasis) => {
                md_push_inline_nodes(
                    line,
                    &emphasis.children,
                    md_style_with_attribute(base_style, Attribute::Italic),
                );
            }
            mdast::Node::Delete(delete) => {
                line.push(span::del(md_inline_text(&delete.children)));
            }
            mdast::Node::Link(link) => {
                let label = md_inline_text(&link.children);
                let text = if label.is_empty() {
                    link.url.clone()
                } else {
                    label
                };

                let mut link_style = md_style_with_attribute(base_style, Attribute::Underlined);
                if link_style.foreground_color.is_none() {
                    link_style.foreground_color = components::info_style().foreground_color;
                }

                line.push(span::hyperlinked_span(link_style, text, link.url.clone()));
            }
            mdast::Node::Break(_) => {
                line.push(span::plain_text(" ".to_string()));
            }
            _ => {
                let text = md_block_text(node);
                if !text.is_empty() {
                    line.push(span::styled_span(base_style, text));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_markdown_renders_common_blocks() {
        let mut container = Container::new();
        container.add_markdown(
            "# Title\n\nParagraph with **bold** and `code`.\n\n- one\n- two\n\n---\n\n> note",
        );

        let rendered = container.render();
        let joined = rendered
            .iter()
            .map(|line| line.to_unstyled())
            .collect::<Vec<_>>()
            .join("\n");

        assert!(joined.contains("Title"));
        assert!(joined.contains("Paragraph with bold and code."));
        assert!(joined.contains("one"));
        assert!(joined.contains("two"));
        assert!(joined.contains("note"));
    }

    #[test]
    fn add_markdown_renders_table() {
        let mut container = Container::new();
        container.add_markdown("| Name | Age |\n| :-- | --: |\n| Ada | 42 |");

        let rendered = container.render();
        let joined = rendered
            .iter()
            .map(|line| line.to_unstyled())
            .collect::<Vec<_>>()
            .join("\n");

        assert!(joined.contains("Name"));
        assert!(joined.contains("Age"));
        assert!(joined.contains("Ada"));
    }
}
