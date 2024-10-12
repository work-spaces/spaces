use clap::ValueEnum;

#[derive(ValueEnum, Clone, Copy, Debug)]
pub enum DocItem {
    Checkout,
    Run,
    Completions,
}

fn show_completions(printer: &mut printer::Printer) -> anyhow::Result<()> {
    let heading = printer::Heading::new(printer,   "Completions")?;
    heading.printer.newline()?;
    Ok(())
}

fn show_run(printer: &mut printer::Printer) -> anyhow::Result<()> {
    printer.info("Run", &"")?;
    Ok(())
}

fn show_checkout(printer: &mut printer::Printer) -> anyhow::Result<()> {
    printer.info("Checkout", &"")?;
    Ok(())
}

pub fn show(printer: &mut printer::Printer, doc_item: DocItem) -> anyhow::Result<()> {
    match doc_item {
        DocItem::Checkout => {
            show_checkout(printer)?;
        }
        DocItem::Run => {
            show_run(printer)?;
        }
        DocItem::Completions => {
            show_completions(printer)?;
        }
    }
    Ok(())
}
