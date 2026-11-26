use super::{
    error,
    process_output::{self, CommandOutput},
    signals,
};
use openrust::{
    console::{ConsoleError, Input, Output},
    contracts::foundation::Application as _,
    foundation::Application,
    process::{Factory, InvokedProcess},
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

fn fingerprint(
    root: &Path,
    paths: &[String],
) -> std::io::Result<BTreeMap<PathBuf, (u64, SystemTime)>> {
    fn visit(
        path: &Path,
        pattern: Option<&str>,
        files: &mut BTreeMap<PathBuf, (u64, SystemTime)>,
    ) -> std::io::Result<()> {
        let metadata = match path.symlink_metadata() {
            Ok(value) => value,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        if metadata.file_type().is_symlink() {
            return Ok(());
        }
        if metadata.is_dir() {
            for child in std::fs::read_dir(path)? {
                visit(&child?.path(), pattern, files)?;
            }
        } else if pattern.is_none_or(|pattern| {
            openrust::support::Str::is(&[pattern], &path.to_string_lossy(), false)
        }) {
            files.insert(path.to_owned(), (metadata.len(), metadata.modified()?));
        }
        Ok(())
    }
    let mut files = BTreeMap::new();
    for path in paths {
        let pattern = root.join(path).to_string_lossy().to_string();
        if let Some(index) = pattern.find(['*', '?']) {
            let prefix = &pattern[..index];
            let parent = Path::new(prefix).parent().unwrap_or(root);
            visit(parent, Some(&pattern), &mut files)?;
        } else {
            visit(&root.join(path), None, &mut files)?;
        }
    }
    Ok(files)
}
fn start(
    app: &Application,
    input: &Input,
    output: &CommandOutput,
) -> Result<InvokedProcess, ConsoleError> {
    let mut args = if app.base_path().join("Cargo.toml").is_file() {
        vec![
            "cargo".into(),
            "run".into(),
            "--quiet".into(),
            "--".into(),
            "artisan".into(),
            "horizon".into(),
        ]
    } else {
        vec![
            std::env::current_exe().map_err(error)?.into_os_string(),
            "artisan".into(),
            "horizon".into(),
        ]
    };
    if let Some(env) = input.matches().get_one::<String>("environment") {
        args.push(format!("--environment={env}").into());
    }
    let process = Factory::new().command(args).path(app.base_path()).forever();
    crate::process_output::start(process, output.handler().as_ref()).map_err(error)
}
pub(super) async fn listen(
    app: &Application,
    input: &Input,
    output: &mut Output,
) -> Result<i32, ConsoleError> {
    let config = app.config().map_err(error)?;
    let paths:Vec<String>=config.get("horizon.watch").and_then(|value|serde_json::from_value(value).ok()).filter(|paths:&Vec<String>|!paths.is_empty()).ok_or_else(||ConsoleError::new("List of directories / files to watch not found. Please update your config/horizon.rs configuration file."))?;
    let mut previous = fingerprint(app.base_path(), &paths).map_err(error)?;
    let child_output = CommandOutput::with_error_output(output);
    let shutdown = input.shutdown().cloned().unwrap_or_default();
    let signals = signals().map_err(error)?;
    tokio::pin!(signals);
    let mut ticker = tokio::time::interval(Duration::from_millis(500));
    process_output::info(output, "Starting Horizon and watching for file changes...");
    let mut process = start(app, input, &child_output)?;
    let result = async {
        loop {
        tokio::select! {_=shutdown.cancelled()=>break,()=&mut signals=>break,_=ticker.tick()=>{}}
        if !process.running() {
            return Ok(process.wait().await.map_err(error)?.exit_code());
        }
        let current = fingerprint(app.base_path(), &paths).map_err(error)?;
        if current != previous {
            previous = current;
            process_output::info(output, "File changed. Restarting Horizon...");
            process.stop(Duration::from_secs(65)).await.map_err(error)?;
            process = start(app, input, &child_output)?;
        }
        }
        process.stop(Duration::from_secs(65)).await.map_err(error)?;
        Ok(0)
    }.await;
    child_output.finish(output);
    result
}
