use std::{collections::BTreeSet, fmt::Write as _, sync::Arc};

use crate::{
    console::Output,
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
};

use super::{
    Connection, DatabaseError, DatabaseFuture, DatabaseManager, DatabaseResult, query::Record,
};

/// A compiled database seeder.
pub trait Seeder: Send + Sync {
    fn run<'a>(
        &'a self,
        context: &'a mut SeederContext<'_>,
    ) -> DatabaseFuture<'a, DatabaseResult<()>>;
}

/// A named seeder registered by the application.
pub struct SeederEntry {
    name: &'static str,
    seeder: &'static dyn Seeder,
}

impl SeederEntry {
    pub const fn new(name: &'static str, seeder: &'static dyn Seeder) -> Self {
        Self { name, seeder }
    }
}

/// The application's compiled seeders.
#[derive(Default)]
pub struct SeederRegistry(pub &'static [SeederEntry]);

impl SeederRegistry {
    fn find(&self, name: &str) -> DatabaseResult<&'static SeederEntry> {
        let name = normalize(name);
        let mut matches = self.0.iter().filter(|entry| normalize(entry.name) == name);
        let entry = matches
            .next()
            .ok_or_else(|| DatabaseError::UnknownSeeder(name.to_owned()))?;
        if matches.next().is_some() {
            return Err(DatabaseError::InvalidSeeder("duplicate registration"));
        }
        Ok(entry)
    }
}

pub(crate) const DEFAULT: &str = "DatabaseSeeder";

fn normalize(name: &str) -> &str {
    let name = name.trim_start_matches('\\');
    name.strip_prefix("Database\\Seeders\\").unwrap_or(name)
}

/// The application, database connection and arguments available during one seeder call.
pub struct SeederContext<'a> {
    application: &'a Application,
    registry: &'a SeederRegistry,
    connection: &'a Connection,
    output: &'a mut Output,
    called: &'a mut BTreeSet<&'static str>,
    stack: Vec<&'static str>,
    parameters: Record,
}

#[allow(non_snake_case, reason = "Matches Laravel's Seeder API")]
impl SeederContext<'_> {
    pub fn application(&self) -> &Application {
        self.application
    }

    pub fn connection(&self) -> &Connection {
        self.connection
    }

    pub fn parameters(&self) -> &Record {
        &self.parameters
    }

    pub async fn call(&mut self, names: &[&str]) -> DatabaseResult<()> {
        for name in names {
            self.invoke(name, Record::new(), false, false).await?;
        }
        Ok(())
    }

    pub async fn callSilent(&mut self, names: &[&str]) -> DatabaseResult<()> {
        for name in names {
            self.invoke(name, Record::new(), true, false).await?;
        }
        Ok(())
    }

    pub async fn callOnce(&mut self, names: &[&str]) -> DatabaseResult<()> {
        for name in names {
            self.invoke(name, Record::new(), false, true).await?;
        }
        Ok(())
    }

    pub async fn callWith(&mut self, name: &str, parameters: Record) -> DatabaseResult<()> {
        self.invoke(name, parameters, false, false).await
    }

    async fn invoke(
        &mut self,
        name: &str,
        parameters: Record,
        silent: bool,
        once: bool,
    ) -> DatabaseResult<()> {
        let entry = self.registry.find(name)?;
        let name = normalize(entry.name);
        if once && self.called.contains(name) {
            return Ok(());
        }
        if self.stack.contains(&name) {
            return Err(DatabaseError::InvalidSeeder("recursive call"));
        }
        if !silent {
            writeln!(self.output.stdout, "{} RUNNING", entry.name).expect("writing to a String");
        }
        let mut stack = self.stack.clone();
        stack.push(name);
        let mut child = SeederContext {
            application: self.application,
            registry: self.registry,
            connection: self.connection,
            output: self.output,
            called: self.called,
            stack,
            parameters,
        };
        entry
            .seeder
            .run(&mut child)
            .await
            .map_err(|error| DatabaseError::SeederFailed {
                seeder: entry.name.to_owned(),
                source: Box::new(error),
            })?;
        self.called.insert(name);
        if !silent {
            writeln!(self.output.stdout, "{} DONE", entry.name).expect("writing to a String");
        }
        Ok(())
    }
}

/// Runs the requested seeder and its children on the selected application connection.
pub(crate) async fn run(
    application: &Application,
    database: Option<&str>,
    name: Option<&str>,
    output: &mut Output,
) -> DatabaseResult<()> {
    let manager = application
        .container()
        .get::<DatabaseManager>("db")
        .map_err(DatabaseError::Unavailable)?;
    let registry = application
        .container()
        .get::<SeederRegistry>(crate::container::ServiceKey::typed::<SeederRegistry>())
        .map_err(DatabaseError::Unavailable)?;
    let name = name.unwrap_or(DEFAULT);
    let selected = match database.filter(|name| !name.is_empty()) {
        Some(database) => database.to_owned(),
        None => manager.getDefaultConnection()?,
    };
    let connection = manager.connection(Some(&selected))?;
    output.info("Seeding database.");
    let mut called = BTreeSet::new();
    let mut context = SeederContext {
        application,
        registry: Arc::as_ref(&registry),
        connection: &connection,
        output,
        called: &mut called,
        stack: Vec::new(),
        parameters: Record::new(),
    };
    manager
        .usingConnection(&selected, async {
            context
                .invoke(name, Record::new(), normalize(name) == DEFAULT, false)
                .await
        })
        .await
}
