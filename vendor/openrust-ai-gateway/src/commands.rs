use openrust::{
    console::{
        AsOutput, AsyncCommandCallback, Command, CommandDefinition, ConsoleError, ConsoleFuture,
        Input, Output, OutputOptions,
    },
    foundation::Application,
    support::{
        Str,
        facades::{Artisan, Facade},
    },
};
use serde_json::{Value, json};

use crate::{
    billing::adjustments::Direction,
    budget::BudgetScope,
    gateway::Gateway,
    localization::NAMESPACE,
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
            definition: CommandDefinition::from_signature(signature)?
                .with_help_namespace(NAMESPACE),
        }))?;
    }
    Artisan::command_with(
        "ai:gateway:usage:list {--request-id= : 请求 ID} {--key-id= : 托管 Key ID} {--before= : 用量记录游标} {--limit= : 返回条数，默认 50} {--billing : 附加计费快照与金额} {--invocation-id= : SDK 调用 ID} {--invocation-scope= : 调用方作用域}",
        AsyncCommandCallback::new(|_, input, output| {
            Box::pin(async move {
                let gateway =
                    Gateway::get_facade_root().map_err(|_| ConsoleError::new("请启用 AI 网关"))?;
                let store = gateway
                    .usage_store
                    .as_ref()
                    .ok_or_else(|| ConsoleError::new("未启用网关用量记录"))?;
                let request = argument(input, "request-id").map(identifier).transpose()?;
                let key = argument(input, "key-id").map(identifier).transpose()?;
                let before = argument(input, "before").map(identifier).transpose()?;
                let limit = positive(input, "limit")?.unwrap_or(50);
                if limit > 100 {
                    return Err(ConsoleError::new("每页最多返回 100 条用量记录"));
                }
                let invocation = argument(input, "invocation-id")
                    .map(identifier)
                    .transpose()?;
                let invocation_scope = argument(input, "invocation-scope")
                    .map(|scope| {
                        if scope.len() == 64 && scope.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                            Ok(scope.to_ascii_lowercase())
                        } else {
                            Err(ConsoleError::new("调用方作用域必须为 64 位十六进制值"))
                        }
                    })
                    .transpose()?;
                let invocation = invocation.map(str::to_ascii_uppercase);
                let (records, charges) = store
                    .list(
                        request,
                        key,
                        before,
                        (invocation.as_deref(), invocation_scope.as_deref()),
                        limit as u64,
                        input.matches().get_flag("billing"),
                    )
                    .await
                    .map_err(|_| ConsoleError::new("无法读取网关用量记录"))?;
                let next = records
                    .last()
                    .filter(|_| records.len() == limit as usize)
                    .map(|record| &record.id);
                let mut result = json!({"usage":records,"next":next});
                if let Some(charges) = charges {
                    result["charges"] = json!(charges);
                }
                write_json(output, result)?;
                Ok(0)
            })
        }),
    )?.with_help_namespace(NAMESPACE)?;
    Artisan::command_with(
        "ai:gateway:billing:import {file : 供应商账单 JSON 文件} {--actor= : 审计操作者}",
        AsyncCommandCallback::new(|_, input, output| {
            Box::pin(async move {
                let gateway =
                    Gateway::get_facade_root().map_err(|_| ConsoleError::new("请启用 AI 网关"))?;
                let store = gateway
                    .usage_store
                    .as_ref()
                    .ok_or_else(|| ConsoleError::new("未启用网关用量记录"))?;
                let path =
                    argument(input, "file").ok_or_else(|| ConsoleError::new("请指定账单文件"))?;
                let actor = argument(input, "actor")
                    .filter(|actor| !actor.is_empty())
                    .ok_or_else(|| ConsoleError::new("请用 --actor 指定审计操作者"))?;
                let file = std::fs::File::open(path)
                    .map_err(|_| ConsoleError::new("无法读取供应商账单文件"))?;
                let report = serde_json::from_reader(std::io::BufReader::new(file))
                    .map_err(|_| ConsoleError::new("供应商账单 JSON 格式无效"))?;
                let result = store
                    .import_cost_report(report, actor.into())
                    .await
                    .map_err(|error| match error {
                        openrust::database::DatabaseError::InvalidQuery(reason) => {
                            ConsoleError::new(reason)
                        }
                        _ => ConsoleError::new("无法导入网关账单"),
                    })?;
                write_json(output, json!(result))?;
                Ok(0)
            })
        }),
    )?
    .with_help_namespace(NAMESPACE)?;
    Artisan::command_with(
        "ai:gateway:billing:adjust {file : 费用调整 JSON 文件} {--actor= : 审计操作者}",
        AsyncCommandCallback::new(|_, input, output| {
            Box::pin(async move {
                let gateway =
                    Gateway::get_facade_root().map_err(|_| ConsoleError::new("请启用 AI 网关"))?;
                let store = gateway
                    .usage_store
                    .as_ref()
                    .ok_or_else(|| ConsoleError::new("未启用网关用量记录"))?;
                let path = argument(input, "file")
                    .ok_or_else(|| ConsoleError::new("请指定费用调整文件"))?;
                let actor = argument(input, "actor")
                    .filter(|actor| !actor.is_empty())
                    .ok_or_else(|| ConsoleError::new("请用 --actor 指定审计操作者"))?;
                let file = std::fs::File::open(path)
                    .map_err(|_| ConsoleError::new("无法读取费用调整文件"))?;
                let adjustment = serde_json::from_reader(std::io::BufReader::new(file))
                    .map_err(|_| ConsoleError::new("费用调整 JSON 格式无效"))?;
                let result = store
                    .apply_adjustment(adjustment, actor.into())
                    .await
                    .map_err(|error| match error {
                        openrust::database::DatabaseError::InvalidQuery(reason) => {
                            ConsoleError::new(reason)
                        }
                        _ => ConsoleError::new("无法写入网关费用调整"),
                    })?;
                write_json(output, json!(result))?;
                Ok(0)
            })
        }),
    )?
    .with_help_namespace(NAMESPACE)?;
    Artisan::command_with(
        "ai:gateway:billing:report {id : 导入返回的供应商账单 ID}",
        AsyncCommandCallback::new(|_, input, output| {
            Box::pin(async move {
                let gateway =
                    Gateway::get_facade_root().map_err(|_| ConsoleError::new("请启用 AI 网关"))?;
                let store = gateway
                    .usage_store
                    .as_ref()
                    .ok_or_else(|| ConsoleError::new("未启用网关用量记录"))?;
                let id = argument(input, "id")
                    .filter(|id| id.len() == 64 && id.bytes().all(|byte| byte.is_ascii_hexdigit()))
                    .ok_or_else(|| ConsoleError::new("供应商账单 ID 必须为 64 位十六进制值"))?;
                let report = store
                    .cost_report(id.to_ascii_lowercase())
                    .await
                    .map_err(|_| ConsoleError::new("无法读取供应商账单记录"))?
                    .ok_or_else(|| ConsoleError::new("未找到供应商账单记录"))?;
                write_json(output, json!(report))?;
                Ok(0)
            })
        }),
    )?
    .with_help_namespace(NAMESPACE)?;
    Artisan::command_with(
        "ai:gateway:budget:show {group : 分组或连接名称} {--account : 按上游连接查询}",
        AsyncCommandCallback::new(|_, input, output| {
            Box::pin(async move {
                let gateway =
                    Gateway::get_facade_root().map_err(|_| ConsoleError::new("请启用 AI 网关"))?;
                let scope = budget_scope(input)?;
                let budget = match scope {
                    BudgetScope::Group(group) => gateway.group_budget(group),
                    BudgetScope::Account(account) => gateway.account_budget(account),
                }
                .ok_or_else(|| ConsoleError::new("此分组或连接未配置货币预算"))?;
                let store = gateway
                    .usage_store
                    .as_ref()
                    .ok_or_else(|| ConsoleError::new("未启用网关用量记录"))?;
                let balance = store
                    .budget_balance(scope, budget)
                    .await
                    .map_err(|_| ConsoleError::new("无法读取网关预算余额"))?;
                write_json(output, json!(balance))?;
                Ok(0)
            })
        }),
    )?
    .with_help_namespace(NAMESPACE)?;
    Artisan::command_with(
        "ai:gateway:budget:reconcile {attempt-id : 用量尝试 ID} {--direction= : cost 或 receivable} {--reference= : 核对凭据编号} {--actor= : 审计操作者}",
        AsyncCommandCallback::new(|_, input, output| {
            Box::pin(async move {
                let gateway =
                    Gateway::get_facade_root().map_err(|_| ConsoleError::new("请启用 AI 网关"))?;
                let store = gateway
                    .usage_store
                    .as_ref()
                    .ok_or_else(|| ConsoleError::new("未启用网关用量记录"))?;
                let id = required(input, "attempt-id")?.to_ascii_uppercase();
                identifier(&id).map_err(|_| ConsoleError::new("用量尝试 ID 无效"))?;
                let direction = match argument(input, "direction") {
                    Some("cost") => Direction::Cost,
                    Some("receivable") => Direction::Receivable,
                    _ => return Err(ConsoleError::new("请用 --direction 指定费用方向")),
                };
                let reference = required(input, "reference")?.to_owned();
                let actor = required(input, "actor")?.to_owned();
                let result = store
                    .reconcile_budget(id, direction, reference, actor)
                    .await
                    .map_err(|error| match error {
                        openrust::database::DatabaseError::InvalidQuery(reason) => {
                            ConsoleError::new(reason)
                        }
                        _ => ConsoleError::new("无法核对网关预算"),
                    })?;
                write_json(output, json!(result))?;
                Ok(0)
            })
        }),
    )?.with_help_namespace(NAMESPACE)?;
    Artisan::command_with(
        "ai:gateway:budget:recover {group : 分组或连接名称} {--account : 按上游连接恢复} {--after= : 最短等待秒数，默认 300} {--limit= : 最多处理条数，默认 50}",
        AsyncCommandCallback::new(|_, input, output| {
            Box::pin(async move {
                let gateway =
                    Gateway::get_facade_root().map_err(|_| ConsoleError::new("请启用 AI 网关"))?;
                let scope = budget_scope(input)?;
                let store = gateway
                    .usage_store
                    .as_ref()
                    .ok_or_else(|| ConsoleError::new("未启用网关用量记录"))?;
                let age = positive(input, "after")?.unwrap_or(300);
                let limit = positive(input, "limit")?.unwrap_or(50);
                if limit > 100 {
                    return Err(ConsoleError::new("每次最多处理 100 条用量记录"));
                }
                let released = store
                    .recover_budget(scope, age, limit as u64)
                    .await
                    .map_err(|_| ConsoleError::new("无法恢复未出站的预算预占"))?;
                write_json(output, json!({"released": released}))?;
                Ok(0)
            })
        }),
    )?.with_help_namespace(NAMESPACE)?;
    Ok(())
}

fn budget_scope(input: &Input) -> Result<BudgetScope<'_>, ConsoleError> {
    let name = argument(input, "group").ok_or_else(|| ConsoleError::new("请指定分组或连接名称"))?;
    Ok(if input.matches().get_flag("account") {
        BudgetScope::Account(name)
    } else {
        BudgetScope::Group(name)
    })
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
