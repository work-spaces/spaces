use crate::{evaluator, stardoc};

pub fn show(console: console::Console) -> anyhow::Result<()> {
    let markdown = utils::markdown::Markdown::new(console);

    let globals = evaluator::get_globals(evaluator::WithRules::Yes).build();

    let mut builtin_docs = Vec::new();

    for (name, doc) in globals.documentation().members {
        builtin_docs.push(stardoc::Docs {
            name: name.into(),
            doc,
        });
    }

    for doc in &builtin_docs {
        let content = doc.to_markdown();
        markdown.console.raw(&content)?;
    }

    markdown.console.write("\n")?;
    Ok(())
}
