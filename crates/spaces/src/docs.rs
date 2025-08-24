use crate::builtins::{checkout, info, run, workspace};
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
    markdown.heading(level, function.name)?;

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
            markdown.list_item(2, format!("`{key}`: {value}").as_str())?;
        }
    }

    markdown.printer.newline()?;

    if let Some(example) = function.example {
        markdown.printer.newline()?;
        markdown.bold("Example")?;
        markdown.printer.newline()?;
        markdown.code_block("python", example)?;
        markdown.printer.newline()?;
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
    show_sorted_functions(run::FUNCTIONS, level + 2, markdown)?;

    Ok(())
}

fn show_checkout(level: u8, markdown: &mut printer::markdown::Markdown) -> anyhow::Result<()> {
    markdown.heading(level, "Checkout Rules")?;

    markdown.paragraph(
        r#"You use checkout rules to build a workspace.
You can fetch git repositories and archives. You can also add assets (local files)
to the workspace root folder (not under version control)."#,
    )?;

    show_sorted_functions(checkout::FUNCTIONS, level + 2, markdown)?;
    Ok(())
}

fn show_info(level: u8, markdown: &mut printer::markdown::Markdown) -> anyhow::Result<()> {
    markdown.heading(level, "Info Functions")?;

    markdown.heading(level + 1, "Description")?;

    markdown.paragraph(
        r#"The `info` functions provide information about spaces and the platform.
Info functions are executed immediately. They are not rule definitions."#,
    )?;

    markdown.heading(level + 1, "Functions")?;
    show_sorted_functions(info::FUNCTIONS, level + 2, markdown)?;

    Ok(())
}

fn show_workspace(level: u8, markdown: &mut printer::markdown::Markdown) -> anyhow::Result<()> {
    markdown.heading(level, "Workspace Functions")?;

    markdown.heading(level + 1, "Description")?;

    markdown.paragraph(
        r#"The `workspace` functions provide information about the workspace
during checkout and run. Workspaces functions are executed immediately. They are not rule definitions."#,
    )?;

    markdown.heading(level + 1, "Functions")?;
    show_sorted_functions(workspace::FUNCTIONS, level + 2, markdown)?;

    Ok(())
}

fn show_sorted_functions(
    functions: &[Function],
    level: u8,
    markdown: &mut printer::markdown::Markdown,
) -> anyhow::Result<()> {
    let mut sorted_functions = Vec::new();
    sorted_functions.extend_from_slice(functions);

    sorted_functions.sort_by(|a, b| a.name.cmp(b.name));

    for function in sorted_functions {
        show_function(&function, level, markdown)?;
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
    show_sorted_functions(starstd::fs::FUNCTIONS, level + 2, markdown)?;

    markdown.heading(level + 1, "`hash` Functions")?;
    show_sorted_functions(starstd::hash::FUNCTIONS, level + 2, markdown)?;

    markdown.heading(level + 1, "`json` Functions")?;
    show_sorted_functions(starstd::json::FUNCTIONS, level + 2, markdown)?;

    markdown.heading(level + 1, "`process` Functions")?;
    show_sorted_functions(starstd::process::FUNCTIONS, level + 2, markdown)?;

    markdown.heading(level + 1, "`script` Functions")?;
    show_sorted_functions(starstd::script::FUNCTIONS, level + 2, markdown)?;

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
    markdown.heading(1, "Spaces Built-in Functions API Documentation")?;
    markdown.printer.newline()?;
    markdown.heading(2, "Table of Contents")?;

    markdown.list(vec![
        "[Checkout Rules](#checkout-rules)".into(),
        "[Run Rules](#run-rules)".into(),
        "[Info Functions](#info-functions)".into(),
        "[Spaces Starlark Standard Functions](#spaces-starlark-standard-functions)".into(),
    ])?;

    markdown.printer.newline()?;

    markdown.heading(2, "Rule Options")?;
    markdown.heading(3, "Platforms")?;

    markdown.paragraph(
        r#"All rules can be run on a list of platforms (default is all). Specify a platform as:"#,
    )?;

    markdown.list(vec![
        "`macos-aarch64`".into(),
        "`macos-x86_64`".into(),
        "`linux-aarch64`".into(),
        "`linux-x86_64`".into(),
        "`windows-aarch64`".into(),
        "`windows-x86_64`".into(),
    ])?;
    markdown.printer.newline()?;

    markdown.heading(3, "Rule Types")?;

    markdown.list(vec![
        "`Checkout`: Assigned by default to all checkout rules".into(),
        "`Setup`: Assign to a run rule to make all other run rules depend on it".into(),
        "`Run`: Runs when running `//:all`".into(),
        "`Optional`: Runs if specifical executed or depended on".into(),
    ])?;

    markdown.printer.newline()?;

    markdown.paragraph("Inspect (evaluate) scripts without executing rules:")?;

    markdown.code_block(
        "sh",
        r#"# show rules with a `help` entry
spaces inspect --has-help
# show all rules
spaces --verbosity=message inspect
# show all rules with all details
spaces --verbosity=debug inspect
"#,
    )?;

    markdown.printer.newline()?;

    show_checkout(2, markdown)?;
    markdown.printer.newline()?;
    show_run(2, markdown)?;
    markdown.printer.newline()?;
    show_workspace(2, markdown)?;
    markdown.printer.newline()?;
    show_info(2, markdown)?;
    markdown.printer.newline()?;
    show_star_std(2, markdown)?;
    markdown.printer.newline()?;

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
