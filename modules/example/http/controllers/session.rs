//! 示例偏好与一次性提示通过公开 Session API 保存。

use openrust::http::axum::http::StatusCode;
use openrust::http::{Json, Request};
use openrust::session::Session;
use openrust::{Error, Result};
use serde_json::{Value, json};

const PREFERENCE: &str = "example.session.preference";
const NOTICE: &str = "example.session.notice";
const MEMO: &str = "example.session.memo";
const LIST: &str = "example.session.list";
const COUNT: &str = "example.session.count";
const MAX_VALUE_BYTES: usize = 256;
const MAX_LIST_ITEMS: usize = 8;

/// 仅读取示例自己的偏好与提示，不暴露会话身份或其他业务属性。
pub(crate) async fn show(session: Session) -> Result<Json<Value>> {
    Ok(Json(snapshot(&session)?))
}

/// 用固定操作维护有限偏好及提示；正文不能指定任意 Session 键。
pub(crate) async fn store(session: Session, request: Request) -> Result<Json<Value>> {
    let Some(Value::String(action)) = request.input("action")? else {
        return Err(action_invalid());
    };
    let mut removed = None;
    match action.as_str() {
        "put" => {
            session.put(PREFERENCE, input_value(&request)?)?;
        }
        "forget" => {
            session.forget(&[PREFERENCE])?;
        }
        "flash" => {
            session.flash(NOTICE, input_value(&request)?)?;
        }
        "now" => {
            session.now(NOTICE, input_value(&request)?)?;
        }
        "keep" => {
            // reflash 会延长所有业务的提示；此处只保留示例自己的提示。
            session.keep(&[NOTICE])?;
        }
        "memo_null" => {
            session.put(MEMO, Value::Null)?;
        }
        "memo_seed" => {
            session.put(MEMO, input_value(&request)?)?;
        }
        "remember" => {
            session.remember(MEMO, || Ok("computed"))?;
        }
        "push" => {
            let list = session.get(LIST)?;
            let size = match list {
                None | Some(Value::Null) => 0,
                Some(Value::Array(items)) => items.len(),
                _ => return Err(state_invalid()),
            };
            if size >= MAX_LIST_ITEMS {
                return Err(Error::custom_with_status(
                    StatusCode::BAD_REQUEST,
                    "example_session_list_full",
                    "示例 Session 列表已满",
                ));
            }
            session.push(LIST, input_value(&request)?)?;
        }
        "increment" => {
            session.increment(COUNT)?;
        }
        "increment_by" => {
            session.increment_by(COUNT, 2)?;
        }
        "decrement" => {
            session.decrement(COUNT)?;
        }
        "decrement_by" => {
            session.decrement_by(COUNT, 2)?;
        }
        "remove" => {
            removed = Some(session.remove(MEMO)?);
        }
        _ => return Err(action_invalid()),
    }
    let mut data = snapshot(&session)?;
    data["action"] = Value::String(action);
    if let Some(value) = removed {
        data["removed"] = json!(value);
    }
    Ok(Json(data))
}

fn input_value(request: &Request) -> Result<String> {
    match request.input("value")? {
        Some(Value::String(value)) if value.len() <= MAX_VALUE_BYTES => Ok(value),
        _ => Err(Error::custom_with_status(
            StatusCode::BAD_REQUEST,
            "example_session_value_invalid",
            "示例 Session 值必须是不超过 256 UTF-8 字节的字符串",
        )),
    }
}

fn snapshot(session: &Session) -> Result<Value> {
    Ok(json!({
        "preference": owned_value(session, PREFERENCE)?,
        "notice": owned_value(session, NOTICE)?,
        "memo": owned_value(session, MEMO)?,
        "list": owned_list(session)?,
        "count": owned_count(session)?,
        "has": {
            "preference": session.has(&[PREFERENCE])?,
            "notice": session.has(&[NOTICE])?,
        },
        "exists": {
            "preference": session.exists(&[PREFERENCE])?,
            "notice": session.exists(&[NOTICE])?,
        },
    }))
}

fn owned_list(session: &Session) -> Result<Option<Vec<String>>> {
    match session.get(LIST)? {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Array(items)) if items.len() <= MAX_LIST_ITEMS => items
            .into_iter()
            .map(|item| match item {
                Value::String(value) if value.len() <= MAX_VALUE_BYTES => Ok(value),
                _ => Err(state_invalid()),
            })
            .collect::<Result<Vec<_>>>()
            .map(Some),
        _ => Err(state_invalid()),
    }
}

fn owned_count(session: &Session) -> Result<Option<i64>> {
    match session.get(COUNT)? {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(value)) => value.as_i64().map(Some).ok_or_else(state_invalid),
        _ => Err(state_invalid()),
    }
}

fn state_invalid() -> Error {
    Error::custom_with_status(
        StatusCode::INTERNAL_SERVER_ERROR,
        "example_session_state_invalid",
        "示例 Session 状态无效",
    )
}

fn owned_value(session: &Session, key: &str) -> Result<Option<Value>> {
    match session.get(key)? {
        value @ (None | Some(Value::Null)) => Ok(value),
        Some(Value::String(value)) if value.len() <= MAX_VALUE_BYTES => {
            Ok(Some(Value::String(value)))
        }
        _ => Err(Error::custom_with_status(
            StatusCode::INTERNAL_SERVER_ERROR,
            "example_session_state_invalid",
            "示例 Session 偏好状态无效",
        )),
    }
}

fn action_invalid() -> Error {
    Error::custom_with_status(
        StatusCode::BAD_REQUEST,
        "example_session_action_invalid",
        "无效的示例 Session 操作",
    )
}
