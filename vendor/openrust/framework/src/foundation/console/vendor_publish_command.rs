use std::{
    collections::BTreeSet,
    fs,
    io::{self, IsTerminal as _},
    path::{Component, Path, PathBuf},
    sync::atomic::Ordering,
};

use chrono::{NaiveDateTime, TimeDelta, Utc};
use clap::{Arg, ArgAction, Command as Syntax};
use console::{Key, Term, truncate_str};

use crate::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output, generator_command::write_generated,
    },
    contracts::foundation::Application as _,
    foundation::{Application, events::VendorTagPublished},
    support::{PublishableResource, facades::Event, publishing::PublishSource},
};

pub(super) fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    artisan.add_command(Box::new(VendorPublishCommand(CommandDefinition::new(
        Syntax::new("vendor:publish")
            .about("Publish files supplied by installed packages")
            .arg(
                Arg::new("provider")
                    .long("provider")
                    .help("The service provider that has assets you want to publish"),
            )
            .arg(
                Arg::new("tag")
                    .long("tag")
                    .action(ArgAction::Append)
                    .help("One or many tags that have assets you want to publish"),
            )
            .arg(
                Arg::new("all")
                    .long("all")
                    .action(ArgAction::SetTrue)
                    .help("Publish assets for all service providers without prompt"),
            )
            .arg(
                Arg::new("force")
                    .long("force")
                    .action(ArgAction::SetTrue)
                    .help("Overwrite any existing files"),
            )
            .arg(
                Arg::new("existing")
                    .long("existing")
                    .action(ArgAction::SetTrue)
                    .help("Publish and overwrite only the files that have already been published"),
            ),
    ))))
}

pub struct VendorPublishCommand(CommandDefinition);

impl VendorPublishCommand {
    /// Preserve package migration timestamps for this application.
    #[allow(non_snake_case, reason = "Matches the Artisan publishing API")]
    pub fn dontUpdateMigrationDates(app: &Application) {
        app.shared
            .keep_published_migration_dates
            .store(true, Ordering::Relaxed);
    }
}

impl Command for VendorPublishCommand {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }

    fn handle<'a>(
        &'a self,
        app: &'a Application,
        input: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let resources = app
                .publishable()
                .lock()
                .expect("publish registry lock")
                .clone();
            let options = input.matches();
            let provider = options.get_one::<String>("provider").cloned();
            let tags: Vec<_> = options
                .get_many::<String>("tag")
                .into_iter()
                .flatten()
                .cloned()
                .collect();
            let all = options.get_flag("all");
            let interactive = !options.get_flag("no-interaction");
            let existing = options.get_flag("existing");
            let force = options.get_flag("force");
            let root = app.base_path().to_path_buf();
            let (provider, tags) = if all {
                (None, Vec::new())
            } else if provider.is_none() && tags.is_empty() {
                if !interactive || !output.streams_to_terminal() || !io::stdin().is_terminal() {
                    output.info("No resources published.");
                    return Ok(0);
                }
                let choices = resources.clone();
                let selection = tokio::task::spawn_blocking(move || select(&choices))
                    .await
                    .map_err(console_error)?
                    .map_err(console_error)?;
                let Some(selection) = selection else {
                    output.info("No resources published.");
                    return Ok(0);
                };
                selection
            } else {
                (provider, tags)
            };
            let tags = if tags.is_empty() {
                vec![None]
            } else {
                tags.into_iter().map(Some).collect()
            };
            let mut timestamp = migration_publication_timestamp(app)?;
            for tag in tags {
                let selected = paths_to_publish(&resources, provider.as_deref(), tag.as_deref());
                if selected.is_empty() {
                    output.info(&format!(
                        "No publishable resources for tag [{}].",
                        tag.as_deref().unwrap_or_default()
                    ));
                    continue;
                }
                match &tag {
                    Some(tag) => output.info(&format!("Publishing [{tag}] assets")),
                    None => output.info("Publishing assets"),
                }
                let paths = selected
                    .iter()
                    .map(|(source, asset)| (source.clone(), root.join(&asset.path)))
                    .collect();
                let publish_root = root.clone();
                let result = tokio::task::spawn_blocking(move || {
                    publish(
                        &publish_root,
                        selected.into_values(),
                        existing,
                        force,
                        &mut timestamp,
                    )
                    .map(|publication| (publication, timestamp))
                })
                .await
                .map_err(console_error)?
                .map_err(console_error)?;
                let (publication, next_timestamp) = result;
                timestamp = next_timestamp;
                for notice in publication.notices {
                    match notice {
                        PublishNotice::Missing(source) => output
                            .stderr
                            .push_str(&format!("Can't locate path: <{}>\n", source.display())),
                        PublishNotice::Copied(kind, source, target) => output.info(&format!(
                            "Copying {kind} [{}] to [{}]",
                            publish_display(&root, &source).display(),
                            publish_display(&root, &target).display()
                        )),
                        PublishNotice::Skipped(target, reason) => output.comment(&format!(
                            "File [{}] {reason} SKIPPED",
                            publish_display(&root, &target).display()
                        )),
                    }
                }
                Event::dispatch(VendorTagPublished { tag, paths })
                    .await
                    .map_err(console_error)?;
            }
            Ok(0)
        })
    }
}

pub(super) fn migration_publication_timestamp(
    app: &Application,
) -> Result<Option<NaiveDateTime>, ConsoleError> {
    Ok(app
        .config()
        .map_err(console_error)?
        .get("database.migrations.update_date_on_publish")
        .and_then(|value| value.as_bool())
        .filter(|enabled| {
            *enabled
                && !app
                    .shared
                    .keep_published_migration_dates
                    .load(Ordering::Relaxed)
        })
        .map(|_| Utc::now().naive_utc()))
}

fn paths_to_publish(
    resources: &[PublishableResource],
    provider: Option<&str>,
    tag: Option<&str>,
) -> indexmap::IndexMap<PathBuf, PublishableResource> {
    let group: indexmap::IndexMap<_, _> = resources
        .iter()
        .filter(|asset| tag.is_some_and(|name| name == asset.tag))
        .map(|asset| (asset.source_path(), asset.clone()))
        .collect();
    if provider.is_none() && tag.is_some() {
        return group;
    }
    let mut providers = indexmap::IndexMap::<_, indexmap::IndexMap<_, _>>::new();
    for asset in resources
        .iter()
        .filter(|asset| provider.is_none_or(|name| name == asset.provider))
    {
        providers
            .entry(asset.provider)
            .or_default()
            .insert(asset.source_path(), asset.clone());
    }
    providers
        .into_values()
        .flatten()
        .filter(|(source, _)| tag.is_none() || group.contains_key(source))
        .collect()
}

type Selection = (Option<String>, Vec<String>);

fn console_error(error: impl std::fmt::Display) -> ConsoleError {
    ConsoleError::new(error.to_string())
}

fn select(resources: &[PublishableResource]) -> io::Result<Option<Selection>> {
    if resources.is_empty() {
        return Ok(None);
    }
    let providers: BTreeSet<_> = resources.iter().map(|asset| asset.provider).collect();
    let tags: BTreeSet<_> = resources.iter().map(|asset| asset.tag.as_str()).collect();
    let choices: Vec<_> =
        std::iter::once(("All providers and tags".to_owned(), (None, Vec::new())))
            .chain(providers.into_iter().map(|name| {
                (
                    format!("Provider: {name}"),
                    (Some(name.to_owned()), Vec::new()),
                )
            }))
            .chain(
                tags.into_iter()
                    .map(|name| (format!("Tag: {name}"), (None, vec![name.to_owned()]))),
            )
            .collect();
    let terminal = Term::stderr();
    if !terminal.is_term() {
        return Ok(None);
    }
    let labels: Vec<_> = choices
        .iter()
        .map(|(label, _)| label.to_lowercase())
        .collect();
    let mut query = String::new();
    let mut selected = 0_usize;
    let mut previous_lines = 0;
    let result = loop {
        let (rows, columns) = terminal.size();
        let visible = usize::from(rows.saturating_sub(3)).clamp(1, 15);
        let width = usize::from(columns.saturating_sub(1)).max(1);
        let search = query.to_lowercase();
        let matches: Vec<_> = labels
            .iter()
            .enumerate()
            .filter_map(|(index, label)| label.contains(&search).then_some(index))
            .collect();
        let start = selected.saturating_sub(visible - 1);
        terminal.clear_last_lines(previous_lines)?;
        terminal.write_line(&truncate_str(
            "Which provider or tag's files would you like to publish?",
            width,
            "",
        ))?;
        let prompt = if query.is_empty() {
            "Search...".to_owned()
        } else {
            format!("Search: {query}")
        };
        terminal.write_line(&truncate_str(&prompt, width, ""))?;
        previous_lines = 2;
        if matches.is_empty() {
            terminal.write_line(&truncate_str("No matches.", width, ""))?;
            previous_lines += 1;
        }
        for (offset, &index) in matches.iter().enumerate().skip(start).take(visible) {
            let marker = if offset == selected { "> " } else { "  " };
            let label = format!("{marker}{}", choices[index].0);
            terminal.write_line(&truncate_str(&label, width, ""))?;
            previous_lines += 1;
        }
        terminal.flush()?;
        match terminal.read_key_raw() {
            Ok(Key::Enter) => {
                if let Some(&index) = matches.get(selected) {
                    break Ok(Some(choices[index].1.clone()));
                }
            }
            Ok(Key::ArrowUp) if !matches.is_empty() => {
                selected = if selected == 0 {
                    matches.len() - 1
                } else {
                    selected - 1
                };
            }
            Ok(Key::ArrowDown) if !matches.is_empty() => {
                selected = (selected + 1) % matches.len();
            }
            Ok(Key::Char(character)) if !character.is_control() => {
                query.push(character);
                selected = 0;
            }
            Ok(Key::Backspace) => {
                query.pop();
                selected = 0;
            }
            Ok(Key::Escape | Key::CtrlC) => break Ok(None),
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => break Ok(None),
            Err(error) => break Err(error),
            _ => {}
        }
    };
    terminal.clear_last_lines(previous_lines)?;
    if let Ok(Some(ref selection)) = result
        && let Some((label, _)) = choices.iter().find(|(_, choice)| choice == selection)
    {
        terminal.write_line(label)?;
    }
    terminal.flush()?;
    result
}

#[derive(Default)]
struct Publication {
    notices: Vec<PublishNotice>,
}

enum PublishNotice {
    Missing(PathBuf),
    Copied(&'static str, PathBuf, PathBuf),
    Skipped(PathBuf, &'static str),
}

fn publish_display<'a>(root: &Path, path: &'a Path) -> &'a Path {
    path.strip_prefix(root).unwrap_or(path)
}

fn file_notice(source: PathBuf, target: PathBuf, skipped: Option<&'static str>) -> PublishNotice {
    match skipped {
        Some(reason) => PublishNotice::Skipped(target, reason),
        None => PublishNotice::Copied("file", source, target),
    }
}

fn publish(
    root: &Path,
    resources: impl IntoIterator<Item = PublishableResource>,
    existing: bool,
    force: bool,
    timestamp: &mut Option<NaiveDateTime>,
) -> io::Result<Publication> {
    let _source_lock = if root.join("Cargo.toml").try_exists()? {
        Some(super::source_edit::lock_manifest(root).map_err(io::Error::other)?)
    } else {
        None
    };
    publish_locked(root, resources, existing, force, timestamp)
}

pub(super) fn publish_package_if_migration_missing(
    root: &Path,
    resources: impl IntoIterator<Item = PublishableResource>,
    mut timestamp: Option<NaiveDateTime>,
) -> io::Result<Option<VendorTagPublished>> {
    let resources: Vec<_> = resources.into_iter().collect();
    for asset in &resources {
        if asset.migration && published_migration(root, &root.join(&asset.path))?.is_some() {
            return Ok(None);
        }
    }
    let paths = resources
        .iter()
        .map(|asset| (asset.source_path(), root.join(&asset.path)))
        .collect();
    publish_locked(root, resources, false, false, &mut timestamp)?;
    Ok(Some(VendorTagPublished { tag: None, paths }))
}

fn publish_locked(
    root: &Path,
    resources: impl IntoIterator<Item = PublishableResource>,
    existing: bool,
    force: bool,
    timestamp: &mut Option<NaiveDateTime>,
) -> io::Result<Publication> {
    let mut publication = Publication::default();
    for resource in resources {
        let mut target = root.join(&resource.path);
        if resource.migration {
            target = migration_destination(root, target, timestamp)?;
        }
        let source_path = resource.source_path();
        match resource.source {
            PublishSource::Embedded(bytes) => {
                let skipped = publish_file(root, &target, existing, force, || Ok(bytes))?;
                publication
                    .notices
                    .push(file_notice(source_path, target, skipped));
            }
            PublishSource::Path(source) => {
                if source.starts_with(root) {
                    checked_path(root, &source)?;
                }
                let kind = match fs::symlink_metadata(&source) {
                    Ok(meta) => meta.file_type(),
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {
                        publication.notices.push(PublishNotice::Missing(source));
                        continue;
                    }
                    Err(error) => return Err(error),
                };
                if kind.is_file() {
                    let skipped =
                        publish_file(root, &target, existing, force, || fs::read(&source))?;
                    publication
                        .notices
                        .push(file_notice(source, target, skipped));
                } else if kind.is_dir() {
                    let mut files = Vec::new();
                    directory_files(&source, Path::new(""), &mut files)?;
                    for path in files {
                        let mut destination = target.join(&path);
                        if resource.migration {
                            destination = migration_destination(root, destination, timestamp)?;
                        }
                        publish_file(root, &destination, existing, force, || {
                            fs::read(source.join(&path))
                        })?;
                    }
                    publication
                        .notices
                        .push(PublishNotice::Copied("directory", source, target));
                } else {
                    return Err(invalid_path(&source));
                }
            }
        }
    }
    Ok(publication)
}

fn directory_files(source: &Path, relative: &Path, files: &mut Vec<PathBuf>) -> io::Result<()> {
    let mut entries = fs::read_dir(source.join(relative))?.collect::<io::Result<Vec<_>>>()?;
    entries.sort_unstable_by_key(fs::DirEntry::file_name);
    for entry in entries {
        let kind = entry.file_type()?;
        let path = relative.join(entry.file_name());
        if kind.is_dir() {
            directory_files(source, &path, files)?;
        } else if kind.is_file() {
            files.push(path);
        } else {
            return Err(invalid_path(&entry.path()));
        }
    }
    Ok(())
}

fn publish_file<B: AsRef<[u8]>>(
    root: &Path,
    target: &Path,
    existing: bool,
    force: bool,
    bytes: impl FnOnce() -> io::Result<B>,
) -> io::Result<Option<&'static str>> {
    checked_path(root, target)?;
    let exists = target.try_exists()?;
    if existing && !exists {
        return Ok(Some("does not exist"));
    }
    if !existing && !force && exists {
        return Ok(Some("already exists"));
    }
    match write_generated(target, bytes()?.as_ref(), exists && (force || existing)) {
        Ok(()) => Ok(None),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists && !force && !existing => {
            Ok(Some("already exists"))
        }
        Err(error) => Err(io::Error::new(
            error.kind(),
            format!("{}: {error}", target.display()),
        )),
    }
}

fn checked_path(root: &Path, target: &Path) -> io::Result<()> {
    let relative = target
        .strip_prefix(root)
        .map_err(|_| invalid_path(target))?;
    if relative
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(invalid_path(target));
    }
    let mut current = root.to_owned();
    for component in relative.components() {
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(meta) if meta.file_type().is_symlink() => return Err(invalid_path(&current)),
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => break,
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn invalid_path(path: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("Unsupported publish path: {}", path.display()),
    )
}

fn migration_destination(
    root: &Path,
    original: PathBuf,
    timestamp: &mut Option<NaiveDateTime>,
) -> io::Result<PathBuf> {
    let Some((_, suffix)) = original
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(migration_name)
    else {
        return Ok(original);
    };
    if let Some(path) = published_migration(root, &original)? {
        return Ok(path);
    }
    let parent = original.parent().expect("publication path has a parent");
    let Some(timestamp) = timestamp else {
        return Ok(original);
    };
    *timestamp += TimeDelta::seconds(1);
    let path = parent.join(format!("{}_{suffix}", timestamp.format("%Y_%m_%d_%H%M%S")));
    Ok(path)
}

fn published_migration(root: &Path, original: &Path) -> io::Result<Option<PathBuf>> {
    checked_path(root, original)?;
    let Some((_, suffix)) = original
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(migration_name)
    else {
        return original
            .try_exists()
            .map(|exists| exists.then(|| original.to_owned()));
    };
    let parent = original.parent().expect("publication path has a parent");
    let mut existing = BTreeSet::new();
    match fs::read_dir(parent) {
        Ok(entries) => {
            for entry in entries {
                let entry = entry?;
                if entry
                    .file_name()
                    .to_str()
                    .and_then(migration_name)
                    .is_some_and(|(_, name)| name == suffix)
                {
                    existing.insert(entry.path());
                }
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    if existing.len() > 1 {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("multiple published migrations match {suffix}"),
        ));
    }
    Ok(existing.pop_first())
}

fn migration_name(name: &str) -> Option<(NaiveDateTime, &str)> {
    let prefix = name.get(..17)?;
    let suffix = name.get(17..)?.strip_prefix('_')?;
    Some((
        NaiveDateTime::parse_from_str(prefix, "%Y_%m_%d_%H%M%S").ok()?,
        suffix,
    ))
}
