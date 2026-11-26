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
    without_model_events: bool,
}

impl SeederEntry {
    pub const fn new(name: &'static str, seeder: &'static dyn Seeder) -> Self {
        Self {
            name,
            seeder,
            without_model_events: false,
        }
    }

    /// Mutes Eloquent model events for this seeder and the seeders it calls.
    #[must_use]
    pub const fn without_model_events(mut self) -> Self {
        self.without_model_events = true;
        self
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
    parameters: &'a Record,
}

impl SeederContext<'_> {
    pub fn application(&self) -> &Application {
        self.application
    }

    pub fn connection(&self) -> &Connection {
        self.connection
    }

    pub fn parameters(&self) -> &Record {
        self.parameters
    }

    pub async fn call(&mut self, names: &[&str]) -> DatabaseResult<()> {
        self.call_with_many(names, Record::new()).await
    }

    pub async fn call_silent(&mut self, names: &[&str]) -> DatabaseResult<()> {
        self.call_silent_with(names, Record::new()).await
    }

    pub async fn call_once(&mut self, names: &[&str]) -> DatabaseResult<()> {
        self.call_once_with(names, false, Record::new()).await
    }

    pub async fn call_with(&mut self, name: &str, parameters: Record) -> DatabaseResult<()> {
        self.call_with_many(&[name], parameters).await
    }

    /// Runs the seeders in order with the same arguments for each.
    pub async fn call_with_many(
        &mut self,
        names: &[&str],
        parameters: Record,
    ) -> DatabaseResult<()> {
        self.call_seeders(names, &parameters, false, false).await
    }

    /// Passes arguments to each seeder while suppressing its progress lines.
    pub async fn call_silent_with(
        &mut self,
        names: &[&str],
        parameters: Record,
    ) -> DatabaseResult<()> {
        self.call_seeders(names, &parameters, true, false).await
    }

    /// Passes arguments to each seeder that has yet to complete in this invocation.
    pub async fn call_once_with(
        &mut self,
        names: &[&str],
        silent: bool,
        parameters: Record,
    ) -> DatabaseResult<()> {
        self.call_seeders(names, &parameters, silent, true).await
    }

    async fn call_seeders(
        &mut self,
        names: &[&str],
        parameters: &Record,
        silent: bool,
        once: bool,
    ) -> DatabaseResult<()> {
        for name in names {
            self.invoke(name, parameters, silent, once).await?;
        }
        Ok(())
    }

    async fn invoke(
        &mut self,
        name: &str,
        parameters: &Record,
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
        let run = async { entry.seeder.run(&mut child).await };
        let result = if entry.without_model_events {
            super::eloquent::concerns::without_events(run).await
        } else {
            run.await
        };
        result.map_err(|error| DatabaseError::SeederFailed {
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
    let manager = application.container().get::<DatabaseManager>("db")?;
    let registry = application
        .container()
        .get::<SeederRegistry>(crate::container::ServiceKey::typed::<SeederRegistry>())?;
    let name = name.unwrap_or(DEFAULT);
    let selected = match database.filter(|name| !name.is_empty()) {
        Some(database) => database.to_owned(),
        None => manager.get_default_connection()?,
    };
    let connection = manager.connection(Some(&selected))?;
    output.info("Seeding database.");
    let mut called = BTreeSet::new();
    let parameters = Record::new();
    let mut context = SeederContext {
        application,
        registry: Arc::as_ref(&registry),
        connection: &connection,
        output,
        called: &mut called,
        stack: Vec::new(),
        parameters: &parameters,
    };
    manager
        .using_connection(
            &selected,
            super::eloquent::concerns::unguarded(context.invoke(
                name,
                &parameters,
                normalize(name) == DEFAULT,
                false,
            )),
        )
        .await
}
