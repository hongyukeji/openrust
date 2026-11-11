use crate::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output,
    },
    contracts::foundation::Application as _,
    database::migrations::MigrationCreator,
    foundation::Application,
};

pub(crate) fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    artisan.add_command(Box::new(NotificationTableCommand(CommandDefinition::new(
        clap::Command::new("make:notifications-table")
            .visible_alias("notifications:table")
            .about("Create a migration for the notifications table"),
    ))))
}

struct NotificationTableCommand(CommandDefinition);

impl Command for NotificationTableCommand {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }

    fn handle<'a>(
        &'a self,
        app: &'a Application,
        _: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let directory = app.base_path().join("database/migrations");
            let path = tokio::task::spawn_blocking(move || {
                MigrationCreator.create_with_body("create_notifications_table", &directory, Some("notifications"), true, Some((
                    "connection.getSchemaBuilder().create(\"notifications\", |table| {\n                openrust::notifications::DatabaseNotification::schema(table, |table| {\n                    table.unsignedBigInteger(\"notifiable_id\");\n                });\n            }).await",
                    "connection.getSchemaBuilder().dropIfExists(\"notifications\").await",
                )))
            }).await.map_err(|error| ConsoleError::new(error.to_string()))?
                .map_err(|error| ConsoleError::new(error.to_string()))?;
            let display = path.strip_prefix(app.base_path()).unwrap_or(&path);
            output.info(&format!(
                "Migration [{}] created successfully.",
                display.display()
            ));
            output
                .stdout
                .push_str("Register the migration in database/migrations/mod.rs and rebuild.\n");
            Ok(0)
        })
    }
}
