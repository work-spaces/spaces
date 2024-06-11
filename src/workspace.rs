use std::collections::{HashMap, VecDeque};

use anyhow::Context;

use crate::{
    archive,
    context::{self, anyhow_error, format_error_context},
    git::{self, BareRepository},
    ledger,
    manifest::{self, Dependency, Workspace, WorkspaceConfig},
};

fn get_current_directory() -> anyhow::Result<String> {
    let current_directory = std::env::current_dir()?;
    let current_directory_str = current_directory.to_str().ok_or(anyhow::anyhow!(
        "Internal Error: Path is not a valid string"
    ))?;
    Ok(current_directory_str.to_string())
}

pub fn create(
    context: context::Context,
    space_name: &String,
    config: &String,
) -> anyhow::Result<()> {
    let workspace_config = WorkspaceConfig::new(config)
        .with_context(|| format_error_context!("Failed to load spaces configuration {config}"))?;

    create_from_config(context, space_name, workspace_config)
}

pub fn create_from_config(
    context: context::Context,
    space_name: &String,
    config: WorkspaceConfig,
) -> anyhow::Result<()> {
    // don't create if we are in a .git repository
    let current_directory = get_current_directory()
        .with_context(|| format_error_context!("while creating workspace using {config:?}"))?;

    {
        let path = std::path::Path::new(&current_directory);
        let mut path = path.join(".git");
        while let Some(parent) = path.parent() {
            let git_path = parent.join(".git");
            if git_path.exists() {
                return Err(anyhow_error!(
                    "Cannot create a spaces workspace in a git repository: {git_path:?}"
                ));
            }
            path.pop();
        }
    }

    let directory = format!("{current_directory}/{space_name}");
    let context = std::sync::Arc::new(context);

    if !context.is_dry_run {
        std::fs::create_dir(std::path::Path::new(space_name)).with_context(|| {
            format_error_context!("When creating workspace {space_name} in current directory")
        })?;
    }

    let workspace = config.to_workspace(space_name).with_context(|| {
        format_error_context!("When creating workspace {space_name} from workspace config")
    })?;
    workspace
        .save(&directory)
        .with_context(|| format_error_context!("When trying to save workspace for {space_name}"))?;

    {
        let mut printer = context
            .printer
            .write()
            .expect("Internal Error: Printer is not set");

        let mut multi_progress = printer::MultiProgress::new(&mut printer);

        let mut handles: Vec<std::thread::JoinHandle<anyhow::Result<(), _>>> = Vec::new();

        for (spaces_key, dependency) in workspace.repositories.iter() {
            let progress_bar = multi_progress.add_progress(spaces_key, None, None);

            let context = context.clone();
            let spaces_key = spaces_key.to_owned();
            let dependency = dependency.clone();
            let directory = directory.clone();

            let handle = std::thread::spawn(move || {
                let mut progress_bar = progress_bar;
                let bare_repository = git::BareRepository::new(
                    context.clone(),
                    &mut progress_bar,
                    &spaces_key,
                    &dependency.git,
                )?;

                let worktree =
                    bare_repository.add_worktree(context.clone(), &mut progress_bar, &directory)?;

                worktree.switch_new_branch(context, &mut progress_bar, &dependency)?;

                Ok::<(), anyhow::Error>(())
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().unwrap()?;
        }
    }

    let mut state = State::new(context.clone(), space_name.clone(), directory.clone())
        .with_context(|| format_error_context!("While creating workspace state"))?;
    state.sync_full_path().with_context(|| {
        format_error_context!("While syncing full path during workspace creation")
    })?;

    let mut printer = context
        .printer
        .write()
        .expect("Internal Error: Printer is not set");

    printer.info(space_name, &workspace)?;

    Ok::<(), anyhow::Error>(())
}

pub fn sync(context: context::Context) -> anyhow::Result<()> {
    let full_path = get_current_directory()?;
    let space_name = std::path::Path::new(&full_path)
        .file_name()
        .ok_or(anyhow_error!(
            "{full_path} directory is not a space workspace"
        ))?
        .to_str()
        .ok_or(anyhow_error!(
            "{full_path} directory is not a space workspace"
        ))?;

    let mut state = State::new(
        std::sync::Arc::new(context),
        space_name.to_string(),
        full_path,
    )?;
    state.sync_full_path()?;
    Ok(())
}

enum SyncDep {
    BareRepository(String, Dependency),
    Repository(BareRepository, Dependency),
    Archive(String, archive::HttpArchive),
    PlatformArchive(String, archive::HttpArchive),
}

struct State {
    context: std::sync::Arc<context::Context>,
    full_path: String,
    _spaces_name: String,
    workspace: Workspace,
    all_deps: VecDeque<SyncDep>,
    deps_map: HashMap<String, Dependency>,
}

impl State {
    fn new(
        context: std::sync::Arc<context::Context>,
        spaces_name: String,
        full_path: String,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            context,
            _spaces_name: spaces_name,
            full_path: full_path.clone(),
            workspace: Workspace::new(&full_path).with_context(|| {
                format_error_context!("{full_path} when creating workspace state")
            })?,
            deps_map: HashMap::new(),
            all_deps: VecDeque::new(),
        })
    }

    fn sync_full_path(&mut self) -> anyhow::Result<()> {
        let context = self.context.clone();

        let log_path = format!("{}/spaces_logs", self.full_path);
        std::fs::create_dir_all(log_path.as_str())
            .with_context(|| format_error_context!("Trying to create {log_path}"))?;

        let mut printer = context
            .printer
            .write()
            .expect("Internal Error: Printer is not set");

        let mut multi_progress = printer::MultiProgress::new(&mut printer);

        self.sync_repositories(&mut multi_progress)
            .with_context(|| format_error_context!("While syncing repositories"))?;
        self.sync_dependencies(&mut multi_progress)
            .with_context(|| format_error_context!("While syncing dependencies"))?;

        self.export_buck_config(&mut multi_progress)
            .with_context(|| format_error_context!("While exporting buck config"))?;

        self.update_cargo(&mut multi_progress)
            .with_context(|| format_error_context!("While updating cargo"))?;

        self.workspace.save(&self.full_path).with_context(|| {
            format_error_context!("While saving workspace in {}", self.full_path)
        })?;

        let mut ledger = ledger::Ledger::new(context.clone())?;
        ledger
            .update(&self.full_path, &self.workspace)
            .with_context(|| {
                format_error_context!("While updating ledger with {}", self.full_path)
            })?;

        Ok(())
    }

    fn sync_repositories(
        &mut self,
        multi_progress: &mut printer::MultiProgress,
    ) -> anyhow::Result<()> {
        let mut handles = Vec::new();

        for (spaces_key, dependency) in self.workspace.repositories.iter() {
            self.deps_map.insert(spaces_key.clone(), dependency.clone());

            let mut progress_bar = multi_progress.add_progress(spaces_key, None, None);
            let context = self.context.clone();
            let spaces_key = spaces_key.to_owned();
            let dependency = dependency.clone();

            let handle = std::thread::spawn(move || {
                let bare_repository = git::BareRepository::new(
                    context,
                    &mut progress_bar,
                    &spaces_key,
                    &dependency.git,
                )?;

                Ok::<_, anyhow::Error>((bare_repository, dependency.clone()))
            });

            handles.push(handle);
        }

        for handle in handles {
            let result = handle
                .join()
                .expect("Internal Error: Failed to join thread");
            match result {
                Ok((bare_repository, dependency)) => {
                    self.all_deps
                        .push_back(SyncDep::Repository(bare_repository, dependency));
                }
                Err(e) => {
                    return Err(e);
                }
            }
        }

        Ok(())
    }

    fn sync_dependencies(
        &mut self,
        multi_progress: &mut printer::MultiProgress,
    ) -> anyhow::Result<()> {
        let mut handles: Vec<Option<std::thread::JoinHandle<Result<_, anyhow::Error>>>> =
            Vec::new();
        loop {
            if let Some(next_dep) = self.all_deps.pop_back() {
                let handle = match next_dep {
                    SyncDep::Archive(spaces_key, http_archive) => {
                        self.sync_archive(multi_progress, spaces_key, http_archive)?
                    }
                    SyncDep::PlatformArchive(spaces_key, http_archive) => {
                        self.sync_platform_archive(multi_progress, spaces_key, http_archive)?
                    }
                    SyncDep::BareRepository(spaces_key, dependency) => {
                        self.sync_bare_repository(multi_progress, spaces_key, dependency)?
                    }
                    SyncDep::Repository(bare_repository, dependency) => {
                        if !self
                            .workspace
                            .repositories
                            .contains_key(&bare_repository.spaces_key)
                        {
                            self.workspace
                                .dependencies
                                .insert(bare_repository.spaces_key.clone(), dependency.clone());
                        }

                        self.sync_dependency(multi_progress, bare_repository, dependency)?
                    }
                };

                handles.push(Some(handle));
            } else {
                let mut all_finished = true;
                for handle_option in handles.iter_mut() {
                    if handle_option.is_some() {
                        let handle = handle_option.take().unwrap();
                        if !handle.is_finished() {
                            all_finished = false;
                            *handle_option = Some(handle);
                        } else {
                            let sync_deps = handle
                                .join()
                                .expect("Internal Error: failed to join handle")?;

                            for sync_dep in sync_deps {
                                self.all_deps.push_back(sync_dep);
                            }

                            *handle_option = None;
                        }
                    }
                }

                if all_finished && self.all_deps.is_empty() {
                    break;
                } else {
                    std::thread::sleep(std::time::Duration::from_millis(500));
                }
            };
        }

        Ok(())
    }

    fn sync_bare_repository(
        &self,
        multi_progress: &mut printer::MultiProgress,
        spaces_key: String,
        dependency: Dependency,
    ) -> anyhow::Result<std::thread::JoinHandle<Result<Vec<SyncDep>, anyhow::Error>>> {
        let mut progress_bar = multi_progress.add_progress(&spaces_key, None, None);
        let context = self.context.clone();

        let handle = std::thread::spawn(move || {
            let mut new_deps = Vec::new();
            let bare_repository = git::BareRepository::new(
                context.clone(),
                &mut progress_bar,
                &spaces_key,
                &dependency.git,
            )?;
            new_deps.push(SyncDep::Repository(bare_repository, dependency));
            Ok::<_, anyhow::Error>(new_deps)
        });

        Ok(handle)
    }

    fn sync_archive(
        &self,
        multi_progress: &mut printer::MultiProgress,
        spaces_key: String,
        mut http_archive: archive::HttpArchive,
    ) -> anyhow::Result<std::thread::JoinHandle<Result<Vec<SyncDep>, anyhow::Error>>> {
        let progress_bar = multi_progress.add_progress(&spaces_key, Some(100), None);
        let context = self.context.clone();
        let full_path = self.full_path.clone();

        let handle = std::thread::spawn(move || {
            let mut new_deps = Vec::new();
            http_archive.sync(context.clone(), full_path.as_str(), progress_bar)?;

            if let Some(deps) =
                manifest::Deps::new(format!("{full_path}/{}", http_archive.spaces_key).as_str())?
            {
                new_deps.extend(Self::get_new_deps(
                    context.clone(),
                    &http_archive.spaces_key,
                    &deps,
                )?);
            }

            Ok::<_, anyhow::Error>(new_deps)
        });

        Ok(handle)
    }

    fn sync_platform_archive(
        &self,
        multi_progress: &mut printer::MultiProgress,
        spaces_key: String,
        mut http_archive: archive::HttpArchive,
    ) -> anyhow::Result<std::thread::JoinHandle<Result<Vec<SyncDep>, anyhow::Error>>> {
        let progress_bar = multi_progress.add_progress(&spaces_key, Some(100), None);
        let context = self.context.clone();
        let full_path = self.full_path.clone();

        let handle = std::thread::spawn(move || {
            http_archive.sync(context.clone(), full_path.as_str(), progress_bar)?;
            Ok::<_, anyhow::Error>(Vec::new())
        });

        Ok(handle)
    }

    fn get_new_deps(
        context: std::sync::Arc<context::Context>,
        parent_spaces_key: &String,
        spaces_deps: &manifest::Deps,
    ) -> anyhow::Result<Vec<SyncDep>> {
        let mut new_deps = Vec::new();
        for (spaces_key, dep) in spaces_deps.deps.iter() {
            new_deps.push(SyncDep::BareRepository(spaces_key.clone(), dep.clone()));
        }

        if let Some(map) = &spaces_deps.archives {
            for (key, archive) in map.iter() {
                let http_archive = archive::HttpArchive::new(context.clone(), key, archive)?;

                new_deps.push(SyncDep::Archive(key.clone(), http_archive));
            }
        }

        if let Some(map) = &spaces_deps.platform_archives {
            for (key, platform_archive) in map.iter() {
                if let Some(archive) = platform_archive.get_archive() {
                    let effective_key = if key.starts_with(manifest::SPACES_OVERLAY) {
                        key.replace(manifest::SPACES_OVERLAY, &parent_spaces_key)
                    } else {
                        key.to_owned()
                    };

                    let http_archive =
                        archive::HttpArchive::new(context.clone(), &effective_key, &archive)?;

                    new_deps.push(SyncDep::PlatformArchive(
                        effective_key.clone(),
                        http_archive,
                    ));
                }
            }
        }

        Ok(new_deps)
    }

    fn sync_dependency(
        &mut self,
        multi_progress: &mut printer::MultiProgress,
        bare_repository: BareRepository,
        dependency: Dependency,
    ) -> anyhow::Result<std::thread::JoinHandle<Result<Vec<SyncDep>, anyhow::Error>>> {
        let mut progress_bar = multi_progress.add_progress(&bare_repository.spaces_key, None, None);

        let context = self.context.clone();
        let full_path = self.full_path.to_string();

        let needs_checked_out = if self.deps_map.contains_key(&bare_repository.spaces_key) {
            //checked out for development
            false
        } else {
            self.deps_map
                .insert(bare_repository.spaces_key.clone(), dependency.clone());
            true
        };

        let handle = std::thread::spawn(move || {
            let mut new_deps = Vec::new();

            let parent_spaces_key = bare_repository.spaces_key.clone();

            let worktree =
                bare_repository.add_worktree(context.clone(), &mut progress_bar, &full_path)?;

            if needs_checked_out {
                worktree.checkout(context.clone(), &mut progress_bar, &dependency)?;
                worktree.checkout_detached_head(context.clone(), &mut progress_bar)?;
            }

            let spaces_deps = worktree.get_deps()?;
            if let Some(spaces_deps) = spaces_deps {
                new_deps.extend(Self::get_new_deps(
                    context.clone(),
                    &parent_spaces_key,
                    &spaces_deps,
                )?);

                if let Some(asset_map) = spaces_deps.assets {
                    for (key, asset) in asset_map.iter() {
                        let path = format!("{}/{key}", worktree.full_path);
                        let dest_path = format!("{full_path}/{}", asset.path);

                        match asset.type_ {
                            manifest::AssetType::HardLink => {
                                if std::path::Path::new(dest_path.as_str()).exists() {
                                    std::fs::remove_file(dest_path.as_str()).with_context(
                                        || format_error_context!("While removing {dest_path}"),
                                    )?;
                                }
                                std::fs::hard_link(path.as_str(), dest_path.as_str())
                                    .with_context(|| {
                                        format_error_context!(
                                            "While creating hard link from {path} to {dest_path}"
                                        )
                                    })?;
                            }
                            manifest::AssetType::Template => {
                                let contents =
                                    std::fs::read_to_string(&path).with_context(|| {
                                        format_error_context!("While reading {path}")
                                    })?;

                                // do the substitutions

                                // create a copy
                                std::fs::write(dest_path.as_str(), contents).with_context(
                                    || format_error_context!("While writing to {dest_path}"),
                                )?;
                            }
                        }
                    }
                }
            }

            Ok::<_, anyhow::Error>(new_deps)
        });

        Ok(handle)
    }

    fn export_buck_config(
        &mut self,
        _multi_progress: &mut printer::MultiProgress,
    ) -> anyhow::Result<()> {
        if let Some(buck) = self.workspace.buck.as_mut() {
            if buck.cells.is_none() {
                buck.cells = Some(HashMap::new());
            }

            if let Some(buck_cells) = buck.cells.as_mut() {
                for (key, _dep) in self.deps_map.iter() {
                    buck_cells.insert(key.clone(), format!("./{key}"));
                }
            }
            buck.export(&self.full_path)?;
        }
        Ok(())
    }

    fn update_cargo(&self, _multi_progress: &mut printer::MultiProgress) -> anyhow::Result<()> {
        let mut config_contents = String::new();

        if let Some(cargo) = self.workspace.get_cargo_patches() {
            for (spaces_key, list) in cargo.iter() {
                //read the cargo toml file to see how the dependency is specified crates-io or git
                let cargo_toml_path = format!("{}/{spaces_key}/Cargo.toml", self.full_path);
                let cargo_toml: toml::Value = {
                    let cargo_toml_contents = std::fs::read_to_string(&cargo_toml_path)?;
                    toml::from_str(&cargo_toml_contents)?
                };

                let dependencies = cargo_toml.get("dependencies").ok_or(anyhow::anyhow!(
                    "Cargo.toml does not have a dependencies section"
                ))?;

                for value in list {
                    let dependency = dependencies.get(value.as_str()).ok_or(anyhow::anyhow!(
                        "Cargo.toml does not have a dependency named {}",
                        value
                    ))?;

                    if let Some(git) = dependency.get("git").and_then(|e| e.as_str()) {
                        let patch = format!("[patch.'{}']\n", git);
                        let path = format!("{value} = {{ path = \"./{value}\" }}\n");
                        config_contents.push_str(patch.as_str());
                        config_contents.push_str(path.as_str());
                    }
                }
            }
        }

        fn write_cargo_section(
            config_contents: &mut String,
            section_name: &str,
            section: &HashMap<String, String>,
        ) {
            config_contents.push_str(&format!("[{}]\n", section_name));
            for (key, value) in section.iter() {
                config_contents.push_str(&format!("{} = \"{}\"\n", key, value));
            }
        }

        if let Some(build) = self.workspace.get_cargo_build() {
            write_cargo_section(&mut config_contents, "build", build);
        }

        if let Some(net) = self.workspace.get_cargo_net() {
            write_cargo_section(&mut config_contents, "net", net);
        }

        if let Some(http) = self.workspace.get_cargo_http() {
            write_cargo_section(&mut config_contents, "http", http);
        }

        if config_contents.is_empty() {
            return Ok(());
        }

        let config_path = format!("{}/.cargo", self.full_path);
        std::fs::create_dir_all(std::path::Path::new(&config_path))
            .with_context(|| format_error_context!("Trying to create {config_path}"))?;
        std::fs::write(format!("{config_path}/config.toml"), config_contents).with_context(
            || format_error_context!("While trying to write contents to {config_path}/config.toml"),
        )?;

        Ok(())
    }
}
