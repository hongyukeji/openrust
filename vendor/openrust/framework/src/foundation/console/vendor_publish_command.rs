use std::{
    collections::BTreeSet,
    fs,
    io::{self, IsTerminal as _},
    path::{Path, PathBuf},
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
    artisan.add_command(Box::new(Publish(CommandDefinition::new(
        Syntax::new("vendor:publish")
            .about("Publish files supplied by installed packages")
            .arg(Arg::new("provider").long("provider"))
            .arg(Arg::new("tag").long("tag").action(ArgAction::Append))
            .arg(Arg::new("all").long("all").action(ArgAction::SetTrue))
            .arg(Arg::new("force").long("force").action(ArgAction::SetTrue))
            .arg(
                Arg::new("existing")
                    .long("existing")
                    .action(ArgAction::SetTrue),
            ),
    ))))
}

struct Publish(CommandDefinition);

impl Command for Publish {
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
            let mut timestamp = Utc::now().naive_utc();
            let mut any_written = false;
            for tag in tags {
                let selected: indexmap::IndexMap<_, _> = resources
                    .iter()
                    .filter(|asset| {
                        provider
                            .as_deref()
                            .is_none_or(|name| name == asset.provider)
                            && tag.as_ref().is_none_or(|name| name == &asset.tag)
                    })
                    .map(|asset| (asset.source_path(), asset.clone()))
                    .collect();
                if selected.is_empty() {
                    continue;
                }
                let paths = selected
                    .iter()
                    .map(|(source, asset)| (source.clone(), root.join(&asset.path)))
                    .collect();
                let root = root.clone();
                let result = tokio::task::spawn_blocking(move || {
                    publish(
                        &root,
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
                for missing in publication.missing {
                    output
                        .stderr
                        .push_str(&format!("Can't locate path: <{}>\n", missing.display()));
                }
                for path in publication.written {
                    any_written = true;
                    output.info(&format!("Published {}.", path.display()));
                }
                Event::dispatch(VendorTagPublished { tag, paths })
                    .await
                    .map_err(console_error)?;
            }
            if !any_written {
                output.info("No resources published.");
            }
            Ok(0)
        })
    }
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
    written: Vec<PathBuf>,
    missing: Vec<PathBuf>,
}

fn publish(
    root: &Path,
    resources: impl IntoIterator<Item = PublishableResource>,
    existing: bool,
    force: bool,
    timestamp: &mut NaiveDateTime,
) -> io::Result<Publication> {
    let _manifest = match fs::File::open(root.join("Cargo.toml")) {
        Ok(file) => {
            file.lock()?;
            Some(file)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    let mut publication = Publication::default();
    for resource in resources {
        let mut target = root.join(&resource.path);
        if resource.migration {
            target = migration_destination(target, timestamp)?;
        }
        match resource.source {
            PublishSource::Embedded(bytes) => {
                publish_file(
                    root,
                    &target,
                    existing,
                    force,
                    || Ok(bytes),
                    &mut publication,
                )?;
            }
            PublishSource::Path(source) => {
                let kind = match fs::symlink_metadata(&source) {
                    Ok(meta) => meta.file_type(),
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {
                        publication.missing.push(source);
                        continue;
                    }
                    Err(error) => return Err(error),
                };
                if kind.is_file() {
                    publish_file(
                        root,
                        &target,
                        existing,
                        force,
                        || fs::read(&source),
                        &mut publication,
                    )?;
                } else if kind.is_dir() {
                    let mut files = Vec::new();
                    directory_files(&source, Path::new(""), &mut files)?;
                    for path in files {
                        publish_file(
                            root,
                            &target.join(&path),
                            existing,
                            force,
                            || fs::read(source.join(&path)),
                            &mut publication,
                        )?;
                    }
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
    publication: &mut Publication,
) -> io::Result<()> {
    let relative = target
        .strip_prefix(root)
        .map_err(|_| invalid_path(target))?;
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
    let exists = target.try_exists()?;
    if (existing && !exists) || (!existing && !force && exists) {
        return Ok(());
    }
    match write_generated(target, bytes()?.as_ref(), exists && (force || existing)) {
        Ok(()) => publication.written.push(relative.to_owned()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists && !force && !existing => {}
        Err(error) => {
            return Err(io::Error::new(
                error.kind(),
                format!("{}: {error}", target.display()),
            ));
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

fn migration_destination(original: PathBuf, timestamp: &mut NaiveDateTime) -> io::Result<PathBuf> {
    let Some((_, suffix)) = original
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(migration_name)
    else {
        return Ok(original);
    };
    if original.try_exists()? {
        return Ok(original);
    }
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
    if let Some(path) = existing.pop_first() {
        return Ok(path);
    }
    *timestamp += TimeDelta::seconds(1);
    let path = parent.join(format!("{}_{suffix}", timestamp.format("%Y_%m_%d_%H%M%S")));
    Ok(path)
}

fn migration_name(name: &str) -> Option<(NaiveDateTime, &str)> {
    let prefix = name.get(..17)?;
    let suffix = name.get(17..)?.strip_prefix('_')?;
    Some((
        NaiveDateTime::parse_from_str(prefix, "%Y_%m_%d_%H%M%S").ok()?,
        suffix,
    ))
}
