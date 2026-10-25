//! Runtime migration command backed by SeaORM migration definitions.

use std::marker::PhantomData;

use crate::core::{App, AppBuilder, BoxFuture, Command, Error, Plugin, Result};
use sea_orm_migration::MigratorTrait;

use crate::db::Db;

/// Registers the `migrate` runtime command for an application's migration set.
///
/// Register this plugin alongside [`DatabasePlugin`](crate::db::DatabasePlugin). The migration type is usually
/// defined in the application's `migration` crate and implements [`MigratorTrait`].
pub struct MigrationPlugin<M>(PhantomData<fn() -> M>);

impl<M> MigrationPlugin<M> {
    /// Creates a migration command plugin for `M`.
    pub fn new() -> Self {
        Self(PhantomData)
    }
}

impl<M> Default for MigrationPlugin<M> {
    fn default() -> Self {
        Self::new()
    }
}

impl<M> Plugin for MigrationPlugin<M>
where
    M: MigratorTrait + Send + Sync + 'static,
{
    fn build(&self, app: &mut AppBuilder) -> Result<()> {
        app.command(MigrationCommand::<M>(PhantomData));
        Ok(())
    }
}

struct MigrationCommand<M>(PhantomData<fn() -> M>);

impl<M> Command for MigrationCommand<M>
where
    M: MigratorTrait + Send + Sync + 'static,
{
    fn name(&self) -> &'static str {
        "migrate"
    }

    fn about(&self) -> &'static str {
        "Apply, roll back, or inspect database migrations"
    }

    fn accepts_args(&self) -> bool {
        true
    }

    fn run<'a>(&'a self, app: &'a App, args: Vec<String>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let action = MigrationAction::parse(&args)?;
            let db = app.state().try_get::<Db>()?;
            let database = db.connection()?;
            let table_names = db.table_names();
            match action {
                MigrationAction::Up(steps) => {
                    table_names
                        .scope(M::up(&database, steps))
                        .await
                        .map_err(Error::internal)?;
                    print_migrations("Migrations applied.\n");
                }
                MigrationAction::Down(steps) => {
                    table_names
                        .scope(M::down(&database, Some(steps)))
                        .await
                        .map_err(Error::internal)?;
                    print_migrations("Migrations rolled back.\n");
                }
                MigrationAction::Status => {
                    let migrations = table_names
                        .scope(M::get_migration_with_status(&database))
                        .await
                        .map_err(Error::internal)?;
                    if migrations.is_empty() {
                        print_migrations("No migrations registered.\n");
                    } else {
                        let mut output = String::new();
                        for migration in migrations {
                            use std::fmt::Write as _;
                            let _ =
                                writeln!(output, "{}: {}", migration.name(), migration.status());
                        }
                        print_migrations(&output);
                    }
                }
            }
            Ok(())
        })
    }
}

enum MigrationAction {
    Up(Option<u32>),
    Down(u32),
    Status,
}

impl MigrationAction {
    fn parse(args: &[String]) -> Result<Self> {
        let invalid = || {
            Error::custom(
                "invalid_migration_command",
                "usage: migrate [up [steps] | down [steps] | status]",
            )
        };
        match args {
            [] => Ok(Self::Up(None)),
            [action] if action == "up" => Ok(Self::Up(None)),
            [action, count] if action == "up" => parse_steps(count)
                .map(|steps| Self::Up(Some(steps)))
                .ok_or_else(invalid),
            [action] if action == "down" => Ok(Self::Down(1)),
            [action, count] if action == "down" => {
                parse_steps(count).map(Self::Down).ok_or_else(invalid)
            }
            [action] if action == "status" => Ok(Self::Status),
            _ => Err(invalid()),
        }
    }
}

fn parse_steps(value: &str) -> Option<u32> {
    value.parse::<u32>().ok().filter(|steps| *steps > 0)
}

#[allow(clippy::print_stdout)]
fn print_migrations(output: &str) {
    print!("{output}");
}

#[cfg(test)]
mod tests {
    use super::MigrationAction;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn migration_actions_default_to_all_up_or_one_down() {
        assert!(matches!(
            MigrationAction::parse(&[]),
            Ok(MigrationAction::Up(None))
        ));
        assert!(matches!(
            MigrationAction::parse(&args(&["up"])),
            Ok(MigrationAction::Up(None))
        ));
        assert!(matches!(
            MigrationAction::parse(&args(&["down"])),
            Ok(MigrationAction::Down(1))
        ));
    }

    #[test]
    fn migration_actions_accept_positive_step_counts_and_status() {
        assert!(matches!(
            MigrationAction::parse(&args(&["up", "2"])),
            Ok(MigrationAction::Up(Some(2)))
        ));
        assert!(matches!(
            MigrationAction::parse(&args(&["down", "3"])),
            Ok(MigrationAction::Down(3))
        ));
        assert!(matches!(
            MigrationAction::parse(&args(&["status"])),
            Ok(MigrationAction::Status)
        ));
    }

    #[test]
    fn invalid_migration_arguments_are_rejected() {
        for invalid in [
            &["fresh"][..],
            &["down", "0"],
            &["up", "-1"],
            &["status", "1"],
        ] {
            assert!(MigrationAction::parse(&args(invalid)).is_err());
        }
    }
}
