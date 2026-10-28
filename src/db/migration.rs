//! 基于 SeaORM migration 定义的运行时迁移命令。

use std::marker::PhantomData;
use std::time::Duration;

use crate::core::{App, BootContext, BoxFuture, Command, Error, Result, ServiceProvider};
use sea_orm_migration::MigratorTrait;

use crate::db::{Databases, Db, migration_lock::MigrationLock};

/// 在 Provider boot 阶段贡献 `migrate` 命令。
pub struct MigrationServiceProvider<M> {
    lock_wait_timeout: Duration,
    marker: PhantomData<fn() -> M>,
}

impl<M> MigrationServiceProvider<M> {
    /// 创建使用默认锁等待期限的迁移命令 Provider。
    pub fn new() -> Self {
        Self {
            lock_wait_timeout: super::migration_lock::default_wait_timeout(),
            marker: PhantomData,
        }
    }

    /// 设置等待其他迁移完成的最长时间。
    #[must_use]
    pub fn with_lock_wait_timeout(mut self, timeout: Duration) -> Self {
        self.lock_wait_timeout = timeout;
        self
    }
}

impl<M> Default for MigrationServiceProvider<M> {
    fn default() -> Self {
        Self::new()
    }
}

impl<M> ServiceProvider for MigrationServiceProvider<M>
where
    M: MigratorTrait + Send + Sync + 'static,
{
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.command(MigrationCommand::<M> {
                lock_wait_timeout: self.lock_wait_timeout,
                marker: PhantomData,
            });
            Ok(())
        })
    }
}

struct MigrationCommand<M> {
    lock_wait_timeout: Duration,
    marker: PhantomData<fn() -> M>,
}

impl<M> Command for MigrationCommand<M>
where
    M: MigratorTrait + Send + Sync + 'static,
{
    fn name(&self) -> &'static str {
        "migrate"
    }

    fn about(&self) -> &'static str {
        "执行、回滚或查看数据库迁移"
    }

    fn accepts_args(&self) -> bool {
        true
    }

    fn run<'a>(&'a self, app: &'a App, args: Vec<String>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let (database_name, action) = MigrationAction::parse(&args)?;
            let db = if let Some(name) = database_name {
                app.state().resolve::<Databases>().await?.get(&name)?
            } else {
                app.state().resolve::<Db>().await?.as_ref().clone()
            };
            let lock = MigrationLock::acquire(&db, self.lock_wait_timeout).await?;
            let database = db.connection()?;
            let table_names = db.table_names();
            let result: Result<()> = async {
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
                                let _ = writeln!(
                                    output,
                                    "{}: {}",
                                    migration.name(),
                                    migration.status()
                                );
                            }
                            print_migrations(&output);
                        }
                    }
                }
                Ok(())
            }
            .await;
            let release = lock.release().await;
            result?;
            release?;
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
    fn parse(args: &[String]) -> Result<(Option<String>, Self)> {
        let invalid = || {
            Error::custom(
                "invalid_migration_command",
                "usage: migrate [--database <name>] [up [steps] | down [steps] | status]",
            )
        };
        let (database_name, action_args) = match args {
            [flag, name, rest @ ..] if flag == "--database" && !name.is_empty() => {
                (Some(name.clone()), rest)
            }
            [flag, ..] if flag == "--database" => return Err(invalid()),
            _ => (None, args),
        };
        let action = match action_args {
            [] => Self::Up(None),
            [action] if action == "up" => Self::Up(None),
            [action, count] if action == "up" => {
                Self::Up(Some(parse_steps(count).ok_or_else(invalid)?))
            }
            [action] if action == "down" => Self::Down(1),
            [action, count] if action == "down" => {
                Self::Down(parse_steps(count).ok_or_else(invalid)?)
            }
            [action] if action == "status" => Self::Status,
            _ => return Err(invalid()),
        };
        Ok((database_name, action))
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
            Ok((None, MigrationAction::Up(None)))
        ));
        assert!(matches!(
            MigrationAction::parse(&args(&["up"])),
            Ok((None, MigrationAction::Up(None)))
        ));
        assert!(matches!(
            MigrationAction::parse(&args(&["down"])),
            Ok((None, MigrationAction::Down(1)))
        ));
    }

    #[test]
    fn migration_actions_accept_positive_step_counts_and_status() {
        assert!(matches!(
            MigrationAction::parse(&args(&["up", "2"])),
            Ok((None, MigrationAction::Up(Some(2))))
        ));
        assert!(matches!(
            MigrationAction::parse(&args(&["down", "3"])),
            Ok((None, MigrationAction::Down(3)))
        ));
        assert!(matches!(
            MigrationAction::parse(&args(&["status"])),
            Ok((None, MigrationAction::Status))
        ));
    }

    #[test]
    fn migration_action_selects_an_explicit_database() {
        assert!(matches!(
            MigrationAction::parse(&args(&["--database", "analytics", "up", "2"])),
            Ok((Some(name), MigrationAction::Up(Some(2)))) if name == "analytics"
        ));
        assert!(MigrationAction::parse(&args(&["--database"])).is_err());
        assert!(
            MigrationAction::parse(&args(&["--database", "analytics", "--database", "other"]))
                .is_err()
        );
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
