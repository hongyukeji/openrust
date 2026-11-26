use openrust::{
    console::{Command, CommandDefinition, ConsoleError, ConsoleFuture, Input, Output},
    database::eloquent::Model,
    foundation::Application,
    support::facades::{Artisan, Lang},
};
use std::marker::PhantomData;

pub(crate) fn register<M: Model>() -> Result<(), ConsoleError> {
    Artisan::add(Box::new(Prune::<M> {
        definition: CommandDefinition::new(
            clap::Command::new("auth:prune-challenges")
                .about("清理过期的用户认证挑战")
                .arg(
                    clap::Arg::new("hours")
                        .long("hours")
                        .default_value("24")
                        .value_parser(clap::value_parser!(i64).range(0..))
                        .help("保留过期记录的小时数，0表示立即清理过期记录"),
                ),
        ),
        model: PhantomData,
    }))
}

struct Prune<M> {
    definition: CommandDefinition,
    model: PhantomData<fn() -> M>,
}
impl<M: Model> Command for Prune<M> {
    fn definition(&self) -> &CommandDefinition {
        &self.definition
    }
    fn handle<'a>(
        &'a self,
        _app: &'a Application,
        input: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let hours = *input
                .matches()
                .get_one::<i64>("hours")
                .expect("选项提供默认值");
            let retention = hours
                .checked_mul(3600)
                .ok_or_else(|| ConsoleError::new("认证证明保留期限超出支持范围"))?;
            let count = crate::starter::prune_challenges_for::<M>(retention)
                .await
                .map_err(ConsoleError::from_source)?;
            let message = Lang::get(
                "user.challenges_pruned",
                &[("count".into(), count.to_string())].into(),
                None,
            )
            .map_err(ConsoleError::from_source)?;
            output.info(&message);
            Ok(0)
        })
    }
}
