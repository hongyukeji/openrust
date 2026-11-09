//! 完整帮助与默认用法的控制台输出边界。

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::{Command, command_help};
use crate::foundation::{Application, BootContext, ProviderBuilder};
use crate::{BoxFuture, Result};

struct HelpFixture(Option<&'static str>, Arc<AtomicUsize>);

impl Command for HelpFixture {
    fn name(&self) -> &'static str {
        "fixture"
    }

    fn about(&self) -> &'static str {
        "命令说明"
    }

    fn usage(&self) -> Option<&'static str> {
        Some("[--default]")
    }

    fn render_help(&self) -> Option<String> {
        self.1.fetch_add(1, Ordering::Relaxed);
        self.0.map(str::to_owned)
    }

    fn run<'a>(&'a self, _: &'a Application, _: Vec<String>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async {
            Err(crate::Error::custom(
                "fixture_executed",
                "帮助不得执行目标命令",
            ))
        })
    }
}

#[test]
fn custom_help_takes_precedence_and_the_output_adds_only_its_own_newline() {
    let renders = Arc::new(AtomicUsize::new(0));
    let fixture = |help| HelpFixture(help, Arc::clone(&renders));
    assert_eq!(
        command_help(&fixture(None)),
        "命令说明\n\n用法\n  fixture [--default]"
    );
    for input in ["完整帮助", "完整帮助\n"] {
        assert_eq!(command_help(&fixture(Some(input))), "完整帮助");
    }
    assert_eq!(command_help(&fixture(Some("完整帮助\n\n"))), "完整帮助\n");
}

struct HelpProvider(Arc<AtomicUsize>);

impl crate::support::ServiceProvider for HelpProvider {
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.command(super::HelpCommand);
            app.command(HelpFixture(
                Some("注册对象自己的帮助\n"),
                Arc::clone(&self.0),
            ));
            Ok(())
        })
    }
}

#[tokio::test]
async fn both_help_routes_use_the_registered_target_without_executing_it() -> Result<()> {
    let renders = Arc::new(AtomicUsize::new(0));
    let mut builder = ProviderBuilder::new();
    builder.provider(HelpProvider(Arc::clone(&renders)));
    let app = Application::from_provider_runtime(builder.start(figment::Figment::new()).await?)?;
    app.run_command("help", vec!["fixture".into()]).await?;
    app.run_command("fixture", vec!["--help".into()]).await?;
    assert_eq!(renders.load(Ordering::Relaxed), 2);
    assert!(app.run_command("fixture", vec![]).await.is_err());
    Ok(())
}
