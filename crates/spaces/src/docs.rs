use crate::{
    info,
    rules::{checkout, run},
};
use clap::ValueEnum;
use starstd::Function;

#[derive(ValueEnum, Clone, Copy, Debug)]
pub enum DocItem {
    Checkout,
    Run,
    Info,
    StarStd,
    Completions,
}

fn show_function(
    function: &Function,
    level: u8,
    markdown: &mut printer::markdown::Markdown,
) -> anyhow::Result<()> {
    markdown.heading(level, format!("{}()", function.name).as_str())?;

    markdown.code_block(
        "python",
        format!(
            "def {}({}) -> {}",
            function.name,
            function
                .args
                .iter()
                .map(|arg| arg.name)
                .collect::<Vec<&str>>()
                .join(", "),
            function.return_type
        )
        .as_str(),
    )?;

    markdown.printer.newline()?;

    markdown.paragraph(function.description)?;

    for arg in function.args {
        markdown.list_item(1, format!("`{}`: {}", arg.name, arg.description).as_str())?;
        for (key, value) in arg.dict {
            markdown.list_item(2, format!("`{}`: {}", key, value).as_str())?;
        }
    }

    markdown.printer.newline()?;

    if let Some(example) = function.example {
        markdown.printer.newline()?;
        markdown.bold("Example")?;
        markdown.printer.newline()?;
        markdown.code_block("python", example)?;
    }

    Ok(())
}

fn show_completions(markdown: &mut printer::markdown::Markdown) -> anyhow::Result<()> {
    markdown.heading(1, "Completions")?;
    Ok(())
}

fn show_run(level: u8, markdown: &mut printer::markdown::Markdown) -> anyhow::Result<()> {
    markdown.heading(level, "Run Rules")?;

    markdown.paragraph(r#"You use run rules to execute tasks in the workspace."#)?;

    for function in run::FUNCTIONS {
        show_function(function, level + 1, markdown)?;
    }

    Ok(())
}

fn show_checkout(level: u8, markdown: &mut printer::markdown::Markdown) -> anyhow::Result<()> {
    markdown.heading(level, "Checkout Rules")?;

    markdown.paragraph(
        r#"You use checkout rules to build a workspace.
You can fetch git repositories and archives. You can also add assets (local files)
to the workspace root folder (not under version control)."#,
    )?;

    for function in checkout::FUNCTIONS {
        show_function(function, level + 1, markdown)?;
    }

    Ok(())
}

fn show_info(level: u8, markdown: &mut printer::markdown::Markdown) -> anyhow::Result<()> {
    markdown.heading(level, "Info Functions")?;

    markdown.heading(level + 1, "Description")?;

    markdown.paragraph(
        r#"The `info` functions provide information about the workspace
during checkout and run. Info functions are executed immediately. They are not rule definitions."#,
    )?;

    markdown.heading(level + 1, "Functions")?;

    for function in info::FUNCTIONS {
        show_function(function, level + 2, markdown)?;
    }

    Ok(())
}

fn show_star_std(level: u8, markdown: &mut printer::markdown::Markdown) -> anyhow::Result<()> {
    markdown.heading(level, "Spaces Starlark Standard Functions")?;

    markdown.heading(level + 1, "Description")?;

    markdown.paragraph(
        r#"The spaces starlark standard library includes
functions for doing things like accessing the filesystem. The functions
in this library are executed immediately."#,
    )?;

    markdown.heading(level + 1, "`fs` Functions")?;

    for function in starstd::fs::FUNCTIONS {
        show_function(function, level + 2, markdown)?;
    }

    markdown.heading(level + 1, "`hash` Functions")?;

    for function in starstd::hash::FUNCTIONS {
        show_function(function, level + 2, markdown)?;
    }

    markdown.heading(level + 1, "`process` Functions")?;

    for function in starstd::process::FUNCTIONS {
        show_function(function, level + 2, markdown)?;
    }

    markdown.heading(level + 1, "`script` Functions")?;

    for function in starstd::script::FUNCTIONS {
        show_function(function, level + 2, markdown)?;
    }

    Ok(())
}

fn show_doc_item(
    markdown: &mut printer::markdown::Markdown,
    doc_item: DocItem,
) -> anyhow::Result<()> {
    match doc_item {
        DocItem::Checkout => show_checkout(1, markdown)?,
        DocItem::Run => show_run(1, markdown)?,
        DocItem::Completions => show_completions(markdown)?,
        DocItem::Info => show_info(1, markdown)?,
        DocItem::StarStd => show_star_std(1, markdown)?,
    }
    Ok(())
}

fn show_all(markdown: &mut printer::markdown::Markdown) -> anyhow::Result<()> {
    markdown.heading(1, "Spaces API Documentation")?;
    markdown.printer.newline()?;

    show_info(2, markdown)?;
    show_star_std(2, markdown)?;
    show_checkout(2, markdown)?;
    show_run(2, markdown)?;

    Ok(())
}

pub fn show(printer: &mut printer::Printer, doc_item: Option<DocItem>) -> anyhow::Result<()> {
    let mut markdown = printer::markdown::Markdown::new(printer);

    if let Some(doc_item) = doc_item {
        show_doc_item(&mut markdown, doc_item)?;
    } else {
        show_all(&mut markdown)?;
    }

    markdown.printer.newline()?;
    Ok(())
}
