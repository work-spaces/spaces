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

use crate::{evaluator, rules, workspace};
use anyhow::Context;
use anyhow_source_location::format_context;

use std::collections::HashMap;
use std::collections::HashSet;
use std::fs;
use std::io;
use std::iter;
use std::path::Path;

use itertools::Either;
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
pub(crate) struct EvalResult<T: Iterator<Item = EvalMessage>> {
    /// The diagnostic and error messages from evaluating a given piece of starlark code.
    pub messages: T,
    /// If the code is only parsed, not run, and there were no errors, this will contain
    /// the parsed module. Otherwise, it will be `None`
    pub ast: Option<AstModule>,
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

        for (name, item) in globals.documentation().members {
            let uri = url::Url::parse(&format!("starlark:/{name}.bzl"))
                .context(format_context!("Failed to parse URL for {name}"))?;
            let uri = LspUrl::try_from(uri.clone())
                .context(format_context!("Failed to convert to uri {uri:?}"))?;
            builtin_docs.insert(uri.clone(), item.render_as_code(&name));
            builtin_symbols.insert(name, uri);
        }

        eprintln!("New spaces context -- ");

        let ctx = Self {
            workspace,
            mode,
            dialect: evaluator::get_dialect(),
            globals,
            builtin_docs,
            builtin_symbols,
        };

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

    fn go(&self, file: &str, ast: AstModule) -> EvalResult<impl Iterator<Item = EvalMessage>> {
        eprintln!("go: {file}");
        let mut warnings = Either::Left(iter::empty());
        let mut errors = Either::Left(iter::empty());
        let final_ast = match self.mode {
            ContextMode::Check => {
                eprintln!("go: ContextMode::Check");
                warnings = Either::Right(self.check(file, &ast));
                Some(ast)
            }
            ContextMode::Run => {
                eprintln!("go: ContextMode::Run");
                errors = Either::Right(self.run(file, &ast).messages);
                Some(ast)
            }
        };
        EvalResult {
            messages: warnings.chain(errors),
            ast: final_ast,
        }
    }

    // Convert a result over iterator of EvalMessage, into an iterator of EvalMessage
    fn err(
        name: &str,
        result: starlark::Result<EvalResult<impl Iterator<Item = EvalMessage>>>,
    ) -> EvalResult<impl Iterator<Item = EvalMessage>> {
        match result {
            Err(e) => {
                eprintln!("{name}: Eval Result error: {e}");
                EvalResult {
                    messages: Either::Left(iter::once(EvalMessage::from_error(
                        Path::new(name),
                        &e,
                    ))),
                    ast: None,
                }
            }
            Ok(res) => {
                eprintln!("{name}: Eval Result Ok");
                EvalResult {
                    messages: Either::Right(res.messages),
                    ast: res.ast,
                }
            }
        }
    }

    pub(crate) fn file_with_contents(
        &self,
        filename: &str,
        content: String,
    ) -> EvalResult<impl Iterator<Item = EvalMessage>> {
        Self::err(
            filename,
            AstModule::parse(filename, content, &self.dialect)
                .map(|module| self.go(filename, module)),
        )
    }

    fn run(&self, file: &str, ast: &AstModule) -> EvalResult<impl Iterator<Item = EvalMessage>> {
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
            eval_result.map(|_| EvalResult {
                messages: iter::empty(),
                ast: Some(ast.clone()),
            }),
        )
    }

    fn is_suppressed(&self, _file: &str, _issue: &str) -> bool {
        false
    }

    fn check(&self, file: &str, ast: &AstModule) -> impl Iterator<Item = EvalMessage> {
        eprintln!("check file {file:?}");
        let mut globals = HashSet::new();
        for (name, _) in self.globals.iter() {
            globals.insert(name.to_owned());
        }
        let mut lints = ast.lint(Some(&globals));
        lints.retain(|issue| !self.is_suppressed(file, &issue.short_name));
        lints.into_iter().map(EvalMessage::from)
    }
}

impl LspContext for SpacesContext {
    fn parse_file_with_contents(&self, uri: &LspUrl, content: String) -> LspEvalResult {
        eprintln!("parse file with contents {uri:?}");
        match uri {
            LspUrl::File(uri) => {
                let EvalResult { messages, ast } =
                    self.file_with_contents(&uri.to_string_lossy(), content);
                LspEvalResult {
                    diagnostics: messages.map(eval_message_to_lsp_diagnostic).collect(),
                    ast,
                }
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
    ) -> anyhow::Result<LspUrl> {
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
                ResolveLoadError::WrongScheme("file://".to_owned(), current_file.clone()).into(),
            ),
        }
    }

    fn resolve_string_literal(
        &self,
        literal: &str,
        current_file: &LspUrl,
        workspace_root: Option<&Path>,
    ) -> anyhow::Result<Option<StringLiteralResult>> {
        eprintln!("resolve_string_literal");
        self.resolve_load(literal, current_file, workspace_root)
            .map(|url| {
                Some(StringLiteralResult {
                    url,
                    location_finder: None,
                })
            })
    }

    fn get_load_contents(&self, uri: &LspUrl) -> anyhow::Result<Option<String>> {
        eprintln!("get_load_contents: {uri:?}");
        match uri {
            LspUrl::File(path) => match path.is_absolute() {
                true => match fs::read_to_string(path) {
                    Ok(contents) => Ok(Some(contents)),
                    Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
                    Err(e) => Err(e.into()),
                },
                false => Err(ContextError::NotAbsolute(uri.clone()).into()),
            },
            LspUrl::Starlark(_) => Ok(self.builtin_docs.get(uri).cloned()),
            _ => Err(ContextError::WrongScheme("file://".to_owned(), uri.clone()).into()),
        }
    }

    fn get_url_for_global_symbol(
        &self,
        current_file: &LspUrl,
        symbol: &str,
    ) -> anyhow::Result<Option<LspUrl>> {
        eprintln!("get_url_for_global_symbol: Symbol: {symbol:?} Current: {current_file:?}");
        Ok(self.builtin_symbols.get(symbol).cloned())
    }

    fn render_as_load(
        &self,
        target: &LspUrl,
        current_file: &LspUrl,
        _workspace_root: Option<&Path>,
    ) -> anyhow::Result<String> {
        eprintln!("render_as_load: Target: {target:?} Current: {current_file:?}");
        Err(anyhow::anyhow!("Not yet implemented, render_as_load"))
    }

    fn get_environment(&self, _uri: &LspUrl) -> DocModule {
        DocModule::default()
    }
}
