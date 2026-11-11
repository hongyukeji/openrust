//! Catalog of planned framework commands, package references, and visible aliases.

use super::{Application, ConsoleError};
use clap::Command;

impl Application {
    /// Registers the framework's planned command catalog before application commands.
    pub fn register_planned_commands(&mut self) -> Result<(), ConsoleError> {
        for &(name, description) in LARAVEL.iter().chain(GORAVEL_ONLY).chain(SKELETON_PACKAGES) {
            self.add_planned(
                Command::new(name).about(description).visible_aliases(
                    ALIASES
                        .iter()
                        .filter_map(move |&(command, alias)| (command == name).then_some(alias)),
                ),
            )?;
        }
        Ok(())
    }
}

// Laravel Framework v13.40.0 public commands, plus console completion.
const LARAVEL: &[(&str, &str)] = &[
    ("about", "Show application and environment information"),
    ("auth:clear-resets", "Remove expired password reset tokens"),
    ("cache:clear", "Clear application cache entries"),
    ("cache:forget", "Delete one cache entry by key"),
    (
        "cache:prune-stale-tags",
        "Remove obsolete references from tagged caches",
    ),
    ("channel:list", "Show registered broadcast channels"),
    (
        "clear-compiled",
        "Remove cached service and package manifests",
    ),
    ("completion", "Generate shell completion scripts"),
    (
        "config:cache",
        "Compile application configuration into a cache",
    ),
    ("config:clear", "Remove the compiled configuration cache"),
    (
        "config:publish",
        "Copy publishable configuration into the application",
    ),
    ("config:show", "Display a configuration file or key"),
    ("db", "Open an interactive database client"),
    ("db:monitor", "Check database connection usage"),
    ("db:seed", "Populate the database with seed data"),
    ("db:show", "Summarize a database and its tables"),
    ("db:table", "Inspect the columns and indexes of a table"),
    ("db:wipe", "Drop database tables, views, and types"),
    ("dev", "Start the configured development processes"),
    ("dev:list", "Show registered development processes"),
    ("docs", "Open the documentation for a topic"),
    ("down", "Put the application into maintenance mode"),
    ("env:decrypt", "Decrypt an encrypted environment file"),
    ("env:encrypt", "Encrypt an environment file"),
    ("event:cache", "Cache event and listener registrations"),
    ("event:clear", "Remove the cached event registrations"),
    ("install:api", "Install the application API scaffolding"),
    (
        "install:broadcasting",
        "Install the broadcasting scaffolding",
    ),
    ("key:generate", "Generate an application encryption key"),
    (
        "lang:publish",
        "Copy language resources into the application",
    ),
    (
        "make:cache-table",
        "Generate a migration for the cache table",
    ),
    ("make:cast", "Generate a model attribute cast"),
    ("make:channel", "Generate a broadcast channel class"),
    ("make:class", "Generate a class"),
    ("make:command", "Generate an Artisan command"),
    ("make:component", "Generate a view component"),
    ("make:config", "Generate a configuration file"),
    ("make:controller", "Generate a controller"),
    ("make:enum", "Generate an enumeration"),
    ("make:event", "Generate an event"),
    ("make:exception", "Generate an exception type"),
    ("make:factory", "Generate a model factory"),
    ("make:interface", "Generate an interface"),
    ("make:job", "Generate a queued job"),
    ("make:job-middleware", "Generate queue job middleware"),
    ("make:listener", "Generate an event listener"),
    ("make:mail", "Generate a mail class"),
    ("make:middleware", "Generate HTTP middleware"),
    ("make:migration", "Generate a database migration"),
    ("make:model", "Generate a database model"),
    ("make:notification", "Generate a notification"),
    (
        "make:notifications-table",
        "Generate a migration for database notifications",
    ),
    ("make:observer", "Generate a model observer"),
    ("make:policy", "Generate an authorization policy"),
    ("make:provider", "Generate a service provider"),
    (
        "make:queue-batches-table",
        "Generate a migration for job batches",
    ),
    (
        "make:queue-failed-table",
        "Generate a migration for failed jobs",
    ),
    ("make:queue-table", "Generate a migration for queued jobs"),
    ("make:request", "Generate a form request"),
    ("make:resource", "Generate an API resource"),
    ("make:rule", "Generate a validation rule"),
    ("make:scope", "Generate a model query scope"),
    ("make:seeder", "Generate a database seeder"),
    (
        "make:session-table",
        "Generate a migration for database sessions",
    ),
    ("make:test", "Generate a test"),
    ("make:trait", "Generate a trait"),
    ("make:view", "Generate a view"),
    ("migrate", "Apply pending database migrations"),
    ("migrate:fresh", "Drop all tables and rerun migrations"),
    ("migrate:install", "Create the migration history table"),
    ("migrate:refresh", "Roll back and rerun migrations"),
    ("migrate:reset", "Roll back all applied migrations"),
    ("migrate:rollback", "Undo the latest migration batch"),
    ("migrate:status", "Show applied and pending migrations"),
    ("model:prune", "Remove model records eligible for pruning"),
    ("model:show", "Inspect a model and its attributes"),
    ("optimize", "Cache application resources for deployment"),
    ("optimize:clear", "Clear cached application resources"),
    (
        "package:discover",
        "Rebuild the discovered package manifest",
    ),
    ("queue:clear", "Remove pending jobs from a queue"),
    ("queue:failed", "List failed jobs"),
    ("queue:flush", "Delete all recorded failed jobs"),
    ("queue:forget", "Remove failed jobs by identifier"),
    ("queue:listen", "Listen for queued jobs and process them"),
    ("queue:monitor", "Check the number of jobs on queues"),
    ("queue:pause", "Pause processing on a queue"),
    ("queue:prune-batches", "Delete old job batch records"),
    ("queue:prune-failed", "Delete old failed job records"),
    (
        "queue:restart",
        "Request a graceful restart of queue workers",
    ),
    ("queue:resume", "Resume processing on a paused queue"),
    ("queue:retry", "Requeue failed jobs"),
    ("queue:retry-batch", "Requeue the failed jobs in a batch"),
    ("queue:work", "Run a queue worker"),
    ("reload", "Reload long-running application services"),
    ("route:cache", "Compile the route table into a cache"),
    ("route:clear", "Remove the cached route table"),
    ("route:list", "Show registered routes"),
    ("schedule:clear-cache", "Remove cached scheduler locks"),
    ("schedule:interrupt", "Interrupt running scheduler loops"),
    ("schedule:list", "Show scheduled tasks"),
    ("schedule:pause", "Pause scheduled task processing"),
    ("schedule:resume", "Resume scheduled task processing"),
    ("schedule:run", "Run the scheduled tasks that are due"),
    ("schedule:test", "Run a selected scheduled task"),
    ("schedule:work", "Run the scheduler continuously"),
    ("schema:dump", "Export the current database schema"),
    ("serve", "Start a local development HTTP server"),
    ("session:garbage-collect", "Remove expired sessions"),
    ("storage:link", "Create the configured storage links"),
    ("storage:unlink", "Remove the configured storage links"),
    (
        "stub:publish",
        "Copy code generation templates into the application",
    ),
    ("up", "Take the application out of maintenance mode"),
    (
        "vendor:publish",
        "Publish files supplied by installed packages",
    ),
    ("view:cache", "Precompile application views"),
    ("view:clear", "Remove compiled views"),
];

// Goravel Framework v1.18.0 public commands without a Laravel counterpart.
const GORAVEL_ONLY: &[(&str, &str)] = &[
    ("build", "Compile the application for a target platform"),
    ("jwt:secret", "Generate a JWT signing secret"),
    ("make:agent", "Generate an AI agent"),
    ("make:filter", "Generate a validation filter"),
    ("make:package", "Generate an extension package"),
    ("make:tool", "Generate an AI tool"),
    (
        "package:install",
        "Install an application extension package",
    ),
    ("package:uninstall", "Remove an installed extension package"),
];

// Commands provided by packages referenced by the Laravel application skeleton.
const SKELETON_PACKAGES: &[(&str, &str)] = &[
    ("pail", "Follow the application's log output"),
    ("test", "Run the application test suite"),
    ("tinker", "Open an interactive application shell"),
];

// Public aliases exposed by the fixed Laravel baseline.
const ALIASES: &[(&str, &str)] = &[
    ("make:cache-table", "cache:table"),
    ("make:config", "config:make"),
    ("make:notifications-table", "notifications:table"),
    ("make:queue-batches-table", "queue:batches-table"),
    ("make:queue-failed-table", "queue:failed-table"),
    ("make:queue-table", "queue:table"),
    ("make:session-table", "session:table"),
    ("queue:resume", "queue:continue"),
    ("schedule:resume", "schedule:continue"),
];
