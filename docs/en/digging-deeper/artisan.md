# Artisan Console

The Console Kernel [boots the application](../architecture-concepts/lifecycle.md) before dispatching an Artisan command. The skeleton’s command listing and help run without an HTTP listener or a database connection.

```sh
./artisan list
./artisan list cache --format=json --short
./artisan help list
./artisan list --help
./artisan inspire
```

The default console routes register `inspire`. Running `./artisan inspire` prints a randomly selected quote and its author, then exits with status 0.

It uses the 41 quotes from the framework baseline. In a color terminal, the quote appears in bold and the attribution in gray. Redirected output is plain text by default. Use `--ansi` to force styles or `--no-ansi` to disable them.

On Windows, use `artisan.bat`. The compiled application accepts the same commands through `main artisan`.

Listing and help exit with status 0. Unknown commands and invalid arguments exit with status 1. Entries marked `Planned` have help available and exit with status 1 when invoked.

Register command types through `with_commands`, using `app/console/commands/mod.rs`. Declare the console routes from `routes/console.rs` through `with_routing` in `bootstrap/app.rs`. A command registered under the same name replaces its planned entry.

In `routes/console.rs`, import `openrust::support::facades::Artisan`. `Artisan::command("inspire", …)` registers the closure with the console bound while `register()` runs and `purpose()` sets the description shown by Artisan help and list. The closure receives the command’s output and returns normally with exit status 0.

`Application::handle_command` calls the Console Kernel’s `handle` and `terminate` methods. After boot, the kernel loads deferred providers and registers the command catalog, built-in commands and application commands before dispatch.


## Source and Release Entrypoints {#source-and-release-entrypoints}

The source `artisan` script changes to the application directory and forwards its arguments unchanged to Cargo. Spaces, empty arguments and non-ASCII text are preserved. The script returns the command's exit status.

A standalone build uses `./main artisan list`, `./main artisan help inspire` or `./main artisan inspire` without Cargo. It finds configuration relative to the executable and works from a different working directory. See [deployment](../getting-started/deployment.md).

Run the compiled `main` without arguments to start the [configured application services](../getting-started/deployment.md#run-services-together). Use `main artisan` to dispatch a console command without starting those services. An unrecognized first argument prints the outer usage and exits with status 2 before creating the application. Artisan dispatch errors exit with status 1.

In CMD, run `artisan.bat inspire`. In PowerShell, use `.\artisan.bat help inspire --format=json`. The source launcher requires `cargo.exe` on `PATH` and switches to the application directory. Read Artisan's exit code with `%ERRORLEVEL%` in CMD or `$LASTEXITCODE` in PowerShell. For a [standalone Windows release](../getting-started/deployment.md#windows-standalone), use `.\main.exe artisan inspire`.

### Development HTTP Server {#development-http-server}

Use `./artisan serve` to run HTTP on its own. The default address is `127.0.0.1:8000`. Set `SERVER_HOST` and `SERVER_PORT` to change the defaults, or pass `--host` and `--port` for one invocation. With no explicit port, `--tries` limits how many successive ports to try when a port is already occupied. The default is ten additional ports.

```sh
./artisan serve
./artisan serve --host=127.0.0.1 --port=8080 --no-reload
```

The server restarts after changes to the selected environment file and keeps its listening port. Use `--env=local` to select an environment and `--no-reload` to keep the current configuration until you stop the server. Press Ctrl+C to stop. Run the compiled `main` without mode arguments to [start configured services together](../getting-started/deployment.md#run-services-together).

## Development Processes {#the-dev-command}

Run the registered development processes together from the application directory. The default development server and queue listener use the same compiled executable as Artisan. Node is used only by Node-based entries such as Vite.

```sh
./artisan dev
./artisan dev --tabs
./artisan dev --stream
./artisan dev --inline --timestamps --no-restart
./artisan dev --json
```

Output lines are labelled by process and retain stdout or stderr. `--timestamps` adds UTC times to inline output. `--json` emits one versioned event per line, including process starts, output, exits, retry attempts and a final `done` event.

A process that fails after running for at least one second is retried up to five times with a one-second delay. A process that exits successfully is finished. A process that fails within a second is reported immediately. Disable retries for a run with `--no-restart`, or for the application with `DevCommands::disable_auto_restart()`. Use `DevCommands::with_timestamps()` to enable timestamps by default. The first permanent failure determines the exit code while other processes can keep running.

A usable Unix terminal opens the tabbed view by default. Start in the interleaved view with `--stream`. Redirected output, small terminals, Windows, `--inline`, and `--json` use the inline path. Interactive sessions return status 0 on normal exit, while inline and JSON runs report the first permanent failure.

Use `1`–`9` to select a tab, or focus the sidebar with Tab and use the arrow keys. Press `s` for the stream, `t` for tabs, `f` to filter the stream, `r` to restart the selected tab's process, and `c` to clear the current view. Search with `/`, then use `n` or `N` to move between matches. Use arrows or Page Up and Page Down to scroll logs.

`--buffer-size` and `--stream-buffer-size` control retained lines per tab and in the stream. The defaults are 2,000 and 10,000 lines. The application can configure these through `DevCommands::buffer_size()` and `stream_buffer_size()`, and select its default mode with `tabs()`, `stream()`, or `inline()`.

Press `q` or Ctrl+C to exit. Unix termination signals also request shutdown and allow up to six seconds for child processes to exit. Artisan restores the terminal and prints the retained stream to the corresponding output streams. The optional [Pail command](../the-basics/logging.md#tailing-log-messages-using-pail) can also run independently.

Press Ctrl+C in the Windows console to request shutdown. Each development command runs in its own Windows job. Artisan sends Ctrl+Break to its console process group and releases any remaining descendants when the grace period expires. With `--no-restart`, a failed command also shuts down the other development commands and returns the failure code.

### Listing Development Processes

Use `dev:list` to inspect the application's development process registry. It lists the [HTTP development server](#development-http-server) and queue listener by default. When the application has a `package.json`, it also lists Vite using Bun, pnpm, Yarn, or npm according to the application's lockfile. The list never starts a process. When the optional Pail provider is loaded, `logs` follows with `pail --timeout=0`, before Vite. This is a framework default, so it survives vendor-only exclusion and is removed by `without_default_commands()`. A package or application entry named `logs` can override it.

```sh
./artisan dev:list
./artisan dev:list --json
./artisan dev:list --no-interaction --filter=queue
```

JSON output is also used in non-interactive sessions. `--filter` matches a substring of the name or command. `--except-vendor` and `--only-vendor` filter package entries. A filter with no matches exits with status 1.

In the `boot` method of your application service provider, use `openrust::foundation::DevCommands` to register processes. A registration with the same name overrides the default, and application registrations take precedence over package registrations.

```rust
DevCommands::artisan("serve --port=9000", Some("server"))?.green();
```

Use `register` for shell commands and `node` or `node_exec` for package-manager commands. Pass `None` to use the first word of the command as its name. `only`, `except`, `order`, `without_vendor_commands`, and `without_default_commands` customize the application registry. Package registrations are recognized by their Rust caller's source path.

## Command Help and Output Formats {#command-help-and-output-formats}

`./artisan help migrate` and `./artisan migrate --help` read the same registered definition. They display arguments, options and descriptions without running the command. Exact command names and aliases take precedence. A unique abbreviation can shorten a complete name or its colon-separated segments.

```sh
./artisan list cache --format=json --short
./artisan help migrate --format=json
./artisan help migrate --format=xml
./artisan help migrate --format=md
```

`list` and `help` accept `txt`, `json`, `xml` and `md`. The text list shows the global options before the command catalog, including when you select a command namespace. Use `list --raw` for just the flat text list. `help --raw` prints the full help as plain text, even when ANSI output is enabled. `list --short` retains names, aliases and implementation status and omits command details. Structured output keeps command and option identifiers stable across locales. Successful help and list commands exit with status 0. Invalid arguments, ambiguous commands and unknown commands exit with status 1.

`--quiet` suppresses normal output. `--silent` also suppresses console errors. The global `--no-interaction`, `--ansi`, `--no-ansi`, `--version` and verbosity flags are available in command definitions.

See [Artisan translations](./localization.md#artisan-translations) for language selection and overrides.

## Programmatically Executing Commands {#programmatically-executing-commands}

Call a registered command from an HTTP handler or from another Artisan command with `Artisan::call(name, arguments, &mut output).await`. The call runs within the current application and returns the command's exit code. Unknown commands and invalid arguments return `ConsoleError`. Use a separate `Output` for each invocation to capture stdout and stderr.

```rust
use std::ffi::OsString;

use openrust::{
    console::{ConsoleError, Output},
    support::facades::Artisan,
};

async fn inspire_help() -> Result<(i32, String), ConsoleError> {
    let mut output = Output::default();
    let arguments = ["inspire", "--format=json"].map(OsString::from);
    let status = Artisan::call("help", &arguments, &mut output).await?;
    Ok((status, output.stdout))
}
```

Pass a separate `OsString` for each positional argument or option. Values that contain spaces stay in one token. Include a switch for a true Boolean flag, and repeat options that take multiple values. Use `Artisan::call_silently(name, arguments).await` to discard both streams and keep the exit code.

### Queueing Artisan Commands {#queueing-artisan-commands}

Prepare a command with `Artisan::queue`. Call `dispatch().await` to send it to the queue and obtain the dispatch result. Specify a destination with `on_connection` and `on_queue`, or use the default connection and its queue.

```rust
use openrust::{
    queue::{DispatchResult, QueueResult},
    support::facades::Artisan,
};

async fn queue_inspire() -> QueueResult<DispatchResult> {
    Artisan::queue("inspire", &[])
        .on_connection("redis")
        .on_queue("commands")
        .dispatch()
        .await
}
```

Start a Worker for the selected destination with `./artisan queue:work redis --queue=commands`. The payload stores the command name and UTF-8 argument tokens as JSON. The Worker resolves the command in its own application, discards its console output and acknowledges a returned exit code, including a nonzero code. Return a `ConsoleError` from the command to use normal queue retries and failure recording.

## Optional Route Installers {#optional-route-installers}

Run these commands from a source checkout to generate the route files and update the module and bootstrap declarations. Then rebuild the application. Existing route files are preserved.

```sh
./artisan install:broadcasting
./artisan install:grpc
```

Use `./artisan install:api` to create and register API routes. It preserves existing routes by default. See [Sanctum installation](../packages/sanctum.md#installation) for the feature flag and migration options. Broadcasting registers `/broadcasting/auth` and requires a driver and channel rules. For gRPC, register Tonic services and set `GRPC_HOST` and `GRPC_PORT` to enable the listener.

## Application initialization {#complete-installation}

Run `./artisan init` in a source checkout (`artisan.bat init` on Windows). The wizard reads existing values, asks for application, database, cache, session and queue settings, hides password input and checks connections before saving. Noninteractive use reads the project environment. Current initialization supports SQLite, MySQL/MariaDB and PostgreSQL, local/database/Redis services, and log/SMTP mail plus the built-in SMS providers. API uses Sanctum; channels and gRPC registration are installed. The Reverb package is installed; REVERB_ENABLED controls whether its server starts with the application. Missing Reverb credentials are generated without overwriting existing values. The default initialization profile installs all supported extensions; the application skeleton itself remains lightweight.

```sh
./artisan init --dry-run
./artisan init
./artisan init --no-interaction
./artisan init --extensions=api --no-interaction
```

Choose `--extensions=all` (the default and the wizard recommendation), or select `api,broadcasting,reverb,grpc,passport,fortify,passkeys,socialite,precognition,oidc,sso`. `api` installs Sanctum; `passport` also selects `api`, `passkeys` selects Fortify, `sso` selects OIDC and Fortify, and `reverb` selects broadcasting. A later run can add extensions. Include Reverb if it is already configured. The API `/user` route retains `auth:sanctum`; Passport receives an independent `api` guard. An incompatible existing guard is preserved and reported as a conflict.

Fortify installs actions, its application provider, configuration and migrations. Newly generated initialization configuration uses `views=false` and enables email verification. Existing custom configuration is preserved. Configure your frontend password-reset URL with `ResetPassword::create_url_using`, or supply the named `password.reset` route; headless initialization does not generate frontend screens. Passkeys is enabled when selected. For a new configuration, set APP_URL to an exact HTTPS origin with a domain and no path; HTTP localhost is allowed for development, for example `APP_URL=http://localhost:8000`. HTTP IP addresses fail preflight before any writes. Socialite and OIDC packages are enabled, but provider credentials, registered callbacks, explicit local account binding and application routes remain application configuration. Selecting SSO does not create an identity provider or configure global logout. Precognition is built in: apply `precognitive` to the routes you choose. Installation status and these external prerequisites are reported separately.

Missing `.env` is copied from the example; existing keys and credentials are retained. Missing application keys use `key:generate`. Shared package services publish all selected resources under the source mutex. If selected installers are not compiled into the running program, one bootstrap build loads them; one final build compiles all generated modules before a single normal migration pass. No individual installation command is recursively run. Production requires confirmation or `--force`. Successful initialization tells you to run `cargo run`; it does not start a background server. `--without-migration` reports preparation with migrations pending. `--force` overwrites publishable configuration and scaffolding and permits production migration; it preserves APP_KEY, Passport keys, unrelated application modules and every historical migration. `--dry-run` does not connect, write, lock, build or migrate. Required failures return nonzero, stop dependent steps and retain completed changes for retry. Concurrent source installation fails immediately.

SMTP selection checks connectivity, TLS and authentication using the existing SMTP transport without sending an email. The wizard collects built-in SMS provider credentials; sender identities and business templates remain provider-specific configuration, and no SMS is sent or delivery claimed. Reverb listener and publishing addresses are prompted when enabled; its default mode is embedded. Existing configuration remains the default during the wizard.

Use `--env=acceptance` to initialize `.env.acceptance`; the same environment is forwarded to configuration clearing, key generation and migrations, leaving `.env` unchanged.

## Generating Commands {#generating-commands}

A destructive command can call `openrust::console::confirm_to_proceed(production, input.matches())?` and proceed only when it returns `true`. Set `production` from the application environment or the command’s confirmation condition. The shared localized prompt defaults to refusal; non-interactive input refuses, and a declared `--force` flag bypasses confirmation.

Create a command from a source checkout. The generated type implements the Artisan command contract.

```sh
./artisan make:command SendReport
./artisan make:command Reports/ArchiveReport --command=reports:archive
```

The first command writes `app/console/commands/send_report.rs` with the signature `app:send-report`. The second writes a command under the `reports` module and sets its signature explicitly. Use `-f` or `--force` to replace an existing file.

Declare the new module and register the command in `app/console/commands/mod.rs`. For `SendReport`, add `mod send_report;` and call `artisan.add_command(Box::new(send_report::SendReport::default()))?;` inside `register`. Then rebuild and run `./artisan app:send-report`.

To customize generated code, add `stubs/console.rs.stub` at the application root. The generator substitutes `{{class}}` and `{{command}}`.

### Generating an accompanying test {#generating-an-accompanying-test}

Add `--test` to create a feature test along with the command.

```sh
./artisan make:command SendReport --test
```

The test goes to `tests/feature/console/commands/send_report_test.rs`. Nested command names keep their subdirectories. Declare the test module and run it through Cargo using the [test setup](../testing/getting-started.md#creating-tests). The template starts with the same health check as `make:test`. Add assertions for your command to the generated test.

`--force` allows both generated files to be replaced. If test generation fails after the command is created, Artisan exits with status 1 and keeps the command file. Resolve the reported error and run `./artisan make:test Console/Commands/SendReportTest` to create the test separately.

## Stub Customization {#stub-customization}

Publish the generator templates to the application’s `stubs/` directory. The command publishes templates for structs, traits, enums, commands, factories, migrations, models, policies, seeders and feature and unit tests. Edit the published template to change the next file that its generator creates.

```sh
./artisan stub:publish
./artisan stub:publish --existing
./artisan stub:publish --force
```

The default preserves existing templates and publishes missing ones. `--existing` replaces only templates already present, including when combined with `--force`. Use `--force` on its own to replace existing templates and publish missing ones.

A synchronous `foundation.publishing_stubs` listener can use the `PublishingStubs` event to add a source file, insert a compiled template, or remove a template before publication. Destinations are filenames within the application’s stubs directory.

## Generating Rust Types {#generating-rust-types}

Artisan generates Rust structs, traits and enums. Declare the resulting modules before using the types.

```sh
./artisan make:class Report
./artisan make:class SendReport --invokable
./artisan make:interface ReportStore
./artisan make:trait ReportFormatting
./artisan make:enum ReportState
./artisan make:enum ReportCode --int
./artisan make:enum ReportKind --string
```

`make:interface` and `make:trait` both generate Rust traits. Interface generation checks `app/contracts` then `app/interfaces`. Trait generation checks `app/concerns` then `app/traits`. Enum generation checks `app/enums` then `app/enumerations`. Each falls back to `app/`. Prefix a name with `App/` to start from `app/` explicitly.

An invokable struct has an explicit `invoke()` method. Integer-backed enums expose an `i64` value and string-backed enums expose a static string through `value()`. Use `-f` or `--force` to replace a generated file. Publish customizable templates with [`stub:publish`](#stub-customization). The interface command uses its built-in template.

## Command signatures and closure input {#command-signatures}

Declare required `{user}`, optional `{user?}`, default `{user=Taylor}`, required arrays `{user*}` or optional arrays `{user?*}`. Options use `{--force}`, `{--queue=}`, `{--queue=default}`, and `{--id=*}`; `{--Q|queue=}` declares a shortcut. Array defaults use `{user=*Taylor,Dayle}` or `{--id=*1,2}`. Put descriptions after ` : ` and split the signature over lines when needed.

```rust
use openrust::{
    console::{ConsoleError, Input, Output},
    support::facades::Artisan,
};

fn register() -> Result<(), ConsoleError> {
    Artisan::command_with(
        "mail:send {user : User ID} {--Q|queue=default} {--id=*} {--force}",
        |input: &Input, output: &mut Output| {
            let user = input.argument("user").unwrap();
            let queue = input.option("queue").unwrap();
            output.comment(&format!("{user}: {queue}"));
            Ok(0)
        },
    )?.purpose("Send a marketing email")
}
```

`CommandDefinition::from_signature` creates the same definition for typed commands. `Input::argument` and `option` return JSON scalar/array values or `None` for unknown/unset names; `arguments` and `options` return each input group. Missing optional scalar values appear as JSON null in the group, while arrays remain arrays. Custom Clap value parsers should read their concrete types from `matches()`; these JSON accessors cover string, Boolean, numeric and UTF-8 OS-string values.

The original `Artisan::command(signature, |output| …)` remains available with exit status 0. `command_with` accepts an explicitly typed input/output closure returning `Result<i32, ConsoleError>`, or an async callback `fn(&Application, &Input, &mut Output) -> ConsoleFuture<Result<i32, ConsoleError>>`. Resolve services with `app.make::<Service>().await` inside that callback; resolution belongs to the current application and command execution, not registration. Explicit signatures avoid changing existing Rust closure type inference. This does not provide PHP parameter reflection or automatic injection of undeclared Rust types.

A callback whose first parameter is `Arc<Service>` is resolved automatically from the current application at execution. Register that service through your provider or the existing container type descriptors; constructors use the same container and are not run merely to list commands.

```rust
use std::sync::Arc;
use openrust::{console::{ConsoleError, ConsoleFuture, Input, Output}, support::facades::Artisan};

struct Mailer;
impl Mailer {
    fn send(&self, user: &str) -> String { format!("Sent to {user}") }
}
fn send<'a>(mailer: Arc<Mailer>, input: &'a Input, output: &'a mut Output)
    -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
    Box::pin(async move {
        output.comment(&mailer.send(input.argument("user").unwrap().as_str().unwrap()));
        Ok(0)
    })
}
fn register_injected() -> Result<(), ConsoleError> {
    Artisan::command_with("mail:send-injected {user}", send)?.purpose("Send with an injected service")
}
```

## Typed command input {#typed-command-input}

`Input::input()` returns a `CommandInput` snapshot. `input_value(key, default)` reads one value. The bag offers `string`/`str`, `integer`, `float`, `boolean`, their `_or` and `_strict` variants, `clamp`, `date`, `interval`, backed `r#enum`/`enums`, `array`, and `collect`, reusing the request input conversion rules. Date parsing uses the application time zone unless supplied, and returns an error for invalid input; the original error remains available through `ConsoleError::source`.

```rust
use openrust::{console::{ConsoleError, Input, Output}, support::facades::Artisan};

fn register_typed() -> Result<(), ConsoleError> {
    Artisan::command_with(
        "reports:send {--from=} {--limit=10} {--active}",
        |input: &Input, output: &mut Output| {
            let data = input.input();
            let date = data.date("from", Some("!Y-m-d"), Some("UTC"))?;
            let limit = data.integer_strict("limit", 10);
            output.comment(&format!("limit={limit}, active={}, date={}", data.boolean("active"), date.is_some()));
            Ok(0)
        },
    )?.purpose("Send a typed report")
}
```

`arguments()` and `options()` keep their groups. For a `CommandInput` built with conflicting groups, the fixed implementation merges options then arguments, so arguments win. `all`/`to_array` return the combined data; `all_keys` retains missing selections as null and an empty selection returns the whole bag, while `only` omits missing paths. `get`, `has`/`exists`, `has_all`, `has_any`, `missing`, `filled`, `is_not_filled`, `any_filled`, and `except` use the existing dot-path rules. False, zero and empty arrays count as filled; empty strings and null do not.

Conditional `when_has`, `when_filled`, `when_missing`, and `when_enum` return `Option<R>` (the enum variant also returns `Result`). `None` means the callback was not invoked; use `or_else` for a fallback. Rust keeps explicit callback return types rather than PHP’s truthiness-based union of the bag and callback value. Read dynamic properties with `get`/`exists`. These adapters do not execute arbitrary PHP expressions.

## Isolatable commands {#isolatable-commands}

Implement `contracts::console::Isolatable` on a command and register it with `Artisan::add_isolatable`, `console::Application::add_isolatable_command`, or the Console Kernel contract’s `register_isolatable_command`. This adds `--isolated[=code]` automatically. Rust explicitly selects the trait-aware registration path because an erased `dyn Command` cannot discover an unrelated trait implementation. The original `add` and `CommandDefinition::isolatable()` remain available; the latter uses the command name and a one-hour lease.

```rust
use openrust::{
    cache::Ttl,
    console::{Command, CommandDefinition, ConsoleError, ConsoleFuture, Input, Output},
    contracts::console::Isolatable,
    foundation::Application,
    support::facades::Artisan,
};

struct SendReports(CommandDefinition);
impl Default for SendReports {
    fn default() -> Self {
        Self(CommandDefinition::from_signature("reports:isolated {user}").expect("valid signature"))
    }
}
impl Command for SendReports {
    fn definition(&self) -> &CommandDefinition { &self.0 }
    fn handle<'a>(&'a self, _app: &'a Application, input: &'a Input, output: &'a mut Output)
        -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            output.comment(&format!("Report for {}", input.argument("user").unwrap()));
            Ok(0)
        })
    }
}
impl Isolatable for SendReports {
    fn isolatable_id(&self, input: &Input) -> Result<Option<String>, ConsoleError> {
        Ok(input.argument("user").and_then(|value| value.as_str().map(str::to_owned)))
    }
    fn isolation_lock_expires_at(&self, _input: &Input) -> Result<Ttl, ConsoleError> {
        Ok(Ttl::Seconds(300))
    }
}
fn register_isolated() -> Result<(), ConsoleError> {
    Artisan::add_isolatable(Box::<SendReports>::default())
}
```

Isolation is optional: normal calls do not acquire a lock. With `--isolated`, contention skips the handler and returns 0, or the code supplied by `--isolated=12`. The default trait methods also use the command name and one hour. Override `isolatable_id` to add an input-dependent suffix and `isolation_lock_expires_at` for `Ttl::Seconds` or `Ttl::Until(SystemTime)`. Empty IDs and absent IDs are distinct.

All participating processes must use the same lock-capable cache store and namespace. Array stores coordinate only within the application process; file stores require a shared filesystem, and database/Redis stores use their configured backends. Normal completion, errors and in-runtime cancellation release owned locks. A terminated process relies on expiry. Expired owners cannot release their replacement’s lock. Database whole-second locks use Unix seconds plus the lease; subsecond leases retain their native rounding policy.

## Global options and validated input edits {#editing-command-input}

Dispatched `Input` helpers include the invoked `command` argument (including aliases) and inherited global options: `verbose` is boolean, `env` is a nullable string, and `ansi` is null/true/false. `no-ansi` contributes to `ansi` rather than appearing as a separate option. `matches()` retains native Clap flags, environment values and verbosity counts; existing CLI parsing and output precedence remain unchanged. A standalone `CommandDefinition::parse` retains its declared argument scope. `Input::clone` keeps the registered definition identity and shutdown token, so an edited clone remains usable by the Console Kernel.

```rust
use openrust::{console::{ConsoleError, Input, Output}, support::facades::Artisan};
use serde_json::json;

fn register_edited() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:edit {user} {--queue=default}", |input: &Input, output: &mut Output| {
        let mut edited = input.clone();
        edited.set_argument("user", ["42"])?;
        edited.set_option("queue", json!("priority"))?;
        let data = edited.input();
        output.comment(&format!("user={}, queue={}", data.string("user"), data.string("queue")));
        Ok(0)
    })?.purpose("Edit parsed command input")
}
```

`set_argument` takes CLI tokens; `set_option` takes JSON strings, Boolean switches, counts, arrays, or null for a bare optional value. Each edit reuses the original Clap parser and constraints and updates `matches()` as well as the typed helpers. The original remains unchanged if validation fails; other values, empty strings and non-UTF-8 OS arguments are preserved.

These are CLI revalidation adapters, not arbitrary PHP object mutation. A value that cannot be represented by the definition, a positional gap, or clearing a nonempty default array returns an error rather than silently rebinding another field or restoring its default. Reading or editing global input does not reconfigure an output stream that has already been created.

## Prompting for missing arguments {#prompting-for-missing-input}

Implement `PromptsForMissingInput` and retain it with `.prompting()` before registering the command. Required arguments are collected in declaration order before final validation and isolation. Supplied arguments are not prompted again; `--no-interaction`, `--quiet`, and `--silent` skip prompting and still rejects missing required arguments.

```rust
use openrust::{
    console::{Command, CommandDefinition, CommandRegistration, ConsoleError,
        ConsoleFuture, Input, MissingInputPrompt, Output},
    contracts::console::PromptsForMissingInput,
    foundation::Application,
    support::facades::Artisan,
};
use std::collections::BTreeMap;

struct Deliver(CommandDefinition);
impl Command for Deliver {
    fn definition(&self) -> &CommandDefinition { &self.0 }
    fn handle<'a>(&'a self, _: &'a Application, input: &'a Input,
        output: &'a mut Output) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            output.stdout.push_str(&format!("user={}, queue={}",
                input.argument("user").unwrap().as_str().unwrap(),
                input.option("queue").unwrap()));
            Ok(0)
        })
    }
}
impl PromptsForMissingInput for Deliver {
    fn prompt_for_missing_arguments_using(&self) -> BTreeMap<String, MissingInputPrompt> {
        BTreeMap::from([("user".into(),
            MissingInputPrompt::text("Which user should receive the report?", "e.g. 123"))])
    }
}
fn register_prompted() -> Result<(), ConsoleError> {
    Artisan::add(Box::new(CommandRegistration::new(Deliver(
        CommandDefinition::from_signature("reports:prompt {user} {--queue}")?
    )).prompting()))
}
```

The default question uses the argument description or name. `MissingInputPrompt::text` supplies a label and placeholder; `MissingInputPrompt::custom` accepts an asynchronous application/mutable-input/output callback returning CLI tokens (`Vec<OsString>`), including multiple array values. A custom callback may return `"0"`; the default text question rejects an empty answer or `"0"`, matching required-text validation. Question definitions are reevaluated for each missing argument.

`after_prompting_for_missing_arguments` runs only after at least one missing argument was collected. It receives mutable `Input` and may call `set_option` or `set_argument`; final validation precedes execution. Compose `.prompting().isolatable()` on the same `CommandRegistration` when both traits are implemented. The adapter retains trait objects before type erasure and uses the existing registration, cache-lock and lifecycle pipeline.

Default text questions require an attached terminal; captured HTTP/programmatic output does not read process stdin. Supply explicit arguments or a custom callback in those environments. No interaction is started for help. Cancellation and callback failures propagate through the existing error handling; callbacks do not hold the isolation lock while awaiting input.

## Command registration lifetime {#command-lifecycle}

Commands are assembled once per application after providers are ready. Subsequent Facade calls, HTTP requests and managed runners reuse the registered command instances and closure captures. Separate application instances and separate CLI processes have separate registries. The following closure returns `1`, then `2` when called twice in the same application.

```rust
use openrust::{console::{ConsoleError, Input, Output}, support::facades::Artisan};
use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};

fn register_counter() -> Result<(), ConsoleError> {
    let calls = Arc::new(AtomicUsize::new(0));
    Artisan::command_with("reports:count", move |_: &Input, output: &mut Output| {
        output.stdout.push_str(&(calls.fetch_add(1, Ordering::SeqCst) + 1).to_string());
        Ok(0)
    })?.purpose("Count calls within this application")
}
```

Each invocation keeps its own input, output, verbosity and lifecycle scope. A command failure does not rebuild the registry. Failed registration does not publish a partial registry and may retry. No registry lock is held during command execution, so nested calls remain possible.

Concurrent requests may execute the same command object. Keep shared fields thread safe, and resolve request-specific inputs, users and dependencies inside `handle` or the current `command_with` callback; do not capture request data while registering commands.

## Writing command output {#writing-command-output}

Use `input.io(output)` to borrow the current command context. `line`, `info`, `comment`, `question`, `warn`, `error`, `alert` and `new_line(count)` write through the same output. `info` writes a green message; `output_components()` retains the existing information/warning/error component layout. Output errors remain available through `ConsoleError::source`.

```rust
use openrust::{
    console::{AsyncCommandCallback, ConsoleError},
    support::facades::Artisan,
};
use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};

fn register_output() -> Result<(), ConsoleError> {
    let calls = Arc::new(AtomicUsize::new(0));
    Artisan::command_with("reports:output", AsyncCommandCallback::new(move |_, input, output| {
        let calls = calls.clone();
        Box::pin(async move {
            tokio::task::yield_now().await;
            let mut io = input.io(output);
            io.info("Report ready")?;
            io.line("<options=bold>Summary</>")?;
            io.comment("Report generated")?;
            io.question("Review the report")?;
            io.warn("One record was skipped")?;
            io.error("A record needs correction")?;
            io.new_line(1)?;
            io.alert("Check complete")?;
            io.output_components().info("Saved");
            io.line(&format!("Calls: {}", calls.fetch_add(1, Ordering::SeqCst) + 1))?;
            Ok(0)
        })
    }))?.purpose("Display a report")
}
```

Named style tags, foreground/background colors, options and hyperlinks are formatted before writing. Unknown tags remain literal; escaped angle brackets remain visible. Unclosed style state belongs to the output object and does not leak into a different output. Malformed nesting, colors or options return `ConsoleFormatError`. `--ansi` and `--no-ansi` control decoration; `--quiet` and `--silent` suppress normal messages.

The normal CLI entry streams these helpers immediately and sends any pending components first. It does not keep an unbounded terminal transcript in `stdout`. Programmatic `Output::default()` captures the text in `stdout` for HTTP responses or assertions. Native buffered command-event metadata contains pending/captured text, rather than an archive of raw terminal writes.

`AsyncCommandCallback::new` supplies the boxed-future return type for capturing asynchronous closures. It passes the same callback to the existing `command_with` dispatcher without a second executor or extra callback layer. Named asynchronous functions remain supported. Input definitions can be checked with `has_argument`/`has_option`, including declared inputs whose current value is missing; `input()` exposes the typed bag and `input_value` reads a value with a fallback.

For per-message verbosity or direct styled writes, use the interfaces below. Questions, tables, and progress bars are separate from these message methods.

## Output style and verbosity {#output-style-verbosity}

`get_output()` returns the actual `console::OutputStyle`; its `get_output()` borrows the underlying `Output`. `write`, `writeln`, and `new_line` use the same formatter and stream. Pass a string or a slice of strings; `writeln` adds a newline to each item. `OutputFormat::Raw` preserves tags, while `Plain` formats then removes remaining markup. Use `OutputOptions` to combine a format with a verbosity threshold. `write_iter` and `writeln_iter` accept string iterators, including owned strings and single-pass iterators; they preserve the same per-item and counter rules.

The default message threshold is `Normal`. Use the `_with` methods to set a threshold without changing the one-argument methods. `line_with` also accepts an optional style. `Verbose`, `VeryVerbose`, and `Debug` require `-v`, `-vv`, and `-vvv`; `Quiet` remains visible with `--quiet`, and `--silent` suppresses every message. `OutputVerbosity::from_name` maps `v`, `vv`, `vvv`, `quiet`, and `normal`; unknown names use `Normal`.

`new_lines_written` starts at one and `new_line_written` starts false. They track writes through this style object; direct buffer edits or raw writes do not update them. Hidden `write` and `writeln` have different counter rules, so check the counter rather than inferring it from captured text. `is_quiet`, `is_verbose`, `is_very_verbose`, and `is_debug` query the current output. `set_input` replaces command input reads; `set_output` accepts a real `OutputStyle` borrowing the replacement stream. Rust borrows keep the replacement input and output alive.

```rust
use openrust::{
    console::{ConsoleError, Input, Output, OutputFormat, OutputVerbosity},
    support::facades::Artisan,
};

pub fn register_styled() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:styled", |input: &Input, output: &mut Output| {
        let mut io = input.io(output);
        io.info_with("Details", OutputVerbosity::Verbose)?;
        io.warn_with("Essential", OutputVerbosity::Quiet)?;
        io.line_with("Summary", Some("info"), None)?;
        let style = io.get_output();
        style.write(&["<raw>", "text"], false, OutputFormat::Raw)?;
        style.new_line(1)?;
        Ok(0)
    })?;
    Ok(())
}
```

## Interactive questions {#interactive-questions}

Use asynchronous command I/O to ask for text, confirmation, hidden text, and completion. `confirm` defaults to false; `OutputStyle::confirm` defaults to true.

```rust
use openrust::{
    console::{AsyncCommandCallback, CompletionCallback, ConsoleError},
    support::facades::Artisan,
};

fn register_questions() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:questions", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let mut io = input.io(output);
            let name = io.ask_with("Name", Some("Guest")).await?;
            let proceed = io.confirm("Continue").await?;
            let token = io.secret("Token").await?;
            let color = io.anticipate("Color", CompletionCallback::values(vec!["blue".into(), "green".into()]), Some("blue")).await?;
            io.line(&serde_json::json!({"name": name, "proceed": proceed, "token_present": token.is_some(), "color": color}).to_string())?;
            Ok(0)
        })
    }))?.purpose("Collect report options")
}
```

`--no-interaction`, `--quiet`, and `--silent` disable questions, including automatic missing-argument prompts; missing required arguments still fail. Captured HTTP calls do not read process stdin. A missing input source returns the default, or a typed error when no default is available, then disables further interaction for that input.

For custom validation, create `Question`, set `QuestionCallback` with `set_normalizer` or `set_validator`, then call `get_output().ask_question(&question).await`. Validators may return structured JSON; `ask_with_validator` and `ask_hidden_with` retain that result. Noninteractive defaults run the validator without the normalizer. `set_max_attempts`, `set_trimmable`, `set_multiline`, and `set_timeout` configure retries and reading.

`Input::set_stream` accepts a `ConsoleInputStream` wrapping an asynchronous buffered reader. Input clones share the stream position but snapshot the interaction setting; supplying Input to the Kernel shares that invocation state. Completion callbacks run in the command application scope, including across await. `CompletionCallback::iter` lazily caches a single-pass iterator, and `pairs` provides string keys followed by values. Ctrl+C returns 130; cancellation, timeout, and validation failure restore terminal mode. Windows terminal acceptance remains part of the release matrix.

## Choices and multiple selections {#choice-questions}

`choice` accepts a list with no default. Use `choice_with` to provide a default, maximum attempts, and the multiple-selection flag. In multiple mode, enter comma-separated indices or values; the result is an array. `choice_pairs_with` preserves declared order and returns association keys. Canonical integer strings are treated as numeric keys; other strings retain their exact spelling.

```rust
use openrust::{
    console::{AsyncCommandCallback, ConsoleError},
    support::facades::Artisan,
};

fn register_choices() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:choices", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let mut io = input.io(output);
            let channel = io.choice_with("Channel", ["email", "sms"], Some(serde_json::json!(0)), Some(2), false).await?;
            let formats = io.choice_with("Formats", ["csv", "json"], Some(serde_json::json!("0,1")), Some(2), true).await?;
            let priority = io.choice_pairs_with("Priority", [("normal", "Normal"), ("urgent", "Urgent")], Some(serde_json::json!("normal")), Some(2), false).await?;
            io.line(&serde_json::json!({"channel": channel, "formats": formats, "priority": priority}).to_string())?;
            Ok(0)
        })
    }))?.purpose("Collect report choices")
}
```

Text, number and boolean choices use the existing scalar comparison rules. Ambiguous duplicate labels require a key instead. A question without a default returns null in noninteractive mode; an invalid default fails validation. `OutputStyle::choice_with` additionally resolves a default value to its key; command I/O uses the supplied default directly.

For custom prompts or error text, create `ChoiceQuestion::new` or `from_pairs`, use `set_prompt`, `set_error_message` (one answer argument: `%s`, repeatable `%1$s`, width/precision, and `%%` for a percent sign), `set_multiselect`, then pass `as_question()` to `ask_question`. `get_choices`, `get_prompt` and `is_multiselect` read the actual definition. Changing the trim setting updates retained default validators. Native Question clones own their settings; explicit callback and completion cache handles remain shared. Format tags should surround complete graphemes. Formatted visible text follows NFC and grapheme boundaries; Raw output, hyperlink targets and returned keys keep their original data.


Use `ConsoleError::from_runtime_source` for operational callback failures: they stop retries and retain the original error type and cause. Ordinary validation errors remain retryable. Missing-input failures use the validated default and disable further interaction. The terminal is restored before a normalizer or validator runs. A template that produces invalid UTF-8 returns `ConsoleQuestionError::InvalidEncoding` with its original source; arbitrary PHP binary strings are not returned as Rust text.

Wrap a type implementing `Display` in `ChoiceLabel(value)` when using application label objects. Labels are captured as text in the native JSON answer model; PHP object identity is not carried through answers. Associative helpers retain the original key, so application objects can remain in the caller’s collection.

## Tables {#tables}

Use `input.io(output).table(headers, rows)` to render a table. Headers and rows accept iterators; JSON scalar cells use the shared scalar conversion without adding JSON quotes. Select model columns in the desired order before passing their values to the table.

```rust
use openrust::{
    console::{ConsoleError, Input, Output},
    support::facades::Artisan,
};

fn register_tables() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:table", |input: &Input, output: &mut Output| {
        let users = serde_json::json!([
            {"name": "Ada", "email": "ada@example.test"},
            {"name": "Li", "email": "li@example.test"}
        ]);
        let rows = users.as_array().unwrap().iter().map(|user| {
            [user["name"].clone(), user["email"].clone()]
        });
        input.io(output).table(["Name", "Email"], rows)?;
        Ok(0)
    })?;
    Ok(())
}
```

`table_with` selects a `TableStyle`; `table_with_columns` additionally accepts `(column_index, style)` pairs. `TableStyle::from_name` provides `default`, `markdown`, `borderless`, `compact`, `symfony-style-guide`, `box`, and `box-double`. Customize a style with its border, padding and format setters. `TablePadding::{Left, Right, Both}` names the side that receives padding; `Right` is the default.

For direct rendering, build `Table::new(io.get_output())`, set headers, rows and column styles, then call `render()`. `TableCell::display(value)` explicitly adapts a business type implementing `Display`; JSON values and ordinary scalars convert directly. Multi-line cells, visible Unicode width, formatter tags, ANSI output and quiet/silent filtering share the original output pipeline. This rectangular-table interface does not claim support for cell spans, row separators, titles or the entire inherited helper SDK.

## Progress Bars {#progress-bars}

Use `with_progress_bar_iter` for a countable collection. The callback receives the item, the current bar, and its original integer or string key; the helper advances only after the callback succeeds and returns the original collection. `Vec`, arrays, ordered maps, base collections, model collections, and borrowed collections implement `ProgressIterable`. No model cloning is required.

```rust
use openrust::{
    console::{ConsoleError, Input, Output, ProgressBar, ProgressFormatter},
    support::facades::Artisan,
};
use std::sync::Arc;

fn register_progress() -> Result<(), ConsoleError> {
    ProgressBar::set_format_definition(
        "reports",
        "%task% %current%/%max% [%bar%] %percent:3s%%",
    )?;
    Artisan::command_with("reports:progress", |input: &Input, output: &mut Output| {
        let mut io = input.io(output);
        let mut processed = Vec::new();
        let items = io.with_progress_bar_iter(vec!["csv", "json", "html"], |item, _, _key| {
            processed.push(item.to_uppercase());
            Ok(())
        })?;
        io.new_line(1)?;

        let count = i64::try_from(items.len()).map_err(ConsoleError::from_source)?;
        let mut bar = io.get_output().create_progress_bar(count)?;
        let label: ProgressFormatter = Arc::new(|bar| {
            Ok(bar.get_message().unwrap_or("work").to_owned())
        });
        bar.set_format("reports")
            .set_message("build")
            .set_placeholder_formatter("task", label);
        bar.start()?;
        let mut total = 0;
        for item in &items {
            total += item.len();
            bar.advance()?;
        }
        bar.finish()?;
        drop(bar);
        io.new_line(1)?;
        io.info(&format!("{}:{total}", processed.join(",")))?;
        Ok(0)
    })?;
    Ok(())
}
```

For a numeric total, use `with_progress_bar(steps, callback)` and call `advance` or `advance_by` from the callback. For manual control, obtain a bar with `create_progress_bar(total)`, then call `start`, `advance`, and `finish`. These callbacks execute synchronously. Callback errors propagate without silently finishing the bar.

The factory uses the existing output: captured command calls retain progress in their buffer; an actual console streams progress to stderr and keeps ordinary stdout separate. Quiet and silent flags suppress rendering. The default redraw interval is 40 ms, with completion always considered for display. Empty collections, unknown totals, and repeated `finish` calls retain their own lifecycle behavior. `progress_start`, `progress_advance`, and `progress_finish` provide stored style-level progress; `progress_finish` additionally writes two newlines and releases that stored state.

Customize a bar with its format, message, bar characters, width, redraw-frequency and placeholder methods. Shared named formats and placeholder definitions belong to the current application; set them during command registration or execution. A bar retains its owning application definitions. A standalone bar can use literal formats and instance placeholders. A placeholder callback borrows the actual bar and returns `Result<String, ConsoleError>`. The memory placeholder reports native process resident memory; it does not report a language VM heap.

The numeric and collection helpers have separate Rust methods so their callback and return types remain explicit. Passing `&items` preserves caller ownership on failure. This section verifies synchronous collection and manual progress; lazy iterator adapters, asynchronous/mutable task callbacks and the remaining inherited output contracts are tracked separately.

## Lazy progress iteration {#lazy-progress-iteration}

Use `OutputStyle::progress_iterate(items)` to iterate through tasks. It starts on the first `next()`, advances when the next item is requested, and finishes only on natural exhaustion. Each item is a `Result`: propagate output errors with `?`. Early `break`, callback failure, or dropping the iterator does not add a completed frame or the final two style newlines.

```rust
use openrust::{
    console::{ConsoleError, Input, Output},
    support::facades::Artisan,
};

fn register_progress_iteration() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:iterate", |input: &Input, output: &mut Output| {
        let mut io = input.io(output);
        let mut sum = 0;
        for number in io.get_output().progress_iterate([2, 4, 6]) {
            sum += number?;
        }
        io.info(&format!("sum:{sum}"))?;
        Ok(0)
    })?;
    Ok(())
}
```

Manual bars provide `iterate(items)` and `iterate_with_max(items, max)`; use `iterator.progress_bar()` to access that same bar between items. The style variants include `progress_iterate_with_max`. `None` and `Some(0)` infer the total from an exact iterator size hint; otherwise the total remains unknown or the existing manual maximum is retained. A positive explicit total overrides inference. Items, tuple keys, and borrowed model identities remain unchanged; conversion to the underlying iterator is also deferred. An empty exact iterator finishes without a start frame. Asynchronous and mutable callback helpers are described below; these entries do not block or run tasks in the background.

## Awaited and mutable progress callbacks {#async-progress-callbacks}

Use `with_progress_bar_async` for a numeric total, `with_progress_bar_iter_async` for read-only items, or `with_progress_bar_iter_mut_async` for explicit in-place changes. These use the same progress and command execution pipeline; each boxed future is awaited before advancing.

```rust
use openrust::{
    console::{AsyncCommandCallback, ConsoleError},
    support::facades::Artisan,
};

fn register_progress_async() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:async", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let mut io = input.io(output);
            io.with_progress_bar_async(1, |bar| Box::pin(async move {
                tokio::task::yield_now().await;
                bar.advance()?;
                Ok(())
            })).await?;
            io.new_line(1)?;

            let mut values = vec![1, 2, 3];
            io.with_progress_bar_iter_mut_async(&mut values, |value, _, _key| {
                Box::pin(async move {
                    tokio::task::yield_now().await;
                    *value *= 2;
                    Ok(())
                })
            }).await?;
            io.new_line(1)?;
            io.info(&format!("values:{}", values.iter().sum::<i32>()))?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

`ProgressIterableMut` supports slices, arrays, vectors, ordered maps, base collections, model collections and mutable references. Keys and model identities are preserved. A mutable model callback can await `model.save()` and propagate its concrete error through `ConsoleError::from_source`. `with_progress_bar_iter_mut` is the synchronous alternative. In-place changes are explicit native behavior; default callbacks remain read-only. The helpers do not implicitly create transactions or roll back completed writes. Errors and cancellation do not finish the bar; passing a mutable reference retains ownership and any changes already made.

## Placeholder output access {#progress-placeholder-output}

Within a `ProgressFormatter`, use `bar.get_output()` to borrow the actual `ProgressOutput` stream. It exposes `write`, `writeln`, single-pass iterable variants, verbosity queries and `is_decorated`. `OutputOptions` controls format and message verbosity. CLI writes target stderr; captured calls retain the same output buffer.

```rust
use openrust::{
    console::{ConsoleError, Input, Output, OutputFormat, ProgressFormatter},
    support::facades::Artisan,
};
use std::sync::Arc;

fn register_progress_output() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:progress-output", |input: &Input, output: &mut Output| {
        let mut io = input.io(output);
        let mut bar = io.get_output().create_progress_bar(2)?;
        let label: ProgressFormatter = Arc::new(|bar| {
            let current = bar.get_progress();
            bar.get_output().writeln(format!("step:{current}").as_str(), OutputFormat::Raw)?;
            Ok(format!("stage:{current}"))
        });
        bar.set_format("%stage%")
            .set_placeholder_formatter("stage", label);
        bar.start()?;
        bar.advance_by(2)?;
        bar.finish()?;
        Ok(0)
    })?;
    Ok(())
}
```

The callback receives the bar; its output is borrowed only while used. This avoids overlapping mutable references to the bar and its output. Read the needed bar state before borrowing the output, then continue after that borrow ends. Writes share the existing formatter and error chain and do not update outer style newline counters. A placeholder may be evaluated repeatedly while formatting; keep business side effects outside it. This view does not claim the complete inherited output SDK: formatter replacement and output configuration setters remain separate entries.
