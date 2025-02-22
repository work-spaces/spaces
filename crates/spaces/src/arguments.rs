use crate::{docs, evaluator, task, runner, singleton, tools, workspace};
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use clap::{CommandFactory, Parser, Subcommand, ValueEnum, ValueHint};
use std::sync::Arc;

type WorkflowsToml = std::collections::HashMap<Arc<str>, Vec<Arc<str>>>;

#[derive(ValueEnum, Clone, Copy, Debug)]
pub enum Level {
    Trace,
    Debug,
    Message,
    Info,
    App,
    Warning,
    Error,
}

impl From<Level> for printer::Level {
    fn from(level: Level) -> Self {
        match level {
            Level::Trace => printer::Level::Trace,
            Level::Debug => printer::Level::Debug,
            Level::Message => printer::Level::Message,
            Level::Info => printer::Level::Info,
            Level::App => printer::Level::App,
            Level::Warning => printer::Level::Warning,
            Level::Error => printer::Level::Error,
        }
    }
}

#[derive(Debug, Parser)]
#[command(version, about, long_about = None)]
pub struct Arguments {
    /// The verbosity level of the output.
    #[arg(short, long, default_value = "app")]
    pub verbosity: Level,
    /// Dont show progress bars
    #[arg(long)]
    pub hide_progress_bars: bool,
    /// Show elapsed time - use with --verbosity=debug to instrument spaces performance
    #[arg(long)]
    pub show_elapsed_time: bool,
    /// If this is passed, info.is_ci() returns true in scripts.
    #[arg(long)]
    ci: bool,
    /// Rescan the workspace for *spaces.star files
    #[arg(long)]
    rescan: bool,
    #[command(subcommand)]
    commands: Commands,
}

fn handle_verbosity(
    printer: &mut printer::Printer,
    verbosity: printer::Level,
    is_ci: bool,
    rescan: bool,
    is_hide_progress_bars: bool,
    show_elapsed_time: bool

) {
    singleton::set_rescan(rescan);
    if is_ci {
        singleton::set_ci(true);
        printer.verbosity.level = printer::Level::Trace;
        printer.verbosity.is_show_progress_bars = false;
        printer.verbosity.is_show_elapsed_time = true;  
    } else {
        printer.verbosity.level = verbosity;
        printer.verbosity.is_show_progress_bars = !is_hide_progress_bars;
        printer.verbosity.is_show_elapsed_time = show_elapsed_time;  
    }
}

pub fn execute() -> anyhow::Result<()> {
    if std::env::args().len() == 1 {
        let mut stdin_contents = String::new();
        use std::io::Read;
        std::io::stdin().read_to_string(&mut stdin_contents)?;
        evaluator::run_starlark_script(workspace::SPACES_STDIN_NAME.into(), stdin_contents.into())
            .context(format_context!("Failed to run starlark script"))?;
        return Ok(());
    }

    if std::env::args().len() >= 2 {
        let filename: Arc<str> = std::env::args().nth(1).unwrap().into();
        let input = std::path::Path::new(filename.as_ref());
        if input.exists() && input.extension().unwrap_or_default() == "star" {
            starstd::script::set_args(std::env::args().skip(1).collect());

            let input_contents = std::fs::read_to_string(input)
                .context(format_context!("Failed to read input file {input:?}"))?;
            evaluator::run_starlark_script(filename.clone(), input_contents.into())
                .context(format_context!("Failed to run starlark script {filename}"))?;

            let exit_code = starstd::script::get_exit_code();
            if exit_code != 0 {
                std::process::exit(exit_code);
            }

            return Ok(());
        }
    }

    let args = Arguments::parse();
    let mut printer = printer::Printer::new_stdout();

    match args {
        Arguments {
            verbosity,
            hide_progress_bars,
            show_elapsed_time,
            ci,
            rescan,
            commands:
                Commands::Checkout {
                    name,
                    script,
                    workflow,
                    create_lock_file,
                    force_install_tools,
                },
        } => {
            handle_verbosity(
                &mut printer,
                verbosity.into(),
                ci,
                rescan,
                hide_progress_bars,
                show_elapsed_time,
            );

            let mut script_inputs: Vec<Arc<str>> = vec![];
            script_inputs.extend(script.clone());

            if let Some(workflow) = workflow {
                let parts: Vec<_> = workflow.split(':').collect();
                if parts.len() != 2 {
                    return Err(format_error!("Invalid workflow format: {}.\n Use --workflow=<directory>:<script>,<script>,...", workflow));
                }
                let directory = parts[0];

                let inputs: Vec<_> = parts[1].split(',').collect();
                let mut scripts: Vec<Arc<str>> = vec![];

                let workflows_json_path =
                    format!("{}/{}", directory, workspace::WORKFLOW_TOML_NAME);
                let mut is_workspace_json_input = false;
                if std::path::Path::new(workflows_json_path.as_str()).exists() && inputs.len() == 1
                {
                    let workflows_json: WorkflowsToml = toml::from_str(
                        std::fs::read_to_string(workflows_json_path.as_str())
                            .context(format_context!("Failed to read workflows json"))?
                            .as_str(),
                    )
                    .context(format_context!("Failed to parse workflows json"))?;

                    if let Some(workflow_scripts) = workflows_json.get(inputs[0]) {
                        is_workspace_json_input = true;
                        scripts.extend(workflow_scripts.clone());
                    }
                }

                if !is_workspace_json_input {
                    scripts.extend(inputs.iter().map(|s| (*s).into()));
                }

                for script in scripts {
                    let short_path = format!("{}/{}", directory, script);
                    let long_path = format!("{}/{}.spaces.star", directory, script);
                    if !std::path::Path::new(long_path.as_str()).exists()
                        && !std::path::Path::new(short_path.as_str()).exists()
                    {
                        return Err(format_error!(
                            "Script file not found: {}/{}",
                            directory,
                            script
                        ));
                    }

                    script_inputs.push(format!("{}/{}", directory, script).into());
                }
            }

            for script_path in script_inputs.iter() {
                if script_path.as_ref().ends_with("env")
                    || script_path.ends_with(workspace::ENV_FILE_NAME)
                {
                    return Err(format_error!("`env.spaces.star` is a reserved script name",));
                }
            }

            tools::install_tools(&mut printer, force_install_tools)
                .context(format_context!("while installing tools"))?;

            runner::checkout(&mut printer, name, script_inputs, create_lock_file)
                .context(format_context!("during runner checkout"))?;
        }

        Arguments {
            verbosity,
            hide_progress_bars,
            show_elapsed_time,
            ci,
            rescan,
            commands: Commands::Sync {},
        } => {
            handle_verbosity(
                &mut printer,
                verbosity.into(),
                ci,
                rescan,
                hide_progress_bars,
                show_elapsed_time
            );
            runner::run_starlark_modules_in_workspace(
                &mut printer,
                task::Phase::Checkout,
                None,
                true,
                runner::RunWorkspace::Target(None, vec![]),
                false,
            )
            .context(format_context!("during runner sync"))?;
        }

        #[cfg(feature = "lsp")]
        Arguments {
            verbosity,
            hide_progress_bars,
            ci,
            commands: Commands::RunLsp {},
        } => {
            handle_verbosity(&mut printer, verbosity.into(), ci, hide_progress_bars);
            runner::run_lsp(&mut printer).context(format_context!("during runner sync"))?;
        }

        Arguments {
            verbosity,
            hide_progress_bars,
            show_elapsed_time,
            ci,
            rescan,
            commands:
                Commands::Run {
                    target,
                    forget_inputs,
                    extra_rule_args,
                },
        } => {
            handle_verbosity(
                &mut printer,
                verbosity.into(),
                ci,
                rescan,
                hide_progress_bars,
                show_elapsed_time
            );

            if target.is_none() && !extra_rule_args.is_empty() {
                return Err(format_error!(
                    "Extra rule arguments are only allowed when a target is specified."
                ));
            }

            runner::run_starlark_modules_in_workspace(
                &mut printer,
                task::Phase::Run,
                None,
                forget_inputs,
                runner::RunWorkspace::Target(target, extra_rule_args),
                false,
            )
            .context(format_context!("while executing run rules"))?;
        }

        Arguments {
            verbosity,
            hide_progress_bars,
            show_elapsed_time,
            ci,
            rescan,
            commands:
                Commands::Inspect {
                    target,
                    filter,
                    has_help,
                },
        } => {
            handle_verbosity(
                &mut printer,
                verbosity.into(),
                ci,
                rescan,
                hide_progress_bars,
                show_elapsed_time
            );

            if printer.verbosity.level > printer::Level::Info {
                printer.verbosity.level = printer::Level::Info;
            }

            let mut filter_globs = std::collections::HashSet::new();
            if let Some(filter) = filter {
                let filter_parts = filter.split(',');
                for glob_expression in filter_parts {
                    let effective_expression =
                        if glob_expression.starts_with('-') || glob_expression.starts_with('+') {
                            glob_expression.to_string()
                        } else if glob_expression.contains('*') {
                            format!("+{}", glob_expression)
                        } else {
                            format!("+**{}**", glob_expression)
                        };
                    filter_globs.insert(effective_expression.into());
                }
            }

            singleton::set_inspect_globs(filter_globs);
            singleton::set_has_help(has_help);

            runner::run_starlark_modules_in_workspace(
                &mut printer,
                task::Phase::Inspect,
                None,
                false,
                runner::RunWorkspace::Target(target, vec![]),
                false,
            )
            .context(format_context!("while executing run rules"))?;
        }

        Arguments {
            verbosity,
            hide_progress_bars,
            show_elapsed_time,
            ci,
            rescan,
            commands: Commands::Completions { shell },
        } => {
            handle_verbosity(
                &mut printer,
                verbosity.into(),
                ci,
                rescan,
                hide_progress_bars,
                show_elapsed_time
            );

            clap_complete::generate(
                shell,
                &mut Arguments::command(),
                "spaces",
                &mut std::io::stdout(),
            );
        }

        Arguments {
            verbosity,
            hide_progress_bars,
            show_elapsed_time,
            ci,
            rescan,
            commands: Commands::Docs { item },
        } => {
            handle_verbosity(
                &mut printer,
                verbosity.into(),
                ci,
                rescan,
                hide_progress_bars,
                show_elapsed_time
            );

            docs::show(&mut printer, item)?;
        }
    }

    Ok(())
}

#[derive(Debug, Subcommand)]
enum Commands {
    #[command(about = r#"
Executes the checkout rules in the specified scripts."#)]
    Checkout {
        /// The name of the workspace to create.
        #[arg(long)]
        name: Arc<str>,
        /// The path(s) to the `spaces.star`` file containing checkout rules. Paths are processed in order.
        #[arg(long, value_hint = ValueHint::FilePath)]
        script: Vec<Arc<str>>,
        #[arg(
            long,
            help = r#"Scripts to process in the format of `--workflow=<directory>:<script>,<script>,...`.
`--script` is processed before `--workflow`. 

If <directory> has `workflows.spaces.toml`, it will be parsed for shortcuts if only one <script> is passed.
- `spaces checkout --workflow=workflows:my-shortcut --name=workspace-name`
  - run scripts list in `my-shortcut` in `workflows/workflows.spaces.toml`
- `spaces checkout --workflow=workflows:preload,my-shortcut --name=workspace-name`
  - run `workflows/preload.spaces.star` then `workflows/my-shortcut.spaces.star`

```toml
my-shortcut = ["preload", "my-shortcut"]
```
"#
        )]
        workflow: Option<Arc<str>>,
        /// Create a lock file for the workspace. This file can be passed on the next checkout as a script to re-create the exact workspace.
        #[arg(long)]
        create_lock_file: bool,
        /// Force install the tools spaces needs to run.
        #[arg(long)]
        force_install_tools: bool,
    },
    /// Runs checkout rules within an existing workspace. This is experimental. Don't use it.
    Sync {},
    #[command(about = r"
Runs a spaces run rule.
- `spaces run`: Run all non-optional rules with dependencies
- `spaces run my-target`: Run a single target plus dependencies
- `spaces run my-target -- --some-arg --some-other-arg`: pass additional arguments to a rule")]
    Run {
        /// The name of the target to run (default is all targets).
        target: Option<Arc<str>>,
        /// Forces rules to run even if input globs are the same as last time.
        #[arg(long)]
        forget_inputs: bool,
        #[arg(
            trailing_var_arg = true,
            help = r"Extra arguments to pass to the rule (passed after `--`)"
        )]
        extra_rule_args: Vec<Arc<str>>,
    },
    #[command(about = r"
Inspect all the scripts in the workspace without running any rules.
- `spaces inspect`: show the rules that have `help` entries: 
- `spaces inspect <target-name>`: show target plus dependencies
- `spaces --verbosity=message inspect`: show all rules
- `spaces --verbosity=debug inspect`: show all rules in detail")]
    Inspect {
        /// The name of the target to evaluate (default is all targets).
        target: Option<Arc<str>>,
        // Filter targets with a glob (e.g. `--filter=**/my-target`)
        #[arg(long)]
        filter: Option<Arc<str>>,
        // Only show rules with the help entry populated
        #[arg(long)]
        has_help: bool,
    },
    /// Generates shell completions for the spaces command.
    Completions {
        /// The shell to generate the completions for
        #[arg(long, value_enum)]
        shell: clap_complete::Shell,
    },
    /// Shows the documentation for spaces starlark modules.
    Docs {
        /// What documentation do you want to see?
        #[arg(value_enum)]
        item: Option<docs::DocItem>,
    },
    /// Run the Spaces language server protocol. Not currently functional.
    #[cfg(feature = "lsp")]
    RunLsp {},
}
