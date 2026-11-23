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

Set the working directory with `path`, add or remove environment variables with `env` and `remove_env`, and send bytes with `input`. The default timeout is 60 seconds. `idle_timeout` limits the time without output and `forever` removes the total deadline. Nonzero exit status is available through `exit_code`. Call `throw` to turn it into an error.

## Running asynchronously {#running-asynchronously}

Use `start` to get a live handle. Inspect `output` and `error_output`, or use `latest_output` and `latest_error_output` for unread bytes. `start_with_output` invokes a callback for each available stdout or stderr chunk. Use `wait` for the result, or `stop` to request termination and force it after the grace period. On macOS and Linux the process group is cleaned up on completion, timeout, or cancellation.

Run named children together with `pool` and `concurrently`. `pipe` passes each successful child’s stdout as input to the next command and stops on failure.

### Waiting for output {#waiting-for-output}

Call `wait_until` on a captured process to receive stdout and stderr chunks until a predicate matches. It returns `true` on a match and `false` if the process exits first. A subsequent wait continues with unread chunks. Match using an accumulator when a marker can span chunks. Call `wait` to obtain the final exit code, or `wait_with_output` to observe the remaining chunks. A timeout carries the captured result through `Error::result()`.

```rust
use openrust::{process::OutputStream, support::facades::Process};
use std::time::Duration;

let mut process = Process::command("printf 'ready\n' >&2; sleep 0.1; printf 'done'")?
    .timeout(Duration::from_secs(3))
    .start()?;
let mut diagnostic = Vec::new();
let ready = process.wait_until(|stream, chunk| {
    if stream == OutputStream::Stderr {
        diagnostic.extend_from_slice(chunk);
    }
    diagnostic.ends_with(b"ready\n")
}).await?;
assert!(ready);
assert_eq!(process.wait().await?.output(), b"done");
```

### Attaching to a terminal {#tty}

Use `tty(true)` to attach all three standard streams to the controlling terminal on Unix. Terminal input and output go directly to the terminal. A process started this way has no buffered output and does not invoke output callbacks. Applications without a controlling terminal receive `Error::TtyUnavailable`. `tty(false)` restores regular capture.

The attached process shares the terminal’s foreground process group. Termination targets the direct child. Captured Unix processes retain their separate group and group cleanup.

```rust
use openrust::support::facades::Process;

let result = Process::command("test -t 0 && test -t 1 && test -t 2 && printf 'terminal ready\n'")?
    .tty(true)
    .run()
    .await?
    .throw()?;
assert!(result.output().is_empty());
```

### Numeric signals {#numeric-signals}

Use `Signal::Raw` to send a platform-specific Unix signal number to the active process. Before running the example, set `PROCESS_EXAMPLE_SIGNAL` to the host’s numeric value for `SIGUSR1`.

```rust
use openrust::{process::Signal, support::facades::Process};
use std::time::Duration;

let number: i32 = std::env::var("PROCESS_EXAMPLE_SIGNAL")?.parse()?;
let mut process = Process::command(
    "trap 'printf caught; exit 6' USR1; printf ready; while :; do sleep 0.02; done",
)?
.timeout(Duration::from_secs(4))
.start()?;
assert!(process.wait_until(|_, bytes| bytes == b"ready").await?);
process.signal(Signal::Raw(number)).await?;
let result = process.wait().await?;
assert_eq!(result.exit_code(), 6);
assert_eq!(result.output(), b"readycaught");
```

## Testing {#testing}

Fakes and recorded invocations belong to a single factory. Use `prevent_stray_processes(true)` to fail unmatched calls before launching an executable.

```rust
use openrust::process::Factory;

let processes = Factory::new();
processes.fake_command("deploy *", Factory::result("deployed", "", 0));
processes.prevent_stray_processes(true);
let result = processes.run(["deploy", "preview"]).await?;
assert!(result.successful());
processes.assert_ran(|process, _| process.command == ["deploy", "preview"].into());
```

### Scripted fake processes {#scripted-fakes}

Build a scripted fake with `Factory::describe()`. Specify an ID, ordered output lines, the number of checks for which `running()` reports true, and an exit code. Register the built result with `fake_command` or a fake sequence. Waiting plays the ordered lines and records signals locally.

```rust
use openrust::process::{Factory, OutputStream, Signal};

let processes = Factory::new();
processes.fake_command("worker", Factory::describe()
    .id(321)
    .runs_for(2)
    .error_output("ready")
    .output("finished")
    .exit_code(0)
    .result());
processes.prevent_stray_processes(true);

let mut worker = processes.start("worker")?;
assert_eq!(worker.id(), Some(321));
assert!(worker.wait_until(|stream, bytes| stream == OutputStream::Stderr && bytes == b"ready\n").await?);
worker.signal(Signal::Terminate).await?;
assert!(worker.has_received_signal(Signal::Terminate));
assert_eq!(worker.wait().await?.output(), b"finished\n");
```


## Ordered assertions

Call `assert_nothing_ran()` before any simulated invocation to require an empty record. `assert_not_ran(|process, result| ...)` rejects any matching record. `assert_ran_in_order(&[&first, &second])` requires exactly that many records in that order; each predicate receives `&RecordedProcess` and `Option<&ProcessResult>`. A failed simulated operation may have no result. Each assertion panics when its expectation is violated; use the same methods on the `Process` facade inside an application scope.
