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

The command catalog contains planned entries in addition to implemented commands. Their names and help reserve the intended contract; they do not imply that the corresponding modules are implemented. General custom command I/O and localization are delivered by later Artisan units. `artisan.bat` has not yet been accepted on a Windows machine.
