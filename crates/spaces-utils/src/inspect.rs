use std::collections::HashSet;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct Options {
    pub target: Option<Arc<str>>,
    pub filter_globs: HashSet<Arc<str>>,
    pub has_help: bool,
    pub markdown: Option<Arc<str>>,
    pub stardoc: Option<Arc<str>>,
    pub fuzzy: Option<Arc<str>>,
    pub details: bool,
    pub json: bool,
}
