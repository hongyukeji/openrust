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

It uses the 41 quotes from the Laravel baseline. In a color terminal, the quote appears in bold and the attribution in gray. Redirected output is plain text by default. Use `--ansi` to force styles or `--no-ansi` to disable them.

On Windows, use `artisan.bat`. The compiled application accepts the same commands through `main artisan`.

Listing and help exit with status 0. Unknown commands and invalid arguments exit with status 1. Entries marked `Planned` have help available and exit with status 1 when invoked.

Register command types through `withCommands`, using `app/console/commands/mod.rs`. Declare the console routes from `routes/console.rs` through `withRouting` in `bootstrap/app.rs`. A command registered under the same name replaces its planned entry.

In `routes/console.rs`, import `openrust::support::facades::Artisan`. `Artisan::command("inspire", …)` registers the closure with the console bound while `register()` runs and `purpose()` sets the description shown by Artisan help and list. The closure receives the command’s output and returns normally with exit status 0.

`Application::handleCommand` calls the Console Kernel’s `handle` and `terminate` methods. After boot, the kernel loads deferred providers and registers the command catalog, built-in commands and application commands before dispatch.


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

A process that fails after running for at least one second is retried up to five times with a one-second delay. A process that exits successfully is finished. A process that fails within a second is reported immediately. Disable retries for a run with `--no-restart`, or for the application with `DevCommands::disableAutoRestart()`. Use `DevCommands::withTimestamps()` to enable timestamps by default. The first permanent failure determines the exit code while other processes can keep running.

A usable Unix terminal opens the tabbed view by default. Start in the interleaved view with `--stream`. Redirected output, small terminals, Windows, `--inline`, and `--json` use the inline path. Interactive sessions return status 0 on normal exit, while inline and JSON runs report the first permanent failure.

Use `1`–`9` to select a tab, or focus the sidebar with Tab and use the arrow keys. Press `s` for the stream, `t` for tabs, `f` to filter the stream, `r` to restart the selected tab's process, and `c` to clear the current view. Search with `/`, then use `n` or `N` to move between matches. Use arrows or Page Up and Page Down to scroll logs.

`--buffer-size` and `--stream-buffer-size` control retained lines per tab and in the stream. The defaults are 2,000 and 10,000 lines. The application can configure these through `DevCommands::bufferSize()` and `streamBufferSize()`, and select its default mode with `tabs()`, `stream()`, or `inline()`.

Press `q` or Ctrl+C to exit. Unix termination signals also request shutdown and allow up to six seconds for child processes to exit. Artisan restores the terminal and prints the retained stream to the corresponding output streams. The optional [Pail command](../the-basics/logging.md#tailing-log-messages-using-pail) can also run independently.

Press Ctrl+C in the Windows console to request shutdown. Each development command runs in its own Windows job. Artisan sends Ctrl+Break to its console process group and releases any remaining descendants when the grace period expires. With `--no-restart`, a failed command also shuts down the other development commands and returns the failure code.

### Listing Development Processes

Use `dev:list` to inspect the application's development process registry. It lists the [HTTP development server](#development-http-server) and queue listener by default. When the application has a `package.json`, it also lists Vite using Bun, pnpm, Yarn, or npm according to the application's lockfile. The list never starts a process. Automatic Pail process registration remains separate work.

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

Use `register` for shell commands and `node` or `nodeExec` for package-manager commands. Pass `None` to use the first word of the command as its name. `only`, `except`, `order`, `withoutVendorCommands`, and `withoutDefaultCommands` customize the application registry. Package registrations are recognized by their Rust caller's source path.

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

Pass a separate `OsString` for each positional argument or option. Values that contain spaces stay in one token. Include a switch for a true Boolean flag, and repeat options that take multiple values. Use `Artisan::callSilently(name, arguments).await` to discard both streams and keep the exit code.

### Queueing Artisan Commands {#queueing-artisan-commands}

Prepare a command with `Artisan::queue`. Call `dispatch().await` to send it to the queue and obtain the dispatch result. Specify a destination with `onConnection` and `onQueue`, or use the default connection and its queue.

```rust
use openrust::{
    queue::{DispatchResult, QueueResult},
    support::facades::Artisan,
};

async fn queue_inspire() -> QueueResult<DispatchResult> {
    Artisan::queue("inspire", &[])
        .onConnection("redis")
        .onQueue("commands")
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

## Generating Commands {#generating-commands}

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
