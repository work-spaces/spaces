use printer::markdown;
use starlark::docs::{DocItem, DocMember, DocModule, DocString};
use std::collections::HashMap;
use std::sync::Arc;

fn doc_string_to_markdown(doc_string: &Option<DocString>) -> Arc<str> {
    let mut result = String::new();
    if let Some(doc_string) = doc_string.as_ref() {
        result.push_str(markdown::paragraph(&doc_string.summary).as_str());

        if let Some(details) = &doc_string.details {
            result.push_str(markdown::paragraph(details).as_str());
        }
    }

    result.into()
}

fn doc_param_to_markdown(param: &starlark::docs::DocParam) -> Arc<str> {
    let summary = param.get_doc_summary().unwrap_or("<not provided>");
    format!("**{}** - {summary}", param.name).into()
}

#[derive(Clone, Debug)]
pub struct Docs {
    pub name: Arc<str>,
    pub doc: DocItem,
}

impl Docs {
    pub fn to_markdown(&self) -> Arc<str> {
        let mut result = String::new();
        result.push_str(markdown::heading(3, &self.name).as_str());

        match &self.doc {
            DocItem::Member(member) => match member {
                DocMember::Function(func) => {
                    result.push_str(doc_string_to_markdown(&func.docs).as_ref());
                    let md_params = func.params.pos_or_named.iter().map(doc_param_to_markdown).collect();
                    result.push_str(markdown::list(md_params).as_str().as_ref());
                }
                DocMember::Property(property) => {
                    if let Some(doc_string) = &property.docs {
                        if let Some(details) = &doc_string.details {
                            result.push_str(markdown::paragraph(details).as_str());
                        }
                    }
                }
            },
            DocItem::Module(module) => {}
            DocItem::Type(ty) => {}
        }

        if let Some(summary) = &self.doc.get_doc_summary() {
            result.push_str(markdown::paragraph(summary).as_str());
        }

        if let Some(doc_string) = &self.doc.get_doc_string() {
            if let Some(details) = &doc_string.details {
                result.push_str(markdown::paragraph(details).as_str());
            }
        }

        result.into()
    }
}

#[derive(Clone, Debug)]
pub struct StarDoc {
    pub entries: HashMap<Arc<str>, Vec<Docs>>,
}

impl Default for StarDoc {
    fn default() -> Self {
        StarDoc {
            entries: HashMap::new(),
        }
    }
}

impl StarDoc {
    pub fn insert(&mut self, name: Arc<str>, items: Vec<(Arc<str>, DocItem)>) {
        let docs = items
            .into_iter()
            .map(|(name, doc)| Docs { name, doc })
            .collect();
        if !self.entries.contains_key(&name) {
            self.entries.insert(name, docs);
        }
    }
}
