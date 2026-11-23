use std::{
    ffi::OsString,
    path::PathBuf,
    process::ExitCode,
    sync::{Arc, Mutex},
};

use clap::{Arg, Command as Syntax};
use openrust::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output,
    },
    contracts::console::Kernel as _,
    foundation::{Application, console::Kernel, run_application},
};
use openrust_pail::{contracts::Printer, printers::CliPrinter, value_objects::MessageLogged};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/printer.json")).unwrap()
}
struct Print(CommandDefinition, Arc<Mutex<Option<Value>>>);
impl Command for Print {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }
    fn handle<'a>(
        &'a self,
        _: &'a Application,
        input: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let data = fixture();
            let index: usize = input
                .matches()
                .get_one::<String>("index")
                .unwrap()
                .parse()
                .unwrap();
            let vector = &data["vectors"][index];
            let message = MessageLogged::from_json(
                &data["records"][vector["record"].as_u64().unwrap() as usize].to_string(),
            )
            .unwrap();
            let mut printer = CliPrinter::new(output, "/app/project");
            let options = printer.optionsHtml(&message)?;
            let trace = printer.traceHtml(&message);
            printer.print(&message)?;
            *self.1.lock().unwrap() = Some(json!({"options":options,"trace":trace}));
            Ok(0)
        })
    }
}
fn artisan(state: Arc<Mutex<Option<Value>>>) -> Artisan {
    let mut artisan = Artisan::new();
    artisan
        .add_command(Box::new(Print(
            CommandDefinition::new(
                Syntax::new("printer:record").arg(Arg::new("index").required(true)),
            ),
            state,
        )))
        .unwrap();
    artisan
}
fn create() -> Result<Application, Box<dyn std::error::Error + Send + Sync>> {
    Ok(Application::configure(
        std::env::var_os("APP_BASE_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir),
    )
    .with_commands(|artisan| {
        artisan.add_command(Box::new(Print(
            CommandDefinition::new(
                Syntax::new("printer:record").arg(Arg::new("index").required(true)),
            ),
            Arc::default(),
        )))
    })
    .create()?)
}
fn normalized(value: &str) -> String {
    value
        .split('\n')
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
}
fn hash(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}
async fn suite(width: u64) {
    let root =
        std::env::temp_dir().join(format!("openrust-pail-printer-{:032x}", fastrand::u128(..)));
    std::fs::create_dir(&root).unwrap();
    let app = Application::configure(root.clone()).create().unwrap();
    let state = Arc::new(Mutex::new(None));
    let mut kernel = Kernel::new(app, artisan(state.clone()));
    let data = fixture();
    let mut evidence = Vec::new();
    for (index, vector) in data["vectors"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
        .filter(|(_, v)| v["width"] == width)
    {
        let mut args: Vec<OsString> = vec!["printer:record".into(), index.to_string().into()];
        match vector["verbosity"].as_u64().unwrap() {
            1 => args.push("-v".into()),
            2 => args.push("-vv".into()),
            _ => {}
        }
        let mut output = Output::default();
        let status = kernel.handle(&args, &mut output).await;
        assert_eq!(status, 0, "{}", output.stderr);
        let mut record = state.lock().unwrap().take().unwrap();
        record["id"] = vector["id"].clone();
        assert!(!output.is_verbose());
        record["output"] = output.stdout.into();
        evidence.push(record);
    }
    drop(kernel);
    std::fs::remove_dir_all(root).unwrap();
    println!("{}", json!(evidence));
}
#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|arg| arg == "artisan") {
        return run_application(create).await;
    }
    if args.first().is_some_and(|arg| arg == "--suite") {
        suite(args[1].parse().unwrap()).await;
        return ExitCode::SUCCESS;
    }
    for (datetime, context) in [
        (
            "invalid",
            json!({"__pail":{"origin":{"type":"console","command":"probe"}}}),
        ),
        (
            "2024-01-01T03:04:05.000000+00:00",
            json!({"__pail":{"origin":{"type":"http"}}}),
        ),
    ] {
        let message = MessageLogged::from_json(&json!({"message":"invalid record","datetime":datetime,"level_name":"INFO","context":context}).to_string()).unwrap();
        let mut output = Output::default();
        assert!(
            CliPrinter::new(&mut output, "/app/project")
                .print(&message)
                .is_err()
        );
        assert!(output.stdout.is_empty());
    }
    let native = MessageLogged::from_json(&json!({
        "message":"native JSON precision", "datetime":"2024-01-01T03:04:05.000000+00:00", "level_name":"INFO",
        "context":{"large_id":u64::MAX,"empty":{},"exception":{"class":"app::domain::import::jobs::Error"},"__pail":{"origin":{"type":"console","command":"probe"}}}
    }).to_string()).unwrap();
    let mut output = Output::default();
    let mut printer = CliPrinter::new(&mut output, "/app/project");
    let options = printer.optionsHtml(&native).unwrap();
    assert!(options.contains("large_id: 18446744073709551615"));
    assert!(options.contains("empty: array (\n)"));
    printer.print(&native).unwrap();
    assert!(output.stdout.contains("app::domain::…::Error"));
    let data = fixture();
    let mut all = Vec::new();
    let mut mismatches = 0;
    for width in [20, 26, 50, 80, 150, 200] {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--suite", &width.to_string()])
            .env("COLUMNS", width.to_string())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let records: Vec<Value> = serde_json::from_slice(&output.stdout).unwrap();
        for record in records {
            let expected = data["vectors"]
                .as_array()
                .unwrap()
                .iter()
                .find(|v| v["id"] == record["id"])
                .unwrap();
            for key in ["output", "options", "trace"] {
                let text = record[key].as_str().unwrap();
                let text = if key == "output" {
                    normalized(text)
                } else {
                    text.into()
                };
                if hash(&text) != expected[format!("{key}_sha256")] {
                    mismatches += 1;
                    if mismatches <= 12 {
                        eprintln!("{} {key} mismatch: {:?}", record["id"], text);
                    }
                }
            }
            all.push(record);
        }
    }
    if let Some(path) = std::env::var_os("PAIL_PRINTER_EVIDENCE") {
        std::fs::write(path, json!(all).to_string()).unwrap();
    }
    assert_eq!(mismatches, 0, "printer oracle mismatches");
    println!("Pail printer: {} pinned PHP vectors passed", all.len());
    ExitCode::SUCCESS
}
