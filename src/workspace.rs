use std::{
    collections::{HashMap, VecDeque},
    path,
};

use crate::{
    archive,
    config::Printer,
    git::{self, BareRepository},
    manifest::{Dependency, Workspace, WorkspaceConfig},
};

fn get_current_directory() -> anyhow::Result<String> {
    let current_directory = std::env::current_dir()?;
    let current_directory_str = current_directory
        .to_str()
        .ok_or(anyhow::anyhow!("Path is not a valid string"))?;
    Ok(current_directory_str.to_string())
}

pub fn create(printer: &mut Printer, space_name: &String) -> anyhow::Result<()> {
    let workspace_config = WorkspaceConfig::new("./")?;
    let heading = printer::Heading::new(printer, "Creating Workspace")?;
    let directory = format!("{}/{space_name}", get_current_directory()?);
    if heading.printer.is_dry_run == false {
        std::fs::create_dir(std::path::Path::new(space_name))?;
    }

    heading.printer.info("name", space_name)?;

    let workspace = Workspace::new_from_workspace_config(&workspace_config, space_name);
    workspace.save(&directory)?;

    {
        let section = printer::Heading::new(heading.printer, "Workspace")?;
        let mut execute_batch = printer::ExecuteBatch::new();
        for (spaces_key, dependency) in workspace.repositories.iter() {
            let (bare_repository, execute_later) =
                git::BareRepository::new(section.printer, spaces_key, &dependency.git)?;
            execute_batch.add(spaces_key, execute_later);

            section.printer.info(spaces_key, &bare_repository)?;

            let (worktree, execute_later) =
                bare_repository.add_worktree(section.printer, &directory)?;
            execute_batch.add(spaces_key, execute_later);
            execute_batch.add(
                spaces_key,
                worktree.switch_new_branch(section.printer, dependency)?,
            );
        }
        execute_batch.execute(section.printer)?;
    }

    let mut state = State::new(heading.printer, &directory)?;

    if let Some(buck) = &workspace_config.buck {
        buck.export(&directory)?;
    }

    state.sync_full_path()?;

    Ok(())
}

pub fn sync(printer: &mut Printer) -> anyhow::Result<()> {
    let full_path = get_current_directory()?;
    let mut state = State::new(printer, &full_path)?;
    state.sync_full_path()
}

struct State<'a> {
    printer: &'a mut Printer,
    full_path: &'a str,
    workspace: Workspace,
    all_deps: VecDeque<(BareRepository, Dependency)>,
    deps_map: HashMap<String, Dependency>,
}

impl<'a> State<'a> {
    fn new(printer: &'a mut Printer, full_path: &'a str) -> anyhow::Result<Self> {
        Ok(Self {
            printer,
            full_path,
            workspace: Workspace::new(full_path)?,
            deps_map: HashMap::new(),
            all_deps: VecDeque::new(),
        })
    }

    fn sync_full_path(&mut self) -> anyhow::Result<()> {
        self.sync_repositories()?;
        self.sync_dependencies()?;
        self.export_buck_config()?;
        self.update_cargo()?;
        Ok(())
    }

    fn sync_repositories(&mut self) -> anyhow::Result<()> {
        let section = printer::Heading::new(self.printer, "Workspace")?;
        let mut execute_batch = printer::ExecuteBatch::new();
        for (spaces_key, dependency) in self.workspace.repositories.iter() {
            self.deps_map.insert(spaces_key.clone(), dependency.clone());
            let (bare_repository, execute_later) =
                git::BareRepository::new(section.printer, spaces_key, &dependency.git)?;

            section.printer.info(spaces_key, &bare_repository)?;
            execute_batch.add(spaces_key, execute_later);

            self.all_deps
                .push_back((bare_repository, dependency.clone()));
        }

        execute_batch.execute(section.printer)?;
        Ok(())
    }

    fn sync_dependencies(&mut self) -> anyhow::Result<()> {
        let heading = printer::Heading::new(self.printer, "Dependencies")?;
        loop {
            if let Some((bare_repository, dependency)) = self.all_deps.pop_back() {
                let mut execute_batch = printer::ExecuteBatch::new();

                let section = printer::Section::new(heading.printer, &bare_repository.spaces_key)?;
                let (worktree, execute_later) =
                    bare_repository.add_worktree(section.printer, self.full_path)?;
                execute_batch.add(bare_repository.spaces_key.as_str(), execute_later);

                if self.deps_map.contains_key(&bare_repository.spaces_key) {
                    section.printer.info("checkout", &"develop")?;
                } else {
                    self.deps_map
                        .insert(bare_repository.spaces_key.clone(), dependency.clone());
                    execute_batch.add(
                        &bare_repository.spaces_key,
                        worktree.checkout(section.printer, &dependency)?,
                    );
                    execute_batch.add(
                        &bare_repository.spaces_key,
                        worktree.checkout_detached_head(section.printer)?,
                    );
                }

                let spaces_deps = worktree.get_deps()?;
                if let Some(spaces_deps) = spaces_deps {
                    section.printer.info("deps", &spaces_deps)?;
                    for (spaces_key, dep) in spaces_deps.deps.iter() {
                        section.printer.info(spaces_key, dep)?;
                        let (bare_repository, execute_later) =
                            git::BareRepository::new(section.printer, spaces_key, &dep.git)?;

                        execute_batch.add(bare_repository.spaces_key.as_str(), execute_later);
                        if !self
                            .workspace
                            .repositories
                            .contains_key(&bare_repository.spaces_key)
                        {
                            self.workspace
                                .dependencies
                                .insert(bare_repository.spaces_key.clone(), dep.clone());
                        }

                        if !self.deps_map.contains_key(&bare_repository.spaces_key) {
                            self.all_deps.push_front((bare_repository, dep.clone()));
                        }
                    }
                    if let Some(archive_map) = spaces_deps.archives {
                        let mut archives = Vec::new();
                        {
                            let mut multi_progress = printer::MultiProgress::new(section.printer);
                            for (key, archive) in archive_map.iter() {
                                let http_archive = archive::HttpArchive::new(
                                    multi_progress.printer,
                                    key,
                                    archive,
                                )?;

                                if http_archive.is_download_required() {
                                    let bar = multi_progress.add_progress(key, Some(100));
                                    let join_handle = http_archive.download(
                                        &multi_progress.printer.context().async_runtime,
                                        bar,
                                    )?;
                                    archives.push((http_archive, Some(join_handle)));
                                } else {
                                    archives.push((http_archive, None));
                                }
                            }

                            loop {
                                let mut is_running = false;
                                for (_archive, join_handle) in archives.iter() {
                                    if let Some(join_handle) = join_handle {
                                        if !join_handle.is_finished() {
                                            is_running = true;
                                        }
                                    }
                                }
                                if !is_running {
                                    break;
                                }
                            }
                        }

                        for (archive, _join_handle) in archives.iter() {
                            archive.extract(section.printer)?;

                            section.printer.info(
                                "symlink",
                                &format!("{}/{}", self.full_path, archive.spaces_key),
                            )?;

                            archive.create_soft_link(self.full_path)?;
                        }
                    }
                } else {
                    section.printer.info("deps", &"No dependencies found")?;
                }
                execute_batch.execute(section.printer)?;
            } else {
                break;
            }
        }

        Ok(())
    }

    fn export_buck_config(&mut self) -> anyhow::Result<()> {
        let workspace = &mut self.workspace;
        let deps_map = &mut self.deps_map;

        if let Some(buck) = workspace.buck.as_mut() {
            if buck.cells.is_none() {
                buck.cells = Some(HashMap::new());
            }

            if let Some(buck_cells) = buck.cells.as_mut() {
                for (key, _dep) in deps_map.iter() {
                    buck_cells.insert(key.clone(), format!("./{key}"));
                }
            }
            buck.export(self.full_path)?;
        }

        workspace.save(self.full_path)?;

        Ok(())
    }

    fn update_cargo(&mut self) -> anyhow::Result<()> {
        if let Some(cargo) = self.workspace.get_cargo_patches() {
            let heading = printer::Heading::new(self.printer, "Cargo")?;

            heading.printer.info("cargo", cargo)?;

            for (spaces_key, list) in cargo.iter() {
                //read the cargo toml file to see how the dependency is specified crates-io or git
                let cargo_toml_path = format!("{}/{spaces_key}/Cargo.toml", self.full_path);
                let mut cargo_toml_contents = std::fs::read_to_string(&cargo_toml_path)?;
                let cargo_toml: toml::Value = toml::from_str(&cargo_toml_contents)?;

                let dependencies = cargo_toml.get("dependencies").ok_or(anyhow::anyhow!(
                    "Cargo.toml does not have a dependencies section"
                ))?;

                const START_WORKSPACE: &str = "\n\n#! spaces_workspace\n";
                const END_WORKSPACE: &str = "#! drop(spaces_workspace)\n";

                if let (Some(start), Some(end)) = (cargo_toml_contents.find(START_WORKSPACE), cargo_toml_contents.find(END_WORKSPACE)) {
                    cargo_toml_contents.replace_range(start..(end + END_WORKSPACE.len()), "");
                }

                cargo_toml_contents.push_str(START_WORKSPACE);

                for value in list {
                    let dependency = dependencies.get(value.as_str()).ok_or(anyhow::anyhow!(
                        "Cargo.toml does not have a dependency named {}",
                        value
                    ))?;

                    if let Some(git) = dependency.get("git").map(|e| e.as_str()).flatten() {
                        let patch = format!("[patch.'{}']\n", git);
                        let path = format!("{value} = {{ path = \"../{value}\" }}\n");
                        cargo_toml_contents.push_str(patch.as_str());
                        cargo_toml_contents.push_str(path.as_str());
                    }
                }
                cargo_toml_contents.push_str(END_WORKSPACE);

                std::fs::write(&cargo_toml_path, cargo_toml_contents)?;
            }
        }

        Ok(())
    }
}
