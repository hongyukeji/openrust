use openrust::{
    console::{Command, CommandDefinition, ConsoleError, ConsoleFuture, Input, Output},
    contracts::debug::SharedException,
    foundation::{Application, configuration::ReportDecision},
    support::{
        facades::{App, Exceptions, Facade},
        report,
    },
};
use std::{
    ffi::OsString,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
struct Probe(CommandDefinition);
impl Command for Probe {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }
    fn handle<'a>(
        &'a self,
        _: &'a Application,
        _: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            report(std::io::Error::other("artisan-recovered"))
                .await
                .map_err(|error| ConsoleError::new(error.to_string()))?;
            output.info("continued");
            Ok(0)
        })
    }
}
fn commands(artisan: &mut openrust::console::Application) -> Result<(), ConsoleError> {
    artisan.add_command(Box::new(Probe(CommandDefinition::new(clap::Command::new(
        "report:recover",
    )))))
}
#[tokio::test]
async fn report_helper_continues_artisan_and_uses_its_application_handler() {
    let root = tempfile::tempdir().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let code = Application::configure(root.path().into())
        .with_commands(commands)
        .with_exceptions(move |exceptions| {
            exceptions.report::<std::io::Error>(move |_| {
                observed.fetch_add(1, Ordering::SeqCst);
                ReportDecision::Continue
            });
        })
        .create()
        .unwrap()
        .handle_command(&[OsString::from("report:recover")])
        .await
        .unwrap();
    assert_eq!(code, 0);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let log = std::fs::read_to_string(root.path().join("storage/logs/app.log")).unwrap();
    assert!(log.contains("artisan-recovered"));
}
#[tokio::test]
async fn helper_keeps_shared_identity_fake_capture_and_application_isolation() {
    let root = tempfile::tempdir().unwrap();
    let mut app = Application::configure(root.path().into())
        .with_exceptions(|exceptions| {
            exceptions.dont_report_duplicates();
        })
        .create()
        .unwrap();
    app.boot().await.unwrap();
    App::set_facade_application(Some(&app), async {
        let fake = Exceptions::fake().unwrap();
        let error = SharedException::new(std::io::Error::other("same-instance"));
        report(error.clone()).await.unwrap();
        report(error.clone()).await.unwrap();
        fake.assert_reported_count(2);
        let retained = fake.reported();
        let first = openrust::contracts::debug::Exception::Error(&retained[0]);
        let second = openrust::contracts::debug::Exception::Error(&retained[1]);
        assert!(std::ptr::eq(
            first.downcast_ref::<std::io::Error>().unwrap(),
            second.downcast_ref::<std::io::Error>().unwrap()
        ));
        Exceptions::assert_reported::<std::io::Error>().unwrap();
    })
    .await;
    assert!(report(std::io::Error::other("outside")).await.is_err());
}

#[tokio::test]
async fn helper_obeys_real_duplicate_policy_without_a_fake() {
    let root = tempfile::tempdir().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let mut app = Application::configure(root.path().into())
        .with_exceptions(move |exceptions| {
            exceptions
                .dont_report_duplicates()
                .report::<std::io::Error>(move |_| {
                    observed.fetch_add(1, Ordering::SeqCst);
                    ReportDecision::Stop
                });
        })
        .create()
        .unwrap();
    app.boot().await.unwrap();
    App::set_facade_application(Some(&app), async {
        let error = SharedException::new(std::io::Error::other("same-instance"));
        report(error.clone()).await.unwrap();
        report(error).await.unwrap();
        report(std::io::Error::other("same-instance"))
            .await
            .unwrap();
    })
    .await;
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}
