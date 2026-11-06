//! 旧版 SQLite 的删列与放宽非空表重建；对应固定 Laravel `SQLiteGrammar::compileAlter`。

use sea_orm::DbErr;
#[cfg(feature = "db-sqlite")]
use sea_orm::RuntimeErr;
#[cfg(feature = "db-sqlite")]
use sqlx::{Row, SqliteConnection, SqlitePool, pool::PoolConnection};
#[cfg(feature = "db-sqlite")]
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(feature = "db-sqlite")]
static NEXT_TABLE: AtomicU64 = AtomicU64::new(0);

#[cfg(feature = "db-sqlite")]
fn database_error(error: sqlx::Error) -> DbErr {
    DbErr::Exec(RuntimeErr::SqlxError(error.into()))
}

fn unsupported() -> DbErr {
    DbErr::Custom("SQLite table definition cannot be rebuilt safely".into())
}

fn identifier(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

#[cfg(feature = "db-sqlite")]
// 取消 future 时不能把 FK 已关闭或事务状态未知的连接送回原池。
struct PhysicalOwner(Option<PoolConnection<sqlx::Sqlite>>);

#[cfg(feature = "db-sqlite")]
impl PhysicalOwner {
    fn connection(&mut self) -> Result<&mut SqliteConnection, DbErr> {
        self.0.as_deref_mut().ok_or_else(unsupported)
    }

    fn release(mut self) {
        drop(self.0.take());
    }

    #[cfg(feature = "db-sqlite")]
    async fn close(mut self) -> Result<(), DbErr> {
        if let Some(connection) = self.0.take() {
            connection.close().await.map_err(database_error)?;
        }
        Ok(())
    }
}

#[cfg(feature = "db-sqlite")]
impl Drop for PhysicalOwner {
    fn drop(&mut self) {
        if let Some(connection) = &mut self.0 {
            connection.close_on_drop();
        }
    }
}

#[cfg(feature = "db-sqlite")]
async fn execute(connection: &mut SqliteConnection, sql: &str) -> Result<(), DbErr> {
    sqlx::query(sqlx::AssertSqlSafe(sql))
        .execute(connection)
        .await
        .map(|_| ())
        .map_err(database_error)
}

#[cfg(feature = "db-sqlite")]
async fn restore_foreign_keys(
    connection: &mut SqliteConnection,
    enabled: i64,
) -> Result<(), DbErr> {
    execute(
        connection,
        if enabled == 0 {
            "PRAGMA foreign_keys=OFF"
        } else {
            "PRAGMA foreign_keys=ON"
        },
    )
    .await?;
    let actual: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
        .fetch_one(connection)
        .await
        .map_err(database_error)?;
    if actual != enabled {
        return Err(DbErr::Custom(
            "SQLite foreign key state could not be restored".into(),
        ));
    }
    Ok(())
}

#[cfg(feature = "db-sqlite")]
pub(in crate::database) async fn drop_column(
    pool: SqlitePool,
    table: String,
    column: String,
) -> Result<(), DbErr> {
    let temporary = format!(
        "__openrust_rebuild_{}_{}",
        std::process::id(),
        NEXT_TABLE.fetch_add(1, Ordering::Relaxed)
    );
    drop_column_named(pool, table, column, temporary).await
}

#[cfg(feature = "db-sqlite")]
#[derive(Clone, Copy)]
enum RebuildOperation {
    DropColumn,
    MakeNullable,
    MakeNotNullable,
}

#[cfg(feature = "db-sqlite")]
pub(in crate::database) async fn make_column_nullable(
    pool: SqlitePool,
    table: String,
    column: String,
) -> Result<(), DbErr> {
    let temporary = format!(
        "__openrust_rebuild_{}_{}",
        std::process::id(),
        NEXT_TABLE.fetch_add(1, Ordering::Relaxed)
    );
    rebuild_named(
        pool,
        table,
        column,
        temporary,
        RebuildOperation::MakeNullable,
    )
    .await
}

#[cfg(feature = "db-sqlite")]
pub(in crate::database) async fn make_column_not_nullable(
    pool: SqlitePool,
    table: String,
    column: String,
) -> Result<(), DbErr> {
    let temporary = format!(
        "__openrust_rebuild_{}_{}",
        std::process::id(),
        NEXT_TABLE.fetch_add(1, Ordering::Relaxed)
    );
    rebuild_named(
        pool,
        table,
        column,
        temporary,
        RebuildOperation::MakeNotNullable,
    )
    .await
}

#[cfg(feature = "db-sqlite")]
async fn drop_column_named(
    pool: SqlitePool,
    table: String,
    column: String,
    temporary: String,
) -> Result<(), DbErr> {
    rebuild_named(pool, table, column, temporary, RebuildOperation::DropColumn).await
}

#[cfg(feature = "db-sqlite")]
async fn rebuild_named(
    pool: SqlitePool,
    table: String,
    column: String,
    temporary: String,
    operation: RebuildOperation,
) -> Result<(), DbErr> {
    let mut owner = PhysicalOwner(Some(pool.acquire().await.map_err(database_error)?));
    let enabled: i64 = match sqlx::query_scalar("PRAGMA foreign_keys")
        .fetch_one(owner.connection()?)
        .await
    {
        Ok(enabled) => enabled,
        Err(error) => {
            owner.close().await?;
            return Err(database_error(error));
        }
    };
    let setup = async {
        execute(owner.connection()?, "PRAGMA foreign_keys=OFF").await?;
        let actual: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
            .fetch_one(owner.connection()?)
            .await
            .map_err(database_error)?;
        if actual != 0 {
            return Err(unsupported());
        }
        execute(owner.connection()?, "BEGIN IMMEDIATE").await
    }
    .await;
    if let Err(error) = setup {
        // BEGIN 是否进入事务无法确认时，不再复用该物理连接。
        let restored = restore_foreign_keys(owner.connection()?, enabled).await;
        let closed = owner.close().await;
        restored?;
        closed?;
        return Err(error);
    }
    let result = match rebuild(owner.connection()?, &table, &column, &temporary, operation).await {
        Ok(()) => execute(owner.connection()?, "COMMIT").await,
        Err(error) => Err(error),
    };
    if result.is_err() {
        let rollback = execute(owner.connection()?, "ROLLBACK").await;
        if let Err(error) = rollback {
            let restored = restore_foreign_keys(owner.connection()?, enabled).await;
            let closed = owner.close().await;
            restored?;
            closed?;
            return Err(error);
        }
    }
    if let Err(error) = restore_foreign_keys(owner.connection()?, enabled).await {
        owner.close().await?;
        return Err(error);
    }
    owner.release();
    result
}

#[cfg(feature = "db-sqlite")]
#[allow(
    clippy::too_many_lines,
    reason = "原定义、数据与附属对象必须在同一重建事务中核对和恢复"
)]
async fn rebuild(
    connection: &mut SqliteConnection,
    table: &str,
    column: &str,
    temporary: &str,
    operation: RebuildOperation,
) -> Result<(), DbErr> {
    let original = sqlx::query(
        "SELECT name,sql FROM main.sqlite_master WHERE type='table' AND name=? COLLATE NOCASE",
    )
    .bind(table)
    .fetch_optional(&mut *connection)
    .await
    .map_err(database_error)?
    .ok_or_else(unsupported)?;
    let table: String = original.try_get("name").map_err(database_error)?;
    let original: String = original.try_get("sql").map_err(database_error)?;
    let parts = table_definition(&original)?;
    let table = table.as_str();
    let information = sqlx::query(sqlx::AssertSqlSafe(format!(
        "PRAGMA main.table_xinfo({})",
        identifier(table)
    )))
    .fetch_all(&mut *connection)
    .await
    .map_err(database_error)?;
    let mut columns = Vec::new();
    let mut known = Vec::new();
    let mut target = false;
    let mut target_not_null = false;
    let mut integer_primary_key = false;
    let mut primary_key_columns = 0;
    for row in information {
        let name: String = row.try_get("name").map_err(database_error)?;
        let hidden: i64 = row.try_get("hidden").map_err(database_error)?;
        let primary: i64 = row.try_get("pk").map_err(database_error)?;
        let declared_type: String = row.try_get("type").map_err(database_error)?;
        if primary > 0 {
            primary_key_columns += 1;
            integer_primary_key = declared_type.eq_ignore_ascii_case("INTEGER")
                && (!name.eq_ignore_ascii_case(column)
                    || !matches!(operation, RebuildOperation::DropColumn));
        }
        if name.eq_ignore_ascii_case(column) {
            target = true;
            target_not_null = row.try_get::<i64, _>("notnull").map_err(database_error)? != 0;
            if !matches!(operation, RebuildOperation::DropColumn) && (primary > 0 || hidden != 0) {
                return Err(unsupported());
            }
        }
        if hidden == 0
            && (!name.eq_ignore_ascii_case(column)
                || !matches!(operation, RebuildOperation::DropColumn))
        {
            columns.push(identifier(&name));
        }
        known.push(name);
    }
    if !target || columns.is_empty() {
        return Err(unsupported());
    }
    // 普通 rowid 表重建也保留隐式行标识；WITHOUT ROWID 表只能复制其真实字段。
    if !without_rowid(parts.suffix) {
        if let Some(alias) = ["_rowid_", "rowid", "oid"]
            .into_iter()
            .find(|alias| !known.iter().any(|name| name.eq_ignore_ascii_case(alias)))
        {
            columns.insert(0, identifier(alias));
        } else {
            // INTEGER PRIMARY KEY DESC 不是 rowid alias；有独立 PK index 时亦不能据类型猜测。
            let primary_indexes = sqlx::query(sqlx::AssertSqlSafe(format!(
                "PRAGMA main.index_list({})",
                identifier(table)
            )))
            .fetch_all(&mut *connection)
            .await
            .map_err(database_error)?;
            let mut separate_primary = false;
            for row in primary_indexes {
                let origin: String = row.try_get("origin").map_err(database_error)?;
                separate_primary |= origin == "pk";
            }
            if !integer_primary_key || primary_key_columns != 1 || separate_primary {
                return Err(unsupported());
            }
        }
    }
    let mut definitions = Vec::new();
    let mut removed = 0;
    let mut parsed_columns = 0;
    for definition in &parts.definitions {
        let (name, quoted, _) = token(definition, 0).ok_or_else(unsupported)?;
        if !quoted
            && ["CONSTRAINT", "PRIMARY", "UNIQUE", "CHECK", "FOREIGN"]
                .iter()
                .any(|keyword| name.eq_ignore_ascii_case(keyword))
        {
            definitions.push((*definition).to_owned());
            continue;
        }
        if !known.iter().any(|known| known.eq_ignore_ascii_case(&name)) {
            return Err(unsupported());
        }
        parsed_columns += 1;
        if name.eq_ignore_ascii_case(column) {
            removed += 1;
            match operation {
                RebuildOperation::DropColumn => {}
                RebuildOperation::MakeNullable => {
                    definitions.push(nullable_definition(definition)?);
                }
                RebuildOperation::MakeNotNullable if target_not_null => {
                    definitions.push((*definition).to_owned());
                }
                RebuildOperation::MakeNotNullable => {
                    definitions.push(not_nullable_definition(definition)?);
                }
            }
        } else {
            definitions.push((*definition).to_owned());
        }
    }
    if removed != 1 || parsed_columns != known.len() {
        return Err(unsupported());
    }
    let objects = sqlx::query("SELECT type,name,sql FROM main.sqlite_master WHERE tbl_name=? AND type IN ('index','trigger') AND sql IS NOT NULL ORDER BY type, name")
        .bind(table).fetch_all(&mut *connection).await.map_err(database_error)?;
    let has_sequence: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM main.sqlite_master WHERE name='sqlite_sequence' AND type='table'",
    )
    .fetch_one(&mut *connection)
    .await
    .map_err(database_error)?;
    let sequence: Option<i64> = if has_sequence == 0 {
        None
    } else {
        sqlx::query_scalar("SELECT seq FROM main.sqlite_sequence WHERE name=?")
            .bind(table)
            .fetch_optional(&mut *connection)
            .await
            .map_err(database_error)?
    };
    let temporary_identifier = format!("main.{}", identifier(temporary));
    let original_identifier = identifier(table);
    let table_identifier = format!("main.{original_identifier}");
    let create = format!(
        "CREATE TABLE {temporary_identifier} ({}){}",
        definitions.join(","),
        parts.suffix
    );
    execute(connection, &create).await?;
    let columns = columns.join(",");
    execute(connection, &format!("INSERT INTO {temporary_identifier} ({columns}) SELECT {columns} FROM {table_identifier}")).await?;
    execute(connection, &format!("DROP TABLE {table_identifier}")).await?;
    execute(
        connection,
        &format!("ALTER TABLE {temporary_identifier} RENAME TO {original_identifier}"),
    )
    .await?;
    for row in objects {
        let sql: String = row.try_get("sql").map_err(database_error)?;
        let name: String = row.try_get("name").map_err(database_error)?;
        let kind: String = row.try_get("type").map_err(database_error)?;
        execute(connection, &main_object_sql(&sql, &name, &kind)?).await?;
    }
    if let Some(sequence) = sequence {
        sqlx::query("DELETE FROM main.sqlite_sequence WHERE name=?")
            .bind(table)
            .execute(&mut *connection)
            .await
            .map_err(database_error)?;
        sqlx::query("INSERT INTO main.sqlite_sequence(name,seq) VALUES (?,?)")
            .bind(table)
            .bind(sequence)
            .execute(&mut *connection)
            .await
            .map_err(database_error)?;
    }
    if !matches!(operation, RebuildOperation::DropColumn) {
        // 最终元数据必须证明非空约束符合目标，不能把未识别语法当成功。
        let actual = sqlx::query(sqlx::AssertSqlSafe(format!(
            "PRAGMA main.table_xinfo({})",
            identifier(table)
        )))
        .fetch_all(&mut *connection)
        .await
        .map_err(database_error)?;
        let mut verified = false;
        for row in actual {
            let name: String = row.try_get("name").map_err(database_error)?;
            if name.eq_ignore_ascii_case(column) {
                let not_null: i64 = row.try_get("notnull").map_err(database_error)?;
                if (not_null != 0) != matches!(operation, RebuildOperation::MakeNotNullable) {
                    return Err(unsupported());
                }
                verified = true;
            }
        }
        if !verified {
            return Err(unsupported());
        }
        let violations = sqlx::query("PRAGMA main.foreign_key_check")
            .fetch_all(&mut *connection)
            .await
            .map_err(database_error)?;
        if !violations.is_empty() {
            return Err(DbErr::Custom(
                "SQLite 外键检查失败，放宽非空迁移已拒绝".into(),
            ));
        }
    }
    Ok(())
}

// SQLite 的明确 main 对象名使 sqlite3FixSrcList 把 ON 表绑定到同一 schema，
// 避免 CREATE INDEX/TRIGGER 因 TEMP 同名表而自动选择 temp。
fn main_object_sql(sql: &str, name: &str, kind: &str) -> Result<String, DbErr> {
    let (create, quoted, mut position) = token(sql, 0).ok_or_else(unsupported)?;
    if quoted || !create.eq_ignore_ascii_case("CREATE") {
        return Err(unsupported());
    }
    let (mut keyword, quoted, next) = token(sql, position).ok_or_else(unsupported)?;
    position = next;
    if !quoted && keyword.eq_ignore_ascii_case("UNIQUE") && kind == "index" {
        let (next_keyword, quoted, next) = token(sql, position).ok_or_else(unsupported)?;
        if quoted {
            return Err(unsupported());
        }
        keyword = next_keyword;
        position = next;
    } else if quoted {
        return Err(unsupported());
    }
    if !keyword.eq_ignore_ascii_case(kind) {
        return Err(unsupported());
    }
    let (word, quoted, next) = token(sql, position).ok_or_else(unsupported)?;
    if !quoted && word.eq_ignore_ascii_case("IF") {
        let (not, quoted_not, after_not) = token(sql, next).ok_or_else(unsupported)?;
        let (exists, quoted_exists, after_exists) =
            token(sql, after_not).ok_or_else(unsupported)?;
        if quoted_not
            || quoted_exists
            || !not.eq_ignore_ascii_case("NOT")
            || !exists.eq_ignore_ascii_case("EXISTS")
        {
            return Err(unsupported());
        }
        position = after_exists;
    }
    let start = skip_trivia(sql, position).ok_or_else(unsupported)?;
    let (mut object, _, mut end) = token(sql, start).ok_or_else(unsupported)?;
    let after = skip_trivia(sql, end).ok_or_else(unsupported)?;
    if sql.as_bytes().get(after) == Some(&b'.') {
        if !object.eq_ignore_ascii_case("main") {
            return Err(unsupported());
        }
        let (actual, _, next) = token(sql, after + 1).ok_or_else(unsupported)?;
        object = actual;
        end = next;
    }
    if !object.eq_ignore_ascii_case(name) {
        return Err(unsupported());
    }
    Ok(format!(
        "{}main.{}{}",
        &sql[..start],
        identifier(name),
        &sql[end..]
    ))
}

struct TableDefinition<'a> {
    definitions: Vec<&'a str>,
    suffix: &'a str,
}

#[derive(Clone, Copy)]
enum Lexical {
    Sql,
    Quote(u8),
    Bracket,
    Line,
    Block,
}

fn table_definition(sql: &str) -> Result<TableDefinition<'_>, DbErr> {
    let (create, quoted, next) = token(sql, 0).ok_or_else(unsupported)?;
    let (table, quoted_table, _) = token(sql, next).ok_or_else(unsupported)?;
    if quoted
        || quoted_table
        || !create.eq_ignore_ascii_case("CREATE")
        || !table.eq_ignore_ascii_case("TABLE")
    {
        return Err(unsupported());
    }
    let mut state = Lexical::Sql;
    let bytes = sql.as_bytes();
    let mut depth = 0_usize;
    let mut start = None;
    let mut definitions = Vec::new();
    let mut position = 0;
    while position < bytes.len() {
        let byte = bytes[position];
        let next = bytes.get(position + 1).copied();
        match state {
            Lexical::Sql => match (byte, next) {
                (b'\'' | b'"' | b'`', _) => state = Lexical::Quote(byte),
                (b'[', _) => state = Lexical::Bracket,
                (b'-', Some(b'-')) => {
                    state = Lexical::Line;
                    position += 1;
                }
                (b'/', Some(b'*')) => {
                    state = Lexical::Block;
                    position += 1;
                }
                (b'(', _) => {
                    depth += 1;
                    if depth == 1 {
                        start = Some(position + 1);
                    }
                }
                (b')', _) => {
                    if depth == 0 {
                        return Err(unsupported());
                    }
                    depth -= 1;
                    if depth == 0 {
                        definitions.push(&sql[start.ok_or_else(unsupported)?..position]);
                        return Ok(TableDefinition {
                            definitions,
                            suffix: &sql[position + 1..],
                        });
                    }
                }
                (b',', _) if depth == 1 => {
                    definitions.push(&sql[start.ok_or_else(unsupported)?..position]);
                    start = Some(position + 1);
                }
                (0, _) => return Err(unsupported()),
                _ => {}
            },
            Lexical::Quote(quote) if quote == byte => {
                if next == Some(quote) {
                    position += 1;
                } else {
                    state = Lexical::Sql;
                }
            }
            Lexical::Bracket if byte == b']' => state = Lexical::Sql,
            Lexical::Line if byte == b'\n' => state = Lexical::Sql,
            Lexical::Block if byte == b'*' && next == Some(b'/') => {
                state = Lexical::Sql;
                position += 1;
            }
            _ => {}
        }
        position += 1;
    }
    Err(unsupported())
}

fn without_rowid(suffix: &str) -> bool {
    let mut position = 0;
    while position < suffix.len() {
        while suffix
            .as_bytes()
            .get(position)
            .is_some_and(|byte| byte.is_ascii_whitespace() || *byte == b',')
        {
            position += 1;
        }
        let Some((word, quoted, next)) = token(suffix, position) else {
            break;
        };
        if !quoted && word.eq_ignore_ascii_case("WITHOUT") {
            return token(suffix, next).is_some_and(|(word, quoted, _)| {
                !quoted && word.trim_end_matches(';').eq_ignore_ascii_case("ROWID")
            });
        }
        position = next;
    }
    false
}

fn skip_trivia(sql: &str, mut position: usize) -> Option<usize> {
    let bytes = sql.as_bytes();
    loop {
        while bytes.get(position).is_some_and(u8::is_ascii_whitespace) {
            position += 1;
        }
        if bytes.get(position..position + 2) == Some(b"--") {
            while bytes.get(position).is_some_and(|byte| *byte != b'\n') {
                position += 1;
            }
        } else if bytes.get(position..position + 2) == Some(b"/*") {
            position += 2;
            while bytes.get(position..position + 2) != Some(b"*/") {
                bytes.get(position)?;
                position += 1;
            }
            position += 2;
        } else {
            break;
        }
    }
    Some(position)
}

fn token(sql: &str, position: usize) -> Option<(String, bool, usize)> {
    let bytes = sql.as_bytes();
    let mut position = skip_trivia(sql, position)?;
    let first = *bytes.get(position)?;
    let quoted = matches!(first, b'\'' | b'"' | b'`' | b'[');
    let end_quote = if first == b'[' { b']' } else { first };
    if quoted {
        position += 1;
        let mut value = Vec::new();
        loop {
            let byte = *bytes.get(position)?;
            position += 1;
            if byte == end_quote {
                if first != b'[' && bytes.get(position) == Some(&end_quote) {
                    value.push(byte);
                    position += 1;
                } else {
                    return String::from_utf8(value)
                        .ok()
                        .map(|name| (name, true, position));
                }
            } else {
                value.push(byte);
            }
        }
    }
    let start = position;
    while bytes.get(position).is_some_and(|byte| {
        !byte.is_ascii_whitespace() && !matches!(*byte, b'(' | b')' | b',' | b'.')
    }) {
        if bytes.get(position..position + 2) == Some(b"/*")
            || bytes.get(position..position + 2) == Some(b"--")
        {
            break;
        }
        position += 1;
    }
    (position > start).then(|| (sql[start..position].to_owned(), false, position))
}

// 仅解析列定义顶层词项；表达式、字符串、注释中的约束文字不参与改写。
type DefinitionWord = (String, bool, usize, usize);

fn definition_words(definition: &str) -> Result<(Vec<DefinitionWord>, usize), DbErr> {
    let mut words = Vec::new();
    let mut position = 0;
    let mut depth = 0_usize;
    let mut last_end = 0;
    while position < definition.len() {
        position = skip_trivia(definition, position).ok_or_else(unsupported)?;
        let Some(byte) = definition.as_bytes().get(position).copied() else {
            break;
        };
        if byte == b'(' {
            depth += 1;
            position += 1;
        } else if byte == b')' {
            depth = depth.checked_sub(1).ok_or_else(unsupported)?;
            position += 1;
            if depth == 0 {
                words.push((String::new(), true, position - 1, position));
            }
        } else if byte == b',' {
            if depth == 0 {
                return Err(unsupported());
            }
            position += 1;
        } else {
            let start = position;
            let (word, quoted, next) = if matches!(byte, b'\'' | b'"' | b'`' | b'[') {
                token(definition, position).ok_or_else(unsupported)?
            } else {
                let mut end = position;
                while definition.as_bytes().get(end).is_some_and(|byte| {
                    !byte.is_ascii_whitespace()
                        && !matches!(*byte, b'(' | b')' | b',' | b'\'' | b'"' | b'`' | b'[')
                }) {
                    if definition.as_bytes().get(end..end + 2) == Some(b"/*")
                        || definition.as_bytes().get(end..end + 2) == Some(b"--")
                    {
                        break;
                    }
                    end += 1;
                }
                if end == position {
                    return Err(unsupported());
                }
                (definition[position..end].to_owned(), false, end)
            };
            if depth == 0 {
                words.push((word, quoted, start, next));
            }
            position = next;
        }
        last_end = position;
    }
    if depth != 0 {
        return Err(unsupported());
    }
    Ok((words, last_end))
}

fn nullable_definition(definition: &str) -> Result<String, DbErr> {
    let (words, _) = definition_words(definition)?;
    let keyword = |index: usize, expected: &str| {
        words
            .get(index)
            .is_some_and(|(word, quoted, _, _)| !quoted && word.eq_ignore_ascii_case(expected))
    };
    let mut ranges = Vec::new();
    for index in 1..words.len() {
        if !keyword(index, "NOT") {
            continue;
        }
        if !keyword(index + 1, "NULL") {
            return Err(unsupported());
        }
        let mut start = words[index].2;
        if index >= 2 && keyword(index - 2, "CONSTRAINT") {
            start = words[index - 2].2;
        }
        let mut end = words[index + 1].3;
        if keyword(index + 2, "ON") {
            if !keyword(index + 3, "CONFLICT")
                || !["ROLLBACK", "ABORT", "FAIL", "IGNORE", "REPLACE"]
                    .iter()
                    .any(|action| keyword(index + 4, action))
            {
                return Err(unsupported());
            }
            end = words[index + 4].3;
        }
        ranges.push((start, end));
    }
    let mut result = definition.to_owned();
    for (start, end) in ranges.into_iter().rev() {
        result.replace_range(start..end, " ");
    }
    Ok(result)
}

fn not_nullable_definition(definition: &str) -> Result<String, DbErr> {
    let (words, last_end) = definition_words(definition)?;
    let keyword = |index: usize, expected: &str| {
        words
            .get(index)
            .is_some_and(|(word, quoted, _, _)| !quoted && word.eq_ignore_ascii_case(expected))
    };
    let mut ranges = Vec::new();
    for index in 1..words.len() {
        if keyword(index, "NOT") {
            return Err(unsupported());
        }
        if !keyword(index, "NULL")
            || keyword(index - 1, "DEFAULT")
            || keyword(index - 1, "CONSTRAINT")
            || keyword(index - 1, "COLLATE")
        {
            continue;
        }
        let mut start = words[index].2;
        if index >= 2 && keyword(index - 2, "CONSTRAINT") {
            start = words[index - 2].2;
        }
        // 此变换不支持独立NULL后的ON CONFLICT，不能猜测未知尾部的归属。
        if keyword(index + 1, "ON") {
            return Err(unsupported());
        }
        ranges.push((start, words[index].3));
    }
    let mut result = definition.to_owned();
    // 插在尾部注释之前，避免行注释吞掉新约束。
    result.insert_str(last_end, " NOT NULL ");
    for (start, end) in ranges.into_iter().rev() {
        result.replace_range(start..end, " ");
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "db-sqlite")]
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

    type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

    #[test]
    fn not_nullable_transform_preserves_default_expressions_and_tail_comments() -> TestResult {
        let changed = not_nullable_definition(
            "email TEXT CONSTRAINT optional NULL DEFAULT NULL CHECK(email IS NULL OR email!='NOT NULL') /* tail */ -- final",
        )?;
        assert!(!changed.contains("CONSTRAINT optional"));
        assert!(changed.contains("DEFAULT NULL"));
        assert!(changed.contains("CHECK(email IS NULL OR email!='NOT NULL')"));
        assert!(changed.contains("NOT NULL  /* tail */ -- final"));
        let changed = not_nullable_definition("email TEXT DEFAULT (NULL) NULL -- final")?;
        assert!(changed.contains("DEFAULT (NULL)"));
        assert!(!changed.contains(") NULL"));
        assert!(changed.contains("NOT NULL"));
        assert!(not_nullable_definition("email TEXT NULL ON CONFLICT FAIL").is_err());
        Ok(())
    }

    #[cfg(feature = "db-sqlite")]
    #[tokio::test]
    async fn nullable_round_trip_and_failed_down_preserve_schema_data_and_objects() -> TestResult {
        let pool = pool().await?;
        for sql in [
            "CREATE TABLE accounts(id INTEGER PRIMARY KEY AUTOINCREMENT,email TEXT NOT NULL COLLATE NOCASE, secret TEXT DEFAULT 'NULL')",
            "CREATE UNIQUE INDEX email_unique ON accounts(email)",
            "CREATE TABLE children(account_id INTEGER REFERENCES accounts(id))",
            "CREATE TABLE events(value TEXT)",
            "CREATE TRIGGER account_event AFTER INSERT ON accounts BEGIN INSERT INTO events VALUES(new.email); END",
            "INSERT INTO accounts(id,email,secret) VALUES(10,'old@example.test','ciphertext')",
            "INSERT INTO accounts(id,email) VALUES(99,'deleted@example.test')",
            "DELETE FROM accounts WHERE id=99",
            "INSERT INTO children VALUES(10)",
        ] {
            sqlx::query(sqlx::AssertSqlSafe(sql)).execute(&pool).await?;
        }
        make_column_nullable(pool.clone(), "accounts".into(), "email".into()).await?;
        make_column_not_nullable(pool.clone(), "accounts".into(), "email".into()).await?;
        make_column_not_nullable(pool.clone(), "accounts".into(), "email".into()).await?;
        assert!(
            sqlx::query("INSERT INTO accounts(email) VALUES(NULL)")
                .execute(&pool)
                .await
                .is_err()
        );
        make_column_nullable(pool.clone(), "accounts".into(), "email".into()).await?;
        sqlx::query("INSERT INTO accounts(email) VALUES(NULL)")
            .execute(&pool)
            .await?;
        let schema: String =
            sqlx::query_scalar("SELECT sql FROM sqlite_master WHERE name='accounts'")
                .fetch_one(&pool)
                .await?;
        assert!(
            make_column_not_nullable(pool.clone(), "accounts".into(), "email".into())
                .await
                .is_err()
        );
        assert_eq!(
            sqlx::query_scalar::<_, String>("SELECT sql FROM sqlite_master WHERE name='accounts'")
                .fetch_one(&pool)
                .await?,
            schema
        );
        assert_eq!(
            sqlx::query_scalar::<_, String>("SELECT secret FROM accounts WHERE id=10")
                .fetch_one(&pool)
                .await?,
            "ciphertext"
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT id FROM accounts WHERE email IS NULL")
                .fetch_one(&pool)
                .await?,
            100
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT seq FROM sqlite_sequence WHERE name='accounts'")
                .fetch_one(&pool)
                .await?,
            100
        );
        assert!(
            sqlx::query("INSERT INTO accounts(email) VALUES('OLD@example.test')")
                .execute(&pool)
                .await
                .is_err()
        );
        assert!(
            sqlx::query("INSERT INTO children VALUES(999)")
                .execute(&pool)
                .await
                .is_err()
        );
        sqlx::query("INSERT INTO accounts(email) VALUES('trigger-check')")
            .execute(&pool)
            .await?;
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM events WHERE value='trigger-check'")
                .fetch_one(&pool)
                .await?,
            1
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("PRAGMA foreign_keys")
                .fetch_one(&pool)
                .await?,
            1
        );
        pool.close().await;
        Ok(())
    }

    #[cfg(feature = "db-sqlite")]
    #[tokio::test]
    async fn explicit_null_constraint_is_removed_without_changing_default_null() -> TestResult {
        let pool = pool().await?;
        sqlx::query("CREATE TABLE accounts(id INTEGER PRIMARY KEY,email TEXT CONSTRAINT optional NULL DEFAULT NULL CHECK(email IS NULL OR length(email)>0) /* tail */)").execute(&pool).await?;
        sqlx::query("INSERT INTO accounts VALUES(1,'existing')")
            .execute(&pool)
            .await?;
        make_column_not_nullable(pool.clone(), "accounts".into(), "email".into()).await?;
        assert!(
            sqlx::query("INSERT INTO accounts(id) VALUES(2)")
                .execute(&pool)
                .await
                .is_err()
        );
        let schema: String =
            sqlx::query_scalar("SELECT sql FROM sqlite_master WHERE name='accounts'")
                .fetch_one(&pool)
                .await?;
        assert!(schema.contains("DEFAULT NULL"));
        assert!(!schema.contains("CONSTRAINT optional"));
        assert!(schema.contains("/* tail */"));
        pool.close().await;
        Ok(())
    }

    #[test]
    fn nullable_transform_only_removes_top_level_constraint() -> TestResult {
        let original = "email TEXT CONSTRAINT required NOT /* note */ NULL ON CONFLICT FAIL DEFAULT 'NOT NULL' CHECK(email != 'NOT NULL' AND length('(')>0) COLLATE NOCASE";
        let changed = nullable_definition(original)?;
        assert!(!changed.contains("CONSTRAINT required"));
        assert!(!changed.contains("ON CONFLICT FAIL"));
        assert!(changed.contains("DEFAULT 'NOT NULL'"));
        assert!(changed.contains("CHECK(email != 'NOT NULL' AND length('(')>0)"));
        assert!(changed.contains("COLLATE NOCASE"));
        assert_eq!(
            nullable_definition("email TEXT CHECK(email IS NOT NULL)")?,
            "email TEXT CHECK(email IS NOT NULL)"
        );
        assert!(nullable_definition("email TEXT NOT NULL ON CONFLICT unknown").is_err());
        Ok(())
    }

    #[cfg(feature = "db-sqlite")]
    #[tokio::test]
    async fn nullable_rebuild_preserves_data_objects_and_nullable_unique() -> TestResult {
        let pool = pool().await?;
        for sql in [
            "CREATE TABLE accounts(id INTEGER PRIMARY KEY AUTOINCREMENT,email TEXT CONSTRAINT required NOT NULL ON CONFLICT FAIL COLLATE NOCASE, secret TEXT DEFAULT 'NOT NULL')",
            "CREATE UNIQUE INDEX email_unique ON accounts(email)",
            "CREATE TABLE children(account_id INTEGER REFERENCES accounts(id))",
            "CREATE TABLE events(value TEXT)",
            "CREATE TRIGGER account_event AFTER INSERT ON accounts BEGIN INSERT INTO events VALUES(new.email); END",
            "INSERT INTO accounts(id,email,secret) VALUES(10,'old@example.test','ciphertext')",
            "INSERT INTO accounts(id,email) VALUES(99,'deleted@example.test')",
            "DELETE FROM accounts WHERE id=99",
            "INSERT INTO children VALUES(10)",
        ] {
            sqlx::query(sqlx::AssertSqlSafe(sql)).execute(&pool).await?;
        }
        make_column_nullable(pool.clone(), "accounts".into(), "email".into()).await?;
        make_column_nullable(pool.clone(), "accounts".into(), "email".into()).await?;
        let secret: String = sqlx::query_scalar("SELECT secret FROM accounts WHERE id=10")
            .fetch_one(&pool)
            .await?;
        assert_eq!(secret, "ciphertext");
        sqlx::query("INSERT INTO accounts(email) VALUES(NULL),(NULL)")
            .execute(&pool)
            .await?;
        let lowest: i64 = sqlx::query_scalar("SELECT MIN(id) FROM accounts WHERE email IS NULL")
            .fetch_one(&pool)
            .await?;
        assert_eq!(lowest, 100);
        let null_events: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM events WHERE value IS NULL")
                .fetch_one(&pool)
                .await?;
        assert_eq!(null_events, 2);
        assert!(
            sqlx::query("INSERT INTO accounts(email) VALUES('OLD@example.test')")
                .execute(&pool)
                .await
                .is_err()
        );
        assert!(
            sqlx::query("INSERT INTO children VALUES(999)")
                .execute(&pool)
                .await
                .is_err()
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("PRAGMA foreign_keys")
                .fetch_one(&pool)
                .await?,
            1
        );
        pool.close().await;
        Ok(())
    }

    #[cfg(feature = "db-sqlite")]
    #[tokio::test]
    async fn nullable_failure_preserves_original_table_and_constraints() -> TestResult {
        let pool = pool().await?;
        sqlx::query("CREATE TABLE accounts(id INTEGER PRIMARY KEY,email TEXT NOT NULL)")
            .execute(&pool)
            .await?;
        sqlx::query("INSERT INTO accounts VALUES(1,'old')")
            .execute(&pool)
            .await?;
        assert!(
            make_column_nullable(pool.clone(), "accounts".into(), "id".into())
                .await
                .is_err()
        );
        assert!(
            make_column_nullable(pool.clone(), "accounts".into(), "missing".into())
                .await
                .is_err()
        );
        // 既存外键违规也不能借本次重建提交；事务回滚必须保留非空约束。
        sqlx::query("PRAGMA foreign_keys=OFF")
            .execute(&pool)
            .await?;
        sqlx::query("CREATE TABLE children(parent INTEGER REFERENCES accounts(id))")
            .execute(&pool)
            .await?;
        sqlx::query("INSERT INTO children VALUES(999)")
            .execute(&pool)
            .await?;
        sqlx::query("PRAGMA foreign_keys=ON").execute(&pool).await?;
        assert!(
            make_column_nullable(pool.clone(), "accounts".into(), "email".into())
                .await
                .is_err()
        );
        assert!(
            sqlx::query("INSERT INTO accounts VALUES(2,NULL)")
                .execute(&pool)
                .await
                .is_err()
        );
        assert_eq!(
            sqlx::query_scalar::<_, String>("SELECT email FROM accounts WHERE id=1")
                .fetch_one(&pool)
                .await?,
            "old"
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("PRAGMA foreign_keys")
                .fetch_one(&pool)
                .await?,
            1
        );
        pool.close().await;
        Ok(())
    }

    #[cfg(feature = "db-sqlite")]
    async fn pool() -> Result<SqlitePool, sqlx::Error> {
        SqlitePoolOptions::new()
            .max_connections(1)
            .min_connections(1)
            .idle_timeout(None)
            .max_lifetime(None)
            .connect_with(
                SqliteConnectOptions::new()
                    .filename(":memory:")
                    .foreign_keys(true),
            )
            .await
    }

    #[test]
    fn table_definition_preserves_nested_commas_quotes_constraints_and_options() -> TestResult {
        let sql = "CREATE TABLE \"a(b\" (\"id\" INTEGER PRIMARY KEY, /* comma , */ [keep,name] TEXT DEFAULT ('a,''b'), `drop``name` TEXT, CONSTRAINT ck CHECK (length([keep,name]) IN (1,2))) WITHOUT ROWID";
        let parts = table_definition(sql)?;
        assert_eq!(parts.definitions.len(), 4);
        assert_eq!(
            parts.definitions[1],
            " /* comma , */ [keep,name] TEXT DEFAULT ('a,''b')"
        );
        assert_eq!(
            token(parts.definitions[2], 0).map(|token| token.0),
            Some("drop`name".into())
        );
        assert_eq!(parts.suffix, " WITHOUT ROWID");
        assert!(without_rowid(parts.suffix));
        assert!(table_definition("CREATE VIRTUAL TABLE v USING fts5(value)").is_err());
        assert!(table_definition("CREATE TABLE invalid (a TEXT").is_err());
        assert_eq!(
            main_object_sql("CREATE UNIQUE INDEX [a.b] ON items(keep)", "a.b", "index")?,
            "CREATE UNIQUE INDEX main.\"a.b\" ON items(keep)"
        );
        assert_eq!(
            main_object_sql(
                "CREATE TRIGGER main.\"t.r\" AFTER INSERT ON items BEGIN SELECT 1; END",
                "t.r",
                "trigger"
            )?,
            "CREATE TRIGGER main.\"t.r\" AFTER INSERT ON items BEGIN SELECT 1; END"
        );
        Ok(())
    }

    #[cfg(feature = "db-sqlite")]
    #[tokio::test]
    async fn rebuild_preserves_columns_generated_constraints_indexes_triggers_and_sequence()
    -> TestResult {
        let pool = pool().await?;
        sqlx::query("CREATE TABLE items (id INTEGER PRIMARY KEY AUTOINCREMENT, keep TEXT NOT NULL DEFAULT 'value,with,commas' CHECK(length(keep)>0), discard TEXT, derived TEXT GENERATED ALWAYS AS (keep || ',generated') STORED, UNIQUE(keep))").execute(&pool).await?;
        sqlx::query("CREATE TABLE audit (item_id INTEGER)")
            .execute(&pool)
            .await?;
        sqlx::query("CREATE INDEX items_keep ON items(keep)")
            .execute(&pool)
            .await?;
        sqlx::query("CREATE TRIGGER items_audit AFTER INSERT ON items BEGIN INSERT INTO audit VALUES (new.id); END").execute(&pool).await?;
        sqlx::query(
            "INSERT INTO items(id,keep,discard) VALUES (100,'deleted','x'),(7,'retained','y')",
        )
        .execute(&pool)
        .await?;
        sqlx::query("DELETE FROM items WHERE id=100")
            .execute(&pool)
            .await?;
        drop_column(pool.clone(), "items".into(), "discard".into()).await?;
        let row = sqlx::query("SELECT id,keep,derived FROM items")
            .fetch_one(&pool)
            .await?;
        assert_eq!(row.try_get::<i64, _>("id")?, 7);
        assert_eq!(row.try_get::<String, _>("keep")?, "retained");
        assert_eq!(row.try_get::<String, _>("derived")?, "retained,generated");
        assert!(
            sqlx::query("SELECT discard FROM items")
                .fetch_all(&pool)
                .await
                .is_err()
        );
        assert!(
            sqlx::query("INSERT INTO items(keep) VALUES ('retained')")
                .execute(&pool)
                .await
                .is_err()
        );
        assert!(
            sqlx::query("INSERT INTO items(keep) VALUES ('')")
                .execute(&pool)
                .await
                .is_err()
        );
        sqlx::query("INSERT INTO items(keep) VALUES ('after')")
            .execute(&pool)
            .await?;
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT id FROM items WHERE keep='after'")
                .fetch_one(&pool)
                .await?,
            101
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM audit WHERE item_id=101")
                .fetch_one(&pool)
                .await?,
            1
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT count(*) FROM sqlite_master WHERE type='index' AND name='items_keep'"
            )
            .fetch_one(&pool)
            .await?,
            1
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("PRAGMA foreign_keys")
                .fetch_one(&pool)
                .await?,
            1
        );
        pool.close().await;
        Ok(())
    }

    #[cfg(feature = "db-sqlite")]
    #[tokio::test]
    async fn invalid_surviving_index_rolls_back_schema_rows_sequence_and_restores_foreign_keys()
    -> TestResult {
        let pool = pool().await?;
        sqlx::query(
            "CREATE TABLE items (id INTEGER PRIMARY KEY AUTOINCREMENT, keep TEXT, discard TEXT)",
        )
        .execute(&pool)
        .await?;
        sqlx::query("CREATE INDEX items_discard ON items(discard)")
            .execute(&pool)
            .await?;
        sqlx::query("INSERT INTO items(id,keep,discard) VALUES (90,'retained','indexed')")
            .execute(&pool)
            .await?;
        sqlx::query("DELETE FROM items").execute(&pool).await?;
        sqlx::query("INSERT INTO items(id,keep,discard) VALUES (3,'retained','indexed')")
            .execute(&pool)
            .await?;
        assert!(
            drop_column(pool.clone(), "items".into(), "discard".into())
                .await
                .is_err()
        );
        assert_eq!(
            sqlx::query_scalar::<_, String>("SELECT discard FROM items")
                .fetch_one(&pool)
                .await?,
            "indexed"
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT seq FROM sqlite_sequence WHERE name='items'")
                .fetch_one(&pool)
                .await?,
            90
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT count(*) FROM sqlite_master WHERE name='items_discard'"
            )
            .fetch_one(&pool)
            .await?,
            1
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT count(*) FROM sqlite_master WHERE name GLOB '__openrust_rebuild_*'"
            )
            .fetch_one(&pool)
            .await?,
            0
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("PRAGMA foreign_keys")
                .fetch_one(&pool)
                .await?,
            1
        );
        pool.close().await;
        Ok(())
    }

    #[cfg(feature = "db-sqlite")]
    #[tokio::test]
    async fn missing_column_and_referenced_check_fail_without_losing_original_table() -> TestResult
    {
        let pool = pool().await?;
        sqlx::query("PRAGMA foreign_keys=OFF")
            .execute(&pool)
            .await?;
        sqlx::query("CREATE TABLE items (keep TEXT, discard INTEGER, CHECK(discard>0))")
            .execute(&pool)
            .await?;
        sqlx::query("INSERT INTO items VALUES ('retained',2)")
            .execute(&pool)
            .await?;
        assert!(
            drop_column(pool.clone(), "items".into(), "absent".into())
                .await
                .is_err()
        );
        assert!(
            drop_column(pool.clone(), "items".into(), "discard".into())
                .await
                .is_err()
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT discard FROM items")
                .fetch_one(&pool)
                .await?,
            2
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("PRAGMA foreign_keys")
                .fetch_one(&pool)
                .await?,
            0
        );
        pool.close().await;
        Ok(())
    }

    #[cfg(feature = "db-sqlite")]
    #[tokio::test]
    async fn without_rowid_and_implicit_rowid_keep_their_identity() -> TestResult {
        let pool = pool().await?;
        sqlx::query(
            "CREATE TABLE keyed (id TEXT PRIMARY KEY, keep TEXT, discard TEXT) WITHOUT ROWID",
        )
        .execute(&pool)
        .await?;
        sqlx::query("INSERT INTO keyed VALUES ('key','kept','removed')")
            .execute(&pool)
            .await?;
        drop_column(pool.clone(), "keyed".into(), "discard".into()).await?;
        assert_eq!(
            sqlx::query_scalar::<_, String>("SELECT keep FROM keyed WHERE id='key'")
                .fetch_one(&pool)
                .await?,
            "kept"
        );
        assert!(
            sqlx::query("SELECT rowid FROM keyed")
                .fetch_all(&pool)
                .await
                .is_err()
        );
        sqlx::query("CREATE TABLE ordinary (keep TEXT, discard TEXT)")
            .execute(&pool)
            .await?;
        sqlx::query("INSERT INTO ordinary(rowid,keep,discard) VALUES (77,'kept','removed')")
            .execute(&pool)
            .await?;
        drop_column(pool.clone(), "ordinary".into(), "discard".into()).await?;
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT rowid FROM ordinary")
                .fetch_one(&pool)
                .await?,
            77
        );
        pool.close().await;
        Ok(())
    }

    #[cfg(feature = "db-sqlite")]
    #[tokio::test]
    #[allow(
        clippy::too_many_lines,
        reason = "同一物理连接上同时核对主表、TEMP shadow、附属对象和失败回滚"
    )]
    async fn main_objects_and_copy_ignore_same_connection_temp_shadows() -> TestResult {
        let pool = pool().await?;
        let temporary = format!(
            "__openrust_rebuild_{}_{}",
            std::process::id(),
            NEXT_TABLE.fetch_add(1, Ordering::Relaxed)
        );
        {
            let mut physical = pool.acquire().await?;
            sqlx::query("CREATE TABLE main.items (id INTEGER PRIMARY KEY AUTOINCREMENT, keep TEXT, discard TEXT)").execute(&mut *physical).await?;
            sqlx::query("CREATE TABLE main.audit(value TEXT)")
                .execute(&mut *physical)
                .await?;
            sqlx::query("CREATE INDEX main.items_keep ON items(keep)")
                .execute(&mut *physical)
                .await?;
            sqlx::query("CREATE TRIGGER main.items_audit AFTER INSERT ON items BEGIN INSERT INTO audit VALUES(new.keep); END").execute(&mut *physical).await?;
            sqlx::query("INSERT INTO main.items VALUES (40,'main-retained','main-removed')")
                .execute(&mut *physical)
                .await?;
            sqlx::query("CREATE TEMP TABLE items(id INTEGER,keep TEXT,discard TEXT)")
                .execute(&mut *physical)
                .await?;
            sqlx::query("INSERT INTO temp.items VALUES (900,'temp-retained','temp-untouched')")
                .execute(&mut *physical)
                .await?;
            execute(
                &mut physical,
                &format!("CREATE TEMP TABLE {}(value TEXT)", identifier(&temporary)),
            )
            .await?;
            execute(
                &mut physical,
                &format!(
                    "INSERT INTO temp.{} VALUES ('temp-rebuild-untouched')",
                    identifier(&temporary)
                ),
            )
            .await?;
        }
        // 显式保留生产命名值，使并行测试不会抢走 next 序号而削弱 shadow 回归。
        drop_column_named(
            pool.clone(),
            "ITEMS".into(),
            "DISCARD".into(),
            temporary.clone(),
        )
        .await?;
        assert_eq!(
            sqlx::query_scalar::<_, String>("SELECT keep FROM main.items WHERE id=40")
                .fetch_one(&pool)
                .await?,
            "main-retained"
        );
        assert!(
            sqlx::query("SELECT discard FROM main.items")
                .fetch_all(&pool)
                .await
                .is_err()
        );
        assert_eq!(
            sqlx::query_scalar::<_, String>("SELECT discard FROM temp.items WHERE id=900")
                .fetch_one(&pool)
                .await?,
            "temp-untouched"
        );
        let temp_value: String = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT value FROM temp.{}",
            identifier(&temporary)
        )))
        .fetch_one(&pool)
        .await?;
        assert_eq!(temp_value, "temp-rebuild-untouched");
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT count(*) FROM main.sqlite_master WHERE type='index' AND name='items_keep'"
            )
            .fetch_one(&pool)
            .await?,
            1
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT count(*) FROM temp.sqlite_master WHERE name IN ('items_keep','items_audit')"
            )
            .fetch_one(&pool)
            .await?,
            0
        );
        sqlx::query("INSERT INTO main.items(keep) VALUES ('main-after')")
            .execute(&pool)
            .await?;
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT id FROM main.items WHERE keep='main-after'")
                .fetch_one(&pool)
                .await?,
            41
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT count(*) FROM main.audit WHERE value='main-after'"
            )
            .fetch_one(&pool)
            .await?,
            1
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM temp.items")
                .fetch_one(&pool)
                .await?,
            1
        );
        assert!(
            drop_column(pool.clone(), "ITEMS".into(), "keep".into())
                .await
                .is_err()
        );
        assert_eq!(
            sqlx::query_scalar::<_, String>("SELECT keep FROM main.items WHERE id=40")
                .fetch_one(&pool)
                .await?,
            "main-retained"
        );
        assert_eq!(
            sqlx::query_scalar::<_, String>("SELECT discard FROM temp.items WHERE id=900")
                .fetch_one(&pool)
                .await?,
            "temp-untouched"
        );
        let preserved_temp: String = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT value FROM temp.{}",
            identifier(&temporary)
        )))
        .fetch_one(&pool)
        .await?;
        assert_eq!(preserved_temp, "temp-rebuild-untouched");
        pool.close().await;
        Ok(())
    }

    #[cfg(feature = "db-sqlite")]
    #[tokio::test]
    async fn inaccessible_hidden_rowid_is_rejected_without_losing_sparse_rows() -> TestResult {
        let pool = pool().await?;
        sqlx::query("CREATE TABLE shadowed(keep TEXT,discard TEXT)")
            .execute(&pool)
            .await?;
        sqlx::query("INSERT INTO shadowed(rowid,keep,discard) VALUES(700,'sparse','untouched')")
            .execute(&pool)
            .await?;
        sqlx::query("ALTER TABLE shadowed ADD COLUMN rowid TEXT")
            .execute(&pool)
            .await?;
        sqlx::query("ALTER TABLE shadowed ADD COLUMN _rowid_ TEXT")
            .execute(&pool)
            .await?;
        sqlx::query("ALTER TABLE shadowed ADD COLUMN oid TEXT")
            .execute(&pool)
            .await?;
        assert!(
            drop_column(pool.clone(), "SHADOWED".into(), "discard".into())
                .await
                .is_err()
        );
        assert_eq!(
            sqlx::query_scalar::<_, String>("SELECT discard FROM shadowed")
                .fetch_one(&pool)
                .await?,
            "untouched"
        );
        sqlx::query("ALTER TABLE shadowed RENAME COLUMN rowid TO visible_rowid")
            .execute(&pool)
            .await?;
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT rowid FROM shadowed")
                .fetch_one(&pool)
                .await?,
            700
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("PRAGMA foreign_keys")
                .fetch_one(&pool)
                .await?,
            1
        );
        sqlx::query("CREATE TABLE aliased(id INTEGER PRIMARY KEY,rowid TEXT,_rowid_ TEXT,oid TEXT,discard TEXT)").execute(&pool).await?;
        sqlx::query("INSERT INTO aliased VALUES(810,'row','under','oid','removed')")
            .execute(&pool)
            .await?;
        drop_column(pool.clone(), "ALIASED".into(), "discard".into()).await?;
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT id FROM aliased")
                .fetch_one(&pool)
                .await?,
            810
        );
        pool.close().await;
        Ok(())
    }
}
