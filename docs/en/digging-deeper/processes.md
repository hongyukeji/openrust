# Processes

## Running processes {#running-processes}

Use `Process` from `openrust::support::facades` within an application. A command string runs through the platform shell. Pass an argument array when values should be passed literally. Outside an application, use `openrust::process::Factory`. The example below uses a Unix shell.

```rust
use openrust::process::Factory;
use std::time::Duration;

let processes = Factory::new();
let result = processes.command(["/bin/sh", "-c", r#"printf '%s' "$MODE""#])
    .env([("MODE", "test")])
    .timeout(Duration::from_secs(2))
    .run()
    .await?
    .throw()?;
assert_eq!(result.output(), b"test");
```

Set the working directory with `path`, add or remove environment variables with `env` and `removeEnv`, and send bytes with `input`. The default timeout is 60 seconds. `idleTimeout` limits the time without output and `forever` removes the total deadline. Nonzero exit status is available through `exitCode`. Call `throw` to turn it into an error.

## Running asynchronously {#running-asynchronously}

Use `start` to get a live handle. Inspect `output` and `errorOutput`, or use `latestOutput` and `latestErrorOutput` for unread bytes. `startWithOutput` invokes a callback for each available stdout or stderr chunk. Use `wait` for the result, or `stop` to request termination and force it after the grace period. On macOS and Linux the process group is cleaned up on completion, timeout, or cancellation.

Run named children together with `pool` and `concurrently`. `pipe` passes each successful child’s stdout as input to the next command and stops on failure.

## Testing {#testing}

Fakes and recorded invocations belong to a single factory. Use `preventStrayProcesses(true)` to fail unmatched calls before launching an executable.

```rust
use openrust::process::Factory;

let processes = Factory::new();
processes.fakeCommand("deploy *", Factory::result("deployed", "", 0));
processes.preventStrayProcesses(true);
let result = processes.run(["deploy", "preview"]).await?;
assert!(result.successful());
processes.assertRan(|process| process.command == ["deploy", "preview"].into());
```
