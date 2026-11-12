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

`Application::handleCommand` calls the Console Kernel’s `handle` and `terminate` methods. After boot, the kernel loads the planned catalog and registers application commands before dispatch.


## Source and Release Entrypoints

The source `artisan` script changes to the application directory and forwards its arguments unchanged to Cargo. Spaces, empty arguments and non-ASCII text are preserved. The script returns the command's exit status.

A standalone build uses `./main artisan list`, `./main artisan help inspire` or `./main artisan inspire` without Cargo. It finds configuration relative to the executable and works from a different working directory. See [deployment](../getting-started/deployment.md).

The compiled entry accepts no mode argument for HTTP, or `artisan` for console commands. Any other first argument prints the outer usage and exits with status 2, before creating the application. Artisan dispatch errors exit with status 1. This outer selection is a Rust single-binary adaptation; Laravel has separate HTTP and `artisan` entry files. The Console Kernel's handle/terminate sequence remains the responsibility of `Application::handleCommand`.

The command catalog contains planned entries in addition to implemented commands. Their names and help reserve the intended contract; they do not imply that the corresponding modules are implemented. `artisan.bat` has not yet been accepted on a Windows machine.

## Command Help and Output Formats {#command-help-and-output-formats}

`./artisan help migrate` and `./artisan migrate --help` read the same registered definition. They display arguments, options and descriptions without running the command. Exact command names and aliases take precedence. A unique abbreviation can shorten a complete name or its colon-separated segments.

```sh
./artisan list cache --format=json --short
./artisan help migrate --format=json
./artisan help migrate --format=xml
./artisan help migrate --format=md
```

`list` and `help` accept `txt`, `json`, `xml` and `md`. Use `--raw` for a flat text list or compact text help. `list --short` retains names, aliases and implementation status and omits command details. Structured output keeps command and option identifiers stable across locales. Successful help and list commands exit with status 0. Invalid arguments, ambiguous commands and unknown commands exit with status 1.

`--quiet` suppresses normal output. `--silent` also suppresses console errors. The global `--no-interaction`, `--ansi`, `--no-ansi`, `--version` and verbosity flags are available in command definitions.

See [Artisan translations](./localization.md#artisan-translations) for language selection and overrides.

## Optional Route Installers {#optional-route-installers}

Run these commands from a source checkout to generate the route files and update the module and bootstrap declarations. Then rebuild the application. Existing route files are preserved.

```sh
./artisan install:api
./artisan install:broadcasting
./artisan install:grpc
```

API routes default to the `/api` prefix and use the `api` middleware group. Add your own endpoints and authentication. Broadcasting registers `/broadcasting/auth` and requires a driver and channel rules. For gRPC, register Tonic services and set `GRPC_HOST` and `GRPC_PORT` to enable the listener.

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
