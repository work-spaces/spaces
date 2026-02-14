use anyhow::Context;
use anyhow_source_location::format_context;
use printer::markdown;
use starlark::docs::{DocItem, DocMember, DocString};
use std::collections::HashMap;
use std::sync::Arc;

fn doc_string_to_markdown(label: Option<&str>, doc_string: Option<&DocString>) -> Arc<str> {
    let mut result = String::new();
    if let Some(doc_string) = doc_string.as_ref() {
        if let Some(label) = label {
            result.push_str(markdown::bold(label).as_str());
            result.push_str("\n\n");
        }
        result.push_str(markdown::paragraph(&doc_string.summary).as_str());

        if let Some(details) = &doc_string.details {
            result.push_str(markdown::paragraph(details).as_str());
        }
    }

    result.into()
}

fn doc_param_to_markdown(param: &starlark::docs::DocParam) -> Arc<str> {
    let summary = param.get_doc_summary().unwrap_or("<not provided>");
    let param_type = param
        .typ
        .as_name()
        .map(|e| format!(": {e}"))
        .unwrap_or("".to_string());
    let name = format!("{}{param_type}", param.name);

    format!("`{name}`: {summary}").into()
}

#[derive(Clone, Debug)]
pub struct Docs {
    pub name: Arc<str>,
    pub doc: DocItem,
}

impl Docs {
    pub fn to_markdown(&self) -> Arc<str> {
        let mut result = String::new();

        match &self.doc {
            DocItem::Member(member) => match member {
                DocMember::Function(func) => {
                    result.push_str(
                        markdown::heading(3, format!("{}()", self.name).as_str()).as_str(),
                    );
                    let mut def_block = format!(
                        "def {}({})",
                        self.name,
                        func.params
                            .pos_or_named
                            .iter()
                            .map(|p| p.name.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                    if let Some(ret_ty) = &func.ret.typ.as_name() {
                        def_block.push_str(" -> ");
                        def_block.push_str(ret_ty);
                    }
                    result.push_str(markdown::code_block("python", def_block.as_str()).as_str());
                    result.push_str(doc_string_to_markdown(None, func.docs.as_ref()).as_ref());

                    let mut md_params: Vec<Arc<str>> = func
                        .params
                        .pos_or_named
                        .iter()
                        .map(doc_param_to_markdown)
                        .collect();

                    md_params.extend(func.params.named_only.iter().map(doc_param_to_markdown));

                    if !md_params.is_empty() {
                        result.push_str(markdown::bold("Args").as_str());
                        result.push_str("\n\n");
                        result.push_str(markdown::list(md_params).as_str().as_ref());
                    }

                    result.push_str(&doc_string_to_markdown(
                        Some("Returns"),
                        func.ret.docs.as_ref(),
                    ));
                }
                DocMember::Property(_property) => {}
            },
            DocItem::Module(module) => {
                result.push_str(markdown::heading(2, &self.name).as_str());
                result.push_str(&doc_string_to_markdown(Some("Docs"), module.docs.as_ref()));
                for (name, doc) in module.members.iter() {
                    let doc = Docs {
                        name: name.to_owned().into(),
                        doc: doc.clone(),
                    };
                    result.push_str(&doc.to_markdown());
                }
                result.push_str(markdown::hline());
            }
            DocItem::Type(_ty) => {}
        }

        result.into()
    }
}

#[derive(Clone, Debug, Default)]
pub struct StarDoc {
    pub entries: HashMap<Arc<str>, Vec<Docs>>,
}

impl StarDoc {
    pub fn insert(&mut self, name: Arc<str>, items: Vec<(Arc<str>, DocItem)>) {
        let docs = items
            .into_iter()
            .map(|(name, doc)| Docs { name, doc })
            .collect();
        self.entries.entry(name).or_insert(docs);
    }

    pub fn generate(&self, base_path: &str) -> anyhow::Result<()> {
        let stardoc_path = std::path::Path::new(base_path);
        for (name, doc_items) in self.entries.iter() {
            let name = name.strip_prefix("//").unwrap_or(name);
            let relative_path = std::path::Path::new(name).with_extension("md");
            // strip the .star suffix
            let output_file = stardoc_path.join(relative_path);
            // append .md suffix
            if let Some(parent) = output_file.parent() {
                std::fs::create_dir_all(parent)
                    .context(format_context!("Failed to create directory {parent:?}"))?;

                let index_path = parent.join("_index.md");
                if !index_path.exists() {
                    std::fs::write(index_path.clone(), "")
                        .context(format_context!("Failed to index file file {index_path:?}"))?;
                }
            }
            let mut content = String::new();
            let mut doc_items_by_name = doc_items.clone();
            doc_items_by_name.sort_by(|a, b| a.name.cmp(&b.name));
            for item in doc_items_by_name {
                content.push_str(item.to_markdown().as_ref());
            }
            std::fs::write(output_file.clone(), content)
                .context(format_context!("Failed to write file {output_file:?}"))?;
        }
        Ok(())
    }
}
