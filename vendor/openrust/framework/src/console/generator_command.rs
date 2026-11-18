use std::{
    borrow::Cow,
    fs::{self, OpenOptions},
    io::{self, ErrorKind, Write as _},
    path::{Path, PathBuf},
};

use crate::foundation::Application;

use super::{Command, ConsoleError, Input};

/// Supplies the primary destination and stub for a file generator.
pub trait GeneratorCommand: Command {
    fn get_path(
        &self,
        app: &Application,
        name: &str,
        input: &Input,
    ) -> Result<PathBuf, ConsoleError>;

    fn get_stub(&self, app: &Application, input: &Input)
    -> Result<Cow<'static, str>, ConsoleError>;
}

pub(crate) fn write_generated(path: &Path, bytes: &[u8], force: bool) -> io::Result<()> {
    let directory = path.parent().expect("generator path has a parent");
    fs::create_dir_all(directory)?;
    if !force {
        return write_new(path, bytes);
    }
    let temporary = directory.join(format!(".generator-{:016x}.tmp", fastrand::u64(..)));
    write_new(&temporary, bytes)?;
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(temporary);
        return Err(error);
    }
    Ok(())
}

fn write_new(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    if let Err(error) = file.write_all(bytes) {
        drop(file);
        let _ = fs::remove_file(path);
        return Err(error);
    }
    Ok(())
}

pub(crate) fn snake_case(name: &str) -> String {
    let bytes = name.as_bytes();
    let mut snake = String::new();
    for (index, byte) in bytes.iter().copied().enumerate() {
        if index > 0
            && byte.is_ascii_uppercase()
            && (bytes[index - 1].is_ascii_lowercase()
                || bytes[index - 1].is_ascii_digit()
                || bytes.get(index + 1).is_some_and(u8::is_ascii_lowercase))
        {
            snake.push('_');
        }
        snake.push(char::from(byte.to_ascii_lowercase()));
    }
    snake
}

pub(crate) fn source_name_parts<'a>(
    name: &'a str,
    invalid_message: &'static str,
) -> Result<impl Iterator<Item = &'a str>, ConsoleError> {
    let parts = name.strip_suffix(".rs").unwrap_or(name).split(['/', '\\']);
    if parts.clone().any(|part| {
        !part.as_bytes().first().is_some_and(u8::is_ascii_uppercase)
            || !part.bytes().all(|byte| byte.is_ascii_alphanumeric())
            || matches!(
                snake_case(part).as_str(),
                "self" | "super" | "crate" | "mod"
            )
    }) {
        return Err(ConsoleError::new(invalid_message));
    }
    Ok(parts)
}

/// A built-in source template and its application override.
#[derive(Clone, Copy)]
pub(crate) enum Stub {
    Job,
    QueuedJob,
    BatchedQueuedJob,
    JobMiddleware,
    Event,
    Listener,
    TypedListener,
    QueuedListener,
    TypedQueuedListener,
    Class,
    InvokableClass,
    Enum,
    IntEnum,
    StringEnum,
    Trait,
    Console,
    Factory,
    Migration,
    MigrationCreate,
    MigrationUpdate,
    Model,
    Mail,
    MarkdownMail,
    ViewMail,
    Notification,
    MarkdownNotification,
    Observer,
    PlainObserver,
    Policy,
    Provider,
    Resource,
    JsonApiResource,
    ResourceCollection,
    Seeder,
    Test,
    UnitTest,
    ViewComponent,
}

impl Stub {
    pub(crate) const ALL: [Self; 37] = [
        Self::Job,
        Self::QueuedJob,
        Self::BatchedQueuedJob,
        Self::JobMiddleware,
        Self::Event,
        Self::Listener,
        Self::TypedListener,
        Self::QueuedListener,
        Self::TypedQueuedListener,
        Self::Class,
        Self::InvokableClass,
        Self::Enum,
        Self::IntEnum,
        Self::StringEnum,
        Self::Trait,
        Self::Console,
        Self::Factory,
        Self::Migration,
        Self::MigrationCreate,
        Self::MigrationUpdate,
        Self::Model,
        Self::Mail,
        Self::MarkdownMail,
        Self::ViewMail,
        Self::Notification,
        Self::MarkdownNotification,
        Self::Observer,
        Self::PlainObserver,
        Self::Policy,
        Self::Provider,
        Self::Resource,
        Self::JsonApiResource,
        Self::ResourceCollection,
        Self::Seeder,
        Self::Test,
        Self::UnitTest,
        Self::ViewComponent,
    ];

    pub(crate) fn file(self) -> (&'static str, &'static str) {
        const MIGRATION: &str = include_str!("../database/migrations/stubs/migration.rs.stub");
        match self {
            Self::Mail => (
                "mail.rs.stub",
                include_str!("../foundation/console/stubs/mail.rs.stub"),
            ),
            Self::MarkdownMail => (
                "markdown-mail.rs.stub",
                include_str!("../foundation/console/stubs/markdown-mail.rs.stub"),
            ),
            Self::ViewMail => (
                "view-mail.rs.stub",
                include_str!("../foundation/console/stubs/view-mail.rs.stub"),
            ),
            Self::Notification => (
                "notification.rs.stub",
                include_str!("../foundation/console/stubs/notification.rs.stub"),
            ),
            Self::MarkdownNotification => (
                "markdown-notification.rs.stub",
                include_str!("../foundation/console/stubs/markdown-notification.rs.stub"),
            ),
            Self::Class => (
                "class.rs.stub",
                include_str!("../foundation/console/stubs/class.rs.stub"),
            ),
            Self::InvokableClass => (
                "class.invokable.rs.stub",
                include_str!("../foundation/console/stubs/class.invokable.rs.stub"),
            ),
            Self::Enum => (
                "enum.rs.stub",
                include_str!("../foundation/console/stubs/enum.rs.stub"),
            ),
            Self::IntEnum => (
                "enum.int.rs.stub",
                include_str!("../foundation/console/stubs/enum.int.rs.stub"),
            ),
            Self::StringEnum => (
                "enum.string.rs.stub",
                include_str!("../foundation/console/stubs/enum.string.rs.stub"),
            ),
            Self::Trait => (
                "trait.rs.stub",
                include_str!("../foundation/console/stubs/trait.rs.stub"),
            ),
            Self::Job => (
                "job.rs.stub",
                include_str!("../foundation/console/stubs/job.rs.stub"),
            ),
            Self::QueuedJob => (
                "job.queued.rs.stub",
                include_str!("../foundation/console/stubs/job.queued.rs.stub"),
            ),
            Self::BatchedQueuedJob => (
                "job.batched.queued.rs.stub",
                include_str!("../foundation/console/stubs/job.batched.queued.rs.stub"),
            ),
            Self::JobMiddleware => (
                "job.middleware.rs.stub",
                include_str!("../foundation/console/stubs/job.middleware.rs.stub"),
            ),
            Self::Event => (
                "event.rs.stub",
                include_str!("../foundation/console/stubs/event.rs.stub"),
            ),
            Self::Listener => (
                "listener.rs.stub",
                include_str!("../foundation/console/stubs/listener.rs.stub"),
            ),
            Self::TypedListener => (
                "listener.typed.rs.stub",
                include_str!("../foundation/console/stubs/listener.typed.rs.stub"),
            ),
            Self::QueuedListener => (
                "listener.queued.rs.stub",
                include_str!("../foundation/console/stubs/listener.queued.rs.stub"),
            ),
            Self::TypedQueuedListener => (
                "listener.typed.queued.rs.stub",
                include_str!("../foundation/console/stubs/listener.typed.queued.rs.stub"),
            ),
            Self::Console => (
                "console.rs.stub",
                include_str!("../foundation/console/stubs/console.rs.stub"),
            ),
            Self::Factory => (
                "factory.rs.stub",
                include_str!("../database/console/factories/stubs/factory.rs.stub"),
            ),
            Self::Migration => ("migration.rs.stub", MIGRATION),
            Self::MigrationCreate => ("migration.create.rs.stub", MIGRATION),
            Self::MigrationUpdate => ("migration.update.rs.stub", MIGRATION),
            Self::Model => (
                "model.rs.stub",
                include_str!("../foundation/console/stubs/model.rs.stub"),
            ),
            Self::Observer => (
                "observer.rs.stub",
                include_str!("../foundation/console/stubs/observer.rs.stub"),
            ),
            Self::PlainObserver => (
                "observer.plain.rs.stub",
                include_str!("../foundation/console/stubs/observer.plain.rs.stub"),
            ),
            Self::Policy => (
                "policy.rs.stub",
                include_str!("../foundation/console/stubs/policy.rs.stub"),
            ),
            Self::Provider => (
                "provider.rs.stub",
                include_str!("../foundation/console/stubs/provider.rs.stub"),
            ),
            Self::Resource => (
                "resource.rs.stub",
                include_str!("../foundation/console/stubs/resource.rs.stub"),
            ),
            Self::JsonApiResource => (
                "resource-json-api.rs.stub",
                include_str!("../foundation/console/stubs/resource-json-api.rs.stub"),
            ),
            Self::ResourceCollection => (
                "resource-collection.rs.stub",
                include_str!("../foundation/console/stubs/resource-collection.rs.stub"),
            ),
            Self::Seeder => (
                "seeder.rs.stub",
                include_str!("../database/console/seeds/stubs/seeder.rs.stub"),
            ),
            Self::Test => (
                "test.rs.stub",
                include_str!("../foundation/console/stubs/test.rs.stub"),
            ),
            Self::UnitTest => (
                "test.unit.rs.stub",
                include_str!("../foundation/console/stubs/test.unit.rs.stub"),
            ),
            Self::ViewComponent => (
                "view-component.rs.stub",
                include_str!("../foundation/console/stubs/view-component.rs.stub"),
            ),
        }
    }

    pub(crate) fn load(self, root: &Path) -> io::Result<Cow<'static, str>> {
        let (name, builtin) = self.file();
        let path = root.join("stubs").join(name);
        match fs::read_to_string(&path) {
            Ok(template) => Ok(Cow::Owned(template)),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(Cow::Borrowed(builtin)),
            Err(error) => Err(io::Error::new(
                error.kind(),
                format!("{}: {error}", path.display()),
            )),
        }
    }
}
