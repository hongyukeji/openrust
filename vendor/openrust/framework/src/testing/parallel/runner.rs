use crate::{
    console::{ConsoleError, Output},
    contracts::foundation::Application as _,
    foundation::Application,
    process::Process,
};
use futures_util::{StreamExt, stream};
use serde_json::Value;
use std::{collections::BTreeSet, ffi::OsString};
fn error(source: impl std::fmt::Display) -> ConsoleError {
    ConsoleError::new(format!("并行测试执行失败：{source}"))
}
fn text(value: &std::ffi::OsStr) -> Result<&str, ConsoleError> {
    value.to_str().ok_or_else(|| error("参数必须是有效UTF-8"))
}
/// 复用Cargo构建及原生libtest，每个测试在独立子进程执行，进程数限制并发上限。
pub(crate) async fn run(
    app: &Application,
    arguments: &[OsString],
    output: &mut Output,
) -> Result<i32, ConsoleError> {
    let mut processes = std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get);
    let mut cargo = Vec::<OsString>::new();
    let mut harness = Vec::<OsString>::new();
    let environment = vec![
        ("APP_ENV".to_owned(), "testing".to_owned()),
        ("LARAVEL_PARALLEL_TESTING".into(), "1".into()),
        ("OPENRUST_PARALLEL_TEST_CASE_PROCESS".into(), "1".into()),
    ];
    let mut after_separator = false;
    let mut take_value = false;
    let mut iterator = arguments.iter();
    while let Some(argument) = iterator.next() {
        let value = text(argument)?;
        if after_separator {
            harness.push(argument.clone());
            continue;
        }
        if take_value {
            cargo.push(argument.clone());
            take_value = false;
            continue;
        }
        if value == "--" {
            after_separator = true;
            continue;
        }
        if value == "--parallel" {
            continue;
        }
        if value == "--processes" || value.starts_with("--processes=") {
            let value = if let Some((_, value)) = value.split_once('=') {
                value
            } else {
                text(
                    iterator
                        .next()
                        .ok_or_else(|| error("--processes需要正整数"))?,
                )?
            };
            processes = value
                .parse()
                .ok()
                .filter(|n| *n > 0)
                .ok_or_else(|| error("--processes需要正整数"))?;
            continue;
        }
        if value == "--message-format" {
            iterator
                .next()
                .ok_or_else(|| error("--message-format缺少值"))?;
            continue;
        }
        if value.starts_with("--message-format=") {
            continue;
        }
        if value.starts_with('-') {
            take_value = [
                "-p",
                "--package",
                "--exclude",
                "--bin",
                "--test",
                "--bench",
                "--example",
                "--features",
                "-F",
                "--target",
                "--target-dir",
                "--manifest-path",
                "--profile",
                "--color",
                "--config",
                "-Z",
                "--lockfile-path",
                "-j",
                "--jobs",
            ]
            .contains(&value);
            cargo.push(argument.clone());
        } else {
            harness.push(argument.clone());
        }
    }
    if take_value {
        return Err(error("Cargo选项缺少值"));
    }
    let compile_only = cargo.iter().any(|v| v == "--no-run");
    let executable = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut command = vec![executable, "test".into()];
    command.extend(cargo);
    if !compile_only {
        command.push("--no-run".into());
    }
    command.push("--message-format=json".into());
    let built = Process::command(command)
        .map_err(error)?
        .path(app.base_path())
        .env(environment.clone())
        .forever()
        .run()
        .await
        .map_err(error)?;
    output
        .stderr
        .push_str(&String::from_utf8_lossy(built.error_output()));
    let mut artifacts = BTreeSet::new();
    for line in String::from_utf8_lossy(built.output()).lines() {
        if let Ok(value) = serde_json::from_str::<Value>(line) {
            if value["reason"] == "compiler-artifact" && value["profile"]["test"] == true {
                if let Some(path) = value["executable"].as_str() {
                    artifacts.insert(path.to_owned());
                }
            } else if value["reason"] == "compiler-message"
                && let Some(rendered) = value["message"]["rendered"].as_str()
            {
                output.stderr.push_str(rendered);
            }
        }
    }
    if built.failed() || compile_only {
        return Ok(built.exit_code());
    }
    let list_only = harness.iter().any(|v| v == "--list");
    let mut runtime = Vec::new();
    let mut list_arguments = Vec::new();
    let mut args = harness.iter();
    while let Some(argument) = args.next() {
        let value = text(argument)?;
        match value {
            "--list" => {}
            "--nocapture" | "--show-output" | "--ignored" | "--include-ignored" => {
                runtime.push(argument.clone());
                list_arguments.push(argument.clone());
            }
            "--exact" => list_arguments.push(argument.clone()),
            "--skip" => {
                list_arguments.push(argument.clone());
                list_arguments.push(args.next().ok_or_else(|| error("--skip缺少值"))?.clone());
            }
            "--format" => {
                args.next().ok_or_else(|| error("--format缺少值"))?;
            }
            value if value.starts_with("--format=") => {}
            value if value.starts_with('-') => {
                return Err(error(format!("并行模式暂不支持测试参数 {value}")));
            }
            _ => list_arguments.push(argument.clone()),
        }
    }
    let mut tests = Vec::new();
    for executable in artifacts {
        let mut command = vec![executable.clone().into()];
        command.extend(list_arguments.clone());
        command.extend(["--list".into(), "--format=terse".into()]);
        let listed = Process::command(command)
            .map_err(error)?
            .path(app.base_path())
            .env(environment.clone())
            .forever()
            .run()
            .await
            .map_err(error)?;
        if listed.failed() {
            output
                .stdout
                .push_str(&String::from_utf8_lossy(listed.output()));
            output
                .stderr
                .push_str(&String::from_utf8_lossy(listed.error_output()));
            return Ok(listed.exit_code());
        }
        let list = std::str::from_utf8(listed.output()).map_err(error)?;
        for line in list.lines() {
            if let Some(name) = line.strip_suffix(": test") {
                if list_only {
                    output.stdout.push_str(line);
                    output.stdout.push('\n');
                } else {
                    tests.push((executable.clone(), name.to_owned()));
                }
            }
        }
    }
    if list_only {
        return Ok(0);
    }
    let mut work = stream::iter(tests.into_iter().enumerate().map(
        |(index, (executable, name))| {
            let mut command = vec![
                executable.into(),
                name.clone().into(),
                "--exact".into(),
                "--test-threads=1".into(),
            ];
            command.extend(runtime.clone());
            let mut environment = environment.clone();
            environment.push(("TEST_TOKEN".into(), (index + 1).to_string()));
            let path = app.base_path().to_owned();
            async move {
                let result = Process::command(command)
                    .map_err(error)?
                    .path(path)
                    .env(environment)
                    .forever()
                    .run()
                    .await
                    .map_err(error)?;
                Ok::<_, ConsoleError>((name, result))
            }
        },
    ))
    .buffer_unordered(processes);
    let mut failed = false;
    while let Some(result) = work.next().await {
        let (name, result) = result?;
        output.stdout.push_str(&format!("\n{name}\n"));
        output
            .stdout
            .push_str(&String::from_utf8_lossy(result.output()));
        output
            .stderr
            .push_str(&String::from_utf8_lossy(result.error_output()));
        failed |= result.failed();
    }
    Ok(i32::from(failed))
}
