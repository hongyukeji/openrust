use openrust::database::{DatabaseError, DatabaseResult, Transaction, query::Record};
use openrust_ai::TextEndpoint;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{managed_keys::now, session_affinity};

const TABLE: &str = "ai_gateway_output_quota";
const DAY: i64 = 86_400;
// 数据库统一使用有符号整数，为单次回报超额预留安全范围。
const MAX_DAILY: i64 = i64::MAX / 4;

#[derive(Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OutputQuota {
    pub(crate) output_tokens_per_day: i64,
    pub(crate) max_output_tokens: i64,
}

impl OutputQuota {
    pub(crate) fn valid(self) -> bool {
        (1..=MAX_DAILY).contains(&self.output_tokens_per_day)
            && (1..=self.output_tokens_per_day).contains(&self.max_output_tokens)
    }

    pub(crate) fn prepare(
        self,
        endpoint: TextEndpoint,
        native_openai: bool,
        payload: &mut Value,
    ) -> Result<Option<i64>, &'static str> {
        if endpoint.is_counting()
            || endpoint == TextEndpoint::Embeddings
            || endpoint.is_gemini_embedding()
        {
            return Ok(None);
        }
        limit_output(self.max_output_tokens, endpoint, native_openai, payload).map(Some)
    }
}

pub(crate) fn limit_output(
    max_output_tokens: i64,
    endpoint: TextEndpoint,
    native_openai: bool,
    payload: &mut Value,
) -> Result<i64, &'static str> {
    let fields: &[&str] = match endpoint {
        TextEndpoint::ChatCompletions if native_openai => &["max_completion_tokens", "max_tokens"],
        TextEndpoint::ChatCompletions => &["max_tokens", "max_completion_tokens"],
        TextEndpoint::Responses => &["max_output_tokens"],
        TextEndpoint::Messages => &["max_tokens"],
        _ => return Err("分组输出配额不支持此上游生成端点"),
    };
    let data = payload.as_object_mut().ok_or("上游请求格式无效")?;
    let present: Vec<_> = fields
        .iter()
        .copied()
        .filter(|field| data.get(*field).is_some_and(|value| !value.is_null()))
        .collect();
    if present.len() > 1 {
        return Err("请只设置一种输出 Token 限制");
    }
    let field = present.first().copied().unwrap_or(fields[0]);
    let candidates = if endpoint == TextEndpoint::ChatCompletions {
        positive(data.get("n"))?.unwrap_or(1)
    } else {
        1
    };
    let max_each = max_output_tokens / candidates;
    if max_each == 0 {
        return Err("候选数量超出分组的单次输出 Token 上限");
    }
    let cap = positive(data.get(field))?.unwrap_or(max_each).min(max_each);
    if fields.len() == 2 {
        for field in fields {
            data.remove(*field);
        }
    }
    data.insert(field.into(), json!(cap));
    Ok(cap * candidates)
}

fn positive(value: Option<&Value>) -> Result<Option<i64>, &'static str> {
    value
        .filter(|value| !value.is_null())
        .map(|value| {
            value
                .as_i64()
                .filter(|amount| *amount > 0)
                .ok_or("输出上限及候选数量须为正整数")
        })
        .transpose()
}

#[derive(Clone)]
pub(crate) struct QuotaReservation {
    pub(crate) id: String,
    pub(crate) amount: i64,
    group: String,
    window_start: i64,
    limit: i64,
}

impl QuotaReservation {
    pub(crate) fn new(group: &str, budget: OutputQuota, amount: i64) -> Self {
        let window_start = now().div_euclid(DAY) * DAY;
        Self {
            id: session_affinity::key(&["output_quota", group, &window_start.to_string()]),
            amount,
            group: group.into(),
            window_start,
            limit: budget.output_tokens_per_day,
        }
    }

    pub(crate) fn retry_after(&self) -> u64 {
        (self.window_start + DAY - now()).max(1) as u64
    }

    pub(crate) async fn reserve(&self, transaction: &mut Transaction) -> DatabaseResult<bool> {
        transaction
            .table(TABLE)
            .insert_or_ignore(&[Record::from([
                ("id".into(), self.id.as_str().into()),
                ("group_name".into(), self.group.as_str().into()),
                ("window_start".into(), self.window_start.into()),
                ("charged_tokens".into(), 0.into()),
                ("overrun".into(), false.into()),
            ])])
            .await?;
        let updated = transaction
            .table(TABLE)
            .r#where("id", "=", self.id.as_str())
            .r#where("charged_tokens", "<=", self.limit - self.amount)
            .r#where("overrun", "=", false)
            .increment("charged_tokens", self.amount, Record::new())
            .await?;
        Ok(updated == 1)
    }

    pub(crate) async fn settle(
        &self,
        transaction: &mut Transaction,
        output_tokens: Option<i64>,
    ) -> DatabaseResult<()> {
        let Some(output_tokens) = output_tokens else {
            return Ok(());
        };
        if output_tokens == self.amount {
            return Ok(());
        }
        let query = transaction
            .table(TABLE)
            .r#where("id", "=", self.id.as_str());
        if output_tokens > self.amount {
            // 供应商超出请求上限。本窗口停止准入，真实值仍记录在用量表。
            let changed = query
                .update(Record::from([("overrun".into(), true.into())]))
                .await?;
            if changed == 0
                && !transaction
                    .table(TABLE)
                    .r#where("id", "=", self.id.as_str())
                    .r#where("overrun", "=", true)
                    .exists()
                    .await?
            {
                return Err(DatabaseError::InvalidQuery("AI 网关输出配额结算失败"));
            }
            return Ok(());
        }
        release_reservation(transaction, &self.id, self.amount - output_tokens).await
    }
}

pub(crate) async fn release_reservation(
    transaction: &mut Transaction,
    id: &str,
    amount: i64,
) -> DatabaseResult<()> {
    if amount <= 0 {
        return Err(DatabaseError::InvalidScalar);
    }
    let changed = transaction
        .table(TABLE)
        .r#where("id", "=", id)
        .r#where("charged_tokens", ">=", amount)
        .decrement("charged_tokens", amount, Record::new())
        .await?;
    if changed != 1 {
        return Err(DatabaseError::InvalidQuery("AI 网关输出配额结算失败"));
    }
    Ok(())
}
