use openrust::database::{
    Connection, DatabaseError, DatabaseResult, Transaction, Value as DatabaseValue,
    query::{Builder, Record},
};
use openrust_ai::TextEndpoint;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    billing::{Amount, ChargePlan},
    managed_keys::{integer, now},
    output_quota::limit_output,
    session_affinity,
    usage::DAY,
};

pub(crate) mod settlements;

const TABLE: &str = "ai_gateway_budgets";
const MAX_BUDGET_ATOMS: i64 = i64::MAX / 4;
pub(crate) const DENIED: &str = "AI 网关分组货币预算不足";
pub(crate) const ACCOUNT_DENIED: &str = "AI 网关上游连接成本预算不足";

#[derive(Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Unknown {
    #[default]
    Reject,
    Monitor,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Budget {
    currency: String,
    daily_limit: Amount,
    pub(crate) max_output_tokens: i64,
    #[serde(default)]
    unknown: Unknown,
}

#[derive(Serialize)]
pub(crate) struct BudgetBalance {
    #[serde(skip_serializing_if = "Option::is_none")]
    group: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    account: Option<String>,
    currency: String,
    window_start: i64,
    window_end: i64,
    limit: Amount,
    reserved: Amount,
    booked_within_limit: Amount,
    available: Amount,
    overrun: bool,
}

#[derive(Clone, Copy)]
pub(crate) enum BudgetScope<'a> {
    Group(&'a str),
    Account(&'a str),
}

impl Budget {
    fn window(&self, scope: BudgetScope<'_>) -> (i64, String) {
        let window = now().div_euclid(DAY) * DAY;
        let (kind, name) = match scope {
            BudgetScope::Group(group) => ("budget", group),
            BudgetScope::Account(account) => ("account_budget", account),
        };
        let key = session_affinity::key(&[kind, name, &self.currency, &window.to_string()]);
        (window, key)
    }

    pub(crate) async fn balance(
        &self,
        connection: &Connection,
        scope: BudgetScope<'_>,
    ) -> DatabaseResult<BudgetBalance> {
        let (window, key) = self.window(scope);
        let row = connection
            .table(TABLE)
            .use_write_pdo()
            .select(["occupied_atoms", "held_atoms", "overrun"])
            .r#where("id", "=", key)
            .first()
            .await?;
        let (occupied, held, overrun) = match row {
            Some(row) => {
                let required = |column| integer(&row, column)?.ok_or(DatabaseError::InvalidScalar);
                let overrun = match row.get("overrun") {
                    Some(DatabaseValue::Bool(value)) => *value,
                    Some(DatabaseValue::Integer(0) | DatabaseValue::Unsigned(0)) => false,
                    Some(DatabaseValue::Integer(1) | DatabaseValue::Unsigned(1)) => true,
                    _ => return Err(DatabaseError::InvalidScalar),
                };
                (
                    required("occupied_atoms")?,
                    required("held_atoms")?,
                    overrun,
                )
            }
            None => (0, 0, false),
        };
        if held < 0 || occupied < held {
            return Err(DatabaseError::InvalidScalar);
        }
        let limit = self.daily_limit.atoms().expect("已校验预算配置");
        let (group, account) = match scope {
            BudgetScope::Group(group) => (Some(group.into()), None),
            BudgetScope::Account(account) => (None, Some(account.into())),
        };
        Ok(BudgetBalance {
            group,
            account,
            currency: self.currency.clone(),
            window_start: window,
            window_end: window + DAY,
            limit: self.daily_limit,
            reserved: Amount::from_atoms(held),
            booked_within_limit: Amount::from_atoms(occupied - held),
            available: Amount::from_atoms(if overrun {
                0
            } else {
                (limit - occupied).max(0)
            }),
            overrun,
        })
    }

    pub(crate) fn valid(&self) -> bool {
        self.currency.len() == 3
            && self.currency.bytes().all(|byte| byte.is_ascii_uppercase())
            && self
                .daily_limit
                .atoms()
                .is_some_and(|limit| (1..=MAX_BUDGET_ATOMS).contains(&limit))
            && self.max_output_tokens > 0
    }

    pub(crate) fn prepare(
        &self,
        scope: BudgetScope<'_>,
        endpoint: TextEndpoint,
        native_openai: bool,
        payload: &mut Value,
        charges: Option<&ChargePlan>,
        output_quota: Option<i64>,
    ) -> Result<Option<BudgetReservation>, &'static str> {
        let bounded_text = matches!(
            endpoint,
            TextEndpoint::Responses | TextEndpoint::ChatCompletions | TextEndpoint::Messages
        ) && billable_text_only(endpoint, payload)
            || endpoint == TextEndpoint::GeminiBatchGenerateContent
                && crate::gemini_batch_input::text_output(payload);
        let bounded_speech = endpoint == TextEndpoint::XaiTextToSpeech;
        if let Some(charges) = charges
            && (bounded_text || endpoint.is_embedding() || charges.by_image || bounded_speech)
        {
            let mut capped = payload.clone();
            let items = if endpoint.is_gemini_batch() {
                crate::gemini_batch_input::requests(payload)
                    .and_then(|items| i64::try_from(items.len()).ok())
                    .filter(|count| *count > 0)
                    .ok_or("无法确认 Gemini 批次项数")?
            } else {
                1
            };
            let output = if charges.by_image {
                if payload["n"].is_null() {
                    1
                } else {
                    payload["n"]
                        .as_i64()
                        .filter(|count| *count > 0)
                        .ok_or("无法确认图片生成数量上限")?
                }
            } else if endpoint.is_embedding() || bounded_speech {
                0
            } else {
                let cap = output_quota.map_or(self.max_output_tokens, |quota| {
                    quota.min(self.max_output_tokens)
                });
                limit_output(cap, endpoint, native_openai, &mut capped)?
            };
            let atoms = match scope {
                BudgetScope::Group(_) => charges.receivable_ceiling(&self.currency, output, items),
                BudgetScope::Account(_) => charges.cost_ceiling(&self.currency, output, items),
            };
            if let Some(atoms) = atoms {
                *payload = capped;
                let (window, id) = self.window(scope);
                let (group, account) = match scope {
                    BudgetScope::Group(group) => (group.into(), None),
                    BudgetScope::Account(account) => (String::new(), Some(account.into())),
                };
                return Ok(Some(BudgetReservation {
                    id,
                    group,
                    account,
                    currency: self.currency.clone(),
                    window,
                    atoms,
                    limit: self.daily_limit.atoms().expect("已校验预算配置"),
                }));
            }
        }
        match self.unknown {
            Unknown::Reject => Err("无法确认此上游请求的货币费用上限"),
            Unknown::Monitor => Ok(None),
        }
    }
}

fn billable_text_only(endpoint: TextEndpoint, payload: &Value) -> bool {
    let present = |key| payload.get(key).is_some_and(|value| !value.is_null());
    if [
        "web_search_options",
        "audio",
        "modalities",
        "mcp_servers",
        "container",
        "service_tier",
        "previous_response_id",
        "context_management",
    ]
    .into_iter()
    .any(present)
    {
        return false;
    }
    payload.get("tools").is_none_or(|tools| {
        tools.is_null()
            || tools.as_array().is_some_and(|tools| {
                tools.iter().all(|tool| match endpoint {
                    TextEndpoint::Messages => {
                        tool.get("input_schema").is_some_and(Value::is_object)
                            && tool
                                .get("type")
                                .is_none_or(|kind| kind.as_str() == Some("custom"))
                    }
                    _ => tool["type"].as_str() == Some("function"),
                })
            })
    })
}

#[derive(Clone)]
pub(crate) struct BudgetReservation {
    pub(crate) id: String,
    pub(crate) atoms: i64,
    group: String,
    account: Option<String>,
    currency: String,
    window: i64,
    limit: i64,
}

#[derive(Clone, Default)]
pub(crate) struct BudgetReservations {
    pub(crate) group: Option<BudgetReservation>,
    pub(crate) account: Option<BudgetReservation>,
}

impl BudgetReservations {
    pub(crate) fn is_empty(&self) -> bool {
        self.group.is_none() && self.account.is_none()
    }
}

impl BudgetReservation {
    pub(crate) fn retry_after(&self) -> u64 {
        (self.window + DAY - now()).max(1) as u64
    }

    pub(crate) async fn reserve(&self, transaction: &mut Transaction) -> DatabaseResult<()> {
        let mut record = Record::from([
            ("id".into(), self.id.as_str().into()),
            ("group_name".into(), self.group.as_str().into()),
            ("currency".into(), self.currency.as_str().into()),
            ("window_start".into(), self.window.into()),
            ("occupied_atoms".into(), 0.into()),
            ("held_atoms".into(), 0.into()),
            ("overrun".into(), false.into()),
        ]);
        if let Some(account) = &self.account {
            record.insert("account_name".into(), account.as_str().into());
        }
        let denied = if self.account.is_some() {
            ACCOUNT_DENIED
        } else {
            DENIED
        };
        transaction.table(TABLE).insert_or_ignore(&[record]).await?;
        let allowed = transaction
            .table(TABLE)
            .r#where("id", "=", self.id.as_str())
            .r#where("overrun", "=", false)
            .r#where("occupied_atoms", "<=", self.limit - self.atoms);
        if self.atoms == 0 {
            if !allowed.exists().await? {
                return Err(DatabaseError::InvalidQuery(denied));
            }
            return Ok(());
        }
        if allowed
            .increment("occupied_atoms", self.atoms, Record::new())
            .await?
            != 1
        {
            return Err(DatabaseError::InvalidQuery(denied));
        }
        if self
            .scope(transaction)
            .increment("held_atoms", self.atoms, Record::new())
            .await?
            != 1
        {
            return Err(DatabaseError::InvalidQuery("AI 网关预算预占状态无效"));
        }
        Ok(())
    }

    fn scope<'a>(&self, transaction: &'a mut Transaction) -> Builder<'a> {
        transaction
            .table(TABLE)
            .r#where("id", "=", self.id.as_str())
    }

    pub(crate) async fn settle(
        &self,
        transaction: &mut Transaction,
        amount: Option<Amount>,
    ) -> DatabaseResult<()> {
        settle_reserved(transaction, &self.id, self.atoms, amount).await
    }
}

pub(crate) async fn settle_reserved(
    transaction: &mut Transaction,
    id: &str,
    reserved: i64,
    amount: Option<Amount>,
) -> DatabaseResult<()> {
    let Some(amount) = amount else {
        return Ok(());
    };
    if let Some(charged) = amount.atoms().filter(|amount| *amount <= reserved) {
        release_reservation(transaction, id, reserved, charged).await?;
    } else {
        // 回报超出声明上界时保留预占，并停止本账期准入。
        transaction
            .table(TABLE)
            .r#where("id", "=", id)
            .update(Record::from([("overrun".into(), true.into())]))
            .await?;
    }
    Ok(())
}

pub(crate) async fn release_reservation(
    transaction: &mut Transaction,
    id: &str,
    reserved: i64,
    charged: i64,
) -> DatabaseResult<()> {
    if reserved < charged || charged < 0 {
        return Err(DatabaseError::InvalidScalar);
    }
    if reserved != 0
        && transaction
            .table(TABLE)
            .r#where("id", "=", id)
            .r#where("held_atoms", ">=", reserved)
            .decrement("held_atoms", reserved, Record::new())
            .await?
            != 1
    {
        return Err(DatabaseError::InvalidQuery("AI 网关预算释放状态无效"));
    }
    let returned = reserved - charged;
    if returned != 0
        && transaction
            .table(TABLE)
            .r#where("id", "=", id)
            .r#where("occupied_atoms", ">=", returned)
            .decrement("occupied_atoms", returned, Record::new())
            .await?
            != 1
    {
        return Err(DatabaseError::InvalidQuery("AI 网关预算结算状态无效"));
    }
    Ok(())
}
