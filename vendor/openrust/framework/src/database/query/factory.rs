//! 空查询、显式 FROM 与带绑定的受信 SQL 表达式。

use super::{
    Alias, Asterisk, DatabaseTransaction, Db, DbBackend, Error, Expr, PredicateChain, Query,
    QueryBuilder, QueryState, RawSql, Result, Value, unsupported_backend,
};

impl<'tx: 'cap, 'cap> QueryState<'tx, 'cap> {
    pub(super) fn empty(db: Db, transaction: Option<&'tx DatabaseTransaction>) -> Self {
        Self {
            db,
            table: None,
            transaction,
            select: Query::select().column(Asterisk).to_owned(),
            has_joins: false,
            predicates: PredicateChain::all(),
            callbacks: Vec::new(),
            unions: Vec::new(),
            projection: None,
            limit: None,
            offset: 0,
            orders: Vec::new(),
            union_orders: Vec::new(),
            union_limit: None,
            union_offset: None,
            grouped: false,
            having: false,
            failure: None,
        }
    }
    pub(super) fn required_table(&self) -> Result<&str> {
        self.table
            .as_deref()
            .ok_or_else(|| Error::custom("query_table_required", "写入或关联查询需要明确的FROM表"))
    }
    fn from(mut self, logical_table: &str) -> Result<Self> {
        let table = self.db.table_name(logical_table)?;
        self.select.from_clear().from(Alias::new(&table));
        self.table = Some(table);
        Ok(self)
    }
    fn select_raw(mut self, sql: RawSql, values: Vec<Value>) -> Result<Self> {
        let RawSql(sql) = sql;
        let expression = raw_expression(&sql, values, self.db.backend()?)?;
        if self.projection.is_none() {
            self.select.clear_selects();
        }
        self.select.expr(expression.clone());
        self.projection
            .get_or_insert_with(Vec::new)
            .push(expression);
        Ok(self)
    }
}
impl<'tx: 'cap, 'cap> QueryBuilder<'tx, 'cap> {
    /// 显式选择逻辑FROM表；保留同一连接、实际事务与现有条件。
    pub fn from(self, logical_table: &str) -> Result<Self> {
        self.transform(|state| state.from(logical_table))?;
        Ok(self)
    }
    pub(in crate::database) fn alias_relation_table(self, alias: &str) -> Result<Self> {
        super::validate_identifier(alias)?;
        self.transform(|mut state| {
            let table = state.required_table()?.to_owned();
            state
                .select
                .from_clear()
                .from_as(Alias::new(table), Alias::new(alias));
            Ok(state)
        })?;
        Ok(self)
    }

    /// 追加可信SQL投影；初始隐式星号被替换，后续显式select仍可重置投影。
    ///
    /// 参数用 `?` 绑定；PG 问号操作符用 `??`，其余后端的代码区拒绝 `??`。
    /// 数据问号使用引用或绑定，代码区原生参数明确拒绝。
    /// PostgreSQL 普通字符串拒绝引号前的奇数反斜线。请用值绑定、美元引用或显式 `E` 字符串；
    /// PostgreSQL 以换行衔接的 `E` 字符串片段延续转义规则。
    /// `MySQL` 的反斜线字面量存在 `SQL mode` 歧义，请使用绑定；未知版本执行注释参数明确拒绝。
    pub fn select_raw(self, sql: RawSql, bindings: Vec<Value>) -> Result<Self> {
        self.transform(|state| state.select_raw(sql, bindings))?;
        Ok(self)
    }
}

fn raw_invalid() -> Error {
    Error::custom(
        "query_raw_bindings_invalid",
        "原始 SQL 的占位符与绑定不匹配",
    )
}

// 只识别占位符所在词法区间；SQL语法仍交由数据库，不在此解析或执行SQL。
pub(super) fn raw_expression(sql: &str, values: Vec<Value>, backend: DbBackend) -> Result<Expr> {
    let postgres = match backend {
        DbBackend::Postgres => true,
        DbBackend::MySql | DbBackend::Sqlite => false,
        _ => return Err(unsupported_backend()),
    };
    let bytes = sql.as_bytes();
    let mut index = 0;
    let mut start = 0;
    let mut raw = String::new();
    let mut pieces = Vec::new();
    let mut values = values.into_iter();
    while index < bytes.len() {
        let byte = bytes[index];
        if let Some(next) = skip_region(sql, index, backend)? {
            index = next;
            continue;
        }
        if byte == b'?' {
            if backend == DbBackend::Sqlite
                && (bytes.get(index + 1).is_some_and(u8::is_ascii_digit)
                    || (bytes.get(index + 1) == Some(&b'?')
                        && bytes.get(index + 2).is_some_and(u8::is_ascii_digit)))
            {
                return Err(native_bindings());
            }
            raw.push_str(&sql[start..index]);
            index += 1;
            if bytes.get(index) == Some(&b'?') {
                if !postgres {
                    return Err(native_bindings());
                }
                raw.push('?');
                index += 1;
            } else {
                if !raw.is_empty() {
                    pieces.push(Expr::cust(std::mem::take(&mut raw)));
                }
                pieces.push(Expr::val(values.next().ok_or_else(raw_invalid)?));
            }
            start = index;
        } else {
            index += 1;
        }
    }
    if values.next().is_some() {
        return Err(raw_invalid());
    }
    raw.push_str(&sql[start..]);
    if !raw.is_empty() {
        pieces.push(Expr::cust(raw));
    }
    if pieces.is_empty() {
        return Ok(Expr::cust(""));
    }
    // 模板仅含框架标记；原SQL作为Custom节点，不会被第二次解释$N或?。
    let template = if postgres {
        (1..=pieces.len())
            .map(|slot| format!("${slot}"))
            .collect::<Vec<_>>()
            .join(" ")
    } else {
        vec!["?"; pieces.len()].join(" ")
    };
    Ok(Expr::cust_with_exprs(template, pieces))
}

fn skip_region(sql: &str, index: usize, backend: DbBackend) -> Result<Option<usize>> {
    let bytes = sql.as_bytes();
    let byte = bytes[index];
    if matches!(byte, b'\'' | b'"' | b'`') || (backend == DbBackend::Sqlite && byte == b'[') {
        return quoted_end(sql, index, backend).map(Some);
    }
    let dash = byte == b'-'
        && bytes.get(index + 1) == Some(&b'-')
        && (backend != DbBackend::MySql
            || bytes
                .get(index + 2)
                .is_none_or(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control()));
    if dash || (backend == DbBackend::MySql && byte == b'#') {
        let mut end = index + 1;
        // PostgreSQL 把 CR 和 LF 都视为换行，MySQL 与 SQLite 的行注释只在 LF 处结束。
        while end < bytes.len()
            && bytes[end] != b'\n'
            && (backend != DbBackend::Postgres || bytes[end] != b'\r')
        {
            end += 1;
        }
        return Ok(Some(end));
    }
    if byte == b'/' && bytes.get(index + 1) == Some(&b'*') {
        let end = block_end(bytes, index, backend == DbBackend::Postgres)?;
        if backend == DbBackend::MySql && bytes.get(index + 2) == Some(&b'!') {
            let mut body = index + 3;
            while bytes.get(body).is_some_and(u8::is_ascii_digit) {
                body += 1;
            }
            // 无版本门控才能证明会执行；未知版本的参数不能静默占用外部绑定。
            if body == index + 3 {
                return Ok(Some(body));
            }
            if contains_code_question(&sql[body..end - 2], backend)? {
                return Err(Error::custom(
                    "query_raw_executable_comment_bindings",
                    "可执行注释是否生效依赖不可见的服务端版本，不能绑定其中参数",
                ));
            }
        }
        return Ok(Some(end));
    }
    if backend == DbBackend::Postgres && byte == b'$' && !identifier_before(sql, index) {
        if bytes.get(index + 1).is_some_and(u8::is_ascii_digit) {
            return Err(native_bindings());
        }
        return Ok(Some(dollar_end(sql, index)?.unwrap_or(index + 1)));
    }
    if backend == DbBackend::Sqlite
        && matches!(byte, b':' | b'@' | b'$' | b'#')
        && (byte != b'$' || !identifier_before(sql, index))
        && sqlite_variable(bytes, index)
    {
        return Err(native_bindings());
    }
    Ok(None)
}
// 对应锁定SQLite tokenizer CC_VARALPHA/CC_DOLLAR及IdChar；冒号对可先于名称。
fn sqlite_variable(bytes: &[u8], start: usize) -> bool {
    let mut index = start + 1;
    let mut named = false;
    while let Some(byte) = bytes.get(index) {
        if byte.is_ascii_alphanumeric() || matches!(*byte, b'_' | b'$') || !byte.is_ascii() {
            named = true;
            index += 1;
        } else if *byte == b':' && bytes.get(index + 1) == Some(&b':') {
            index += 2;
        } else {
            break;
        }
    }
    named
}
fn contains_code_question(sql: &str, backend: DbBackend) -> Result<bool> {
    let mut index = 0;
    while index < sql.len() {
        if let Some(next) = skip_region(sql, index, backend)? {
            index = next;
            continue;
        }
        if sql.as_bytes()[index] == b'?' {
            return Ok(true);
        }
        index += 1;
    }
    Ok(false)
}
fn quoted_end(sql: &str, mut index: usize, backend: DbBackend) -> Result<usize> {
    let bytes = sql.as_bytes();
    let quote = if bytes[index] == b'[' {
        b']'
    } else {
        bytes[index]
    };
    let escape = backend == DbBackend::Postgres
        && quote == b'\''
        && index > 0
        && matches!(bytes[index - 1], b'e' | b'E')
        && !identifier_before(sql, index - 1);
    index += 1;
    while index < bytes.len() {
        if bytes[index] == b'\\' {
            if backend == DbBackend::MySql && quote != b'`' {
                return Err(Error::custom(
                    "query_raw_sql_mode_ambiguous",
                    "原始 SQL 中的反斜线文本请改用值绑定",
                ));
            }
            if escape {
                index += 2;
                continue;
            }
            if backend == DbBackend::Postgres && quote == b'\'' {
                let start = index;
                while bytes.get(index) == Some(&b'\\') {
                    index += 1;
                }
                if !(index - start).is_multiple_of(2) && bytes.get(index) == Some(&quote) {
                    return Err(Error::custom(
                        "query_raw_sql_mode_ambiguous",
                        "PostgreSQL 普通字符串的引号边界随会话变化，请改用值绑定、美元引用或显式 E 字符串",
                    ));
                }
                continue;
            }
        }
        if bytes[index] == quote {
            index += 1;
            if index < bytes.len() && bytes[index] == quote {
                index += 1;
                continue;
            }
            if escape && let Some(next) = postgres_escape_continuation(bytes, index) {
                index = next;
                continue;
            }
            return Ok(index);
        }
        index += 1;
    }
    Err(raw_invalid())
}

// PostgreSQL quotecontinue 须跨 LF/CR，仅容许词法空白和行注释。
fn postgres_escape_continuation(bytes: &[u8], mut index: usize) -> Option<usize> {
    let mut newline = false;
    loop {
        match *bytes.get(index)? {
            b'\n' | b'\r' => {
                newline = true;
                index += 1;
            }
            b' ' | b'\t' | b'\x0b' | b'\x0c' => index += 1,
            b'-' if bytes.get(index + 1) == Some(&b'-') => {
                index += 2;
                while bytes
                    .get(index)
                    .is_some_and(|byte| !matches!(*byte, b'\n' | b'\r'))
                {
                    index += 1;
                }
            }
            b'\'' if newline => return Some(index + 1),
            _ => return None,
        }
    }
}

fn identifier_before(sql: &str, index: usize) -> bool {
    sql[..index]
        .chars()
        .next_back()
        .is_some_and(identifier_continue)
}
fn identifier_continue(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '_' | '$') || !character.is_ascii()
}
fn native_bindings() -> Error {
    Error::custom(
        "query_raw_native_bindings_unsupported",
        "原始 SQL 只接受统一问号参数模板",
    )
}
fn block_end(bytes: &[u8], mut index: usize, nested: bool) -> Result<usize> {
    index += 2;
    let mut depth = 1;
    while index < bytes.len() {
        if nested && bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'*') {
            depth += 1;
            index += 2;
        } else if bytes[index] == b'*' && bytes.get(index + 1) == Some(&b'/') {
            depth -= 1;
            index += 2;
            if depth == 0 {
                return Ok(index);
            }
        } else {
            index += 1;
        }
    }
    Err(raw_invalid())
}
fn dollar_end(sql: &str, index: usize) -> Result<Option<usize>> {
    let mut characters = sql[index + 1..].char_indices();
    let Some((_, first)) = characters.next() else {
        return Ok(None);
    };
    if first != '$' && !(first.is_ascii_alphabetic() || first == '_' || !first.is_ascii()) {
        return Ok(None);
    }
    let end = if first == '$' {
        index + 1
    } else {
        let mut delimiter_end = None;
        for (offset, character) in characters {
            if character == '$' {
                delimiter_end = Some(index + 1 + offset);
                break;
            }
            if !identifier_continue(character) {
                return Ok(None);
            }
        }
        let Some(end) = delimiter_end else {
            return Ok(None);
        };
        end
    };
    let delimiter = &sql[index..=end];
    let content = end + 1;
    let close = sql[content..].find(delimiter).ok_or_else(raw_invalid)?;
    Ok(Some(content + close + delimiter.len()))
}

#[cfg(test)]
#[path = "factory_tests.rs"]
mod tests;
