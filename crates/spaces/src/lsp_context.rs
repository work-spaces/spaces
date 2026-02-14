/*
 * Copyright 2019 The Starlark in Rust Authors.
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     https://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

use crate::label;
use crate::{evaluator, rules, workspace};
use anyhow::Context;
use anyhow_source_location::format_context;
use starlark::syntax::DialectTypes;
use std::sync::Arc;

use std::collections::HashMap;
use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::Path;

use starlark::analysis::AstModuleLint;
use starlark::docs::DocModule;
use starlark::environment::FrozenModule;
use starlark::environment::Globals;
use starlark::errors::EvalMessage;
use starlark::eval::FileLoader;
use starlark::syntax::AstModule;
use starlark::syntax::Dialect;
use starlark_lsp::error::eval_message_to_lsp_diagnostic;
use starlark_lsp::server::LspContext;
use starlark_lsp::server::LspEvalResult;
use starlark_lsp::server::LspUrl;
use starlark_lsp::server::StringLiteralResult;

#[derive(Debug)]
pub(crate) enum ContextMode {
    #[allow(unused)]
    Check,
    Run,
}

#[derive(Debug, thiserror::Error)]
enum ContextError {
    /// The provided Url was not absolute and it needs to be.
    #[error("Path for URL `{}` was not absolute", .0)]
    NotAbsolute(LspUrl),
    /// The scheme provided was not correct or supported.
    #[error("Url `{}` was expected to be of type `{}`", .1, .0)]
    WrongScheme(String, LspUrl),
}

#[derive(Debug)]
pub(crate) struct SpacesContext {
    pub(crate) mode: ContextMode,
    pub(crate) dialect: Dialect,
    pub(crate) globals: Globals,
    pub(crate) builtin_docs: HashMap<LspUrl, String>,
    pub(crate) builtin_symbols: HashMap<String, LspUrl>,
    pub(crate) workspace: workspace::WorkspaceArc,
}

impl FileLoader for SpacesContext {
    fn load(&self, path: &str) -> starlark::Result<FrozenModule> {
        eprintln!("Load path {path:?}");
        self.load_path(Path::new(path))
    }
}

/// The outcome of evaluating (checking, parsing or running) given starlark code.
pub(crate) struct EvalResult {
    /// The diagnostic and error messages from evaluating a given piece of starlark code.
    pub lsp_eval_result: LspEvalResult,
    pub module: Option<FrozenModule>,
}

/// Errors when [`LspContext::resolve_load()`] cannot resolve a given path.
#[derive(thiserror::Error, Debug)]
enum ResolveLoadError {
    /// The scheme provided was not correct or supported.
    #[error("Url `{}` was expected to be of type `{}`", .1, .0)]
    WrongScheme(String, LspUrl),
}

impl SpacesContext {
    pub(crate) fn new(
        workspace: workspace::WorkspaceArc,
        mode: ContextMode,
    ) -> anyhow::Result<Self> {
        let mut builtin_docs: HashMap<LspUrl, String> = HashMap::new();
        let mut builtin_symbols: HashMap<String, LspUrl> = HashMap::new();
        let globals = evaluator::get_globals(evaluator::WithRules::Yes).build();
        let workspace_path = workspace.read().get_absolute_path();

        for (name, item) in globals.documentation().members {
            let path = format!("{workspace_path}/.spaces/builtins/globals_{name}.bzl");
            let as_code = item.render_as_code(&name);
            let uri = LspUrl::Starlark(path.into());
            builtin_docs.insert(uri.clone(), as_code);
            builtin_symbols.insert(name, uri);
        }

        let mut dialect = evaluator::get_dialect();

        dialect.enable_positional_only_arguments = true;
        dialect.enable_types = DialectTypes::ParseOnly;
        dialect.enable_keyword_only_arguments = true;

        let ctx = Self {
            workspace,
            mode,
            dialect,
            globals,
            builtin_docs,
            builtin_symbols,
        };

        eprintln!("-- New spaces context -- ");

        Ok(ctx)
    }

    fn load_path(&self, path: &Path) -> starlark::Result<FrozenModule> {
        eprintln!("Load path {path:?}");
        let workspace_path = self.workspace.read().get_absolute_path();

        let content =
            fs::read_to_string(path).context(format_context!("Failed to read {path:?}"))?;

        let frozen_module = evaluator::evaluate_module(
            None,
            workspace_path,
            path.to_string_lossy().into(),
            content,
            evaluator::WithRules::Yes,
        )?;

        Ok(frozen_module)
    }

    fn go(&self, file: &str, ast: AstModule) -> EvalResult {
        eprintln!("go: {file}");
        match self.mode {
            ContextMode::Check => {
                eprintln!("go: ContextMode::Check");
                self.check(file, ast)
            }
            ContextMode::Run => {
                eprintln!("go: ContextMode::Run");
                self.run(file, ast)
            }
        }
    }

    // Convert a result over iterator of EvalMessage, into an iterator of EvalMessage
    fn err(name: &str, result: starlark::Result<EvalResult>) -> EvalResult {
        match result {
            Err(e) => {
                eprintln!("{name}: Eval Result error: {e}");
                let message = EvalMessage::from_error(std::path::Path::new(name), &e);
                EvalResult {
                    lsp_eval_result: LspEvalResult {
                        diagnostics: vec![eval_message_to_lsp_diagnostic(message)],
                        ast: None,
                    },
                    module: None,
                }
            }
            Ok(res) => {
                eprintln!("{name}: Eval Result Ok");
                EvalResult {
                    lsp_eval_result: res.lsp_eval_result,
                    module: res.module,
                }
            }
        }
    }

    pub(crate) fn file_with_contents(&self, filename: &str, content: String) -> EvalResult {
        Self::err(
            filename,
            AstModule::parse(filename, content, &self.dialect)
                .map(|module| self.go(filename, module)),
        )
    }

    fn run(&self, file: &str, ast: AstModule) -> EvalResult {
        let workspace_path = self.workspace.read().get_absolute_path();
        let name = file
            .strip_prefix(format!("{workspace_path}").as_str())
            .unwrap_or(file);

        let name = name.trim_start_matches("/");

        rules::set_latest_starlark_module(name.into());
        eprintln!("run: {name}");

        let eval_result = evaluator::evaluate_ast(
            ast.clone(),
            name.into(),
            None,
            workspace_path.clone(),
            evaluator::WithRules::Yes,
        );

        eprintln!("run: {name} - got result");

        Self::err(
            name,
            eval_result.map(|result| EvalResult {
                lsp_eval_result: LspEvalResult {
                    diagnostics: vec![],
                    ast: Some(ast),
                },
                module: Some(result),
            }),
        )
    }

    fn is_suppressed(&self, _file: &str, _issue: &str) -> bool {
        false
    }

    fn check(&self, file: &str, ast: AstModule) -> EvalResult {
        eprintln!("check file {file:?}");
        let mut globals = HashSet::new();
        for (name, _) in self.globals.iter() {
            globals.insert(name.to_owned());
        }
        let mut lints = ast.lint(Some(&globals));
        lints.retain(|issue| !self.is_suppressed(file, &issue.short_name));
        EvalResult {
            lsp_eval_result: LspEvalResult {
                diagnostics: vec![],
                ast: Some(ast),
            },
            module: None,
        }
    }

    fn find_target(ast: &AstModule, target: String) -> Option<starlark_syntax::codemap::Span> {
        let mut ret = None;

        use starlark::syntax::ast::Argument;
        use starlark::syntax::ast::Expr;
        use starlark::syntax::ast::{AstExpr, AstLiteral};
        use starlark_syntax::codemap::Span;
        use starlark_syntax::codemap::Spanned;

        fn visit_expr(ret: &mut Option<Span>, name: &str, node: &AstExpr) {
            if ret.is_some() {
                return;
            }

            match node {
                Spanned {
                    node: Expr::Call(identifier, arguments),
                    ..
                } => {
                    if matches!(&identifier.node, Expr::Identifier(_) | Expr::Dot(_, _)) {
                        let found =
                            arguments
                                .args
                                .iter()
                                .enumerate()
                                .find_map(|(index, argument)| match &argument.node {
                                    Argument::Named(
                                        arg_name,
                                        Spanned {
                                            node: Expr::Literal(AstLiteral::String(s)),
                                            ..
                                        },
                                    ) if arg_name.node == "name" && s.node == name => {
                                        Some(identifier.span)
                                    }
                                    Argument::Positional(Spanned {
                                        node: Expr::Literal(AstLiteral::String(s)),
                                        ..
                                    }) if index == 0 && s.node == name => Some(identifier.span),
                                    _ => None,
                                });
                        if found.is_some() {
                            *ret = found;
                        }
                    }
                }
                _ => node.visit_expr(|x| visit_expr(ret, name, x)),
            }
        }

        ast.statement()
            .visit_expr(|x| visit_expr(&mut ret, target.as_str(), x));
        eprintln!("Find function call with name {target} -> {ret:?}");
        ret
    }

    fn get_source_file_absolute_path(&self, source_file: &str) -> Arc<std::path::Path> {
        let workspace_path = self.workspace.read().get_absolute_path();
        let path = std::path::Path::new(workspace_path.as_ref()).join(source_file);
        path.into()
    }
}

impl LspContext for SpacesContext {
    fn parse_file_with_contents(&self, uri: &LspUrl, content: String) -> LspEvalResult {
        eprintln!("parse file with contents {uri:?}");
        match uri {
            LspUrl::File(uri) => {
                let EvalResult {
                    lsp_eval_result,
                    #[allow(unused)]
                    module,
                } = self.file_with_contents(&uri.to_string_lossy(), content);
                lsp_eval_result
            }
            LspUrl::Starlark(name) => {
                let EvalResult {
                    lsp_eval_result,
                    #[allow(unused)]
                    module,
                } = self.file_with_contents(&name.to_string_lossy(), content);
                lsp_eval_result
            }
            _ => {
                eprintln!("Default LspEvalResult on Non-file URI");
                LspEvalResult::default()
            }
        }
    }

    fn resolve_load(
        &self,
        path: &str,
        current_file: &LspUrl,
        _workspace_root: Option<&Path>,
    ) -> anyhow::Result<LspUrl, String> {
        eprintln!("resolve load for {path}");
        match current_file {
            LspUrl::File(current_file_path) => {
                let workspace_path_str = self.workspace.read().get_absolute_path();
                let workspace_path = std::path::Path::new(workspace_path_str.as_ref());
                let resolved_path = if let Some(path_in_workspace) = path.strip_prefix("//") {
                    eprintln!("join to workspace {}", workspace_path.display());
                    workspace_path.join(path_in_workspace)
                } else {
                    let parent = current_file_path.parent().unwrap_or(workspace_path);
                    eprintln!("join to parent {}", parent.display());
                    parent.join(path)
                };

                eprintln!("Current file path is {}", current_file_path.display());
                eprintln!("Resolved file path is {}", resolved_path.display());
                Ok(LspUrl::File(resolved_path))
            }
            _ => Err(
                ResolveLoadError::WrongScheme("file://".to_owned(), current_file.clone())
                    .to_string(),
            ),
        }
    }

    fn resolve_string_literal(
        &self,
        literal: &str,
        current_file: &LspUrl,
        _workspace_root: Option<&Path>,
    ) -> anyhow::Result<Option<StringLiteralResult>, String> {
        eprintln!("resolve_string_literal {literal} from {current_file}");

        // the string literal value could be a build target
        // For example, "//spaces:install_dev" could be listed as a dep and this
        // will allow the LSP to find where the target is defined

        let source_file = {
            let source = label::get_source_from_label(literal);
            if source.is_empty() {
                if literal.starts_with(':') {
                    eprintln!("Match leading : -> {current_file:?}");
                    match current_file {
                        LspUrl::File(path) => Some(path.to_string_lossy().into()),
                        _ => None,
                    }
                } else {
                    None
                }
            } else {
                Some(source)
            }
        };
        if let Some(source_file) = source_file {
            let source_file_path = self.get_source_file_absolute_path(source_file.as_str());
            if source_file_path.exists() {
                eprintln!("This is a rule label: {source_file}");
                let name = label::get_rule_name_from_label(literal).to_owned();
                return Ok(Some(StringLiteralResult {
                    url: LspUrl::File(source_file_path.as_ref().into()),
                    location_finder: Some(Box::new(|ast| Ok(Self::find_target(ast, name)))),
                }));
            }
        }

        Ok(None)
    }

    fn get_load_contents(&self, uri: &LspUrl) -> anyhow::Result<Option<String>, String> {
        eprintln!("get_load_contents: {uri:?}");
        match uri {
            LspUrl::File(path) => match path.is_absolute() {
                true => match fs::read_to_string(path) {
                    Ok(contents) => Ok(Some(contents)),
                    Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
                    Err(e) => Err(format!("{e}")),
                },
                false => Err(ContextError::NotAbsolute(uri.clone()).to_string()),
            },
            LspUrl::Starlark(starlark) => {
                eprintln!("get starlark: {}", starlark.display());
                if let Some(docs) = self.builtin_docs.get(uri).cloned() {
                    eprintln!("got some docs: {docs}");
                    Ok(Some(docs))
                } else {
                    Ok(None)
                }
            }
            _ => Err(ContextError::WrongScheme("file://".to_owned(), uri.clone()).to_string()),
        }
    }

    fn get_url_for_global_symbol(
        &self,
        current_file: &LspUrl,
        symbol: &str,
    ) -> anyhow::Result<Option<LspUrl>, String> {
        eprintln!("get_url_for_global_symbol: Symbol: {symbol:?} Current: {current_file:?}");
        let url = self.builtin_symbols.get(symbol).cloned();
        if let Some(url) = url {
            eprintln!("url for {symbol} is {url}");
            if !url.path().exists() {
                if let Some(parent) = url.path().parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                if let Some(content) = self.builtin_docs.get(&url) {
                    let _ = std::fs::write(url.path(), content);
                }
            }
            Ok(Some(url))
        } else {
            Ok(None)
        }
    }

    fn render_as_load(
        &self,
        target: &LspUrl,
        current_file: &LspUrl,
        _workspace_root: Option<&Path>,
    ) -> anyhow::Result<String, String> {
        eprintln!("render_as_load: Target: {target:?} Current: {current_file:?}");
        Err("Not yet implemented, render_as_load".to_string())
    }

    fn get_environment(&self, _uri: &LspUrl) -> DocModule {
        DocModule::default()
    }
}
