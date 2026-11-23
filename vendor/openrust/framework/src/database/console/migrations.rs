use super::confirm;

use std::{
    fmt::Write as _,
    io::{self, IsTerminal as _, Write as _},
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
        Connection, DatabaseError, DatabaseManager, DatabaseResult,
        events::DatabaseRefreshed,
        migrations::{
            MigrationCreator, MigrationOutcome, MigrationResult, Migrator, RollbackOptions,
            RunOptions, WipeOptions,
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
    Dump,
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
        (Kind::Dump, "schema:dump", "Dump the given database schema"),
    ] {
        let mut syntax = Syntax::new(name).about(description);
        if kind == Kind::Dump {
            syntax = syntax
                .arg(Arg::new("database").long("database"))
                .arg(Arg::new("path").long("path"))
                .arg(flag("prune"))
                .arg(flag("without-migration-data"));
        } else if kind == Kind::Make {
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
            if matches!(kind, Kind::Run | Kind::Fresh) {
                syntax = syntax.arg(Arg::new("schema-path").long("schema-path"));
            }
            if kind == Kind::Run {
                syntax = syntax.arg(flag("graceful"));
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
                syntax = syntax.arg(
                    Arg::new("pending")
                        .long("pending")
                        .num_args(0..=1)
                        .default_missing_value("0")
                        .value_parser(clap::value_parser!(u8)),
                );
            }
        }
        let definition = CommandDefinition::new(syntax);
        let definition = if kind == Kind::Run {
            definition.isolatable()
        } else {
            definition
        };
        artisan.add_command(Box::new(MigrationCommand { definition, kind }))?;
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
            let destructive = matches!(
                self.kind,
                Kind::Rollback | Kind::Reset | Kind::Refresh | Kind::Fresh | Kind::Wipe
            );
            if destructive
                && app
                    .container()
                    .get::<DatabaseManager>("db")
                    .map_err(console_error)?
                    .destructive_commands_prohibited()
            {
                output.comment("This command is prohibited from running in this environment.");
                return Ok(1);
            }
            if (destructive || self.kind == Kind::Run) && !confirm(app, matches)? {
                output.stdout.push_str(&crate::console::command_cancelled());
                output.stdout.push('\n');
                return Ok(1);
            }
            let migrator = app
                .container()
                .get::<Migrator>("migrator")
                .map_err(console_error)?;
            let database = matches.get_one::<String>("database").map(String::as_str);
            if self.kind == Kind::Dump {
                let prune = matches.get_flag("prune");
                migrator
                    .dump_schema(
                        database,
                        matches.get_one::<String>("path").map(Path::new),
                        !matches.get_flag("without-migration-data"),
                        prune,
                    )
                    .await
                    .map_err(console_error)?;
                output.info(if prune {
                    "Database schema dumped and pruned successfully."
                } else {
                    "Database schema dumped successfully."
                });
                return Ok(0);
            }
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
                        .status_with_skips(database, &paths)
                        .await
                        .map_err(console_error)?
                    else {
                        output.stderr.push_str("Migration table not found.\n");
                        return Ok(1);
                    };
                    let pending_exit = matches.get_one::<u8>("pending").copied();
                    let mut rows = status
                        .into_iter()
                        .filter(|(row, skipped)| {
                            pending_exit.is_none() || (row.batch.is_none() && !skipped)
                        })
                        .peekable();
                    if rows.peek().is_none() {
                        output.info(if pending_exit.is_some() {
                            "No pending migrations"
                        } else {
                            "No migrations found"
                        });
                        return Ok(0);
                    }
                    output.stdout.push_str("Migration name / Batch / Status\n");
                    for (row, skipped) in rows {
                        match row.batch {
                            Some(batch) => writeln!(output.stdout, "{} [{}] Ran", row.name, batch),
                            None if skipped => writeln!(output.stdout, "{} Skipped", row.name),
                            None => writeln!(output.stdout, "{} Pending", row.name),
                        }
                        .unwrap();
                    }
                    return Ok(i32::from(pending_exit.unwrap_or(0)));
                }
                Kind::Run => {
                    let pretend = matches.get_flag("pretend");
                    let confirm_create =
                        |connection: &Connection| confirm_database_creation(matches, connection);
                    let can_confirm = matches.get_flag("force")
                        || (!matches.get_flag("no-interaction") && io::stdin().is_terminal());
                    let result = async {
                        let (rows, loaded) = migrator
                            .run_with_schema(
                                database,
                                &paths,
                                RunOptions {
                                    pretend,
                                    step: matches.get_flag("step"),
                                },
                                matches.get_one::<String>("schema-path").map(Path::new),
                                if can_confirm {
                                    Some(&confirm_create)
                                } else {
                                    None
                                },
                            )
                            .await
                            .map_err(console_error)?;
                        if loaded {
                            output.info("Loading stored database schemas.");
                        }
                        report(output, rows, pretend, "Nothing to migrate.");
                        if matches.get_flag("seed") && !pretend {
                            seed(app, database, matches, output).await?;
                        }
                        Ok::<(), ConsoleError>(())
                    }
                    .await;
                    if let Err(error) = result {
                        if !matches.get_flag("graceful") {
                            return Err(error);
                        }
                        output.comment(&format!("  WARN  {error}"));
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
                    let step = matches.get_one::<u64>("step").copied();
                    let result = migrator
                        .refresh_with_schema(database, &paths, step)
                        .await
                        .map_err(console_error)?;
                    report(output, result.down, false, "Nothing to rollback.");
                    if result.loaded {
                        output.info("Loading stored database schemas.");
                    }
                    report(output, result.up, false, "Nothing to migrate.");
                }
                Kind::Fresh => {
                    let (rows, loaded) = migrator
                        .fresh_with_schema(
                            database,
                            &paths,
                            matches.get_flag("step"),
                            wipe_options(matches),
                            matches.get_one::<String>("schema-path").map(Path::new),
                            Some(&|_| Ok(true)),
                        )
                        .await
                        .map_err(console_error)?;
                    if loaded {
                        output.info("Loading stored database schemas.");
                    }
                    report(output, rows, false, "Nothing to migrate.");
                }
                Kind::Wipe => {
                    migrator
                        .wipe_with_options(database, wipe_options(matches))
                        .await
                        .map_err(console_error)?;
                    output.info("Database wiped successfully.");
                }
                Kind::Make | Kind::Dump => unreachable!("handled before database commands"),
            }
            if matches!(self.kind, Kind::Fresh | Kind::Refresh) {
                let seeding = matches.get_flag("seed") || matches.contains_id("seeder");
                app.container()
                    .get::<DatabaseManager>("db")
                    .map_err(console_error)?
                    .dispatch_migration_event(DatabaseRefreshed {
                        database: database.map(str::to_owned),
                        seeding,
                    })
                    .await
                    .map_err(console_error)?;
                if seeding {
                    seed(app, database, matches, output).await?;
                }
            }
            Ok(0)
        })
    }
}

fn wipe_options(matches: &clap::ArgMatches) -> WipeOptions {
    WipeOptions {
        drop_views: matches.get_flag("drop-views"),
        drop_types: matches.get_flag("drop-types"),
    }
}

fn confirm_database_creation(
    matches: &clap::ArgMatches,
    connection: &Connection,
) -> DatabaseResult<bool> {
    if matches.get_flag("force") {
        return Ok(true);
    }
    let mut output = io::stderr().lock();
    if connection.is_sqlite() {
        writeln!(
            output,
            "The SQLite database configured for this application does not exist: {}",
            connection.get_database_name()
        )
    } else {
        writeln!(
            output,
            "The database {:?} does not exist on the {:?} connection.",
            connection.get_database_name(),
            connection.get_name()
        )
    }
    .map_err(DatabaseError::SchemaIo)?;
    write!(output, "Would you like to create it? [Y/n] ").map_err(DatabaseError::SchemaIo)?;
    output.flush().map_err(DatabaseError::SchemaIo)?;
    let mut response = String::new();
    let read = io::stdin()
        .read_line(&mut response)
        .map_err(DatabaseError::SchemaIo)?;
    Ok(read != 0
        && matches!(
            response.trim().to_ascii_lowercase().as_str(),
            "" | "y" | "yes"
        ))
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
    let default_directory = root.join("database/migrations");
    let path = match matches.get_one::<String>("path") {
        Some(path) => migration_path(root, path, matches.get_flag("realpath"))?,
        None => default_directory.clone(),
    };
    let name = matches
        .get_one::<String>("name")
        .expect("name required by Clap");
    let create = matches.get_one::<String>("create");
    let table = create
        .filter(|table| !table.is_empty())
        .or_else(|| matches.get_one::<String>("table"))
        .map(String::as_str);
    let auto_discovered = path == default_directory;
    let path = MigrationCreator
        .create_from_stubs(name, &path, table, create.is_some(), None, Some(root))
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
    let next_step = if auto_discovered {
        "Rebuild the application to include the migration.\n"
    } else {
        "Add the migration from this custom directory to your application registry and rebuild.\n"
    };
    output.stdout.push_str(next_step);
    Ok(0)
}

fn console_error(error: impl std::fmt::Display) -> ConsoleError {
    ConsoleError::new(error.to_string())
}
