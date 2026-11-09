//! 受保护命令与普通贡献共享同一应用命令表。

use super::CommandRegistry;
use crate::console::Command;
use crate::foundation::{Application, BootContext, ProviderBuilder};
use crate::{BoxFuture, Result};

struct Fixture(&'static str, &'static str);

impl Command for Fixture {
    fn name(&self) -> &'static str {
        self.0
    }

    fn about(&self) -> &'static str {
        self.1
    }

    fn run<'a>(&'a self, _: &'a Application, _: Vec<String>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            if self.1 == "保留" {
                Ok(())
            } else {
                Err(crate::Error::custom("fixture_command_not_reserved", self.1))
            }
        })
    }
}

struct Contribution<const STAGE: u8>(fn(&mut BootContext<'_>));

impl<const STAGE: u8> crate::support::ServiceProvider for Contribution<STAGE> {
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            (self.0)(app);
            Ok(())
        })
    }
}

#[tokio::test]
async fn reserved_winner_survives_late_providers_and_ordinary_commands_replace() -> Result<()> {
    let mut builder = ProviderBuilder::new();
    builder.provider(Contribution::<0>(|boot| {
        boot.command(Fixture("build", "先到普通对象"));
        boot.command(Fixture("another", "普通原值"));
    }));
    builder.provider(Contribution::<1>(|boot| {
        boot.reserve_command(Fixture("build", "保留"));
        boot.command(Fixture("build", "后到普通对象"));
        boot.reserve_command(Fixture("build", "后到保留对象"));
        boot.command(Fixture("another", "普通替换"));
    }));
    let mut app =
        Application::from_provider_runtime(builder.start(figment::Figment::new()).await?)?;
    app.register_provider(Contribution::<2>(|boot| {
        boot.command(Fixture("build", "运行期覆盖"));
    }))
    .await?;
    let commands = app
        .commands()
        .map(|entry| (entry.name(), entry.about()))
        .collect::<Vec<_>>();
    assert_eq!(commands, [("build", "保留"), ("another", "普通替换")]);
    app.run_command("build", vec![]).await?;
    Ok(())
}

#[test]
fn taking_entries_clears_the_reservation_in_all_shared_handles() {
    let registry = CommandRegistry::default();
    let shared = registry.clone();
    registry.reserve(Fixture("build", "保留"));
    let removed = shared.take_entries();
    assert_eq!(removed[0].about(), "保留");
    assert!(registry.snapshot().is_empty());

    shared.register(Fixture("build", "重新登记"));
    assert_eq!(registry.snapshot()[0].about(), "重新登记");
}
