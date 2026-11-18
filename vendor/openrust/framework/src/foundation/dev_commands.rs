//! Application-scoped process registration for the development commands.

use std::{
    panic::Location,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
};

use indexmap::IndexMap;
use serde::Serialize;

use crate::{
    console::ConsoleError, contracts::container::Container as _,
    support::facades::current_container,
};

const COLORS: [&str; 6] = [
    "#93c5fd", "#c4b5fd", "#fb7185", "#fdba74", "#86efac", "#fcd34d",
];

/// A registered development process. Color changes apply to the registered entry.
#[derive(Clone)]
pub struct DevCommand {
    name: String,
    command: String,
    artisan: bool,
    source: String,
    priority: u8,
    color: Arc<Mutex<Option<String>>>,
}

impl DevCommand {
    pub const PRIORITY_DEFAULT: u8 = 0;
    pub const PRIORITY_VENDOR: u8 = 1;
    pub const PRIORITY_USERLAND: u8 = 2;

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn priority(&self) -> u8 {
        self.priority
    }

    pub fn color(self, color: impl Into<String>) -> Self {
        *self.color.lock().expect("dev command color lock poisoned") = Some(color.into());
        self
    }

    pub fn blue(self) -> Self {
        self.color(COLORS[0])
    }

    pub fn purple(self) -> Self {
        self.color(COLORS[1])
    }

    pub fn pink(self) -> Self {
        self.color(COLORS[2])
    }

    pub fn orange(self) -> Self {
        self.color(COLORS[3])
    }

    pub fn green(self) -> Self {
        self.color(COLORS[4])
    }

    pub fn yellow(self) -> Self {
        self.color(COLORS[5])
    }
}

/// The resolved development process as displayed by Artisan.
#[derive(Serialize)]
pub struct DevProcess {
    pub command: String,
    pub name: String,
    pub color: String,
    pub source: String,
    pub priority: u8,
}

/// The development process display selected by the application.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DevCommandMode {
    #[default]
    Tabs,
    Stream,
    Inline,
}

pub(super) struct RegisteredProcess {
    pub(super) definition: DevProcess,
    pub(super) artisan: bool,
}

pub(super) struct Registry {
    base_path: PathBuf,
    defaults: OnceLock<IndexMap<String, DevCommand>>,
    entries: Mutex<IndexMap<String, DevCommand>>,
    filters: Mutex<Filters>,
}

#[derive(Default)]
struct Filters {
    only: Vec<String>,
    except: Vec<String>,
    order: Vec<String>,
    without_vendor: bool,
    without_default: bool,
    timestamps: bool,
    disable_restart: bool,
    mode: DevCommandMode,
    buffer_size: Option<usize>,
    stream_buffer_size: Option<usize>,
}

impl Registry {
    pub(super) fn new(base_path: PathBuf) -> Self {
        Self {
            base_path,
            defaults: OnceLock::new(),
            entries: Mutex::default(),
            filters: Mutex::default(),
        }
    }

    fn defaults(&self) -> &IndexMap<String, DevCommand> {
        self.defaults.get_or_init(|| {
            let mut defaults = IndexMap::new();
            let mut add = |name: &str, command: String, artisan| {
                defaults.insert(
                    name.to_owned(),
                    DevCommand {
                        name: name.to_owned(),
                        command,
                        artisan,
                        source: "OpenRust Foundation".to_owned(),
                        priority: DevCommand::PRIORITY_DEFAULT,
                        color: Arc::default(),
                    },
                );
            };
            add("server", "./artisan serve".to_owned(), true);
            add(
                "queue",
                "./artisan queue:listen --tries=1 --timeout=0".to_owned(),
                true,
            );
            if self.base_path.join("package.json").is_file() {
                add("vite", format!("{} dev", self.node_prefix(false)), false);
            }
            defaults
        })
    }

    fn node_prefix(&self, exec: bool) -> &str {
        let has = |file| self.base_path.join(file).is_file();
        let (run, execute) = if has("bun.lock") || has("bun.lockb") {
            ("bun run", "bunx")
        } else if has("pnpm-lock.yaml") {
            ("pnpm run", "pnpm dlx")
        } else if has("yarn.lock") {
            ("yarn run", "yarn dlx")
        } else {
            ("npm run", "npx")
        };
        if exec { execute } else { run }
    }

    fn register(
        &self,
        command: String,
        name: &str,
        artisan: bool,
        caller: &Location<'_>,
    ) -> DevCommand {
        let caller_file = Path::new(caller.file());
        let relative = caller_file
            .strip_prefix(&self.base_path)
            .unwrap_or(caller_file);
        let vendor = relative
            .components()
            .any(|component| matches!(component.as_os_str().to_str(), Some("vendor" | "registry")));
        let entry = DevCommand {
            command,
            artisan,
            name: name.to_owned(),
            source: format!("{}:{}", relative.display(), caller.line()),
            priority: if vendor {
                DevCommand::PRIORITY_VENDOR
            } else {
                DevCommand::PRIORITY_USERLAND
            },
            color: Arc::default(),
        };
        let mut entries = self
            .entries
            .lock()
            .expect("dev command registry lock poisoned");
        if entries
            .get(name)
            .is_none_or(|other| entry.priority >= other.priority)
        {
            entries.insert(name.to_owned(), entry.clone());
        }
        entry
    }

    fn commands(&self) -> Vec<RegisteredProcess> {
        let defaults = self.defaults();
        let mut entries = defaults.clone();
        entries.extend(
            self.entries
                .lock()
                .expect("dev command registry lock poisoned")
                .clone(),
        );
        let filters = self
            .filters
            .lock()
            .expect("dev command filters lock poisoned");
        entries.retain(|name, entry| {
            (filters.only.is_empty() || filters.only.contains(name))
                && !filters.except.contains(name)
                && (entry.priority != DevCommand::PRIORITY_VENDOR || !filters.without_vendor)
                && (entry.priority != DevCommand::PRIORITY_DEFAULT || !filters.without_default)
        });
        if !filters.order.is_empty() {
            entries.sort_by(|left, _, right, _| {
                let position = |key| {
                    filters
                        .order
                        .iter()
                        .position(|name| name == key)
                        .unwrap_or(usize::MAX)
                };
                position(left).cmp(&position(right))
            });
        }

        let mut resolved: Vec<_> = entries
            .into_values()
            .map(|entry| RegisteredProcess {
                artisan: entry.artisan,
                definition: DevProcess {
                    command: entry.command,
                    name: entry.name,
                    color: entry
                        .color
                        .lock()
                        .expect("dev command color lock poisoned")
                        .clone()
                        .unwrap_or_default(),
                    source: entry.source,
                    priority: entry.priority,
                },
            })
            .collect();
        let mut recycled = COLORS.iter().cycle();
        for index in 0..resolved.len() {
            if resolved[index].definition.color.is_empty() {
                resolved[index].definition.color = COLORS
                    .iter()
                    .find(|color| {
                        !resolved
                            .iter()
                            .any(|entry| entry.definition.color == **color)
                    })
                    .unwrap_or_else(|| recycled.next().expect("non-empty color palette"))
                    .to_string();
            }
        }
        resolved
    }
}

/// Registers processes for the current application, typically from a service provider.
#[allow(non_snake_case, reason = "Preserves Laravel development command names")]
pub struct DevCommands;

#[allow(non_snake_case, reason = "Preserves Laravel development command names")]
impl DevCommands {
    #[track_caller]
    pub fn register(command: &str, name: Option<&str>) -> Result<DevCommand, ConsoleError> {
        Ok(active_registry()?.register(
            command.to_owned(),
            name.unwrap_or_else(|| name_from(command)),
            false,
            Location::caller(),
        ))
    }

    #[track_caller]
    pub fn artisan(command: &str, name: Option<&str>) -> Result<DevCommand, ConsoleError> {
        Ok(active_registry()?.register(
            format!("./artisan {command}"),
            name.unwrap_or_else(|| name_from(command)),
            true,
            Location::caller(),
        ))
    }

    #[track_caller]
    pub fn node(command: &str, name: Option<&str>) -> Result<DevCommand, ConsoleError> {
        Self::register_node(command, name, false)
    }

    #[track_caller]
    pub fn nodeExec(command: &str, name: Option<&str>) -> Result<DevCommand, ConsoleError> {
        Self::register_node(command, name, true)
    }

    #[track_caller]
    fn register_node(
        command: &str,
        name: Option<&str>,
        exec: bool,
    ) -> Result<DevCommand, ConsoleError> {
        let registry = active_registry()?;
        Ok(registry.register(
            format!("{} {command}", registry.node_prefix(exec)),
            name.unwrap_or_else(|| name_from(command)),
            false,
            Location::caller(),
        ))
    }

    pub fn commands() -> Result<Vec<DevProcess>, ConsoleError> {
        Ok(Self::registered()?
            .into_iter()
            .map(|process| process.definition)
            .collect())
    }

    pub(super) fn registered() -> Result<Vec<RegisteredProcess>, ConsoleError> {
        Ok(active_registry()?.commands())
    }

    pub fn only(names: &[&str]) -> Result<(), ConsoleError> {
        Self::filter(|filters| filters.only = names.iter().map(|name| (*name).to_owned()).collect())
    }

    pub fn except(names: &[&str]) -> Result<(), ConsoleError> {
        Self::filter(|filters| {
            filters.except = names.iter().map(|name| (*name).to_owned()).collect()
        })
    }

    pub fn order(names: &[&str]) -> Result<(), ConsoleError> {
        Self::filter(|filters| {
            filters.order = names.iter().map(|name| (*name).to_owned()).collect()
        })
    }

    pub fn withoutVendorCommands() -> Result<(), ConsoleError> {
        Self::filter(|filters| filters.without_vendor = true)
    }

    pub fn withoutDefaultCommands() -> Result<(), ConsoleError> {
        Self::filter(|filters| filters.without_default = true)
    }

    pub fn withTimestamps() -> Result<(), ConsoleError> {
        Self::filter(|filters| filters.timestamps = true)
    }

    pub fn disableAutoRestart() -> Result<(), ConsoleError> {
        Self::filter(|filters| filters.disable_restart = true)
    }

    pub fn shouldIncludeTimestamps() -> Result<bool, ConsoleError> {
        Ok(active_registry()?
            .filters
            .lock()
            .expect("dev command filters lock poisoned")
            .timestamps)
    }

    pub fn shouldAutoRestart() -> Result<bool, ConsoleError> {
        Ok(!active_registry()?
            .filters
            .lock()
            .expect("dev command filters lock poisoned")
            .disable_restart)
    }

    pub fn inline() -> Result<(), ConsoleError> {
        Self::filter(|filters| filters.mode = DevCommandMode::Inline)
    }

    pub fn stream() -> Result<(), ConsoleError> {
        Self::filter(|filters| filters.mode = DevCommandMode::Stream)
    }

    pub fn tabs() -> Result<(), ConsoleError> {
        Self::filter(|filters| filters.mode = DevCommandMode::Tabs)
    }

    pub fn mode() -> Result<DevCommandMode, ConsoleError> {
        Self::setting(|filters| filters.mode)
    }

    pub fn bufferSize(lines: usize) -> Result<(), ConsoleError> {
        Self::filter(|filters| filters.buffer_size = Some(lines))
    }

    pub fn streamBufferSize(lines: usize) -> Result<(), ConsoleError> {
        Self::filter(|filters| filters.stream_buffer_size = Some(lines))
    }

    pub fn getBufferSize() -> Result<Option<usize>, ConsoleError> {
        Self::setting(|filters| filters.buffer_size)
    }

    pub fn getStreamBufferSize() -> Result<Option<usize>, ConsoleError> {
        Self::setting(|filters| filters.stream_buffer_size)
    }

    fn setting<T>(read: impl FnOnce(&Filters) -> T) -> Result<T, ConsoleError> {
        Ok(read(
            &active_registry()?
                .filters
                .lock()
                .expect("dev command filters lock poisoned"),
        ))
    }

    fn filter(configure: impl FnOnce(&mut Filters)) -> Result<(), ConsoleError> {
        configure(
            &mut active_registry()?
                .filters
                .lock()
                .expect("dev command filters lock poisoned"),
        );
        Ok(())
    }
}

fn active_registry() -> Result<Arc<Registry>, ConsoleError> {
    current_container()
        .and_then(|container| container.make::<Registry>().ok())
        .ok_or_else(|| ConsoleError::new("No active development command registry."))
}

fn name_from(command: &str) -> &str {
    command.split_once(' ').map_or(command, |(name, _)| name)
}
