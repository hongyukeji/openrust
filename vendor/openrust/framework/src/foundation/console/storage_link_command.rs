use std::{
    collections::BTreeMap,
    fs, io,
    path::{Component, Path, PathBuf},
};

use clap::{Arg, ArgAction};

use crate::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output,
    },
    contracts::foundation::Application as _,
    foundation::Application,
};

struct StorageLinkCommand(CommandDefinition);

impl Command for StorageLinkCommand {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }

    fn handle<'a>(
        &'a self,
        application: &'a Application,
        input: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let relative = input.matches().get_flag("relative");
            let force = input.matches().get_flag("force");
            for (link, target) in links(application)? {
                let actual = if relative {
                    relative_path(
                        link.parent()
                            .ok_or_else(|| ConsoleError::new("storage link has no parent"))?,
                        &target,
                    )
                    .ok_or_else(|| {
                        ConsoleError::new("storage link and target must share a filesystem prefix")
                    })?
                } else {
                    target.clone()
                };
                if let Ok(metadata) = fs::symlink_metadata(&link) {
                    let symbolic = metadata.file_type().is_symlink();
                    if !symbolic || (!force && fs::metadata(&link).is_ok()) {
                        output
                            .stderr
                            .push_str(&format!("The [{}] link already exists.\n", link.display()));
                        continue;
                    }
                    remove_symlink(&link).map_err(command_error)?;
                }
                make_symlink(&actual, &link).map_err(command_error)?;
                output.info(&format!(
                    "The [{}] link has been connected to [{}].",
                    link.display(),
                    target.display()
                ));
            }
            Ok(0)
        })
    }
}

pub(super) fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    artisan.add_command(Box::new(StorageLinkCommand(CommandDefinition::new(
        clap::Command::new("storage:link")
            .about("Create the symbolic links configured for the application")
            .arg(
                Arg::new("relative")
                    .long("relative")
                    .action(ArgAction::SetTrue)
                    .help("Create the symbolic link using relative paths"),
            )
            .arg(
                Arg::new("force")
                    .long("force")
                    .action(ArgAction::SetTrue)
                    .help("Recreate existing symbolic links"),
            ),
    ))))
}

pub(super) fn links(application: &Application) -> Result<BTreeMap<PathBuf, PathBuf>, ConsoleError> {
    let config = application
        .configuration_repository()
        .map_err(|error| ConsoleError::new(error.to_string()))?;
    let base = application.base_path();
    match config
        .get("filesystems.links")
        .filter(|value| !value.is_null())
    {
        Some(links) => {
            let links: BTreeMap<PathBuf, PathBuf> = serde_json::from_value(links)
                .map_err(|_| ConsoleError::new("invalid filesystems.links configuration"))?;
            links
                .into_iter()
                .map(|(link, target)| {
                    if link.as_os_str().is_empty() || target.as_os_str().is_empty() {
                        return Err(ConsoleError::new(
                            "storage link and target must be nonempty",
                        ));
                    }
                    Ok((base.join(link), base.join(target)))
                })
                .collect()
        }
        None => Ok(BTreeMap::from([(
            application.publicPath().join("storage"),
            base.join("storage/app/public"),
        )])),
    }
}

pub(super) fn command_error(error: io::Error) -> ConsoleError {
    ConsoleError::new(format!("storage link operation failed: {error}"))
}

fn relative_path(parent: &Path, target: &Path) -> Option<PathBuf> {
    fn normalized(path: &Path) -> Vec<Component<'_>> {
        let mut parts = Vec::new();
        for part in path.components() {
            match part {
                Component::ParentDir if matches!(parts.last(), Some(Component::Normal(_))) => {
                    parts.pop();
                }
                Component::CurDir | Component::ParentDir => {}
                _ => parts.push(part),
            }
        }
        parts
    }
    let parent = normalized(parent);
    let target = normalized(target);
    let shared = parent
        .iter()
        .zip(&target)
        .take_while(|(left, right)| left == right)
        .count();
    if shared == 0 {
        return None;
    }
    let mut relative = PathBuf::new();
    for _ in &parent[shared..] {
        relative.push("..");
    }
    for part in &target[shared..] {
        relative.push(part.as_os_str());
    }
    if relative.as_os_str().is_empty() {
        relative.push(".");
    }
    Some(relative)
}

#[cfg(unix)]
fn make_symlink(target: &Path, link: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn make_symlink(target: &Path, link: &Path) -> io::Result<()> {
    let absolute = link.parent().unwrap_or(Path::new(".")).join(target);
    if absolute.is_file() {
        std::os::windows::fs::symlink_file(target, link)
    } else {
        std::os::windows::fs::symlink_dir(target, link)
    }
}

#[cfg(not(any(unix, windows)))]
fn make_symlink(_: &Path, _: &Path) -> io::Result<()> {
    Err(io::ErrorKind::Unsupported.into())
}

pub(super) fn remove_symlink(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_symlink() {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileTypeExt as _;
        if metadata.file_type().is_symlink_dir() {
            return fs::remove_dir(path);
        }
    }
    fs::remove_file(path)
}
