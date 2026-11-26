use openrust::{
    console::{
        AsOutput, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input, Output,
        OutputOptions,
    },
    foundation::Application,
    support::{
        Str,
        facades::{Artisan, Facade},
    },
};
use serde_json::{Value, json};

use crate::{
    gateway::Gateway,
    managed_keys::{KeyRecord, PREFIX, now},
};

pub(crate) fn register() -> Result<(), ConsoleError> {
    for (kind, signature) in [
        (
            Kind::Create,
            "ai:gateway:key:create {name : Key 名称} {group : 授权分组} {--expires-in= : 有效秒数} {--requests-per-minute= : 每分钟请求限额}",
        ),
        (Kind::Rotate, "ai:gateway:key:rotate {id : Key ID}"),
        (Kind::Revoke, "ai:gateway:key:revoke {id : Key ID}"),
        (
            Kind::List,
            "ai:gateway:key:list {--before= : 从该 Key ID 之前开始} {--limit= : 返回条数，默认 50}",
        ),
    ] {
        Artisan::add(Box::new(KeyCommand {
            kind,
            definition: CommandDefinition::from_signature(signature)?,
        }))?;
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum Kind {
    Create,
    Rotate,
    Revoke,
    List,
}

struct KeyCommand {
    kind: Kind,
    definition: CommandDefinition,
}

impl Command for KeyCommand {
    fn definition(&self) -> &CommandDefinition {
        &self.definition
    }

    fn handle<'a>(
        &'a self,
        _: &'a Application,
        input: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let gateway =
                Gateway::get_facade_root().map_err(|_| ConsoleError::new("请启用 AI 网关"))?;
            let store = gateway
                .managed_keys
                .as_ref()
                .ok_or_else(|| ConsoleError::new("未启用数据库托管 Key"))?;
            let result = match self.kind {
                Kind::Create => {
                    let name = required(input, "name")?;
                    let group = required(input, "group")?;
                    if name.trim().is_empty()
                        || name.chars().count() > 128
                        || name.chars().any(char::is_control)
                    {
                        return Err(ConsoleError::new("Key 名称无效"));
                    }
                    if !gateway.has_group(group) {
                        return Err(ConsoleError::new("网关分组未配置"));
                    }
                    let created_at = now();
                    let expires_at = positive(input, "expires-in")?
                        .map(|ttl| {
                            created_at
                                .checked_add(ttl)
                                .ok_or_else(|| ConsoleError::new("Key 有效期过长"))
                        })
                        .transpose()?;
                    let key = KeyRecord {
                        id: Str::ulid().to_string(),
                        name: name.into(),
                        group: group.into(),
                        requests_per_minute: positive(input, "requests-per-minute")?,
                        created_at,
                        updated_at: created_at,
                        expires_at,
                        revoked_at: None,
                    };
                    let token = credential()?;
                    store.create(&key, &token).await.map_err(storage_error)?;
                    json!({"id":key.id,"token":token})
                }
                Kind::Rotate => {
                    let id = identifier(required(input, "id")?)?;
                    let token = credential()?;
                    if !store.rotate(id, &token).await.map_err(storage_error)? {
                        return Err(ConsoleError::new("Key 已失效，或其状态已更新"));
                    }
                    json!({"id":id,"token":token})
                }
                Kind::Revoke => {
                    let id = identifier(required(input, "id")?)?;
                    if !store.revoke(id).await.map_err(storage_error)? {
                        return Err(ConsoleError::new("Key 不存在或已撤销"));
                    }
                    json!({"id":id,"revoked":true})
                }
                Kind::List => {
                    let before = argument(input, "before").map(identifier).transpose()?;
                    let limit = positive(input, "limit")?.unwrap_or(50);
                    if limit > 100 {
                        return Err(ConsoleError::new("每页最多返回 100 个 Key"));
                    }
                    let keys = store
                        .list(before, limit as u64)
                        .await
                        .map_err(storage_error)?;
                    let next = keys
                        .last()
                        .filter(|_| keys.len() == limit as usize)
                        .map(|key| &key.id);
                    json!({"keys":keys,"next":next})
                }
            };
            write_json(output, result)?;
            Ok(0)
        })
    }
}

fn credential() -> Result<String, ConsoleError> {
    let uuid = || Str::uuid().map_err(|_| ConsoleError::new("无法生成网关凭证"));
    Ok(format!("{PREFIX}{}{}", uuid()?.simple(), uuid()?.simple()))
}

fn argument<'a>(input: &'a Input, name: &str) -> Option<&'a str> {
    input.matches().get_one::<String>(name).map(String::as_str)
}

fn required<'a>(input: &'a Input, name: &str) -> Result<&'a str, ConsoleError> {
    argument(input, name).ok_or_else(|| ConsoleError::new(format!("缺少参数 {name}")))
}

fn positive(input: &Input, name: &str) -> Result<Option<i64>, ConsoleError> {
    argument(input, name)
        .map(|value| {
            value
                .parse::<i64>()
                .ok()
                .filter(|number| *number > 0)
                .ok_or_else(|| ConsoleError::new(format!("选项 --{name} 必须为正整数")))
        })
        .transpose()
}

fn identifier(id: &str) -> Result<&str, ConsoleError> {
    let digits = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    if id.len() == 26 && id.bytes().all(|byte| digits.contains(&byte)) {
        Ok(id)
    } else {
        Err(ConsoleError::new("Key ID 无效"))
    }
}

fn storage_error(_: openrust::database::DatabaseError) -> ConsoleError {
    ConsoleError::new("无法访问网关 Key 存储，请检查迁移和数据库连接")
}

fn write_json(output: &mut Output, value: Value) -> Result<(), ConsoleError> {
    output
        .as_output()
        .writeln(&value.to_string(), OutputOptions::default())?;
    Ok(())
}
