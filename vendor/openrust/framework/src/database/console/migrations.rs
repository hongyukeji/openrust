use super::confirm;

use std::{
    fmt::Write as _,
    path::{Component, Path, PathBuf},
};

use clap::{Arg, ArgAction, Command as Syntax};

use crate::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output,
    },
    contracts::{container::Container as _, foundation::Application as _},
    database::{
        migrations::{
            MigrationCreator, MigrationOutcome, MigrationResult, Migrator, RollbackOptions,
            RunOptions,
        },
        seeder,
    },
    foundation::Application,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Run,
    Status,
    Rollback,
    Install,
    Reset,
    Refresh,
    Fresh,
    Wipe,
    Make,
}

pub(crate) fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    for (kind, name, description) in [
        (Kind::Run, "migrate", "Run the database migrations"),
        (
            Kind::Status,
            "migrate:status",
            "Show the status of each migration",
        ),
        (
            Kind::Rollback,
            "migrate:rollback",
            "Rollback the last database migration",
        ),
        (
            Kind::Install,
            "migrate:install",
            "Create the migration repository",
        ),
        (
            Kind::Reset,
            "migrate:reset",
            "Rollback all database migrations",
        ),
        (
            Kind::Refresh,
            "migrate:refresh",
            "Reset and re-run all migrations",
        ),
        (
            Kind::Fresh,
            "migrate:fresh",
            "Drop all tables and re-run all migrations",
        ),
        (Kind::Wipe, "db:wipe", "Drop all tables, views, and types"),
        (Kind::Make, "make:migration", "Create a new migration file"),
    ] {
        let mut syntax = Syntax::new(name).about(description);
        if kind == Kind::Make {
            syntax = syntax
                .arg(Arg::new("name").required(true))
                .arg(
                    Arg::new("create")
                        .long("create")
                        .num_args(0..=1)
                        .default_missing_value(""),
                )
                .arg(Arg::new("table").long("table"))
                .arg(Arg::new("path").long("path"))
                .arg(flag("realpath"))
                .arg(flag("fullpath"));
        } else {
            syntax = syntax.arg(Arg::new("database").long("database"));
            if !matches!(kind, Kind::Install | Kind::Wipe) {
                syntax = syntax
                    .arg(Arg::new("path").long("path").action(ArgAction::Append))
                    .arg(flag("realpath"));
            }
            if matches!(
                kind,
                Kind::Run | Kind::Rollback | Kind::Reset | Kind::Refresh | Kind::Fresh | Kind::Wipe
            ) {
                syntax = syntax
                    .arg(flag("force"))
                    .arg(flag("no-interaction").short('n'));
            }
            if matches!(kind, Kind::Run | Kind::Rollback | Kind::Reset) {
                syntax = syntax.arg(flag("pretend"));
            }
            if matches!(kind, Kind::Run | Kind::Fresh) {
                syntax = syntax.arg(flag("step"));
            }
            if matches!(kind, Kind::Rollback | Kind::Refresh) {
                syntax = syntax.arg(
                    Arg::new("step")
                        .long("step")
                        .value_parser(clap::value_parser!(u64)),
                );
            }
            if kind == Kind::Rollback {
                syntax = syntax.arg(
                    Arg::new("batch")
                        .long("batch")
                        .value_parser(clap::value_parser!(u64)),
                );
            }
            if matches!(kind, Kind::Fresh | Kind::Wipe) {
                syntax = syntax.arg(flag("drop-views")).arg(flag("drop-types"));
            }
            if matches!(kind, Kind::Run | Kind::Fresh | Kind::Refresh) {
                syntax = syntax
                    .arg(flag("seed"))
                    .arg(Arg::new("seeder").long("seeder"));
            }
            if kind == Kind::Status {
                syntax = syntax.arg(flag("pending"));
            }
        }
        artisan.add_command(Box::new(MigrationCommand {
            definition: CommandDefinition::new(syntax),
            kind,
        }))?;
    }
    Ok(())
}

fn flag(name: &'static str) -> Arg {
    Arg::new(name).long(name).action(ArgAction::SetTrue)
}

struct MigrationCommand {
    definition: CommandDefinition,
    kind: Kind,
}

impl Command for MigrationCommand {
    fn definition(&self) -> &CommandDefinition {
        &self.definition
    }

    fn handle<'a>(
        &'a self,
        app: &'a Application,
        input: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let matches = input.matches();
            if self.kind == Kind::Make {
                return make(app.base_path(), matches, output);
            }
            if matches!(
                self.kind,
                Kind::Run | Kind::Rollback | Kind::Reset | Kind::Refresh | Kind::Fresh | Kind::Wipe
            ) && !confirm(app, matches)?
            {
                output.stdout.push_str("Command cancelled.\n");
                return Ok(1);
            }
            let migrator = app
                .container()
                .get::<Migrator>("migrator")
                .map_err(console_error)?;
            let database = matches.get_one::<String>("database").map(String::as_str);
            let paths = if matches!(self.kind, Kind::Install | Kind::Wipe) {
                Vec::new()
            } else {
                matches
                    .get_many::<String>("path")
                    .into_iter()
                    .flatten()
                    .map(|path| migration_path(migrator.root(), path, matches.get_flag("realpath")))
                    .collect::<Result<Vec<_>, _>>()?
            };
            match self.kind {
                Kind::Install => {
                    let created = migrator.install(database).await.map_err(console_error)?;
                    output.info(if created {
                        "Migration table created successfully."
                    } else {
                        "Migration table already exists."
                    });
                }
                Kind::Status => {
                    let Some(status) = migrator
                        .status(database, &paths)
                        .await
                        .map_err(console_error)?
                    else {
                        output.stderr.push_str("Migration table not found.\n");
                        return Ok(1);
                    };
                    output.stdout.push_str("Migration name / Batch / Status\n");
                    for row in status {
                        match row.batch {
                            Some(batch) if !matches.get_flag("pending") => {
                                writeln!(output.stdout, "{} [{}] Ran", row.name, batch).unwrap()
                            }
                            Some(_) => {}
                            None => writeln!(output.stdout, "{} Pending", row.name).unwrap(),
                        }
                    }
                }
                Kind::Run => {
                    let pretend = matches.get_flag("pretend");
                    let rows = migrator
                        .run(
                            database,
                            &paths,
                            RunOptions {
                                pretend,
                                step: matches.get_flag("step"),
                            },
                        )
                        .await
                        .map_err(console_error)?;
                    report(output, rows, pretend, "Nothing to migrate.");
                    if matches.get_flag("seed") && !pretend {
                        seed(app, database, matches, output).await?;
                    }
                }
                Kind::Rollback => {
                    let options = RollbackOptions {
                        pretend: matches.get_flag("pretend"),
                        step: matches.get_one::<u64>("step").copied(),
                        batch: matches.get_one::<u64>("batch").copied(),
                    };
                    let pretend = options.pretend;
                    let rows = migrator
                        .rollback(database, &paths, options)
                        .await
                        .map_err(console_error)?;
                    report(output, rows, pretend, "Nothing to rollback.");
                }
                Kind::Reset => {
                    let pretend = matches.get_flag("pretend");
                    let rows = migrator
                        .reset(database, &paths, pretend)
                        .await
                        .map_err(console_error)?;
                    report(output, rows, pretend, "Nothing to rollback.");
                }
                Kind::Refresh => {
                    let step = matches
                        .get_one::<u64>("step")
                        .copied()
                        .filter(|step| *step > 0);
                    let (down, up) = migrator
                        .refresh(database, &paths, step)
                        .await
                        .map_err(console_error)?;
                    report(output, down, false, "Nothing to rollback.");
                    report(output, up, false, "Nothing to migrate.");
                    if matches.get_flag("seed") || matches.contains_id("seeder") {
                        seed(app, database, matches, output).await?;
                    }
                }
                Kind::Fresh => {
                    if matches.get_flag("drop-types") {
                        return Err(ConsoleError::new(
                            "Dropping types is only supported by PostgreSQL.",
                        ));
                    }
                    let rows = migrator
                        .fresh(
                            database,
                            &paths,
                            matches.get_flag("step"),
                            matches.get_flag("drop-views"),
                        )
                        .await
                        .map_err(console_error)?;
                    report(output, rows, false, "Nothing to migrate.");
                    if matches.get_flag("seed") || matches.contains_id("seeder") {
                        seed(app, database, matches, output).await?;
                    }
                }
                Kind::Wipe => {
                    if matches.get_flag("drop-types") {
                        return Err(ConsoleError::new(
                            "Dropping types is only supported by PostgreSQL.",
                        ));
                    }
                    migrator
                        .wipe(database, matches.get_flag("drop-views"))
                        .await
                        .map_err(console_error)?;
                    output.info("Database wiped successfully.");
                }
                Kind::Make => unreachable!("handled before database commands"),
            }
            Ok(0)
        })
    }
}

async fn seed(
    app: &Application,
    database: Option<&str>,
    matches: &clap::ArgMatches,
    output: &mut Output,
) -> Result<(), ConsoleError> {
    let name = matches
        .get_one::<String>("seeder")
        .map(String::as_str)
        .filter(|name| !name.is_empty());
    seeder::run(app, database, name, output)
        .await
        .map_err(console_error)
}

fn report(output: &mut Output, rows: Vec<MigrationOutcome>, pretend: bool, empty: &str) {
    if rows.is_empty() {
        output.info(empty);
    }
    for row in rows {
        if pretend {
            for sql in row.statements {
                writeln!(output.stdout, "{}: {sql}", row.name).unwrap();
            }
        } else {
            let result = match row.result {
                MigrationResult::Success => "DONE",
                MigrationResult::Failure => "FAIL",
                MigrationResult::Skipped => "SKIPPED",
            };
            writeln!(output.stdout, "{} {result}", row.name).unwrap();
        }
    }
}

fn migration_path(root: &Path, path: &str, realpath: bool) -> Result<PathBuf, ConsoleError> {
    let path = Path::new(path);
    if realpath != path.is_absolute() {
        return Err(ConsoleError::new(
            "Use --realpath with an absolute migration path.",
        ));
    }
    let path = root.join(path);
    let mut normalized = PathBuf::new();
    for part in path.components() {
        if part == Component::ParentDir {
            normalized.pop();
        } else {
            normalized.push(part);
        }
    }
    Ok(normalized)
}

fn make(root: &Path, matches: &clap::ArgMatches, output: &mut Output) -> Result<i32, ConsoleError> {
    let path = match matches.get_one::<String>("path") {
        Some(path) => migration_path(root, path, matches.get_flag("realpath"))?,
        None => root.join("database/migrations"),
    };
    let name = matches
        .get_one::<String>("name")
        .expect("name required by Clap");
    let create = matches.get_one::<String>("create");
    let table = create
        .filter(|table| !table.is_empty())
        .or_else(|| matches.get_one::<String>("table"))
        .map(String::as_str);
    let path = MigrationCreator
        .create(name, &path, table, create.is_some())
        .map_err(console_error)?;
    let display = if matches.get_flag("fullpath") {
        &path
    } else {
        path.strip_prefix(root).unwrap_or(&path)
    };
    output.info(&format!(
        "Migration [{}] created successfully.",
        display.display()
    ));
    output
        .stdout
        .push_str("Register the migration in database/migrations/mod.rs and rebuild.\n");
    Ok(0)
}

fn console_error(error: impl std::fmt::Display) -> ConsoleError {
    ConsoleError::new(error.to_string())
}
