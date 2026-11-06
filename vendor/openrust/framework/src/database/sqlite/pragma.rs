//! Laravel PRAGMA 标量插值与 PDO SQLite 的首语句边界。
//!
//! 扫描仅覆盖 PRAGMA 输入域的引号、注释和首句边界，不是完整 SQLite lexer。
//! PRAGMA grammar 不接受 VARIABLE；`$name(...)` 的内部词法未作通用建模，
//! 保留其非法前缀交给原生 parser 拒绝，不以此执行额外尾部语句。

use serde_json::Value;

/// 保留 SQLite token 语义，但不把 PDO 忽略的后续语句交给 SQLx 执行。
pub(super) fn statement(name: &str, value: &Value) -> crate::Result<String> {
    let scalar = match value {
        Value::String(value) => value.clone(),
        Value::Number(value) => value.to_string(),
        Value::Bool(true) => "1".to_owned(),
        Value::Bool(false) | Value::Null => String::new(),
        Value::Array(_) | Value::Object(_) => {
            return Err(crate::config::ConfigError::diagnostic(
                "database.pragmas",
                "invalid_sqlite_pragma_value",
            )
            .into());
        }
    };
    Ok(first_statement(format!("PRAGMA {name} = {scalar}")))
}

#[derive(Clone, Copy)]
enum State {
    Sql,
    Quoted(u8),
    Bracket,
    LineComment,
    BlockComment,
    Invalid,
}

/// 隔离首句的 EOF 注释和错误 token，避免改变下一条初始化语句。
pub(super) fn batch_fragment(sql: &str) -> String {
    let boundary = boundary(sql);
    let first = &sql[..boundary.end];
    if boundary.terminated {
        return format!("{first}\n");
    }
    match boundary.state {
        State::Sql => format!("{first};\n"),
        State::LineComment => format!("{first}\n;\n"),
        State::BlockComment => format!("{first}*/;\n"),
        // SQLite 单句应在这里失败；不能让后续配置中的引号偶然修复它。
        State::Quoted(_) | State::Bracket | State::Invalid => "PRAGMA = ;\n".to_owned(),
    }
}

struct Boundary {
    end: usize,
    state: State,
    terminated: bool,
}

fn first_statement(mut sql: String) -> String {
    sql.truncate(boundary(&sql).end);
    sql
}

fn boundary(sql: &str) -> Boundary {
    let bytes = sql.as_bytes();
    let mut state = State::Sql;
    let mut position = 0;
    let mut end = bytes.len();
    let mut terminated = false;
    while position < bytes.len() {
        let byte = bytes[position];
        // SQLite 的 UTF-8 prepare 接口即使带显式长度，也将零字节视为 SQL 终点。
        if byte == 0 {
            end = position;
            break;
        }
        let next = bytes.get(position + 1).copied();
        match state {
            State::Sql => match (byte, next) {
                (b';', _) => {
                    end = position + 1;
                    terminated = true;
                    break;
                }
                (b'\'' | b'"' | b'`', _) => state = State::Quoted(byte),
                (b'[', _) => state = State::Bracket,
                (b'-', Some(b'-')) => {
                    state = State::LineComment;
                    position += 1;
                }
                (b'/', Some(b'*')) => {
                    // SQLite CC_SLASH 仅在第三字节存在且非 NUL 时识别块注释。
                    // 裸 /* 必须维持语法错误，不能在 batch 追加字符后变成有效注释。
                    if bytes.get(position + 2).is_none_or(|byte| *byte == 0) {
                        state = State::Invalid;
                    } else {
                        state = State::BlockComment;
                    }
                    position += 1;
                }
                _ => {}
            },
            State::Quoted(quote) if byte == quote => {
                if next == Some(quote) {
                    position += 1;
                } else {
                    state = State::Sql;
                }
            }
            // SQLite 方括号名称由第一个 ] 结束；不存在 ]] 转义规则。
            State::Bracket if byte == b']' => state = State::Sql,
            // SQLite -- 注释只在 LF 结束；CR 本身仍属于注释。
            State::LineComment if byte == b'\n' => state = State::Sql,
            State::BlockComment if byte == b'*' && next == Some(b'/') => {
                state = State::Sql;
                position += 1;
            }
            _ => {}
        }
        position += 1;
    }
    Boundary {
        end,
        state,
        terminated,
    }
}

#[cfg(test)]
mod tests {
    use super::{batch_fragment, statement};
    use serde_json::{Value, json};

    #[test]
    fn executable_tail_from_value_or_name_is_not_returned() -> crate::Result<()> {
        assert_eq!(
            statement("synchronous", &json!("OFF; DROP TABLE accounts;"))?,
            "PRAGMA synchronous = OFF;"
        );
        assert_eq!(
            statement("foreign_keys=OFF; DROP TABLE accounts;--", &json!(1))?,
            "PRAGMA foreign_keys=OFF;"
        );
        Ok(())
    }

    #[test]
    fn quoted_values_and_identifiers_keep_semicolons_and_doubled_quotes() -> crate::Result<()> {
        for value in ["'a;''b'", "\"a;\"\"b\"", "`a;``b`", "[a;b]"] {
            assert_eq!(
                statement("main.synchronous", &json!(value))?,
                format!("PRAGMA main.synchronous = {value}")
            );
        }
        assert_eq!(
            statement(
                "\"main\".\"sync;hronous\"",
                &json!("'FULL'; DROP TABLE accounts")
            )?,
            "PRAGMA \"main\".\"sync;hronous\" = 'FULL';"
        );
        Ok(())
    }

    #[test]
    fn comments_preserve_semicolons_without_hiding_later_executable_tail() -> crate::Result<()> {
        assert_eq!(
            statement(
                "synchronous",
                &json!("/* ; ' \" */ FULL; DROP TABLE accounts")
            )?,
            "PRAGMA synchronous = /* ; ' \" */ FULL;"
        );
        assert_eq!(
            statement(
                "synchronous",
                &json!("FULL -- ; ' \"\r still comment\n; DROP TABLE accounts")
            )?,
            "PRAGMA synchronous = FULL -- ; ' \"\r still comment\n;"
        );
        // 未闭合注释/引号交由 SQLite 判定，不能擅自补齐或改变其 token。
        assert_eq!(
            statement("synchronous", &json!("'unterminated; DROP TABLE accounts"))?,
            "PRAGMA synchronous = 'unterminated; DROP TABLE accounts"
        );
        assert_eq!(
            statement("synchronous", &json!("FULL /* ; DROP TABLE accounts"))?,
            "PRAGMA synchronous = FULL /* ; DROP TABLE accounts"
        );
        Ok(())
    }

    #[test]
    fn nul_terminates_even_inside_quotes_or_comments() -> crate::Result<()> {
        for (value, expected) in [
            ("FULL\0; DROP TABLE accounts", "FULL"),
            ("'FULL\0'; DROP TABLE accounts", "'FULL"),
            ("/* \0 */; DROP TABLE accounts", "/* "),
            ("-- \0\n; DROP TABLE accounts", "-- "),
        ] {
            assert_eq!(
                statement("synchronous", &json!(value))?,
                format!("PRAGMA synchronous = {expected}")
            );
        }
        assert_eq!(
            statement("foreign_keys\0; DROP TABLE accounts", &json!(1))?,
            "PRAGMA foreign_keys"
        );
        Ok(())
    }

    #[test]
    fn scalars_keep_laravel_interpolation_and_containers_are_rejected() -> crate::Result<()> {
        assert_eq!(
            statement("foreign_keys", &json!(true))?,
            "PRAGMA foreign_keys = 1"
        );
        assert_eq!(
            statement("foreign_keys", &json!(false))?,
            "PRAGMA foreign_keys = "
        );
        assert_eq!(
            statement("foreign_keys", &Value::Null)?,
            "PRAGMA foreign_keys = "
        );
        assert_eq!(
            statement("synchronous", &json!(2))?,
            "PRAGMA synchronous = 2"
        );
        assert_eq!(
            statement("busy_timeout", &json!(-1))?,
            "PRAGMA busy_timeout = -1"
        );
        assert_eq!(
            statement("cache_size", &json!(1.25))?,
            "PRAGMA cache_size = 1.25"
        );
        for value in [json!([]), json!({"secret": "marker"})] {
            let error = statement("cache_size", &value)
                .err()
                .ok_or_else(|| crate::Error::custom("missing_error", "container accepted"))?;
            assert!(!format!("{error} {error:?}").contains("marker"));
        }
        Ok(())
    }

    #[test]
    fn sqlite_does_not_treat_backslashes_or_nested_comments_as_escapes() -> crate::Result<()> {
        assert_eq!(
            statement("synchronous", &json!("'a\\'; DROP TABLE accounts"))?,
            "PRAGMA synchronous = 'a\\';"
        );
        assert_eq!(
            statement(
                "synchronous",
                &json!("/* outer /* nested */ FULL; DROP TABLE accounts")
            )?,
            "PRAGMA synchronous = /* outer /* nested */ FULL;"
        );
        assert_eq!(
            statement("[a]]; DROP TABLE accounts", &json!(1))?,
            "PRAGMA [a]];"
        );
        Ok(())
    }

    #[test]
    fn batch_fragments_stop_at_first_statement_and_always_end_on_new_line() {
        assert_eq!(
            batch_fragment("PRAGMA synchronous=FULL; DROP TABLE secret"),
            "PRAGMA synchronous=FULL;\n"
        );
        assert_eq!(
            batch_fragment("PRAGMA synchronous='FULL;'; -- tail"),
            "PRAGMA synchronous='FULL;';\n"
        );
        assert_eq!(
            batch_fragment("PRAGMA synchronous=FULL"),
            "PRAGMA synchronous=FULL;\n"
        );
        assert_eq!(
            batch_fragment("PRAGMA synchronous=FULL\0; DROP TABLE secret"),
            "PRAGMA synchronous=FULL;\n"
        );
    }

    #[test]
    fn eof_comments_are_closed_without_swallowing_next_pragma() {
        for (input, expected) in [
            (
                "PRAGMA synchronous=FULL -- ;",
                "PRAGMA synchronous=FULL -- ;\n;\n",
            ),
            (
                "PRAGMA synchronous=FULL /* ;",
                "PRAGMA synchronous=FULL /* ;*/;\n",
            ),
            (
                "PRAGMA synchronous=FULL -- ignored\0 hidden",
                "PRAGMA synchronous=FULL -- ignored\n;\n",
            ),
            (
                "PRAGMA synchronous=FULL /* ignored\0 hidden",
                "PRAGMA synchronous=FULL /* ignored*/;\n",
            ),
        ] {
            assert_eq!(batch_fragment(input), expected);
            let batch = format!("{}PRAGMA foreign_keys=ON;\n", batch_fragment(input));
            // 同一边界扫描确认下一条位于首句之后，而非仍在注释内。
            let first = super::boundary(&batch);
            assert!(first.terminated);
            assert_eq!(&batch[first.end..], "\nPRAGMA foreign_keys=ON;\n");
        }
    }

    #[test]
    fn malformed_quotes_fail_without_being_closed_by_later_batch_values() {
        for input in [
            "PRAGMA synchronous='private-secret",
            "PRAGMA synchronous=\"private-secret",
            "PRAGMA `private-secret",
            "PRAGMA [private-secret",
            "PRAGMA synchronous='private-secret\0'",
            "PRAGMA [private-secret\0]",
        ] {
            let isolated = batch_fragment(input);
            assert_eq!(isolated, "PRAGMA = ;\n");
            assert!(!isolated.contains("private-secret"));
            let batch = format!("{isolated}PRAGMA synchronous='FULL';\n");
            let first = super::boundary(&batch);
            assert!(first.terminated);
            assert_eq!(&batch[first.end..], "\nPRAGMA synchronous='FULL';\n");
        }
    }

    #[test]
    fn bare_block_opener_stays_invalid_through_statement_then_batch() -> crate::Result<()> {
        for value in ["FULL /*", "FULL /*\0", "FULL /*\0 hidden", "FULL\0/*"] {
            let first = statement("synchronous", &json!(value))?;
            if value.starts_with("FULL /*") {
                assert_eq!(first, "PRAGMA synchronous = FULL /*");
                assert_eq!(batch_fragment(&first), "PRAGMA = ;\n");
            } else {
                assert_eq!(batch_fragment(&first), "PRAGMA synchronous = FULL;\n");
            }
        }
        assert_eq!(batch_fragment("PRAGMA synchronous=FULL /*"), "PRAGMA = ;\n");
        let first = statement("synchronous", &json!("FULL /*;"))?;
        assert_eq!(first, "PRAGMA synchronous = FULL /*;");
        assert_eq!(batch_fragment(&first), "PRAGMA synchronous = FULL /*;*/;\n");
        // 非 NUL 第三字节使其成为真正的 EOF 块注释，分号仍在注释内。
        let first = statement("synchronous", &json!("FULL /*;\0 hidden"))?;
        assert_eq!(first, "PRAGMA synchronous = FULL /*;");
        assert_eq!(batch_fragment(&first), "PRAGMA synchronous = FULL /*;*/;\n");
        Ok(())
    }

    #[test]
    fn invalid_variable_prefix_never_returns_executable_tail() -> crate::Result<()> {
        for (value, prefix) in [
            ("$name(; DROP TABLE secret)", "$name("),
            ("$name::suffix(; DROP TABLE secret)", "$name::suffix("),
            ("$name(unclosed; DROP TABLE secret", "$name(unclosed"),
        ] {
            let first = statement("synchronous", &json!(value))?;
            assert_eq!(first, format!("PRAGMA synchronous = {prefix};"));
            let isolated = batch_fragment(&first);
            // PRAGMA 值域不接受 VARIABLE；保留原生错误前缀而非把它清洗为有效配置。
            assert!(isolated.starts_with("PRAGMA synchronous = $name"));
            assert!(!isolated.contains("DROP TABLE"));
            let batch = format!("{isolated}PRAGMA foreign_keys=ON;\n");
            let boundary = super::boundary(&batch);
            assert!(boundary.terminated);
            assert_eq!(&batch[boundary.end..], "\nPRAGMA foreign_keys=ON;\n");
        }
        Ok(())
    }
}
