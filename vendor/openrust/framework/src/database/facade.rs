use crate::{container::ServiceKey, support::facades::Facade};

use super::{Connection, DatabaseError, DatabaseManager, DatabaseResult, Row, Value};

/// Executes queries on the current application's database connections.
pub struct DB;

impl Facade for DB {
    type Service = DatabaseManager;

    fn getFacadeAccessor() -> ServiceKey {
        "db".into()
    }
}

macro_rules! statements {
    ($($method:ident -> $result:ty;)+) => {
        $(pub async fn $method(sql: &str, bindings: &[Value]) -> DatabaseResult<$result> {
            Self::connection(None)?.$method(sql, bindings).await
        })+
    };
}

#[allow(non_snake_case, reason = "Matches Laravel's DB Facade API")]
impl DB {
    pub fn connection(name: Option<&str>) -> DatabaseResult<Connection> {
        Self::getFacadeRoot()
            .map_err(DatabaseError::Unavailable)?
            .connection(name)
    }

    statements! {
        select -> Vec<Row>;
        selectOne -> Option<Row>;
        scalar -> Value;
        insert -> bool;
        update -> u64;
        delete -> u64;
        statement -> bool;
        affectingStatement -> u64;
    }

    pub async fn unprepared(sql: &str) -> DatabaseResult<bool> {
        Self::connection(None)?.unprepared(sql).await
    }
}
