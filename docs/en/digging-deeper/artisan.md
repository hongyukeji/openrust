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
            io.output_components().info("Saved")?;
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

For direct rendering, build `Table::new(io.get_output())?`, set headers, rows and column styles, then call `render()`. `TableCell::display(value)` explicitly adapts a business type implementing `Display`; JSON values and ordinary scalars convert directly. Multi-line cells, visible Unicode width, formatter tags, ANSI output and quiet/silent filtering share the original output pipeline. This rectangular-table interface does not claim support for the entire inherited helper SDK.

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

The callback receives the bar; its output is borrowed only while used. This avoids overlapping mutable references to the bar and its output. Read the needed bar state before borrowing the output, then continue after that borrow ends. Writes share the existing formatter and error chain and do not update outer style newline counters. A placeholder may be evaluated repeatedly while formatting; keep business side effects outside it. This view does not claim the complete inherited output SDK: formatter replacement and output configuration are described below.

## Output configuration {#output-configuration}

`Output` and `OutputStyle` expose `set_verbosity` and `set_decorated`; the style also provides `get_verbosity`, `is_decorated` and `is_silent`. These change the actual backend. Already accepted messages stay visible when a later setting becomes Quiet or Silent. Runtime setters can explicitly override CLI levels; the example respects an initial Quiet/Silent request.

```rust
use openrust::{
    console::{ConsoleError, Input, Output, OutputOptions, OutputVerbosity},
    support::facades::Artisan,
};

fn register_output_configuration() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:output-config", |input: &Input, output: &mut Output| {
        let mut io = input.io(output);
        let style = io.get_output();
        let previous = style.get_verbosity();
        if previous <= OutputVerbosity::Quiet {
            return Ok(0);
        }
        style.set_verbosity(OutputVerbosity::Verbose);
        style.write("details;", false, OutputVerbosity::Verbose)?;
        style.set_verbosity(OutputVerbosity::Quiet);
        style.write("quiet;", false, OutputVerbosity::Quiet)?;
        style.set_verbosity(previous);
        let decorated = style.is_decorated();
        style.set_decorated(false);
        style.write("<info>plain</info>", false, OutputOptions::default())?;
        style.set_decorated(decorated);
        Ok(0)
    })?;
    Ok(())
}
```

A progress output view has its own `set_verbosity` and `set_decorated`. On a real console they configure stderr independently; a primary style setter updates both streams. Captured single-stream output shares the same settings. Progress formatting reads its selected stream, and nested explicit CLI flags temporarily apply to both. The existing command scope restores parent settings on completion and cancellation. Native direct string-buffer edits remain subject to command filtering; prefer the output methods for level-aware writing. See Output formatters for the sharing rules after explicit replacement.

## Styles and the style stack {#formatter-styles}

Low-level `OutputFormatterStyle` provides foreground, background, options and hyperlink configuration. It implements the public `OutputFormatterStyleInterface`; `FormatterStyle` is a shared trait-object handle. `apply` produces ANSI text independently of output decoration. Use Raw only when intentionally writing prepared control sequences.

```rust
use openrust::{
    console::{ConsoleError, FormatterStyle, Input, Output, OutputFormat,
        OutputFormatterStyle, OutputFormatterStyleStack},
    contracts::service::ResetInterface,
    support::facades::Artisan,
};
use std::sync::Arc;

fn register_formatter_styles() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:styles", |input: &Input, output: &mut Output| {
        let style = Arc::new(OutputFormatterStyle::new(Some("green"), None, &["bold"])?);
        let shared: FormatterStyle = style.clone();
        let mut stack = OutputFormatterStyleStack::default();
        stack.push(shared.clone());
        shared.set_foreground(Some("blue"))?;
        let rendered = stack.get_current().apply("ready")?;
        let mut io = input.io(output);
        io.get_output().write(rendered.as_str(), false, OutputFormat::Raw)?;
        let reset: &mut dyn ResetInterface = &mut stack;
        reset.reset();
        io.new_line(1)?;
        io.info(&format!("stack:{}", stack.get_current().apply("plain")?))?;
        Ok(0)
    })?;
    Ok(())
}
```

A cloned handle retains the same object; cloning the concrete style creates an independent configuration. `OutputFormatterStyleStack` retains references, supports ordinary and matching-style pops, and shares its styles when cloned. `reset` clears entries while keeping the configured empty style and is also available through `contracts::service::ResetInterface`. Matching uses actual empty-text style output, including hyperlink behavior.

Setters return typed errors and may preserve the previous compiled display while later operations validate the latest fields. Propagate failures with `?`; `ConsoleError` retains their sources. Generic iterator and string adapters run before acquiring the default style lock. These types reuse the existing formatter pipeline. Named-style registration and whole-formatter replacement are described in Output formatters.

## Output formatters {#output-formatters}

`OutputFormatter` provides named style registration and lookup, escaping, optional text formatting and grapheme-based wrapping. Width zero disables wrapping. Formatting retains unclosed style state between calls; `get_style_stack()` returns a handle to that same stack. Cloning the concrete formatter deep-copies named styles while retaining stack style references.

```rust
use openrust::{
    console::{ConsoleError, Input, Output, OutputFormat, OutputFormatter,
        OutputFormatterStyle},
    support::facades::Artisan,
};
use std::sync::Arc;

fn register_formatter() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:format", |input: &Input, output: &mut Output| {
        let formatter = Arc::new(OutputFormatter::new(false));
        formatter.set_style("report", Arc::new(OutputFormatterStyle::new(
            Some("green"), None, &["bold"],
        )?));
        let wrapped = formatter.format_and_wrap(Some("one two three"), 7)?;
        let mut io = input.io(output);
        let style = io.get_output();
        style.set_formatter(formatter);
        style.write("<report>ready</report>", true, OutputFormat::Normal)?;
        style.write(wrapped.as_str(), true, OutputFormat::Raw)?;
        style.get_formatter().set_decorated(true);
        style.write("<report>done</report>", true, OutputFormat::Normal)?;
        Ok(0)
    })?;
    Ok(())
}
```

`OutputFormatterInterface` supports custom implementations and optional output; returning `None` emits no text. Implement `WrappableOutputFormatterInterface` and return it from `as_wrappable` to expose wrapping through a type-erased handle. The native capability method avoids unsafe trait-object casts. Propagate errors with `?`; their concrete sources are retained.

`Output`, `OutputStyle` and `ProgressOutput` expose `get_formatter` and `set_formatter`. Cloning the returned Arc shares the same object. Explicit primary replacement on a real console shares the object with stderr; changing decoration through either handle then affects both. Without replacement their default formatters remain independent. Replacing only the progress output affects stderr; captured commands have a single shared stream. Raw output bypasses the formatter, including progress cursor control sequences.

Nested CLI flags restore decoration when their original output objects remain installed. Explicit replacement keeps the new formatter’s configuration instead of overwriting it with the previous object’s flags. Custom style callbacks can safely register styles, reset the same stack or perform finite nested formatting. Formatter calls are synchronous; no async work runs while holding the ordering lock.

## Colors and terminal capabilities {#colors-terminal}

`Color::new(foreground, background, options)` compiles ANSI sequences. `set` and `unset` expose the opening and closing sequences; `apply` combines them with text. `Color::default()` is empty. These low-level methods do not read output decoration: respect the output flag before writing pre-rendered text with Raw. Invalid colors and options return typed errors.

```rust
use openrust::{
    console::{Color, ConsoleError, Input, Output, OutputFormat, Terminal},
    support::facades::Artisan,
};

fn register_colors() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:colors", |input: &Input, output: &mut Output| {
        let color = Color::new("green", "", &["bold"])?;
        let terminal = Terminal::new();
        let mut io = input.io(output);
        let style = io.get_output();
        let text = if style.is_decorated() {
            color.apply("ready")
        } else {
            "ready".to_owned()
        };
        style.write(text.as_str(), true, OutputFormat::Raw)?;
        style.write(
            format!("terminal:{}x{}", terminal.get_width(), terminal.get_height()).as_str(),
            true, OutputFormat::Raw,
        )?;
        Ok(0)
    })?;
    Ok(())
}
```

`AnsiColorMode::{Ansi4, Ansi8, Ansi24}` provides `convert_from_hex_to_ansi_color_code`. `Terminal::get_color_mode()` detects COLORTERM before TERM and caches the first result. The default is `Terminal::DEFAULT_COLOR_MODE` (Ansi4). `Terminal::set_color_mode(Some(mode))` forces a process-wide mode; `None` resets detection. Configure it during startup rather than per request. Existing Color objects keep their compiled sequences; future objects and rebuilt styles use the current mode.

`Terminal::get_width` and `get_height` read COLUMNS and LINES on every call, including zero, negative and numeric-prefix values. Without overrides they cache native device dimensions, falling back to 80 columns and 50 rows. Unix reads the stdin terminal device directly, including when stdout and stderr are redirected; Windows uses ANSICON when configured and the native console otherwise. `has_stty_available()` separately caches the result of querying stty; ordinary dimension reads do not launch it. Windows execution remains part of platform release acceptance.

## Table widths and titles {#table-layout-controls}

Use `set_column_width` for one minimum or `set_column_widths` for a sequential list. The bulk setter replaces earlier minima. `set_column_widths_with_keys` accepts indexed pairs for sparse columns. `set_column_max_width` checks the active formatter’s wrapping capability immediately and returns an error if it is unavailable; the built-in formatter supports it. A maximum takes precedence over a minimum.

```rust
use openrust::{
    console::{ConsoleError, Input, Output, Table},
    support::facades::Artisan,
};

fn register_layout() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:layout", |input: &Input, output: &mut Output| {
        let mut io = input.io(output);
        let mut table = Table::new(io.get_output())?;
        table.set_headers(["Name", "Count"])
            .set_rows([["longer name", "42"]])
            .set_column_widths([12, 8])
            .set_header_title(Some("Report"))
            .set_footer_title(Some("done"));
        table.set_column_max_width(0, 8)?;
        table.get_style_mut().set_header_title_format("[%s]");
        table.render()?;
        Ok(0)
    })?;
    Ok(())
}
```

Titles use `set_header_title` and `set_footer_title`; pass `None` to clear them. The header treats empty text and "0" as absent, while an explicitly empty footer still uses its title format. `get_style_mut` and `get_column_style_mut` provide mutable access without removing the existing read-only getters. An unconfigured column falls back to the table’s actual style.

Widths remain signed: non-positive values follow their rendering rules rather than being silently replaced with defaults. A zero maximum does not wrap text; incompatible effective widths can fail during rendering after earlier output has already been written. Propagate `render` failures. Wrapping reuses the active `WrappableOutputFormatterInterface`, including custom implementations. Titles are centered and truncated using their visible width and configured formats. Named definitions and merged layouts are covered in [Named Table Styles](#table-style-definitions) and [Spanning Cells](#table-cell-spans).

Table uses returned wrapping text for line expansion only when it contains LF. Otherwise it keeps the original cell; existing raw newlines remain raw rather than being expanded into table rows. Custom wrapping is not a general cell-content transformation hook.

## Table rows and orientation {#table-rows-orientation}

`add_row` and `add_rows` return `Result`; use `?` to propagate exhausted numeric-index errors instead of panicking. Successful earlier additions remain. `set_row` replaces an integer or string key without changing insertion order, or appends a new key. Canonical numeric strings reuse integer keys. Negative keys influence the next numeric index, and `set_rows` resets both rows and indexing.

```rust
use openrust::{
    console::{ConsoleError, Input, Output, Table},
    support::facades::Artisan,
};

fn register_rows() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:rows", |input: &Input, output: &mut Output| {
        let mut io = input.io(output);
        let mut table = Table::new(io.get_output())?;
        table.set_headers(["Name", "Value"])
            .set_rows([["alpha", "1"]]);
        table.add_rows([["beta", "2"]])?;
        table.set_row(0_i64, ["changed", "1"])
            .set_vertical(true);
        table.render()?;
        Ok(0)
    })?;
    Ok(())
}
```

`set_headers_rows` accepts multiple header rows; the existing `set_headers` remains the single-row form. `set_horizontal(true)` transposes the first header row into leading header cells. `set_vertical(true)` formats records with labels from that same first header row and separators between records. Both setters with false restore the default orientation rather than a previous direction. Layout derives from stored rows on each render and preserves width/title settings. See [Named Table Styles](#table-style-definitions), [Row Separators](#table-row-separators) and [Spanning Cells](#table-cell-spans) for those layout options.

## Named Table Styles {#table-style-definitions}

Use `Table::set_style_definition` and `get_style_definition` inside the current application scope. Each application has its own built-in and custom definitions; they persist across commands and requests in that application. A table keeps its creating application's definitions even after the outer scope changes. A standalone table has private built-ins; changing named definitions without an application returns an error.

Pass `&style` to share one mutable style object; `style.clone()` explicitly copies its configuration. Modifying a shared definition also changes tables already using it. Replacing a definition by name affects subsequent lookups, while existing tables keep the original object. `TableStyle::from_name` remains an independent copy of a built-in preset.

`Table::new` returns `Result` because resolving application services can fail. Style text and array getters return owned snapshots, so shared mutation cannot invalidate a borrowed reference. Use `TableStyle::from(table.get_style())` or the column getter to retain a shared handle. Formatting callbacks run outside style locks.

```rust
use openrust::{
    console::{ConsoleError, Input, Output, Table, TableStyle},
    support::facades::Artisan,
};

fn register_table_styles() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:table-styles", |input: &Input, output: &mut Output| {
        let mut style = TableStyle::from_name("box")?;
        Table::set_style_definition("report", &style)?;
        let mut io = input.io(output);
        let mut table = Table::new(io.get_output())?;
        table.set_headers(["A", "B"]).set_rows([["one", "two"]]);
        table.set_style_named("report")?;
        style.set_horizontal_border_chars("!", None);
        table.render()?;
        Table::set_style_definition("report", TableStyle::from_name("compact")?)?;
        table.render()?;
        table.set_style_named("report")?.render()?;
        Ok(0)
    })?;
    Ok(())
}
```

`set_style_named` and `set_column_style_named` resolve names and return errors for undefined definitions. The typed setters also accept borrowed styles. The `table` convenience method uses the application's current default. Use `table_named` or `table_with_named_columns` for named table and column styles. Separator cells and spans are described in [Spanning Cells](#table-cell-spans).

## Explicit Row Separators {#table-row-separators}

Use `TableRow` for a collection containing both data rows and whole-row separators. `TableSeparator::new()` is a separator, while an empty data array is an empty row. Ordinary arrays and iterators still convert to data rows automatically.

```rust
use openrust::{
    console::{ConsoleError, Input, Output, TableRow, TableSeparator},
    support::facades::Artisan,
};

fn register_separators() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:separators", |input: &Input, output: &mut Output| {
        let rows = [
            TableRow::from(["one", "two"]),
            TableSeparator::new().into(),
            TableRow::from(["three"]),
        ];
        input.io(output).table_named(["A", "B"], rows, "box-double")?;
        Ok(0)
    })?;
    Ok(())
}
```

`set_rows`, `add_rows`, `add_row` and the table convenience methods accept these heterogeneous rows. `add_row` and `add_rows` continue to propagate index exhaustion with `?`; `set_row` replaces a separator with a data array at the same key. Leading, trailing and consecutive separators retain their declared order. Horizontal and vertical layouts omit explicit whole-row separators; vertical layout continues to generate its own record separators. Cell-level separator options and spans are described in [Spanning Cells](#table-cell-spans).

## Cell Styles and Inner Separators {#table-cell-styles}

Create `TableCellStyle` from option pairs, or use `Default` for `fg=default`, `bg=default`, no text options, left alignment and no custom format. Supported keys are `fg`, `bg`, `options`, `align` and `cellFormat`; the last key retains its configuration spelling. `align` accepts `left`, `center` or `right`. Unknown keys and invalid non-null alignment fail during construction.

`get_options` and `get_tag_options` return independent maps; null tag values are omitted. `get_pad_by_align` and `get_cell_format` return `Result`, preserving delayed failures for explicit null alignment and invalid format types. Scalar formats follow the existing string conversion. `DEFAULT_ALIGN` is `left`.

```rust
use openrust::{
    console::{ConsoleError, Input, Output, TableCell, TableCellStyle, TableSeparator},
    support::facades::Artisan,
};

fn register_cell_styles() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:cell-styles", |input: &Input, output: &mut Output| {
        let style = TableCellStyle::new([
            ("fg", serde_json::json!("green")),
            ("align", serde_json::json!("right")),
        ])?;
        let rows = vec![
            vec![TableCell::new("Ada").with_style(style), "42".into()],
            vec![TableSeparator::new().into(), TableSeparator::new().into()],
            vec!["Li".into(), "7".into()],
        ];
        input.io(output).table_named(["Name", "Score"], rows, "box-double")?;
        Ok(0)
    })?;
    Ok(())
}
```

Use `TableCell::with_style` to attach an immutable style and `get_style` to inspect it. Existing complete formatter tags keep their text styling, while the cell alignment still applies. Without such tags, `cellFormat` overrides the generated color tag. Width wrapping and multi-line expansion create plain line cells and can discard the original per-cell style; horizontal layout preserves moved cell styles, while vertical layout converts values to text.

A `TableSeparator` converted into a cell draws that column's inside border using its column style. It differs from a whole-row `TableRow::Separator`. For spans and separator constructor options, see [Spanning Cells](#table-cell-spans).

Each physical output line snapshots its row text format at the start. Formatting callbacks can change subsequent borders and the next line; a format change does not rewrite cells already being rendered.

## Spanning Cells and Constructor Options {#table-cell-spans}

Use `with_colspan` and `with_rowspan` for merged cells. `get_colspan` and `get_rowspan` return signed integers using the shared scalar conversion rules. Both default to `1`; constructor options retain supplied values, including zero and negative values, and the layout propagates calculation errors when those values cannot be used.

```rust
use openrust::{
    console::{ConsoleError, Input, Output, Table, TableCell},
    support::facades::Artisan,
};

fn register_spans() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:spans", |input: &Input, output: &mut Output| {
        let headers = vec![
            vec![TableCell::new("Summary").with_colspan(2)],
            vec!["Name".into(), "Score".into()],
        ];
        let rows = vec![
            vec![TableCell::new("Ada").with_rowspan(2), "42".into()],
            vec!["7".into()],
            vec![TableCell::new("Total").with_colspan(2)],
        ];
        let mut io = input.io(output);
        Table::new(io.get_output())?
            .set_headers_rows(headers)
            .set_rows(rows)
            .set_style_named("box-double")?
            .render()?;
        Ok(0)
    })?;
    Ok(())
}
```

`TableCellOptions` groups `colspan`, `rowspan` and `style`; pass it to `with_options` on a cell or separator. Span fields store JSON values, while `style` is `Option<TableCellStyle>`. Unknown fields and values pretending to be a style object fail type checking. `TableCell::default()` creates an empty cell; wrapping an existing cell with `TableCell::new` starts with default options again.

`TableSeparator` retains its options and inherited getters, and supports `Clone`. Use a clone when reusing one separator in multiple owned rows. Spans interact with multi-row headers, row filling, orientation and column wrapping. Layout can update column minima; `set_column_widths([])` clears those minima and retains maximum-width constraints. Section redraw is described below.


## Console sections and table updates {#console-sections}

`Output::section()` creates a section on the current output stream. `write`/`writeln`, `clear`, `overwrite`, `set_max_height`, `get_content` and `get_visible_content` operate on that section. Rewriting an earlier section erases and restores later sections in order; maximum height scrolls visible content. Without decoration, writes remain ordinary output and clearing does not redraw.

`ConsoleSectionOutput` temporarily borrows the original output. Keep its `handle()` and use `section_output(&handle)` to borrow it again after writing another section. Handles are bound to one output and are rejected by another. `style(input)` binds the existing style/table pipeline to the section. This explicit borrowing replaces shared mutable stream references; it creates no additional output stream.

```rust
use openrust::{
    console::{ConsoleError, Input, Output, OutputOptions, Table},
    support::facades::Artisan,
};

fn register_sections() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:sections", |_input: &Input, output: &mut Output| {
        let first = {
            let mut section = output.section();
            section.writeln("prefix", OutputOptions::default())?;
            section.handle()
        };
        {
            let mut section = output.section();
            section.writeln("other section", OutputOptions::default())?;
        }
        let mut section = output.section_output(&first)?;
        let mut table = Table::new(&mut section)?;
        table.set_headers(["A", "B"]).set_rows([["x", "y"]]);
        table.append_row(["one", "two"])?;
        table.append_row(["longer", "last"])?;
        Ok(0)
    })?;
    Ok(())
}
```

`append_row` requires a section, clears the calculated table region after the first render, adds the row and renders it again while preserving later sections. On ordinary output it returns an error before adding the row; use `add_row` to build a table without redrawing. Section verbosity is independent; the formatter is shared initially and can be replaced for that section. Selected progress and question output are described below.


## Selected output, progress and questions {#section-consumers}

`OutputStyle::get_output()` returns an `OutputReference` carrying the currently selected stream and section identity. `OutputStyle::new`, `ProgressBar::new` and `new_with_redraw_interval` accept it as well as the original `&mut Output`; ordinary calls keep the same syntax. Helpers with explicit `&mut Output` parameter annotations should accept `&mut impl AsOutput` when they need to support sections. The reference allows reading the original capture buffer, and forwards configuration and writes to the selected output; it cannot be mutably dereferenced to bypass the section.

Progress clear/redraw uses section line accounting and restores later sections. `bar.get_output()` provides the same selected stream for callback writes and configuration; `as_section()` exposes section operations or returns an error on ordinary output. This sequential borrow is the native alternative to capturing both a mutable stream and its progress bar in one closure.

```rust
use openrust::{
    console::{ConsoleError, Input, Output, OutputOptions, OutputStyle, ProgressBar},
    support::facades::Artisan,
};

fn register_section_progress() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:section-progress", |input: &Input, output: &mut Output| {
        let first = {
            let mut section = output.section();
            section.writeln("prefix", OutputOptions::default())?;
            section.handle()
        };
        {
            let mut section = output.section();
            section.writeln("later", OutputOptions::default())?;
        }
        let mut section = output.section_output(&first)?;
        let mut style = OutputStyle::new(input, &mut section);
        let mut bar = ProgressBar::new(style.get_output(), 3)?;
        bar.set_format("<info>%current%/%max%</info>")
            .min_seconds_between_redraws(0.0)
            .max_seconds_between_redraws(0.0)
            .set_redraw_frequency(Some(1));
        bar.start()?;
        bar.advance()?;
        bar.display()?;
        bar.get_output().writeln("notice", OutputOptions::default())?;
        bar.advance()?;
        bar.clear()?;
        bar.display()?;
        bar.finish()?;
        Ok(0)
    })?;
    Ok(())
}
```

Questions use the selected formatter and account for the submitted input line before the following blank line. Validation retries prepend the block once rather than repeating it on every attempt. Normal terminal editing echoes input directly without storing the echo a second time in section content. Stream-input questions, confirmation, hidden input, retries, cancellation and a normal interactive PTY have been verified. Completion and multiline behavior are described below.


## Section completion and multiline input {#section-editors}

Completion starts without a selected suggestion. Up/Down select the last/first initial candidate; typing filters candidates, Tab accepts the current candidate, and Enter accepts a selected candidate or the entered/default value. When a suggestion is visible, Backspace first removes the suggestion; another Backspace removes the entered character. Choice and multiple-choice questions use the same editor, including comma-separated matches.

```rust
use openrust::{
    console::{AsyncCommandCallback, ConsoleError, OutputOptions, Question},
    support::facades::Artisan,
};

fn register_editor() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:editor", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let mut section = output.section();
            let mut style = section.style(input);
            let mut question = Question::new("Name", Some("Guest".into()))?;
            question.set_autocompleter_values(Some(vec![
                "Ada".into(), "Adam".into(), "Beta".into(),
            ]))?;
            let answer = style.ask_question(&question).await?;
            style.writeln(
                format!("answer={}", answer.as_str().unwrap_or_default()).as_str(),
                OutputOptions::default(),
            )?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

`Question::set_multiline(true)` reads until the native EOF key, preserving embedded line breaks. An initial empty Enter uses the default. If completion and multiline are both enabled, completion takes precedence and Enter finishes that answer. Plain multiline echo uses the native asynchronous editor rather than the operating system canonical echo; its terminal redraw bytes differ, while the answer and section bookkeeping match.

Ctrl+C keeps exit status `130` and restores the terminal mode before command handling finishes. Cancellation and timeout keep the existing question error/default rules. Windows terminal acceptance remains part of the release checks.

## Nested output styles {#nested-output-styles}

`OutputStyle::new(input, &mut parent)` wraps the existing style. It forwards writes and settings to that same output, and preserves the parent’s newline and question-buffer state. `console::contracts::NewLineAware` exposes the real newline contract to helpers.

```rust
use openrust::console::{
    AsyncCommandCallback, ConsoleError, OutputOptions, OutputStyle,
    contracts::NewLineAware,
};
use openrust::support::facades::Artisan;

fn register_nested() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:nested", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let mut first = OutputStyle::new(input, output);
            first.new_line(1)?;
            {
                let mut second = OutputStyle::new(input, &mut first);
                second.write("A", false, OutputOptions::default())?;
                second.new_line(2)?;
                let inner = second.get_output().as_new_line_aware().unwrap();
                assert_eq!(NewLineAware::new_lines_written(&inner), 2);
                second.writeln("B", OutputOptions::default())?;
            }
            let count = NewLineAware::new_lines_written(&first);
            first.writeln(format!("count={count}").as_str(), OutputOptions::default())?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

`new_lines_written()` delegates to the deepest wrapped style; `new_line_written()` also preserves a true value on any wrapping layer. `new_line(n)` writes newline text to the parent, rather than calling its `new_line(n)`. These operations therefore have different counting behavior.

`get_output()` borrows the immediate selected output. On an `OutputReference`, `as_new_line_aware()` is `Some` only when that target is a wrapped style; `get_output()` can then borrow its underlying output. A plain stream has neither capability. Rust’s exclusive borrow prevents concurrent writes through a parent while a child is using it.

A wrapped style is not a direct console section: table append/redraw rejects it, and progress uses its ordinary cursor output without redirecting it to stderr. Use the section directly when section-specific behavior is required. General custom output streams and the remaining component helpers are separate capabilities.

## Headings, text and lists {#style-text-layout}

`OutputStyle` provides `title`, `section`, `text` and `listing`. Titles use `=` underlines and sections use `-`, measured with the selected formatter and Unicode display width. Text has a single-space indent; lists use ` * `. Existing output determines the blank lines between blocks.

```rust
use openrust::console::{AsyncCommandCallback, ConsoleError, OutputStyle};
use openrust::support::facades::Artisan;

fn register_text_layout() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:text-layout", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let mut style = OutputStyle::new(input, output);
            style.title("Hello")?;
            style.section("Part")?;
            style.text(&["one", "two"])?;
            style.listing(["A", "B"])?;
            style.title("")?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

`text` accepts a string or a borrowed string array. `listing` accepts a string iterator. Empty arrays retain each helper’s spacing behavior. Inline formatter tags remain active; title underlines ignore decoration, and trailing backslashes remain visible. Invalid formatting returns the existing typed console error and preserves output written before the failure.

`OutputStyle::section("Part")` writes a heading. `Output::section()` creates a redrawable output region. These are separate APIs. All four helpers support selected sections and borrowed nested styles; they preserve the same formatter, verbosity and newline state.

## Styled message blocks {#styled-message-blocks}

Use `comment`, `success`, `error`, `warning`, `note`, `info` and `caution` for standard message blocks. `block` exposes their shared renderer with explicit `BlockOptions`.

```rust
use openrust::console::{AsyncCommandCallback, BlockOptions, ConsoleError, OutputStyle};
use openrust::support::facades::Artisan;

fn register_blocks() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:blocks", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let mut style = OutputStyle::new(input, output);
            style.comment("Preparing report")?;
            style.success("Report ready")?;
            style.warning("Review the queue")?;
            style.block(&["Build finished", "Ready to deploy"], BlockOptions {
                kind: Some("BUILD"),
                style: Some("fg=blue"),
                padding: true,
                ..BlockOptions::default()
            })?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

`BlockOptions::default()` has no `kind` or `style`, a single-space `prefix`, `padding: false` and `escape: true`. `kind` is the optional bracketed label. Borrowed options represent optional arguments without copying application strings. All helpers accept the same borrowed string or string-array messages as `text`.

Standard status blocks escape message tags; `comment` keeps inline formatting active. Set `escape: false` only when the message should use formatter markup. `padding` adds top and bottom rows only to decorated output. Blocks wrap at the width captured when the style is constructed, capped at 120 columns; a configured zero width uses 120 before the one-column Windows margin. Changing `COLUMNS` later does not change that style’s block width.

`error` prints an error block; the command’s return value still controls its exit status. Invalid formatting or a zero-width section can return a typed console error after partial output. Question validation retries use this same error-block renderer, including its formatter, escaping and captured width.

## Style-level question helpers {#style-question-helpers}

The style also exposes the question helpers directly. Use the short method for defaults and the `_with` variants for optional arguments. They share `Question`, `ChoiceQuestion`, `ConfirmationQuestion`, input streams and cancellation with `ask_question`.

```rust
use openrust::console::{
    AsyncCommandCallback, ConsoleError, OutputFormat, OutputStyle, QuestionCallback,
};
use openrust::support::facades::Artisan;
use serde_json::json;

fn register_style_helpers() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:style-questions", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let mut style = OutputStyle::new(input, output);
            let profile = style.ask_with_validator("Name", Some("Ada"), Some(
                QuestionCallback::sync(|value| Ok(json!({"name": value}))),
            )).await?;
            let _token = style.ask_hidden("Token").await?;
            let confirmed = style.confirm("Continue").await?;
            let color = style.choice_with(
                "Color", ["red", "green"], Some(json!("green")), false,
            ).await?;
            style.writeln(
                json!({"profile": profile, "confirmed": confirmed, "color": color})
                    .to_string().as_str(),
                OutputFormat::Raw,
            )?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

`ask` and `ask_with` return `Option<String>`. `ask_with_validator` returns the validator’s complete JSON result. `ask_hidden` returns hidden text; `ask_hidden_with` also retains a structured validator result. The example uses the token without printing it.

`OutputStyle::confirm` defaults to `true`; the command-level `CommandIo::confirm` defaults to `false`. Both have `confirm_with` for an explicit default. Style `choice_with` resolves a matching default value to its choice key; `choice_pairs_with` preserves named keys. The final boolean enables multiselect.

A non-null noninteractive default runs the validator. A null default returns null without calling it. Validation errors follow the existing retry rules; `ConsoleError::from_runtime_source` explicitly marks an error that must stop immediately. Create a `Question` and call `ask_question` when configuring attempt limits, completion or timeout.

## Table output targets and the style factory {#table-output-factory}

`Table::new` accepts the existing `AsOutput` adapters: `Output`, `ConsoleSectionOutput`, `OutputReference`, `OutputStyle` and boxed adapters. A plain output can construct a table without an input object. The table now has a single output-borrow lifetime: explicit `Table<'a, 'o>` annotations become `Table<'a>`; ordinary constructor calls keep their syntax.

`OutputStyle::create_table` selects the underlying output, clones the named style-guide definition and uses the `info` header format. A real console gets a dedicated section; capture output stays a capture stream. It does not add a trailing blank line.

```rust
use openrust::console::{AsyncCommandCallback, ConsoleError, OutputStyle};
use openrust::support::facades::Artisan;

fn register_factory_table() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:factory-table", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
        let mut style = OutputStyle::new(input, output);
        let mut table = style.create_table()?;
        table.set_headers(["H", "V"]).set_rows([["x", "y"]]);
        table.render()?;
        Ok(0)
        })
    }))?;
    Ok(())
}
```

The actual constructor target determines append support. Use `Table::new(&mut section)` or `Table::new(style.get_output())` for a directly selected section. Passing `&mut style` wraps that style, so `append_row` rejects it even if the style ultimately writes to a section. Render and configuration still pass through the same formatter and verbosity rules.

Factory style changes are local to the returned table. They do not mutate the application’s named style-guide definition. The constructor adapters above do not claim support for the remaining general custom-output interface.

## Style tables and definition lists {#style-table-helpers}

`OutputStyle::table` and `horizontal_table` use the existing table factory and add one blank line only after successful rendering. Neither method inserts a leading block separator. Rows accept `TableRow`, including explicit `TableSeparator` values. Flat header arrays work directly; `TableHeaders::rows` distinguishes multiple header rows from cell values.

`definition_list` accepts an iterator of `DefinitionListItem`: a title spans two columns, `entry` provides one key/value pair, `entries` preserves ordered array input and renders only its first pair, and `TableSeparator::new().into()` adds a separator. An empty `Entries` value retains an empty item. This enum expresses the source string/array/separator union; unsupported types are rejected during compilation.

```rust
use openrust::console::{AsyncCommandCallback, ConsoleError, DefinitionListItem, OutputStyle, TableCell, TableHeaders};
use openrust::support::facades::Artisan;

fn register_style_tables() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:style-tables", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let rows = vec![
                vec![TableCell::from("Ada"), TableCell::from(42)],
                vec![TableCell::from("Lin"), serde_json::Value::Null.into()],
            ];
            let mut style = OutputStyle::new(input, output);
            style.table(["Name", "Value"], rows)?;
            style.horizontal_table(TableHeaders::rows([["A", "B"], ["C", "D"]]), [["x", "y"]])?;
            style.definition_list([
                DefinitionListItem::from("Profile"),
                DefinitionListItem::entry("Name", "Ada"),
                DefinitionListItem::entry("Count", 42),
            ])?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

Use `serde_json::Value::Null.into()` for a nullable scalar cell. A null value and an empty string display alike in an ordinary cell, but horizontal tables must distinguish them: null under a spanning title is omitted, while an empty string remains a cell. Explicit styled/spanning cells remain cell objects. Definition-list titles use this same general table behavior, rather than a separate renderer. Formatting errors return the existing `ConsoleError` with its original source and preserve partial output; no success blank line is added after an error.

## The style interface {#style-interface}

`openrust::console::style::StyleInterface` is a public trait; `openrust::console::StyleInterface` re-exports the same type. It exposes titles, sections, lists, text, success/error/warning/note/caution messages, tables, four question helpers, newlines and progress start/advance/finish. `OutputStyle` implements all 18 entries, so services can accept a generic style or `&mut dyn StyleInterface`.

```rust
use openrust::console::{AsyncCommandCallback, ConsoleError, OutputStyle};
use openrust::console::style::StyleInterface;
use openrust::support::facades::Artisan;

fn render_report<S: StyleInterface + ?Sized>(style: &mut S) -> Result<(), ConsoleError> {
    style.title("Report")?;
    style.table(["Name", "Value"].into(), vec![["Ada", "42"].into()])?;
    style.success("Ready".into())
}

fn register_style_interface() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:interface", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let mut style = OutputStyle::new(input, output);
            render_report(&mut style)?;
            let dynamic: &mut dyn StyleInterface = &mut style;
            render_report(dynamic)?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

Trait calls use explicit parameters instead of optional PHP arguments: `ask` takes an optional default and validator, `ask_hidden` an optional validator, `confirm` a boolean default, and `choice` an ordered `Vec<(ChoiceKey, Value)>` with an optional default. Integer keys express sequential choices; string keys preserve associative results. Questions retain the complete `serde_json::Value` result. `table` takes `TableHeaders` and `Vec<TableRow>`, messages use the existing `OutputMessages`, and `new_line`/`progress_advance` take explicit counts. The existing concrete-type convenience methods remain available.

The four asynchronous trait methods return the existing `ConsoleFuture`. This adds one future box at the trait boundary to support dynamic dispatch; directly calling the concrete `OutputStyle` async methods does not add this layer. Synchronous trait methods do not box futures. The trait requires `Send` for the existing asynchronous command execution, and does not require `NewLineAware`. Consecutive questions share the same input and output history, including hidden input and failure recovery.

## Error styles and the base style type {#error-style}

`OutputStyle::get_error_style` returns the base `ConsoleStyle`, also available at `openrust::console::ConsoleStyle`. The base provides the same existing rendering helpers and implements `StyleInterface`, but does not implement `NewLineAware` or expose the derived newline counters. You can also construct it with `ConsoleStyle::new(input, output)`.

A directly wrapped real console selects stderr. Capture streams, sections and wrapped styles keep their selected output; the method does not force all output into a separate error buffer. Formatter, decoration and verbosity settings follow that selected target. Changing a direct console error style’s settings does not change stdout settings.

```rust
use openrust::console::{AsyncCommandCallback, ConsoleError, OutputStyle};
use openrust::console::style::ConsoleStyle;
use openrust::support::facades::Artisan;

fn register_error_style() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:error-style", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let mut original = OutputStyle::new(input, output);
            {
                let mut errors: ConsoleStyle<'_> = original.get_error_style();
                errors.warning("Review")?;
                errors.table(["Name", "Value"], [["Ada", "42"]])?;
            }
            original.info("Done")?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

The returned style borrows its output exclusively. End its scope before writing through the original style; it shares the real target rather than copying buffered output. Newline-counter lookup stops at a base-style boundary, while actual writes still reach the underlying target. Nested error-style creation returns another base style.

`ConsoleStyle::write_iter` and `writeln_iter` write each item before requesting the next one. The derived `OutputStyle` retains its existing preparation for newline statistics. Both types use the same formatter and rendering engine; this does not add general third-party `OutputInterface` support. Tree helpers and the remaining component APIs are tracked separately.

## Tree nodes and lazy children {#tree-nodes}

`TreeNode` stores a read-only label and shared node identity. Use `new(value)` for an empty node, `with_children(value, children)` to add initial children, or `default()` for an empty label. `add_child` accepts a node, a string or `TreeChild::lazy`; cloning a node shares its identity rather than copying the tree.

```rust
use openrust::console::{AsyncCommandCallback, ConsoleError, OutputOptions, OutputStyle, TreeChild, TreeNode, TreeValue};
use openrust::support::facades::Artisan;

fn register_tree_nodes() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:tree-nodes", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let root = TreeNode::from_values(
                [("languages", TreeValue::branch([(0_i64, "Rust"), (1_i64, "Go")]))],
                Some(TreeNode::new("Project")),
            )?;
            root.add_child("Ready");
            root.add_child(TreeChild::lazy(|| [Ok(TreeNode::new("Generated"))]));
            OutputStyle::new(input, output).writeln(
                format!("{}:{}", root.get_value(), root.count()?).as_str(),
                OutputOptions::default(),
            )?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

`from_values` accepts ordered key/value pairs. `TreeValue::branch` represents nested values; nested keys become labels, while scalar keys are ignored. A `TreeValue::Node` is iterable and expands its children under the supplied key, discarding that node's label. Use `add_child(node)` when you want to retain the node itself. Passing `Some(root)` appends to that same root; `None` creates an empty root.

`get_children`, `get_iterator` and iteration over `&TreeNode` return `Result<TreeNode, ConsoleError>` items. They snapshot children on the first advance. Changes made before that advance are included; later additions are seen by a new traversal. Factories run outside the node lock and are invoked again by every traversal, including `count()`. Factories return owned `Send` iterators and may propagate errors. `lazy_keyed` preserves `ArrayKey` keys, available through the iterator's `key()`; `lazy` numbers each factory's items from zero. Direct children have their own sequential keys.

Node ownership uses standard `Arc`, without a cycle collector. Use `downgrade`/`WeakTreeNode::upgrade` for callback backreferences. If you explicitly create a strong-reference cycle, call `clear_children` to break it when finished; `ptr_eq` compares node identity. Expanding a cyclic node through `from_values` returns an error instead of recursing indefinitely. The ordinary root and its children are released when their owners and iterators are dropped.

Fallible input producers use `try_from_values` with `Result<(key, value), ConsoleError>` items; an error leaves already appended siblings intact.

Nested one-shot producers use `TreeValue::iterable` or `try_iterable`. They own a `Send + 'static` iterator; the latter accepts fallible items. They are consumed in order, without collecting first, and the nested branch is attached only after it has finished successfully. `TreeValue` does not clone or replay a one-shot producer.

## Trees and tree cursors {#trees}

`OutputStyle` and the base `ConsoleStyle` provide `tree(values, root)` and `create_tree(values, root)`. They use the same `TreeHelper` as the direct constructor. Pass an empty string for the default empty root; even an empty tree writes its root line. Tree output does not add a blank line after rendering.

```rust
use openrust::console::{AsyncCommandCallback, ConsoleError, OutputStyle, TreeHelper, TreeNode, TreeStyle, TreeValue, TreeValues};
use openrust::support::facades::Artisan;

fn register_tree_output() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:trees", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let mut style = OutputStyle::new(input, output);
            style.tree([(0_i64, "A"), (1_i64, "B")], "Project")?;
            {
                let mut tree = style.create_tree(
                    [("Group", TreeValue::branch([(0_i64, "Leaf")]))], "Root",
                )?;
                tree.render()?;
            }
            let root = TreeNode::with_children("Styled", ["Leaf"]);
            TreeHelper::create_tree(
                &mut style, root, TreeValues::new(), Some(TreeStyle::rounded()),
            )?.render()?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

`TreeHelper::create_tree(output, root, values, style)` accepts a root label, an existing `TreeNode` or `None`. Pass an empty `TreeValues` to keep only existing children, and `None` for the default style. Values append to the same root node. A style factory creates a section only when directly wrapping a real console; captures, existing sections and wrapped styles retain their selected output. Error styles retain their stdout/stderr selection.

The helper exposes `current`, `key`, `advance`, `rewind`, `valid`, `has_children` and `get_children`. Construction primes the first child. `advance()` advances without returning a node; standard iterators keep their element-returning `next()`; `current()` returns the same shared node identity, and `key()` returns an integer. Invalid current/key access returns `ConsoleTreeError`. Numeric string keys follow the existing weak scalar rules; nonnumeric or overflowing keys fail. Use `for item in &mut helper` for direct children, or `recursive_iterator()` for depth-first `TreeEntry { node, line }` items. The latter omits the root and does not write output. `TreeStyle::apply_prefixes` configures that iterator; `render()` performs duplicate-node detection.

The cursor is single-use. Rewinding is allowed before it advances past the first yield, and empty cursors remain rewindable. Rewinding an advanced nonempty cursor fails, including a second `render()`; rendering writes the root before that rewind error. Create a fresh helper for another traversal. `get_children()` borrows the selected output: end the child helper's scope before using its parent again. The last yielded node remains owned until the cursor is dropped, even when `valid()` is false.

Factories may run during construction, child probing and child cursor creation, so do not assume one call per node. Child-probe failures are treated as no children by the underlying traversal; failures while advancing the iterator propagate and can leave partial output. During rendering, revisiting the same node identity raises `ConsoleTreeError::Cycle`, including reuse in a DAG. Distinct nodes with equal labels remain valid. Visited nodes stay owned throughout rendering so temporary-node allocation cannot cause false duplicate detection.

`TreeStyle` offers `default`, `r#box`, `box_double`, `compact`, `light`, `minimal` and `rounded`. `r#box` uses Rust's raw identifier syntax because `box` is reserved. `TreeStyle::new` takes six immutable prefixes in this order: end-with-next, end-last, left, middle-with-next, middle-last, right. Presets use static strings; custom prefix definitions are shared when child helpers are created.

## Component text mutators {#component-mutators}

The four public mutators share the object-safe `Mutator::apply(&str) -> Result<String, ConsoleError>` interface. Explicit `apply` replaces callable objects; implementing Rust’s callable traits directly would require unstable language features. Text input is a string; this API does not reproduce PHP weak conversion for arbitrary objects or null.

```rust
use openrust::console::{ConsoleError, view::components::mutators::{EnsureDynamicContentIsHighlighted, EnsureNoPunctuation, EnsurePunctuation, EnsureRelativePaths, Mutator}};

fn transform_component_text(value: &str) -> Result<String, ConsoleError> {
    let value = EnsureDynamicContentIsHighlighted.apply(value)?;
    let value = EnsurePunctuation.apply(&value)?;
    let value = EnsureRelativePaths.apply(&value)?;
    EnsureNoPunctuation.apply(&value)
}
```

Highlighting wraps nonempty `[content]` in bold markup. Punctuation recognizes only final ASCII `.`, `?`, `!` and `:`; it does not trim whitespace or normalize Unicode punctuation. `EnsureNoPunctuation` removes exactly one final character. Relative paths remove every exact `base_path + "/"` occurrence inside the current application scope, without canonicalizing the path. Outside an application or without its `path.base` binding, text is unchanged. This is display formatting, not path validation. These types are available independently.

## Line components {#line-components}

`Info`, `Success`, `Warn` and `Error` add a badge, highlight bracketed dynamic content and ensure sentence punctuation. Each type accepts the selected output through `new(output)` and provides `render(text)` at normal verbosity or `render_with(text, verbosity)` for an explicit threshold. `render_with` also accepts `OutputOptions` to preserve raw or plain output modes alongside the verbosity threshold.

```rust
use openrust::console::{AsyncCommandCallback, ConsoleError, OutputStyle, OutputVerbosity,
    view::components::{Error as ErrorComponent, Info, Line, Success, Warn}};
use openrust::support::facades::Artisan;

fn register_line_components() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:components", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let mut style = OutputStyle::new(input, output);
            Info::new(&mut style).render("Creating [reports]")?;
            Success::new(&mut style).render("Ready")?;
            Warn::new(&mut style).render("Check [configuration]")?;
            ErrorComponent::new(&mut style).render("Missing [field]")?;
            Line::new(&mut style).render("info", "Custom [line]")?;
            Line::new(&mut style).render_with("warn", "Verbose detail", OutputVerbosity::Verbose)?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

`Line` accepts the `info`, `success`, `warn` and `error` style keys; an unknown key returns `ConsoleError` without writing a line. Components borrow the same selected output, formatter and newline state. An `Error` component renders an ERROR badge on that output; use `get_error_style()` to select stderr when the underlying console exposes it.

Component text collapses ASCII spaces, tabs and line endings. NUL becomes a space and other C0 controls are removed, including raw ANSI escape characters; nonbreaking spaces are retained. Use formatter tags for styled content. Long text is not automatically wrapped. Existing newline-aware styles determine the leading margin; base styles and raw captures use a one-line margin.

`OutputFormat::Normal` retains the existing default value 0. `ExplicitNormal` represents the explicit normal flag 1, preserving newline counting when combined with verbosity. Exhaustive matches over `OutputFormat` need this additional variant.

## Alerts, lists and two-column details {#display-components}

These component types borrow the selected output and keep its formatter and stdout/stderr selection. `render` uses normal verbosity; `render_with` accepts `OutputOptions` or a verbosity threshold. `COLUMNS` takes precedence when determining terminal width.

```rust
use openrust::console::{AsyncCommandCallback, ConsoleError, OutputStyle, OutputVerbosity,
    view::components::{Alert, BulletList, TwoColumnDetail}};
use openrust::support::facades::Artisan;

fn register_display_components() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:display", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let mut style = OutputStyle::new(input, output);
            Alert::new(&mut style).render("Review [reports]")?;
            BulletList::new(&mut style).render(["CSV.", "JSON.", "HTML."])?;
            TwoColumnDetail::new(&mut style).render("Report [daily].", Some("Ready"))?;
            TwoColumnDetail::new(&mut style).render("Optional value.", None)?;
            Alert::new(&mut style).render_with("Verbose alert", OutputVerbosity::Verbose)?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

`Alert` centers uppercase text on a full-width background. Text longer than the available width follows styled-text truncation rules; an impossible width returns `ConsoleError`. `BulletList` highlights bracketed values, removes one final ASCII punctuation character and resolves relative paths for each item. Input items are collected once, then each mutation stage runs over all items before any output is written. An empty list still writes one newline.

`TwoColumnDetail` removes final punctuation from the first cell and retains it in the second. `None` and `Some("")` omit the second cell; other input keeps its cell position after text normalization. Dot padding uses a layout width capped at 150 columns; long cell content remains intact. Formatter changes through the original shared handle remain visible after component construction.

## Task result values {#task-results}

`console::view::TaskResult` defines `Success = 1`, `Failure = 2` and `Skipped = 3`. `value()` returns the integer backing; `cases()` retains declaration order. `TryFrom<i64>` rejects other values instead of treating them as success.

```rust
use openrust::console::{AsyncCommandCallback, ConsoleError, OutputOptions, OutputStyle,
    view::TaskResult};
use openrust::support::facades::Artisan;

fn register_task_results() -> Result<(), ConsoleError> {
    Artisan::command_with("tasks:states", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let mut style = OutputStyle::new(input, output);
            for state in TaskResult::cases() {
                style.writeln(format!("{state:?}:{}", state.value()).as_str(), OutputOptions::default())?;
            }
            let failure = TaskResult::try_from(2)?;
            assert_eq!(failure, TaskResult::Failure);
            Ok(0)
        })
    }))?;
    Ok(())
}
```

`TaskResult` serializes as its integer backing, not its Rust case name. JSON deserialization requires an integer and a known case. Request `enum::<TaskResult>` reads the same type through `BackedEnum` and retains the existing request scalar conversion and default behavior. For example, a query value of `2.0` resolves to `Failure`, while an unrecognized integer uses the request default. Keep JSON deserialization and request input conversion distinct.

## Tasks {#task-components}

`Task` borrows the selected output. `render` displays a completed task without a callback or duration. `render_task` runs a synchronous callback once and supplies a safe mutable borrow of that same output. The `_with` variants accept output options; suppressing the display does not skip the callback.

```rust
use openrust::console::{AsyncCommandCallback, ConsoleError, OutputStyle,
    view::{TaskResult, components::Task}};
use openrust::support::facades::Artisan;

fn register_task_components() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:tasks", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let mut style = OutputStyle::new(input, output);
            let mut task = Task::new(&mut style);
            task.render("Ready")?;
            task.render_task("Build [report].", |selected| {
                selected.write("CSV ", false, openrust::console::OutputOptions::default())?;
                Ok(TaskResult::Success)
            })?;
            task.render_task("Optional [CSV].", |_| Ok(TaskResult::Skipped))?;
            task.render_task("Boolean [false].", |_| Ok(false))?;
            task.render_task("Failed status.", |_| Ok(TaskResult::Failure))?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

`TaskResult` converts to its integer backing for task callbacks. Only integer `2` displays FAIL and integer `3` displays SKIPPED; `false`, null (`()`), floats and strings display DONE. Callback return values use explicit `Into<serde_json::Value>` conversion, without implicit serialization of arbitrary objects. A displayed FAIL alone does not change the command exit status. Returning `Err(ConsoleError)` writes FAIL before propagating the error; Rust unwinding also finishes the display before resuming the panic.

Duration uses wall time. Up to and including one second it uses grouped milliseconds with two decimals and half-up rounding; longer durations use the reference default short units (`yr`, `mo`, `w`, `d`, `h`, `m`, `s`) and discard fractional seconds. A month is four weeks and a year twelve such months for this display. `run_time_for_humans(start, Some(end))` accepts explicit epoch seconds for formatting. `set_run_time_formatter(Some(Box::new(...)))` replaces the duration method through composition; `None` restores the default. The callback accepts the start and optional end and returns `Result<String, ConsoleError>`, allowing application-specific duration wording. This display API does not provide a general date interval or locale factory.

## Question helpers and temporary scopes {#question-helpers}

`QuestionHelper` renders component prompts: two leading spaces, bold text, final punctuation and the `❯` cursor. `DefaultQuestionHelper` renders the regular style prompt. Both expose `ask(input, output, question)` and `get_name()`. Direct helper calls use exactly the supplied input and selected output; raw console output selects stderr, while an existing style keeps its stream.

```rust
use openrust::console::{AsyncCommandCallback, ConsoleError, OutputOptions, OutputStyle,
    QuestionHelper};
use openrust::support::facades::Artisan;
use std::sync::Arc;

fn register_question_helpers() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:question-helpers", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let mut style = OutputStyle::new(input, output);
            let first = {
                let mut scoped = style.using_question_helper(Arc::new(QuestionHelper::new()));
                scoped.ask_with("Name", Some("Guest")).await?
            };
            let second = style.ask_with("Next", Some("Guest")).await?;
            style.writeln(format!("Users:{}/{}", first.unwrap_or_default(), second.unwrap_or_default()).as_str(), OutputOptions::default())?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

`using_question_helper` borrows the same style and restores the original helper instance when its scope ends, including errors, unwinding and cancellation. If no helper was initialized, the regular default helper is restored. `OutputStyle` and `ConsoleStyle` both support these scopes. `QuestionStyle` provides the same operations through a trait, and `QuestionHelperScope::new` accepts a dynamic style.

Implement `QuestionHelperBehavior` to customize punctuation, prompts or retry errors, then install it with `set_question_helper(Some(Arc::new(...)))` or a temporary scope. `write_prompt` writes the prompt and returns its formatted final cursor text for native line editing. `write_error` receives whether the original output is a style. `Question::get_kind()` provides a borrowed `QuestionKind` view for text, confirmation and choice questions, including original choice keys and values. A helper-set registry is not exposed by these helpers.

The existing native input engine preserves complete UTF-8 lines and uses native terminal APIs instead of shell commands. `disable_stty()` retains the no-external-stty guarantee; it does not disable native completion or hidden input. This differs from a shell-backed terminal implementation. Prompt hints continue to use the application translator.

## Question components {#question-components}

`Ask`, `AskWithCompletion`, `Choice`, `Confirm` and `Secret` borrow the current `QuestionStyle`; both `OutputStyle` and `ConsoleStyle` are supported. Each render temporarily selects the component helper and then restores the original helper. Constructor or validation errors and cancellation follow the same restoration path.

```rust
use openrust::console::{AsyncCommandCallback, CompletionCallback, ConsoleError, OutputOptions,
    OutputStyle, view::components::{Ask, AskWithCompletion, Choice, Confirm, Secret}};
use openrust::support::facades::Artisan;
use serde_json::json;

fn register_question_components() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:question-components", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let mut style = OutputStyle::new(input, output);
            let name = Ask::new(&mut style).render_with("Name", Some(json!("Guest")), false).await?;
            let color = AskWithCompletion::new(&mut style).render_with_callback(
                "Color", CompletionCallback::sync(|typed| {
                    Ok(["red", "green"].into_iter().filter(|value| value.starts_with(&typed)).map(str::to_owned).collect())
                }), Some(json!("red")),
            ).await?;
            let confirmed = Confirm::new(&mut style).render("Continue").await?;
            let format = Choice::new(&mut style).render_with(
                "Format", ["CSV", "JSON"], Some(json!(0)), None, false,
            ).await?;
            let secret = Secret::new(&mut style).render("Password").await?;
            style.writeln(format!("User:{name}; Color:{color}; Confirmed:{confirmed}; Format:{format}; Secret supplied:{}", !secret.is_null()).as_str(), OutputOptions::default())?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

`Ask::render` has no default and reads one line; `render_with` accepts a scalar JSON default and a multiline flag. `AskWithCompletion::render` accepts candidate strings, `render_with` adds a default, and `render_with_callback` uses the existing asynchronous-capable completion callback. Candidate input is consumed once. These components return `serde_json::Value`; `Confirm` returns `bool` and defaults to false. `Secret::render_with(question, false)` disables visible fallback when native hidden input is unavailable. Do not print the returned secret.

`Choice::render` uses a list without a default or attempt limit. `render_with` adds the default, maximum attempts and multiple-selection flag; zero attempts returns an error. `render_pairs_with` preserves associative and sparse integer keys. A component treats every non-list key layout as associative: sparse numeric keys return string keys such as `"1"`. A proper list returns its selected value. Ordinary `ChoiceQuestion::from_pairs` retains its existing string-key classification.

`Choice::get_choice_question` and `get_choice_question_pairs` expose the prepared definition for composition. `is_assoc` examines normalized key/value entries from `ChoiceQuestion::get_choices`. `ChoiceQuestion::from_pairs_with_associative` accepts a `Send + Sync` key classifier over normalized entries; its boolean is captured when constructing or rebuilding the validator, not recomputed per answer. Completion candidates keep their construction snapshot. Cloning preserves that classification while retaining the existing independent trim setting.


### Component factories {#component-factories}

`CommandIo::components()` and `output_components()` return a borrowed `Factory`, not a raw output stream. Its typed methods reuse the component classes: `info`, `success`, `warn`, `error`, `line`, `alert`, `bullet_list`, `two_column_detail`, `task`, `ask`, `ask_with_completion`, `choice`, `confirm` and `secret`. Display methods return `Result<(), ConsoleError>`; question methods are asynchronous. Use `get_output()` or `AsOutput` when direct stream access is required.

```rust
use openrust::{console::{AsyncCommandCallback, ConsoleError}, support::facades::Artisan};
fn register_component_factories() -> Result<(), ConsoleError> {
    Artisan::command_with("components:save", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let mut io = input.io(output);
            let name = io.components().ask_with("Name", Some("Guest".into()), false).await?;
            io.output_components().info(&format!("Saved {name}"))?;
            io.components().task_callback("Index", |_| Ok(true))?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

The factory retains the command's original output style and its question input. `set_output` replaces the ordinary output only; repeated replacements do not move component output or question input. `set_input` changes command input access without replacing that original style. A standalone `Factory::new(&mut output)` supports display on `Output`, output references and sections. Questions require an `OutputStyle`, `ConsoleStyle`, or a custom `ComponentOutput::question_style` implementation; a plain stream returns a runtime error before printing a question. Task callbacks receive the same selected output reference and retain task failure and error propagation behavior.


### Dynamic component calls {#dynamic-component-calls}

Use `Factory::call(name, parameters).await` for a component name chosen at runtime. Names preserve the dynamic string contract: `askWithCompletion`, `bulletList` and `twoColumnDetail`; case is ignored, but underscore spellings are not aliases. Unknown names return `ConsoleError`. The ordinary Rust methods continue to use snake case.

```rust
use openrust::{
    console::{AsyncCommandCallback, CompletionCallback, ConsoleError, view::components::ComponentArgument},
    support::facades::Artisan,
};
use serde_json::json;
fn register_dynamic_components() -> Result<(), ConsoleError> {
    Artisan::command_with("components:dynamic", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let mut io = input.io(output);
            let color = io.components().call("askWithCompletion", vec![
                "Color".into(), CompletionCallback::sync(|_| Ok(vec!["red".into(), "green".into()])).into(),
                json!("red").into(),
            ]).await?;
            io.components().call("info", vec!["Saved".into()]).await?;
            io.components().call("task", vec!["Index".into(), ComponentArgument::task(|_| Ok(true))]).await?;
            io.line(&format!("Choice:{color}"))?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

`ComponentArgument::Value` carries JSON data. Display calls return JSON null, questions return their existing values, and confirmation returns a JSON boolean. `Completion` accepts the existing `CompletionCallback`; `ComponentArgument::task` owns a compiled `FnOnce` callback, including borrowed `Send` state for the duration of the call. It runs once and retains task finalization before error propagation. Strings are data, not executable function names. `Options` accepts `OutputOptions`. Integer options accept one verbosity and one format flag from the documented constants; conflicting masks and invalid arguments return errors. Existing ownership, question cancellation and temporary-helper restoration rules still apply.

Invalid text arrays are rejected before rendering. An error cannot leave a partially compiled HTML template in a process output buffer.


### Custom console components {#custom-console-components}

Register compiled constructors in your application's Provider. Each dynamic call constructs a new `RenderableComponent`, gives it the original output context, awaits `render` and then releases the instance. The constructor is shared within its application's `ComponentRegistry`; the mutable component instance is not shared. Register `BadgeProvider` in the normal Provider list and `register_custom_components` in console routes.

```rust
use openrust::{
    console::{AsOutput as _, AsyncCommandCallback, ConsoleError, ConsoleFuture, OutputFormat,
        view::components::{ComponentArgument, ComponentConstructor, ComponentContext, ComponentRegistry, RenderableComponent}},
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
    support::{ProviderResult, ServiceProvider, facades::Artisan},
};
use serde_json::{Value, json};
use std::sync::Arc;
struct Badge<'a>(ComponentContext<'a>);
struct BadgeConstructor;
impl ComponentConstructor for BadgeConstructor {
    fn make<'a>(&'a self, output: ComponentContext<'a>) -> Result<Box<dyn RenderableComponent + 'a>, ConsoleError> {
        Ok(Box::new(Badge(output)))
    }
}
impl RenderableComponent for Badge<'_> {
    fn render<'a>(&'a mut self, parameters: Vec<ComponentArgument<'a>>) -> ConsoleFuture<'a, Result<Value, ConsoleError>> {
        Box::pin(async move {
            let label = match parameters.first() {
                Some(ComponentArgument::Value(Value::String(label))) => label,
                _ => return Err(ConsoleError::new("需要文本标签")),
            };
            self.0.as_output().writeln(&format!("Badge:{label}"), OutputFormat::Raw)?;
            Ok(json!({"label": label}))
        })
    }
}
struct BadgeProvider;
impl ServiceProvider for BadgeProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> std::pin::Pin<Box<dyn std::future::Future<Output = ProviderResult> + Send + 'a>> {
        Box::pin(async move {
            app.container().make::<ComponentRegistry>()?.register("Badge", Arc::new(BadgeConstructor))?;
            Ok(())
        })
    }
}
fn register_custom_components() -> Result<(), ConsoleError> {
    Artisan::command_with("components:badge", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let mut io = input.io(output);
            let result = io.components().call("Badge", vec!["Ready".into()]).await?;
            io.line(&result.to_string())?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

Names may contain namespace separators, for example `Widgets\Badge`. Registration is case insensitive and rejects duplicate names, invalid names and existing built-in component names. Constructors and their destructors run outside registry locks. Constructor and render errors propagate; cancellation releases the component and its output borrow.

`Factory::new` records the current application container for custom-component lookup. Entering another application's scope does not change that registration owner. Standalone consumers can use `Factory::with_registry(&mut output, registry)` with an explicit `Arc<ComponentRegistry>`. Data and callbacks still use `ComponentArgument`.

`ComponentContext` implements `AsOutput`. `question_style()` returns the original question style or a runtime error before a prompt is printed. Custom output adapters implement `ComponentOutput::component_context`: return `ComponentContext::Style` to retain question input, or `ComponentContext::Output` for an ordinary stream. The default `ComponentOutput::question_style` follows this context; existing explicit question-style overrides remain valid for built-in questions.


### Component data and question scopes {#component-data-scopes}

`Component` is a reusable composition base. Construct it with a borrowed `ComponentOutput`, or use `from_context` inside a registered component constructor. `get_output` and `AsOutput` borrow the original output; `question_style` exposes its existing question capability without replacing input.

```rust
use openrust::{
    console::{AsyncCommandCallback, ConsoleError, OutputFormat, OutputStyle, Question,
        view::components::{Component, DefaultMutatorConstructor, mutators::EnsureNoPunctuation}},
    support::facades::Artisan,
};
use serde_json::json;
fn register_component_data() -> Result<(), ConsoleError> {
    Artisan::command_with("components:data", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let mut style = OutputStyle::new(input, output);
            let mut component = Component::new(&mut style);
            let mutator = DefaultMutatorConstructor::<EnsureNoPunctuation>::new();
            let data = component.mutate(json!({"message": "Created [report].", "active": true}), &[&mutator])?;
            let mut calls = 0;
            let name = component.using_question_helper(async |mut style| {
                calls += 1;
                let question = Question::new("Name", Some(json!("Guest")))?;
                style.ask_question(&question).await
            }).await?;
            component.get_output().writeln(
                format!("Data:{data}; Name:{name}; Calls:{calls}").as_str(), OutputFormat::Raw,
            )?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

`mutate` runs constructors in order, creating each mutator once per pass and applying it to each top-level value. Arrays retain their positions and objects retain their keys. An empty constructor list returns the original data. `DefaultMutatorConstructor<M>` uses `M::default`; a custom `MutatorConstructor` can create a stateful mutator or return a construction error. An error stops the remaining pipeline.

`Mutator::apply` remains the string interface. `apply_value` handles JSON values and can be overridden by custom mutators. Removing punctuation retains the original value when no change is needed; numbers, booleans and arrays are not silently converted to strings. Highlighting casts values to text. Relative-path replacement retains values when no application base path is bound; with a base path, an array subject keeps its keys and converts each immediate value, matching string replacement behavior.

`using_question_helper` accepts a lending async closure. It temporarily installs the component helper, then restores the exact previous helper on success, error, panic unwinding or cancellation. An uninitialized helper is restored to the ordinary default helper. Borrowed local state can be updated inside the closure. Plain outputs fail before the closure is called. Prefer `Result` for recoverable failures.

The callback receives a `QuestionStyleReference`, a short-lived view of the original style implementing `QuestionStyle` and `AsOutput`. Bind it as `async |mut style|`. This named reference makes the borrow lifetime explicit for `Send` command handlers; it does not allocate another style or copy input.
