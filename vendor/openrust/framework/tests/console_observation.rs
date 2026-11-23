use openrust::contracts::console::Kernel as _;
use openrust::{
    console::{
        Command, CommandDefinition, ConsoleError, ConsoleFuture, Input, Output,
        events::{CommandFinished, CommandStarting},
    },
    contracts::events::{Dispatcher as _, Event, Listener},
    events::{Delivery, EventContext, EventFuture, EventResult, ListenerResponse},
    foundation::Application,
};
use std::sync::{Arc, Mutex};

struct Probe(CommandDefinition);
impl Command for Probe {
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
            let value = input.matches().get_one::<String>("value").unwrap();
            output.stdout.push_str(value);
            if value == "throw" {
                return Err(ConsoleError::new("probe failure"));
            }
            Ok(if value == "nonzero" { 7 } else { 0 })
        })
    }
}
fn commands(artisan: &mut openrust::console::Application) -> Result<(), ConsoleError> {
    artisan.add_command(Box::new(Probe(CommandDefinition::new(
        clap::Command::new("observe:probe").arg(clap::Arg::new("value").required(true)),
    ))))
}
struct Recorder(Arc<Mutex<Vec<String>>>);
impl Listener for Recorder {
    fn handle<'a>(
        &'a self,
        event: &'a dyn Event,
        _: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        Box::pin(async move {
            if let Some(event) = event.downcast_ref::<CommandStarting>() {
                self.0.lock().unwrap().push(format!(
                    "start:{}:{}",
                    event.command,
                    event.input.get_one::<String>("value").unwrap()
                ));
            }
            if let Some(event) = event.downcast_ref::<CommandFinished>() {
                assert!(event.duration.as_secs() < 10);
                self.0.lock().unwrap().push(format!(
                    "finish:{}:{}:{}",
                    event.command, event.exit_code, event.output
                ));
            }
            Ok(None)
        })
    }
}
#[tokio::test]
async fn native_command_events_keep_input_output_exit_code_and_exception_boundaries() {
    let root = tempfile::tempdir().unwrap();
    let mut app = Application::configure(root.path().into())
        .with_commands(commands)
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let dispatcher = app.make::<openrust::events::Dispatcher>().await.unwrap();
    let trace = Arc::new(Mutex::new(Vec::new()));
    for name in [CommandStarting::NAME, CommandFinished::NAME] {
        dispatcher
            .listen(name, Arc::new(Recorder(trace.clone())), Delivery::Sync)
            .unwrap();
    }
    let mut kernel =
        openrust::foundation::console::Kernel::new(app, openrust::console::Application::new());
    let mut output = Output::default();
    for (value, code) in [("success", 0), ("nonzero", 7), ("throw", 1)] {
        assert_eq!(
            kernel
                .handle(&["observe:probe".into(), value.into()], &mut output)
                .await,
            code
        );
    }
    assert_eq!(
        *trace.lock().unwrap(),
        [
            "start:observe:probe:success",
            "finish:observe:probe:0:success",
            "start:observe:probe:nonzero",
            "finish:observe:probe:7:nonzero",
            "start:observe:probe:throw"
        ]
    );
}
