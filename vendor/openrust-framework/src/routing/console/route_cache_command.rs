use crate::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output,
    },
    foundation::Application,
    routing::route_cache::RouteCache,
};

struct RouteCacheCommand {
    definition: CommandDefinition,
    clear: bool,
}

impl Command for RouteCacheCommand {
    fn definition(&self) -> &CommandDefinition {
        &self.definition
    }

    fn handle<'a>(
        &'a self,
        app: &'a Application,
        _: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let path = app
                .get_cached_routes_path()
                .ok_or_else(|| ConsoleError::new("Route cache path is unavailable"))?;
            if self.clear {
                RouteCache::clear(path).map_err(error)?;
                output.info("Route cache cleared successfully.");
            } else {
                app.compile_route_cache()
                    .and_then(|cache| cache.save(path))
                    .map_err(error)?;
                output.info("Routes cached successfully.");
            }
            Ok(0)
        })
    }
}

fn error(source: std::io::Error) -> ConsoleError {
    ConsoleError::new(source.to_string())
}

pub(crate) fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    for (name, description, clear) in [
        ("route:cache", "Cache the route matching index", false),
        ("route:clear", "Remove the route matching index", true),
    ] {
        artisan.add_command(Box::new(RouteCacheCommand {
            definition: CommandDefinition::new(clap::Command::new(name).about(description)),
            clear,
        }))?;
    }
    Ok(())
}
