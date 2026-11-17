use std::{
    io,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use sqlx::{Connection as _, MySqlConnection, MySqlPool};

const MARKER: &str = "openrust_fixture_owner";
const SESSION_TIMEOUT: Duration = Duration::from_secs(5);
const CLOSE_TIMEOUT: Duration = Duration::from_secs(2);

pub(crate) struct MySqlTestOwner {
    name: String,
    token: String,
    server: String,
    session: u64,
    active: AtomicBool,
    pools: Mutex<Vec<MySqlPool>>,
}

impl MySqlTestOwner {
    pub(crate) fn active(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }

    pub(crate) fn attach(&self, pool: &MySqlPool) {
        let mut pools = self
            .pools
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if self.active() {
            pools.push(pool.clone());
        } else {
            drop(pool.close());
        }
    }

    fn deactivate(&self) -> Vec<MySqlPool> {
        self.active.store(false, Ordering::Release);
        let mut registered = self
            .pools
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let pools = std::mem::take(&mut *registered);
        for pool in &pools {
            drop(pool.close());
        }
        pools
    }

    async fn owns(&self, connection: &mut MySqlConnection) -> Result<bool, sqlx::Error> {
        let marker: Option<String> = sqlx::query_scalar(
            "SELECT ROUTINE_COMMENT FROM information_schema.ROUTINES
             WHERE BINARY ROUTINE_SCHEMA = BINARY ? AND ROUTINE_NAME = ?
             AND ROUTINE_TYPE = 'PROCEDURE' AND SECURITY_TYPE = 'INVOKER'",
        )
        .bind(&self.name)
        .bind(MARKER)
        .fetch_optional(connection)
        .await?;
        Ok(marker.as_deref() == Some(&self.token))
    }

    pub(crate) async fn verify_pool(
        &self,
        connection: &mut MySqlConnection,
    ) -> Result<(), sqlx::Error> {
        let rejected = || sqlx::Error::Protocol("test MySQL schema verification failed".into());
        if !self.active() {
            return Err(rejected());
        }
        let identity: (String, Option<String>, Option<u64>) =
            sqlx::query_as("SELECT @@server_uuid, DATABASE(), IS_USED_LOCK(?)")
                .bind(&self.name)
                .fetch_one(&mut *connection)
                .await
                .map_err(|_| rejected())?;
        if identity.0 != self.server
            || identity.1.as_deref() != Some(&self.name)
            || identity.2 != Some(self.session)
            || !self.owns(connection).await.map_err(|_| rejected())?
            || !self.active()
        {
            return Err(rejected());
        }
        Ok(())
    }
}

pub(crate) struct MySqlTestDatabase {
    pub(crate) owner: Arc<MySqlTestOwner>,
    pub(crate) url: String,
    admin: Option<MySqlConnection>,
}

impl MySqlTestDatabase {
    pub(crate) async fn create(admin_url: &str) -> io::Result<Self> {
        let mut url = url::Url::parse(admin_url)
            .ok()
            .filter(|url| url.scheme() == "mysql")
            .ok_or_else(|| io::Error::other("expected a MySQL test server URL"))?;
        let random = || {
            crate::support::Str::uuid()
                .map(|id| id.simple().to_string())
                .map_err(|_| io::Error::other("could not generate a MySQL test schema identifier"))
        };
        let name = format!("openrust_test_{}", random()?);
        let token = random()?;
        let mut admin = tokio::time::timeout(SESSION_TIMEOUT, MySqlConnection::connect(admin_url))
            .await
            .ok()
            .and_then(Result::ok)
            .ok_or_else(|| io::Error::other("could not connect to the MySQL test server"))?;
        let (server, session): (String, u64) =
            sqlx::query_as("SELECT @@server_uuid, CONNECTION_ID()")
                .fetch_one(&mut admin)
                .await
                .map_err(|_| failure(&name, "could not verify the management session"))?;

        let held: Option<i64> = sqlx::query_scalar("SELECT GET_LOCK(?, 0)")
            .bind(&name)
            .fetch_one(&mut admin)
            .await
            .map_err(|_| failure(&name, "could not acquire the lease"))?;
        if held != Some(1) {
            return Err(failure(&name, "the lease is already held"));
        }
        // Both the schema name and the marker token use generated hexadecimal identifiers.
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!("CREATE DATABASE `{name}`")))
            .execute(&mut admin)
            .await
            .map_err(|_| failure(&name, "database creation was not confirmed"))?;
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
            "CREATE PROCEDURE `{name}`.`{MARKER}`() COMMENT '{token}'
             SQL SECURITY INVOKER BEGIN END"
        )))
        .execute(&mut admin)
        .await
        .map_err(|_| failure(&name, "could not install the ownership marker"))?;
        url.set_path(&format!("/{name}"));
        Ok(Self {
            owner: Arc::new(MySqlTestOwner {
                name,
                token,
                server,
                session,
                active: AtomicBool::new(true),
                pools: Mutex::default(),
            }),
            url: url.into(),
            admin: Some(admin),
        })
    }

    pub(crate) async fn close(mut self) -> io::Result<()> {
        let pools = self.owner.deactivate();
        if let Some(admin) = self.admin.take() {
            cleanup(admin, &self.owner, pools).await?;
        }
        Ok(())
    }
}

impl Drop for MySqlTestDatabase {
    fn drop(&mut self) {
        let pools = self.owner.deactivate();
        if let Some(admin) = self.admin.take()
            && let Ok(runtime) = tokio::runtime::Handle::try_current()
        {
            let owner = self.owner.clone();
            runtime.spawn(async move {
                if let Err(error) = cleanup(admin, &owner, pools).await {
                    eprintln!("{error}");
                }
            });
        }
    }
}

async fn cleanup(
    mut admin: MySqlConnection,
    owner: &MySqlTestOwner,
    pools: Vec<MySqlPool>,
) -> io::Result<()> {
    let name = &owner.name;
    tokio::time::timeout(
        CLOSE_TIMEOUT,
        futures_util::future::join_all(pools.iter().map(MySqlPool::close)),
    )
    .await
    .map_err(|_| failure(name, "application pools are still busy"))?;
    let verify = async {
        loop {
            let (server, connection_id, holder): (String, u64, Option<u64>) =
                sqlx::query_as("SELECT @@server_uuid, CONNECTION_ID(), IS_USED_LOCK(?)")
                    .bind(name)
                    .fetch_one(&mut admin)
                    .await
                    .map_err(|_| failure(name, "could not verify the management session"))?;
            if server != owner.server
                || connection_id != owner.session
                || holder != Some(owner.session)
            {
                return Err(failure(name, "the management lease changed"));
            }
            if !owner.owns(&mut admin).await.unwrap_or(false) {
                return Err(failure(name, "the ownership marker changed"));
            }
            let lost: Option<String> = sqlx::query_scalar(
                "SELECT VARIABLE_VALUE FROM performance_schema.global_status
                 WHERE VARIABLE_NAME = 'Performance_schema_thread_instances_lost'",
            )
            .fetch_optional(&mut admin)
            .await
            .map_err(|_| failure(name, "could not verify server activity"))?;
            let (observer, users): (i64, i64) = sqlx::query_as(
                "SELECT CAST(COUNT(CASE WHEN PROCESSLIST_ID = CONNECTION_ID() THEN 1 END) AS SIGNED),
                 CAST(COUNT(CASE WHEN BINARY PROCESSLIST_DB = BINARY ?
                 AND PROCESSLIST_ID <> CONNECTION_ID() THEN 1 END) AS SIGNED)
                 FROM performance_schema.threads WHERE TYPE = 'FOREGROUND'",
            )
            .bind(name)
            .fetch_one(&mut admin)
            .await
            .map_err(|_| failure(name, "could not verify database activity"))?;
            if lost.as_deref() != Some("0") || observer != 1 {
                return Err(failure(name, "server activity information is incomplete"));
            }
            if users == 0 {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    };
    tokio::time::timeout(CLOSE_TIMEOUT, verify)
        .await
        .map_err(|_| failure(name, "database is busy or the ownership check timed out"))??;
    let drop_schema =
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!("DROP DATABASE `{name}`"))).execute(&mut admin);
    tokio::time::timeout(CLOSE_TIMEOUT, drop_schema)
        .await
        .ok()
        .and_then(Result::ok)
        .ok_or_else(|| failure(name, "database cleanup was not confirmed"))?;
    Ok(())
}

fn failure(name: &str, reason: &str) -> io::Error {
    io::Error::other(format!("MySQL test schema {name}: {reason}"))
}
