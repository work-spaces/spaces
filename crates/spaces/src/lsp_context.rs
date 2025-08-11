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

use crate::{evaluator, singleton};
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use std::sync::Arc;

use std::collections::HashMap;
use std::collections::HashSet;
use std::fs;
use std::io;
use std::iter;
use std::path::Path;
use std::path::PathBuf;

use itertools::Either;
use lsp_types::Url;
use starlark::analysis::AstModuleLint;
use starlark::docs::DocModule;
use starlark::environment::FrozenModule;
use starlark::environment::Globals;
use starlark::environment::Module;
use starlark::errors::EvalMessage;
use starlark::eval::FileLoader;
use starlark::eval::{Evaluator, ReturnFileLoader};
use starlark::syntax::AstModule;
use starlark::syntax::Dialect;
use starlark::StarlarkResultExt;
use starlark_lsp::error::eval_message_to_lsp_diagnostic;
use starlark_lsp::server::LspContext;
use starlark_lsp::server::LspEvalResult;
use starlark_lsp::server::LspUrl;
use starlark_lsp::server::StringLiteralResult;

#[derive(Debug)]
pub(crate) enum ContextMode {
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
    pub(crate) print_non_none: bool,
    pub(crate) prelude: Vec<FrozenModule>,
    pub(crate) module: Option<Module>,
    pub(crate) dialect: Dialect,
    pub(crate) globals: Globals,
    pub(crate) builtin_docs: HashMap<LspUrl, String>,
    pub(crate) builtin_symbols: HashMap<String, LspUrl>,
    pub(crate) current_directory: Option<PathBuf>,
    pub(crate) workspace_path: Arc<str>,
}

impl FileLoader for SpacesContext {
    fn load(&self, path: &str) -> starlark::Result<FrozenModule> {
        eprintln!("Load path {:?}", path);
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
    /// Attempted to resolve a relative path, but no current_file_path was provided,
    /// so it is not known what to resolve the path against.
    #[error("Relative path `{}` provided, but current_file_path could not be determined", .0.display())]
    MissingCurrentFilePath(PathBuf),
    /// The scheme provided was not correct or supported.
    #[error("Url `{}` was expected to be of type `{}`", .1, .0)]
    WrongScheme(String, LspUrl),
}

impl SpacesContext {
    pub(crate) fn new(
        workspace_path: Arc<str>,
        mode: ContextMode,
        print_non_none: bool,
        prelude: &[PathBuf],
        module: bool,
    ) -> anyhow::Result<Self> {
        let mut builtin_docs: HashMap<LspUrl, String> = HashMap::new();
        let mut builtin_symbols: HashMap<String, LspUrl> = HashMap::new();
        let globals = evaluator::get_globals(evaluator::WithRules::Yes).build();

        for (name, item) in globals.documentation().members {
            let uri = Url::parse(&format!("starlark:/{name}.bzl"))
                .context(format_context!("Failed to parse URL for {name}"))?;
            let uri = LspUrl::try_from(uri.clone())
                .context(format_context!("Failed to convert to uri {uri:?}"))?;
            builtin_docs.insert(uri.clone(), item.render_as_code(&name));
            builtin_symbols.insert(name, uri);
        }

        eprintln!("New spaces context -- ");

        let mut ctx = Self {
            workspace_path,
            mode,
            print_non_none,
            prelude: Vec::new(),
            module: None,
            dialect: evaluator::get_dialect(),
            globals,
            builtin_docs,
            builtin_symbols,
            current_directory: None,
        };

        ctx.prelude = prelude
            .iter()
            .map(|x| ctx.load_path(x))
            .collect::<starlark::Result<_>>()
            .into_anyhow_result()?;

        ctx.module = if module {
            Some(Self::new_module(&ctx.prelude))
        } else {
            None
        };

        Ok(ctx)
    }

    fn load_path(&self, path: &Path) -> starlark::Result<FrozenModule> {
        let env = Module::new();

        eprintln!("Load path {path:?}");

        let content =
            fs::read_to_string(path).context(format_context!("Failed to read {path:?}"))?;

        let frozen_module = evaluator::evaluate_module(
            None,
            self.workspace_path.clone(),
            path.to_string_lossy().into(),
            content.into(),
            evaluator::WithRules::Yes,
        )
        .context(format_context!("Failed to evaluate {path:?}"))?;

        Ok(frozen_module)
    }

    fn new_module(prelude: &[FrozenModule]) -> Module {
        let module = Module::new();
        eprintln!("new module");

        for p in prelude {
            module.import_public_symbols(p);
        }
        module
    }

    fn go(&self, file: &str, ast: AstModule) -> EvalResult<impl Iterator<Item = EvalMessage>> {
        eprintln!("go");
        let mut warnings = Either::Left(iter::empty());
        let mut errors = Either::Left(iter::empty());
        let final_ast = match self.mode {
            ContextMode::Check => {
                warnings = Either::Right(self.check(file, &ast));
                Some(ast)
            }
            ContextMode::Run => {
                errors = Either::Right(self.run(file, ast).messages);
                None
            }
        };
        EvalResult {
            messages: warnings.chain(errors),
            ast: final_ast,
        }
    }

    // Convert a result over iterator of EvalMessage, into an iterator of EvalMessage
    fn err(
        file: &str,
        result: starlark::Result<EvalResult<impl Iterator<Item = EvalMessage>>>,
    ) -> EvalResult<impl Iterator<Item = EvalMessage>> {
        eprintln!("err: {file}");
        match result {
            Err(e) => EvalResult {
                messages: Either::Left(iter::once(EvalMessage::from_error(Path::new(file), &e))),
                ast: None,
            },
            Ok(res) => EvalResult {
                messages: Either::Right(res.messages),
                ast: res.ast,
            },
        }
    }

    pub(crate) fn expression(
        &self,
        content: String,
    ) -> EvalResult<impl Iterator<Item = EvalMessage>> {
        eprintln!("expression: {content}");
        let file = "expression";
        Self::err(
            file,
            AstModule::parse(file, content, &self.dialect)
                .map(|module| self.go(file, module))
                .map_err(Into::into),
        )
    }

    pub(crate) fn file(&self, file: &Path) -> EvalResult<impl Iterator<Item = EvalMessage>> {
        let filename = &file.to_string_lossy();
        eprintln!("file");

        Self::err(
            filename,
            fs::read_to_string(file)
                .map(|content| self.file_with_contents(filename, content))
                .map_err(|e| anyhow::Error::from(e).into()),
        )
    }

    pub(crate) fn file_with_contents(
        &self,
        filename: &str,
        content: String,
    ) -> EvalResult<impl Iterator<Item = EvalMessage>> {
        Self::err(
            filename,
            AstModule::parse(filename, content, &self.dialect)
                .map(|module| self.go(filename, module))
                .map_err(Into::into),
        )
    }

    fn run(&self, file: &str, ast: AstModule) -> EvalResult<impl Iterator<Item = EvalMessage>> {
        let new_module;
        let module = match self.module.as_ref() {
            Some(module) => module,
            None => {
                new_module = Self::new_module(&self.prelude);
                &new_module
            }
        };

        let loads_result = evaluator::evaluate_loads(
            &ast,
            file.into(),
            None,
            self.workspace_path.clone(),
            evaluator::WithRules::Yes,
        );

        let module = Module::new();
        let eval_result = match loads_result {
            Ok(loads) => {
                let modules = loads.iter().map(|(a, b)| (a.as_str(), b)).collect();
                let loader = ReturnFileLoader { modules: &modules };
                let mut eval = Evaluator::new(&module);
                eval.enable_terminal_breakpoint_console();
                eval.set_loader(&loader);
                eval.eval_module(ast, &self.globals)
                    .map_err(|e| anyhow::anyhow!("{e:?}"))
            }
            Err(loads_err) => Err(loads_err.into()),
        };

        Self::err(
            file,
            eval_result
                .map(|v| {
                    if self.print_non_none && !v.is_none() {
                        eprintln!("{}", v);
                    }
                    EvalResult {
                        messages: iter::empty(),
                        ast: None,
                    }
                })
                .map_err(Into::into),
        )
    }

    fn is_suppressed(&self, _file: &str, _issue: &str) -> bool {
        false
    }

    fn check(&self, file: &str, module: &AstModule) -> impl Iterator<Item = EvalMessage> {
        let mut globals = HashSet::new();
        for (name, _) in self.globals.iter() {
            globals.insert(name.to_owned());
        }
        let mut lints = module.lint(Some(&globals));
        lints.retain(|issue| !self.is_suppressed(file, &issue.short_name));
        lints.into_iter().map(EvalMessage::from)
    }
}

impl LspContext for SpacesContext {
    fn parse_file_with_contents(&self, uri: &LspUrl, content: String) -> LspEvalResult {
        match uri {
            LspUrl::File(uri) => {
                let EvalResult { messages, ast } =
                    self.file_with_contents(&uri.to_string_lossy(), content);
                LspEvalResult {
                    diagnostics: messages.map(eval_message_to_lsp_diagnostic).collect(),
                    ast,
                }
            }
            _ => LspEvalResult::default(),
        }
    }

    fn resolve_load(
        &self,
        path: &str,
        current_file: &LspUrl,
        _workspace_root: Option<&Path>,
    ) -> anyhow::Result<LspUrl> {
        let path = PathBuf::from(path);
        match current_file {
            LspUrl::File(current_file_path) => {
                let current_file_dir = current_file_path.parent();
                let absolute_path = match (current_file_dir, path.is_absolute()) {
                    (_, true) => Ok(path),
                    (Some(current_file_dir), false) => Ok(current_file_dir.join(&path)),
                    (None, false) => Err(ResolveLoadError::MissingCurrentFilePath(path)),
                }?;
                Ok(Url::from_file_path(absolute_path).unwrap().try_into()?)
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
        _current_file: &LspUrl,
        symbol: &str,
    ) -> anyhow::Result<Option<LspUrl>> {
        Ok(self.builtin_symbols.get(symbol).cloned())
    }

    fn render_as_load(
        &self,
        _target: &LspUrl,
        _current_file: &LspUrl,
        _workspace_root: Option<&Path>,
    ) -> anyhow::Result<String> {
        Err(anyhow::anyhow!("Not yet implemented, render_as_load"))
    }

    fn get_environment(&self, _uri: &LspUrl) -> DocModule {
        DocModule::default()
    }
}
