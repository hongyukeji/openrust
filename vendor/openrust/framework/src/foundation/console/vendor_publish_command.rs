use std::{
    collections::BTreeSet,
    fs,
    io::{self, IsTerminal as _, Write as _},
    path::{Path, PathBuf},
};

use chrono::{NaiveDateTime, TimeDelta, Utc};
use clap::{Arg, ArgAction, Command as Syntax};

use crate::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output, generator_command::write_generated,
    },
    contracts::foundation::Application as _,
    foundation::Application,
    support::PublishableResource,
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
            let paths = tokio::task::spawn_blocking(move || {
                let (provider, tags) = if all {
                    (None, Vec::new())
                } else if provider.is_none() && tags.is_empty() {
                    if !interactive || !io::stdin().is_terminal() {
                        return Ok(Vec::new());
                    }
                    let Some(selection) = select(&resources)? else {
                        return Ok(Vec::new());
                    };
                    selection
                } else {
                    (provider, tags)
                };
                publish(
                    &root,
                    &resources,
                    provider.as_deref(),
                    &tags,
                    existing,
                    force,
                )
            })
            .await
            .map_err(|error| ConsoleError::new(error.to_string()))?
            .map_err(|error| ConsoleError::new(error.to_string()))?;
            if paths.is_empty() {
                output.info("No resources published.");
            }
            for path in paths {
                output.info(&format!("Published {}.", path.display()));
            }
            Ok(0)
        })
    }
}

type Selection = (Option<String>, Vec<String>);

fn select(resources: &[PublishableResource]) -> io::Result<Option<Selection>> {
    if resources.is_empty() {
        return Ok(None);
    }
    let providers: BTreeSet<_> = resources.iter().map(|asset| asset.provider).collect();
    let tags: BTreeSet<_> = resources.iter().map(|asset| asset.tag.as_str()).collect();
    let mut choices = Vec::from([(None, Vec::new())]);
    let mut stderr = io::stderr().lock();
    writeln!(
        stderr,
        "Which provider or tag's files would you like to publish?"
    )?;
    writeln!(stderr, "0) All providers and tags")?;
    for provider in providers {
        writeln!(stderr, "{}) Provider: {provider}", choices.len())?;
        choices.push((Some(provider.to_owned()), Vec::new()));
    }
    for tag in tags {
        writeln!(stderr, "{}) Tag: {tag}", choices.len())?;
        choices.push((None, vec![tag.to_owned()]));
    }
    loop {
        write!(stderr, "Selection (blank to cancel): ")?;
        stderr.flush()?;
        let mut line = String::new();
        if io::stdin().read_line(&mut line)? == 0 || line.trim().is_empty() {
            return Ok(None);
        }
        if let Some(choice) = line
            .trim()
            .parse::<usize>()
            .ok()
            .and_then(|index| choices.get(index))
        {
            return Ok(Some(choice.clone()));
        }
        writeln!(stderr, "Choose a number from the list.")?;
    }
}

fn publish(
    root: &Path,
    resources: &[PublishableResource],
    provider: Option<&str>,
    tags: &[String],
    existing: bool,
    force: bool,
) -> io::Result<Vec<PathBuf>> {
    let _manifest = match fs::File::open(root.join("Cargo.toml")) {
        Ok(file) => {
            file.lock()?;
            Some(file)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    let mut published = Vec::new();
    let mut seen = BTreeSet::new();
    let mut timestamp = Utc::now().naive_utc();
    for resource in resources {
        if provider.is_some_and(|provider| provider != resource.provider)
            || (!tags.is_empty() && !tags.contains(&resource.tag))
            || !seen.insert(&resource.path)
        {
            continue;
        }
        let mut path = root.join(&resource.path);
        if resource.migration {
            path = migration_destination(path, &mut timestamp)?;
        }
        let exists = path.try_exists()?;
        if (existing && !exists) || (!existing && !force && exists) {
            continue;
        }
        match write_generated(&path, resource.bytes, exists && (force || existing)) {
            Ok(()) => published.push(path.strip_prefix(root).unwrap_or(&path).to_owned()),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists && !force && !existing => {}
            Err(error) => {
                return Err(io::Error::new(
                    error.kind(),
                    format!("{}: {error}", path.display()),
                ));
            }
        }
    }
    Ok(published)
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
