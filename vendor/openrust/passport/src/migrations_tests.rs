//! 三后端静态SQL结构与候选回滚顺序；未证明数据库或实际Migrator执行通过。

use super::*;
use std::collections::BTreeMap;

macro_rules! sql {
    ($statement:expr, $backend:expr) => {
        match $backend {
            DbBackend::MySql => $statement.to_string(MysqlQueryBuilder),
            DbBackend::Postgres => $statement.to_string(PostgresQueryBuilder),
            DbBackend::Sqlite => $statement.to_string(SqliteQueryBuilder),
            _ => return Err(DbErr::Migration("静态SQL测试不支持此数据库后端".into())),
        }
        .replace('`', "\"")
        .to_lowercase()
    };
}

#[test]
fn four_migration_names_preserve_upstream_order_and_reverse_drop_order() -> Result<(), DbErr> {
    let migrations = default_passport_migrations();
    assert_eq!(
        migrations
            .iter()
            .map(|migration| migration.name())
            .collect::<Vec<_>>(),
        NAMES
    );
    assert_eq!(
        migrations
            .iter()
            .rev()
            .map(|migration| migration.name())
            .collect::<Vec<_>>(),
        [NAMES[3], NAMES[2], NAMES[1], NAMES[0]]
    );
    for backend in [DbBackend::MySql, DbBackend::Postgres, DbBackend::Sqlite] {
        let drops = (0..4)
            .rev()
            .map(|position| {
                let plan = plan(position, &DatabaseTableNames::default(), backend)?;
                Ok(sql!(plan.drop, backend))
            })
            .collect::<Result<Vec<_>, DbErr>>()?;
        assert_eq!(
            drops,
            [
                "oauth_clients",
                "oauth_refresh_tokens",
                "oauth_access_tokens",
                "oauth_auth_codes"
            ]
            .map(|table| format!("drop table if exists \"{table}\""))
        );
    }
    Ok(())
}

#[test]
fn all_backends_keep_nullable_defaults_indexes_and_official_column_sets() -> Result<(), DbErr> {
    for backend in [DbBackend::MySql, DbBackend::Postgres, DbBackend::Sqlite] {
        let uuid = if backend == DbBackend::Postgres {
            "uuid"
        } else {
            "char(36)"
        };
        let numeric = match backend {
            DbBackend::MySql => "bigint unsigned",
            DbBackend::Postgres => "bigint",
            _ => "integer",
        };
        let boolean = if backend == DbBackend::Postgres {
            "bool"
        } else {
            "tinyint(1)"
        };
        let expires = match backend {
            DbBackend::Postgres => "timestamp without time zone",
            _ => "datetime",
        };
        let timestamps = if backend == DbBackend::Sqlite {
            "datetime"
        } else {
            "timestamp"
        };
        for position in 0..4 {
            let plan = plan(position, &DatabaseTableNames::default(), backend)?;
            let ddl = sql!(plan.create, backend);
            assert!(!ddl.contains("if not exists"));
            assert!(!ddl.contains("default"));
            assert!(!ddl.contains("foreign key"));
            assert!(!ddl.contains("cascade"));
            assert!(!ddl.contains("auto_increment"));
            assert!(!ddl.contains("autoincrement"));
            assert_eq!(plan.indexes.len(), 1);
            assert!(
                ddl.contains(&format!("\"revoked\" {boolean} not null")),
                "{ddl}"
            );
            if position < 3 {
                assert!(
                    ddl.contains("\"id\" char(80) not null primary key"),
                    "{ddl}"
                );
                assert!(
                    ddl.contains(&format!("\"expires_at\" {expires} null")),
                    "{ddl}"
                );
            } else {
                assert!(
                    ddl.contains(&format!("\"id\" {uuid} not null primary key")),
                    "{ddl}"
                );
            }
            let index = sql!(plan.indexes[0], backend);
            assert_specific_shape(position, &ddl, &index, numeric, uuid)?;
            if position == 1 || position == 3 {
                for field in ["created_at", "updated_at"] {
                    assert!(
                        ddl.contains(&format!("\"{field}\" {timestamps} null")),
                        "{ddl}"
                    );
                }
            }
            assert!(!index.contains("unique"));
        }
    }
    Ok(())
}

fn assert_specific_shape(
    position: usize,
    ddl: &str,
    index: &str,
    numeric: &str,
    uuid: &str,
) -> Result<(), DbErr> {
    match position {
        0 | 1 => {
            let nullable = if position == 0 { "not null" } else { "null" };
            assert!(
                ddl.contains(&format!("\"user_id\" {numeric} {nullable}")),
                "{ddl}"
            );
            assert!(
                ddl.contains(&format!("\"client_id\" {uuid} not null")),
                "{ddl}"
            );
            assert!(ddl.contains("\"scopes\" text null"), "{ddl}");
            assert!(index.contains("(\"user_id\")"), "{index}");
            if position == 0 {
                assert!(!ddl.contains("created_at"));
                assert!(!ddl.contains("updated_at"));
                assert!(!ddl.contains("\"name\""));
            } else {
                assert!(ddl.contains("\"name\" varchar(255) null"), "{ddl}");
            }
        }
        2 => {
            assert!(
                ddl.contains("\"access_token_id\" char(80) not null"),
                "{ddl}"
            );
            assert!(index.contains("(\"access_token_id\")"), "{index}");
            for absent in ["user_id", "client_id", "scopes", "created_at", "updated_at"] {
                assert!(!ddl.contains(absent), "{ddl}");
            }
        }
        3 => {
            assert!(ddl.contains("\"owner_type\" varchar(255) null"), "{ddl}");
            assert!(
                ddl.contains(&format!("\"owner_id\" {numeric} null")),
                "{ddl}"
            );
            assert!(ddl.contains("\"name\" varchar(255) not null"), "{ddl}");
            for nullable in ["secret", "provider"] {
                assert!(
                    ddl.contains(&format!("\"{nullable}\" varchar(255) null")),
                    "{ddl}"
                );
            }
            for required in ["redirect_uris", "grant_types"] {
                assert!(
                    ddl.contains(&format!("\"{required}\" text not null")),
                    "{ddl}"
                );
            }
            assert!(index.contains("(\"owner_type\", \"owner_id\")"), "{index}");
            assert!(!ddl.contains("expires_at"));
        }
        _ => return Err(DbErr::Migration("静态四表序号无效".into())),
    }
    Ok(())
}

#[test]
fn resolved_physical_names_and_length_safe_indexes_share_one_mapping() -> Result<(), DbErr> {
    let names = DatabaseTableNames::new(
        "app_",
        BTreeMap::from([("oauth_clients".into(), "selected_clients".into())]),
    )
    .map_err(|error| DbErr::Migration(error.to_string()))?;
    for backend in [DbBackend::MySql, DbBackend::Postgres, DbBackend::Sqlite] {
        let client = plan(3, &names, backend)?;
        assert!(sql!(client.create, backend).contains("\"selected_clients\""));
        assert!(
            sql!(client.indexes[0], backend)
                .contains("\"selected_clients_owner_type_owner_id_index\"")
        );
        assert!(sql!(client.drop, backend).contains("\"selected_clients\""));
        let code = plan(0, &names, backend)?;
        assert!(sql!(code.create, backend).contains("\"app_oauth_auth_codes\""));
        assert!(sql!(code.indexes[0], backend).contains("\"app_oauth_auth_codes_user_id_index\""));
    }
    let first = format!("{}a", "x".repeat(62));
    let second = format!("{}b", "x".repeat(62));
    let first_index = index_name(&first, &["owner_type", "owner_id"]);
    let second_index = index_name(&second, &["owner_type", "owner_id"]);
    assert_eq!(first_index.len(), 63);
    assert_eq!(first_index, index_name(&first, &["owner_type", "owner_id"]));
    assert_ne!(first_index, second_index);
    assert!(
        first_index
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    );
    assert_eq!(
        index_name("oauth_clients", &["owner_type", "owner_id"]),
        "oauth_clients_owner_type_owner_id_index"
    );
    Ok(())
}

#[test]
fn postgres_rejects_overlong_physical_table_instead_of_silent_truncation() -> Result<(), DbErr> {
    let names = DatabaseTableNames::new(
        "",
        BTreeMap::from([("oauth_clients".into(), "x".repeat(64))]),
    )
    .map_err(|error| DbErr::Migration(error.to_string()))?;
    assert!(plan(3, &names, DbBackend::Postgres).is_err());
    assert!(plan(3, &names, DbBackend::MySql).is_ok());
    Ok(())
}

#[cfg(feature = "db-mysql")]
#[path = "migrations_mysql_tests.rs"]
mod mysql;
