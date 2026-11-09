//! Passport固定四表的迁移候选；尚未注册，不是已交付迁移。
//!
//! 对照 Laravel Passport `v13.10.0` 的 `2016_06_01` 四个迁移。
//! 表、字段、nullable、索引和无默认值保持固定源码；自定义模型Schema由应用发布覆盖。
//! PostgreSQL/SQLite没有unsigned bigint，保留对应signed bigint/integer存储适配，
//! 不因此把默认 `user_id` 改为文本或 UUID；未包含后继 `device_codes` 迁移。

// 固定上游许可归因：
// The MIT License (MIT)
//
// Copyright (c) Taylor Otwell
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
// THE SOFTWARE.

use openrust::database::{DatabaseTableNames, DbBackend, sea_orm_migration::prelude::*};
use sha2::{Digest, Sha256};

const TABLES: [&str; 4] = [
    "oauth_auth_codes",
    "oauth_access_tokens",
    "oauth_refresh_tokens",
    "oauth_clients",
];
const NAMES: [&str; 4] = [
    "2016_06_01_000001_create_oauth_auth_codes_table",
    "2016_06_01_000002_create_oauth_access_tokens_table",
    "2016_06_01_000003_create_oauth_refresh_tokens_table",
    "2016_06_01_000004_create_oauth_clients_table",
];

/// 依固定上游顺序返回四个迁移候选；由应用或Provider决定实际注册与目标连接。
#[must_use]
pub fn default_passport_migrations() -> Vec<Box<dyn MigrationTrait>> {
    vec![
        Box::new(CreateOAuthAuthCodesTable),
        Box::new(CreateOAuthAccessTokensTable),
        Box::new(CreateOAuthRefreshTokensTable),
        Box::new(CreateOAuthClientsTable),
    ]
}

macro_rules! migration {
    ($name:ident, $position:expr, $description:literal) => {
        #[doc = $description]
        pub struct $name;
        impl MigrationName for $name {
            fn name(&self) -> &str {
                NAMES[$position]
            }
        }
        #[async_trait::async_trait]
        impl MigrationTrait for $name {
            async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
                let plan = plan($position, &current_names()?, manager.get_database_backend())?;
                manager.create_table(plan.create).await?;
                for index in plan.indexes {
                    manager.create_index(index).await?;
                }
                Ok(())
            }
            async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
                let plan = plan($position, &current_names()?, manager.get_database_backend())?;
                manager.drop_table(plan.drop).await
            }
        }
    };
}

migration!(
    CreateOAuthAuthCodesTable,
    0,
    "创建默认授权码表；用户主键采用 `foreignId` 对应类型。"
);
migration!(
    CreateOAuthAccessTokensTable,
    1,
    "创建默认 access 表；`user_id` 与 `name`、`scope`、期限和时间戳可空。"
);
migration!(
    CreateOAuthRefreshTokensTable,
    2,
    "创建默认 refresh 表；只建立 access 索引，不添加外键或级联。"
);
migration!(
    CreateOAuthClientsTable,
    3,
    "创建默认 UUID 客户端表；owner 默认 nullable numeric morphs。"
);

struct SchemaPlan {
    create: TableCreateStatement,
    indexes: Vec<IndexCreateStatement>,
    drop: TableDropStatement,
}

fn current_names() -> Result<DatabaseTableNames, DbErr> {
    DatabaseTableNames::current()
        .ok_or_else(|| DbErr::Migration("Passport迁移需要统一DatabaseTableNames迁移作用域".into()))
}

fn plan(
    position: usize,
    names: &DatabaseTableNames,
    backend: DbBackend,
) -> Result<SchemaPlan, DbErr> {
    let logical = TABLES
        .get(position)
        .ok_or_else(|| DbErr::Migration("Passport迁移序号无效".into()))?;
    names
        .quote(logical, backend)
        .map_err(|error| DbErr::Migration(error.to_string()))?;
    let physical = names
        .resolve(logical)
        .map_err(|error| DbErr::Migration(error.to_string()))?;
    let mut create = Table::create();
    create.table(Alias::new(&physical));
    let index_columns: Vec<Vec<&str>> = match position {
        0 => {
            create
                .col(
                    ColumnDef::new(Alias::new("id"))
                        .char_len(80)
                        .not_null()
                        .primary_key(),
                )
                .col(numeric_key("user_id", backend).not_null())
                .col(uuid_key("client_id", backend).not_null())
                .col(ColumnDef::new(Alias::new("scopes")).text().null())
                .col(revoked(backend).not_null())
                .col(date_column("expires_at", backend, false).null());
            vec![vec!["user_id"]]
        }
        1 => {
            create
                .col(
                    ColumnDef::new(Alias::new("id"))
                        .char_len(80)
                        .not_null()
                        .primary_key(),
                )
                .col(numeric_key("user_id", backend).null())
                .col(uuid_key("client_id", backend).not_null())
                .col(ColumnDef::new(Alias::new("name")).string_len(255).null())
                .col(ColumnDef::new(Alias::new("scopes")).text().null())
                .col(revoked(backend).not_null())
                .col(date_column("created_at", backend, true).null())
                .col(date_column("updated_at", backend, true).null())
                .col(date_column("expires_at", backend, false).null());
            vec![vec!["user_id"]]
        }
        2 => {
            create
                .col(
                    ColumnDef::new(Alias::new("id"))
                        .char_len(80)
                        .not_null()
                        .primary_key(),
                )
                .col(
                    ColumnDef::new(Alias::new("access_token_id"))
                        .char_len(80)
                        .not_null(),
                )
                .col(revoked(backend).not_null())
                .col(date_column("expires_at", backend, false).null());
            vec![vec!["access_token_id"]]
        }
        3 => {
            client_columns(&mut create, backend);
            vec![vec!["owner_type", "owner_id"]]
        }
        _ => return Err(DbErr::Migration("Passport迁移序号无效".into())),
    };
    let indexes = index_columns
        .into_iter()
        .map(|columns| {
            let mut index = Index::create();
            index
                .name(index_name(&physical, &columns))
                .table(Alias::new(&physical));
            for column in columns {
                index.col(Alias::new(column));
            }
            index.clone()
        })
        .collect();
    Ok(SchemaPlan {
        create: create.clone(),
        indexes,
        drop: Table::drop()
            .table(Alias::new(physical))
            .if_exists()
            .to_owned(),
    })
}

fn client_columns(create: &mut TableCreateStatement, backend: DbBackend) {
    create
        .col(uuid_key("id", backend).not_null().primary_key())
        .col(
            ColumnDef::new(Alias::new("owner_type"))
                .string_len(255)
                .null(),
        )
        .col(numeric_key("owner_id", backend).null())
        .col(
            ColumnDef::new(Alias::new("name"))
                .string_len(255)
                .not_null(),
        )
        .col(ColumnDef::new(Alias::new("secret")).string_len(255).null())
        .col(
            ColumnDef::new(Alias::new("provider"))
                .string_len(255)
                .null(),
        )
        .col(
            ColumnDef::new(Alias::new("redirect_uris"))
                .text()
                .not_null(),
        )
        .col(ColumnDef::new(Alias::new("grant_types")).text().not_null())
        .col(revoked(backend).not_null())
        .col(date_column("created_at", backend, true).null())
        .col(date_column("updated_at", backend, true).null());
}

fn numeric_key(name: &str, backend: DbBackend) -> ColumnDef {
    let mut column = ColumnDef::new(Alias::new(name));
    if backend == DbBackend::MySql {
        column.big_unsigned();
    } else {
        column.big_integer();
    }
    column
}

fn uuid_key(name: &str, backend: DbBackend) -> ColumnDef {
    let mut column = ColumnDef::new(Alias::new(name));
    if backend == DbBackend::Postgres {
        column.uuid();
    } else {
        column.char_len(36);
    }
    column
}

fn revoked(backend: DbBackend) -> ColumnDef {
    let mut column = ColumnDef::new(Alias::new("revoked"));
    // MySQL bool是tinyint(1)别名；显式使用固定Schema的物理表达，不加默认值。
    if backend == DbBackend::MySql || backend == DbBackend::Sqlite {
        column.custom(Alias::new("tinyint(1)"));
    } else {
        column.boolean();
    }
    column
}

fn date_column(name: &str, backend: DbBackend, timestamps: bool) -> ColumnDef {
    let mut column = ColumnDef::new(Alias::new(name));
    // SeaQuery 默认 SQLite *_text 会失去 datetime 声明，显式恢复 datetime affinity。
    if backend == DbBackend::Sqlite {
        column.custom(Alias::new("datetime"));
    } else if timestamps {
        column.timestamp();
    } else {
        column.date_time();
    }
    column
}

fn index_name(physical: &str, columns: &[&str]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let full = format!("{physical}_{}_index", columns.join("_"));
    if full.len() <= 63 {
        return full;
    }
    // 解析器只允许ASCII标识符；统一63字节兼容PG，并以完整名称摘要区分截断前缀。
    let digest = Sha256::digest(full.as_bytes());
    let mut suffix = String::with_capacity(16);
    for byte in &digest[..8] {
        suffix.push(char::from(HEX[usize::from(byte >> 4)]));
        suffix.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    format!("{}_{suffix}", &full[..46])
}

#[cfg(test)]
#[path = "migrations_tests.rs"]
mod tests;
