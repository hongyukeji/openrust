use std::{path::Path, sync::Arc};

use clap::{Arg, ArgAction, Command as Syntax, value_parser};

use crate::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output,
    },
    contracts::{events::Dispatcher as _, foundation::Application as _},
    database::{
        eloquent::ModelPruners,
        events::{ModelPruningFinished, ModelPruningStarting, ModelsPruned},
    },
    events::{Dispatcher, EventContext},
    foundation::Application,
};

pub(crate) fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    let names = |name: &'static str| Arg::new(name).long(name).action(ArgAction::Append);
    let syntax = Syntax::new("model:prune")
        .about("清理符合保留期规则的模型记录")
        .arg(names("model").conflicts_with("except"))
        .arg(names("except"))
        .arg(names("path"))
        .arg(
            Arg::new("chunk")
                .long("chunk")
                .default_value("1000")
                .value_parser(value_parser!(u32).range(1..)),
        )
        .arg(
            Arg::new("pretend")
                .long("pretend")
                .action(ArgAction::SetTrue),
        );
    artisan.add_command(Box::new(PruneCommand(CommandDefinition::new(syntax))))
}

struct PruneCommand(CommandDefinition);

impl Command for PruneCommand {
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
            let matches = input.matches();
            let entries = app
                .make::<ModelPruners>()
                .await
                .map_err(ConsoleError::from_source)?
                .entries();
            let selected = if let Some(names) = matches.get_many::<String>("model") {
                names
                    .filter_map(|name| entries.iter().find(|entry| entry.name == name))
                    .copied()
                    .collect::<Vec<_>>()
            } else {
                let base = app.base_path();
                let directories = matches
                    .get_many::<String>("path")
                    .map(|paths| paths.map(|path| base.join(path)).collect::<Vec<_>>())
                    .unwrap_or_else(|| vec![base.join("app/models")]);
                let excluded = matches.get_many::<String>("except");
                entries
                    .into_iter()
                    .filter(|entry| {
                        directories.iter().any(|directory| {
                            base.join(Path::new(entry.source)).starts_with(directory)
                        }) && !excluded
                            .clone()
                            .is_some_and(|mut names| names.any(|name| name == entry.name))
                    })
                    .collect()
            };
            if selected.is_empty() {
                output.info("没有找到可清理模型");
                return Ok(0);
            }
            if matches.get_flag("pretend") {
                for entry in selected {
                    let count = entry.pruner.count(app).await?;
                    output.info(&format!("[{}] 可清理 {count} 条记录", entry.name));
                }
                return Ok(0);
            }
            let dispatcher = app
                .make::<Dispatcher>()
                .await
                .map_err(ConsoleError::from_source)?;
            let models = selected.iter().map(|entry| entry.name).collect::<Vec<_>>();
            let mut context = EventContext::new();
            dispatcher
                .dispatch(
                    Arc::new(ModelPruningStarting {
                        models: models.clone(),
                    }),
                    &mut context,
                )
                .await
                .map_err(ConsoleError::from_source)?;
            let requested = *matches.get_one::<u32>("chunk").expect("有默认批次大小");
            for entry in selected {
                let chunk = entry.pruner.chunk_size(requested);
                if chunk == 0 {
                    return Err(ConsoleError::new("模型清理批次大小必须大于零"));
                }
                let mut count = 0_u64;
                loop {
                    let removed = entry.pruner.prune_batch(app, chunk).await?;
                    if removed == 0 {
                        break;
                    }
                    count = count
                        .checked_add(removed)
                        .ok_or_else(|| ConsoleError::new("已清理记录计数溢出"))?;
                    dispatcher
                        .dispatch(
                            Arc::new(ModelsPruned {
                                model: entry.name,
                                count,
                            }),
                            &mut context,
                        )
                        .await
                        .map_err(ConsoleError::from_source)?;
                    output.info(&format!("[{}] 已清理 {count} 条记录", entry.name));
                }
                if count == 0 {
                    output.info(&format!("[{}] 无记录可清理", entry.name));
                }
            }
            dispatcher
                .dispatch(Arc::new(ModelPruningFinished { models }), &mut context)
                .await
                .map_err(ConsoleError::from_source)?;
            Ok(0)
        })
    }
}
