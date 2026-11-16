mod prune_stale_tags_command;

use std::sync::Arc;

use clap::{Arg, ArgAction};

use crate::{
    cache::{
        Cache,
        events::{CacheCleared, CacheClearing},
    },
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output,
    },
    contracts::{
        cache::Repository as _,
        container::Container as _,
        events::{Dispatcher as _, Event},
        foundation::Application as _,
    },
    events::{Dispatcher, EventContext},
    foundation::Application,
};

struct CacheCommand {
    definition: CommandDefinition,
    forget: bool,
}

impl Command for CacheCommand {
    fn definition(&self) -> &CommandDefinition {
        &self.definition
    }

    fn handle<'a>(
        &'a self,
        application: &'a Application,
        input: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let input = input.matches();
            let mut tags = Vec::new();
            let mut locks = false;
            if !self.forget {
                locks = input.get_flag("locks");
                if let Some(value) = input.get_one::<String>("tags") {
                    tags = value
                        .split(',')
                        .filter(|tag| !tag.is_empty() && *tag != "0")
                        .collect();
                }
                if locks && !tags.is_empty() {
                    return Err(ConsoleError::new(
                        "Cache tags cannot be used when clearing locks.",
                    ));
                }
            }
            let name = input.get_one::<String>("store");
            let dispatch = !self.forget && !locks;
            let dispatcher = dispatch
                .then(|| application.container().make::<Dispatcher>().ok())
                .flatten();
            if let Some(dispatcher) = &dispatcher {
                notify(
                    dispatcher,
                    CacheClearing {
                        storeName: name.cloned(),
                        tags: tags.iter().map(|tag| (*tag).to_owned()).collect(),
                    },
                )
                .await?;
            }
            let store = Cache::store(name.map(String::as_str)).map_err(console_error)?;
            let cleared = if self.forget {
                let key = input.get_one::<String>("key").expect("required cache key");
                store.forget(key).await
            } else if locks {
                store.flushLocks().await
            } else if tags.is_empty() {
                store.flush().await
            } else {
                store.tags(&tags).map_err(console_error)?.flush().await
            }
            .map_err(console_error)?;
            if cleared {
                if let Some(dispatcher) = &dispatcher {
                    notify(
                        dispatcher,
                        CacheCleared {
                            storeName: name.cloned(),
                            tags: tags.iter().map(|tag| (*tag).to_owned()).collect(),
                        },
                    )
                    .await?;
                }
                output.info(if self.forget {
                    "The key has been removed from the cache."
                } else if locks {
                    "Application cache locks cleared successfully."
                } else {
                    "Application cache cleared successfully."
                });
                Ok(0)
            } else {
                output.stderr.push_str(if locks {
                    "Failed to clear cache locks.\n"
                } else {
                    "The cache entry could not be removed.\n"
                });
                Ok(1)
            }
        })
    }
}

async fn notify(dispatcher: &Dispatcher, event: impl Event) -> Result<(), ConsoleError> {
    dispatcher
        .dispatch(Arc::new(event), &mut EventContext::new())
        .await
        .map_err(console_error)?;
    Ok(())
}

fn console_error(error: impl std::fmt::Display) -> ConsoleError {
    ConsoleError::new(error.to_string())
}

pub(crate) fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    prune_stale_tags_command::register(artisan)?;
    for (name, about, forget) in [
        ("cache:clear", "Flush the application cache", false),
        ("cache:forget", "Remove an item from the cache", true),
    ] {
        let mut command = clap::Command::new(name).about(about);
        if forget {
            command = command.arg(Arg::new("key").required(true));
        } else {
            command = command
                .arg(
                    Arg::new("tags")
                        .long("tags")
                        .help("The cache tags to clear"),
                )
                .arg(
                    Arg::new("locks")
                        .long("locks")
                        .action(ArgAction::SetTrue)
                        .help("Only clear cache locks"),
                );
        }
        command = command.arg(Arg::new("store"));
        artisan.add_command(Box::new(CacheCommand {
            definition: CommandDefinition::new(command),
            forget,
        }))?;
    }
    Ok(())
}
