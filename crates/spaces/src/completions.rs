use std::sync::Arc;

const HANDLE_COLON_BASH: &str = "COMP_WORDBREAKS=${COMP_WORDBREAKS//:/}";
const HANDLE_STARTUP_ZSH: &str = r#"
autoload -Uz compinit
compinit
"#;

pub fn generate_workspace_completions(
    command: &clap::Command,
    shell: clap_complete::Shell,
    run_targets: Vec<Arc<str>>,
) -> anyhow::Result<Vec<u8>> {
    let mut new_command = clap::Command::new("spaces");

    static PINNED_STRINGS: once_cell::sync::OnceCell<Vec<Arc<str>>> =
        once_cell::sync::OnceCell::new();

    let pinned_strings: &'static Vec<Arc<str>> = PINNED_STRINGS.get_or_init(|| run_targets);

    // Iterate and copy subcommands
    for subcommand in command.get_subcommands() {
        // Intercept the "run" subcommand
        if subcommand.get_name() == "run" {
            // Add a new positional argument or extend possible values
            let mut sc_clone = clap::Command::new("run");

            for arg in subcommand.get_arguments() {
                // intercept the target and insert all targets
                if arg.get_id() == "target" {
                    let pinned_strs = pinned_strings.iter().map(|s| s.as_ref());

                    let targets_value_parser =
                        clap::builder::PossibleValuesParser::new(pinned_strs);

                    sc_clone = sc_clone.arg(
                        clap::Arg::new("target")
                            .help(arg.get_help().cloned().unwrap_or_default())
                            .num_args(1)
                            .value_parser(targets_value_parser),
                    );
                } else {
                    sc_clone = sc_clone.arg(arg.clone());
                }
            }
            new_command = new_command.subcommand(sc_clone);
        } else {
            new_command = new_command.subcommand(subcommand.clone());
        }
    }

    // Step 6: Add top-level args if needed
    for arg in command.get_arguments() {
        new_command = new_command.arg(arg.clone());
    }

    // write to a buffer
    let mut buffer = Vec::new();

    match shell {
        clap_complete::Shell::Bash => {
            let line = format!("{HANDLE_COLON_BASH}\n");
            buffer.extend(line.bytes());
        }
        clap_complete::Shell::Zsh => {
            let line = format!("{HANDLE_STARTUP_ZSH}\n");
            buffer.extend(line.bytes());
        }
        _ => {}
    }

    clap_complete::generate(shell, &mut new_command, "spaces", &mut buffer);

    if shell == clap_complete::Shell::Fish {
        let line = format!(
            r#"complete -c spaces -n "__fish_spaces_using_subcommand run" --no-files -a "{}"\n"#,
            pinned_strings.join(" ")
        );
        buffer.extend(line.bytes());
    }

    Ok(buffer)
}
