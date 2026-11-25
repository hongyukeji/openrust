use openrust::{
    console::{Command, CommandDefinition, ConsoleError, ConsoleFuture, Input, Output},
    foundation::Application,
    support::facades::{Artisan, Lang},
};
pub(crate) fn register() -> Result<(), ConsoleError> {
    Artisan::add(Box::new(Prune(CommandDefinition::new(
        clap::Command::new("auth:prune-user-cleanups")
            .about("清理已完成且收据已过期的用户资源清理记录")
            .arg(
                clap::Arg::new("database")
                    .long("database")
                    .help("记录所属数据库连接，默认取用户清理队列的数据库"),
            ),
    ))))
}
struct Prune(CommandDefinition);
impl Command for Prune {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }
    fn handle<'a>(
        &'a self,
        app: &'a Application,
        input: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let service = app
                .make::<crate::starter::resource_cleanup::ResourceCleanup>()
                .await
                .map_err(ConsoleError::from_source)?;
            let count = service
                .prune_completed(
                    input
                        .matches()
                        .get_one::<String>("database")
                        .map(String::as_str),
                )
                .await
                .map_err(ConsoleError::from_source)?;
            let message = Lang::get(
                "user.resources_pruned",
                &[("count".into(), count.to_string())].into(),
                None,
            )
            .map_err(ConsoleError::from_source)?;
            output.info(&message);
            Ok(0)
        })
    }
}
