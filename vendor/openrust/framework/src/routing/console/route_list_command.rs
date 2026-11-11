use std::fmt::Write as _;

use clap::{Arg, ArgAction};

use crate::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output,
    },
    foundation::Application,
    routing::RouteInfo,
};

struct RouteListCommand(CommandDefinition);

impl Command for RouteListCommand {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }

    fn handle<'a>(
        &'a self,
        application: &'a Application,
        input: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let input = input.matches();
            let filter = |name| input.get_one::<String>(name).map(String::as_str);
            let mut routes = application.route_info();
            let method = filter("method").map(str::to_ascii_uppercase);
            routes.retain(|route| {
                method
                    .as_deref()
                    .is_none_or(|method| route.method.contains(method))
                    && filter("name")
                        .is_none_or(|name| route.name.as_deref().unwrap_or_default().contains(name))
                    && filter("domain").is_none_or(|domain| {
                        route.domain.as_deref().unwrap_or_default().contains(domain)
                    })
                    && filter("action").is_none_or(|action| route.action.contains(action))
                    && filter("path").is_none_or(|path| route.uri.contains(path))
                    && filter("except-path")
                        .is_none_or(|paths| !paths.split(',').any(|path| route.uri.contains(path)))
                    && filter("middleware").is_none_or(|name| {
                        route
                            .middleware
                            .iter()
                            .any(|middleware| middleware.contains(name))
                    })
                    && (!input.get_flag("except-vendor") || !route.vendor)
                    && (!input.get_flag("only-vendor") || route.vendor)
            });
            let sort = filter("sort").unwrap_or("uri");
            if sort != "definition" {
                routes.sort_by(|left, right| {
                    sort.split(',')
                        .map(|column| compare(column, left, right))
                        .find(|ordering| !ordering.is_eq())
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
            }
            if input.get_flag("reverse") {
                routes.reverse();
            }
            if input.get_flag("json") {
                let json = serde_json::to_string(&routes)
                    .map_err(|error| ConsoleError::new(error.to_string()))?;
                writeln!(output.stdout, "{json}").unwrap();
            } else {
                for route in &routes {
                    writeln!(
                        output.stdout,
                        "{:12} {}{} {}",
                        route.method,
                        route
                            .domain
                            .as_deref()
                            .map_or(String::new(), |domain| format!("{domain}/")),
                        route.uri,
                        route.name.as_deref().unwrap_or_default(),
                    )
                    .unwrap();
                }
                writeln!(output.stdout, "Showing [{}] routes", routes.len()).unwrap();
            }
            Ok(0)
        })
    }
}

fn compare(column: &str, left: &RouteInfo, right: &RouteInfo) -> std::cmp::Ordering {
    match column {
        "domain" => left.domain.cmp(&right.domain),
        "method" => left.method.cmp(&right.method),
        "name" => left.name.cmp(&right.name),
        "action" => left.action.cmp(&right.action),
        "middleware" => left.middleware.cmp(&right.middleware),
        _ => left.uri.cmp(&right.uri),
    }
}

fn sort_columns(value: &str) -> Result<String, String> {
    if value == "definition"
        || value.split(',').all(|column| {
            matches!(
                column,
                "domain" | "method" | "uri" | "name" | "action" | "middleware"
            )
        })
    {
        Ok(value.to_owned())
    } else {
        Err("Unknown route sort column.".to_owned())
    }
}

pub(crate) fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    let mut command = clap::Command::new("route:list")
        .about("List all registered routes")
        .arg(Arg::new("json").long("json").action(ArgAction::SetTrue))
        .arg(
            Arg::new("reverse")
                .short('r')
                .long("reverse")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("sort")
                .long("sort")
                .default_value("uri")
                .value_parser(sort_columns),
        );
    for name in ["except-vendor", "only-vendor"] {
        command = command.arg(Arg::new(name).long(name).action(ArgAction::SetTrue));
    }
    for name in [
        "method",
        "action",
        "name",
        "domain",
        "path",
        "except-path",
        "middleware",
    ] {
        command = command.arg(Arg::new(name).long(name));
    }
    artisan.add_command(Box::new(RouteListCommand(CommandDefinition::new(command))))
}
