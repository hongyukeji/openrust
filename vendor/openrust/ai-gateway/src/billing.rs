use std::{collections::BTreeMap, io};

use openrust::database::{DatabaseError, DatabaseResult, Row, Transaction, Value, query::Record};
use serde::{Deserialize, Serialize};

use crate::usage::Settlement;

pub(crate) const TABLE: &str = "ai_gateway_charges";
pub(crate) const COLUMNS: [&str; 7] = [
    "ai_gateway_charges.attempt_id",
    "ai_gateway_charges.direction",
    "ai_gateway_charges.version",
    "ai_gateway_charges.currency",
    "ai_gateway_charges.tariff",
    "ai_gateway_charges.amount",
    "ai_gateway_charges.state",
];
const SCALE: i128 = 1_000_000_000_000;
const PER_MILLION: i128 = 1_000_000;

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(try_from = "String", into = "String")]
pub(crate) struct Amount(i128);

impl Amount {
    pub(crate) fn from_atoms(atoms: i64) -> Self {
        Self(i128::from(atoms))
    }

    pub(crate) fn atoms(self) -> Option<i64> {
        self.0.try_into().ok()
    }
}

impl TryFrom<String> for Amount {
    type Error = &'static str;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        let (whole, fraction) = text.split_once('.').unwrap_or((&text, ""));
        if whole.is_empty()
            || whole.len() > 12
            || fraction.len() > 12
            || !whole
                .bytes()
                .chain(fraction.bytes())
                .all(|byte| byte.is_ascii_digit())
        {
            return Err("费率必须是非负十进制金额，整数和小数各最多 12 位");
        }
        let whole = whole.parse::<i128>().map_err(|_| "无效费率")? * SCALE;
        let fraction = if fraction.is_empty() {
            0
        } else {
            fraction.parse::<i128>().map_err(|_| "无效费率")?
                * 10_i128.pow(12 - fraction.len() as u32)
        };
        Ok(Self(whole + fraction))
    }
}

impl From<Amount> for String {
    fn from(value: Amount) -> Self {
        let fraction = format!("{:012}", value.0 % SCALE);
        let fraction = fraction.trim_end_matches('0');
        let whole = value.0 / SCALE;
        if fraction.is_empty() {
            whole.to_string()
        } else {
            format!("{whole}.{fraction}")
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Tariff {
    currency: String,
    input: Amount,
    output: Amount,
    cache_read: Option<Amount>,
    cache_write: Option<Amount>,
    reasoning: Option<Amount>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    input_token_limit: Option<i64>,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PriceBook {
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    version: String,
    #[serde(default)]
    upstream: BTreeMap<String, BTreeMap<String, Tariff>>,
    #[serde(default)]
    downstream: BTreeMap<String, Tariff>,
}

impl PriceBook {
    pub(crate) fn enabled(self, can_record: bool) -> io::Result<Option<Self>> {
        if !self.enabled {
            return Ok(None);
        }
        if !can_record
            || self.version.is_empty()
            || self.version.len() > 128
            || !self.version.bytes().all(|byte| byte.is_ascii_graphic())
        {
            return Err(invalid("启用 AI 网关计费需要用量账本及有效的价格版本"));
        }
        if self
            .upstream
            .values()
            .flatten()
            .map(|(_, tariff)| tariff)
            .chain(self.downstream.values())
            .any(|tariff| {
                tariff.currency.len() != 3
                    || !tariff
                        .currency
                        .bytes()
                        .all(|byte| byte.is_ascii_uppercase())
            })
        {
            return Err(invalid("计费币种须为三个大写 ASCII 字母"));
        }
        if self
            .upstream
            .values()
            .flatten()
            .map(|(_, tariff)| tariff)
            .chain(self.downstream.values())
            .any(|tariff| tariff.input_token_limit.is_some_and(|limit| limit < 1))
        {
            return Err(invalid("计费输入 Token 上限须为正整数"));
        }
        Ok(Some(self))
    }

    pub(crate) fn select(&self, provider: &str, upstream_model: &str, model: &str) -> ChargePlan {
        ChargePlan {
            version: self.version.clone(),
            upstream_model: upstream_model.to_owned(),
            upstream: self
                .upstream
                .get(provider)
                .and_then(|models| models.get(upstream_model))
                .cloned(),
            downstream: self.downstream.get(model).cloned(),
        }
    }
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

#[derive(Clone)]
pub(crate) struct ChargePlan {
    version: String,
    upstream_model: String,
    upstream: Option<Tariff>,
    downstream: Option<Tariff>,
}

impl ChargePlan {
    pub(crate) fn receivable_ceiling(&self, currency: &str, output: i64) -> Option<i64> {
        let cap = self.upstream.as_ref()?.input_token_limit?;
        let tariff = self
            .downstream
            .as_ref()
            .filter(|rate| rate.currency == currency)?;
        let input_rate = [Some(tariff.input), tariff.cache_read, tariff.cache_write]
            .into_iter()
            .flatten()
            .map(|rate| rate.0)
            .max()?;
        let output_rate = tariff
            .output
            .0
            .max(tariff.reasoning.map_or(0, |rate| rate.0));
        let units = input_rate
            .checked_mul(i128::from(cap))?
            .checked_add(output_rate.checked_mul(i128::from(output))?)?;
        Amount(units.checked_add(PER_MILLION - 1)? / PER_MILLION).atoms()
    }

    pub(crate) fn receivable(&self, settlement: &Settlement) -> Option<Amount> {
        self.charge("receivable", self.downstream.as_ref(), settlement)
            .1
    }

    fn tariffs(&self) -> [(&'static str, Option<&Tariff>); 2] {
        [
            ("cost", self.upstream.as_ref()),
            ("receivable", self.downstream.as_ref()),
        ]
    }

    pub(crate) async fn register(
        &self,
        transaction: &mut Transaction,
        attempt: &str,
    ) -> DatabaseResult<()> {
        let records = self.tariffs().map(|(direction, tariff)| {
            let snapshot = tariff.map(|rate| serde_json::to_string(rate).expect("费率可序列化"));
            Record::from([
                ("attempt_id".into(), attempt.into()),
                ("direction".into(), direction.into()),
                ("version".into(), self.version.as_str().into()),
                (
                    "currency".into(),
                    tariff.map_or(Value::Null, |rate| rate.currency.as_str().into()),
                ),
                ("tariff".into(), snapshot.map_or(Value::Null, Into::into)),
                ("state".into(), "pending".into()),
            ])
        });
        if transaction.table(TABLE).insert(&records).await? {
            Ok(())
        } else {
            Err(openrust::database::DatabaseError::InvalidQuery(
                "AI 网关未保存价格快照",
            ))
        }
    }

    pub(crate) async fn settle(
        &self,
        transaction: &mut Transaction,
        attempt: &str,
        settlement: &Settlement,
    ) -> DatabaseResult<()> {
        for (direction, tariff) in self.tariffs() {
            let (state, amount) = self.charge(direction, tariff, settlement);
            let affected = transaction
                .table(TABLE)
                .r#where("attempt_id", "=", attempt)
                .r#where("direction", "=", direction)
                .r#where("state", "=", "pending")
                .update(Record::from([
                    ("state".into(), state.into()),
                    (
                        "amount".into(),
                        amount.map_or(Value::Null, |amount| String::from(amount).into()),
                    ),
                ]))
                .await?;
            if affected != 1 {
                return Err(openrust::database::DatabaseError::InvalidQuery(
                    "AI 网关价格快照状态无效",
                ));
            }
        }
        Ok(())
    }

    fn charge(
        &self,
        direction: &str,
        tariff: Option<&Tariff>,
        settlement: &Settlement,
    ) -> (&'static str, Option<Amount>) {
        if settlement.state == "not_dispatched"
            || (direction == "receivable" && settlement.outcome != "completed")
        {
            // 内部失败尝试不会形成下游应收。
            return ("not_billable", tariff.map(|_| Amount(0)));
        }
        let Some(tariff) = tariff else {
            return ("missing_price", None);
        };
        if direction == "cost"
            && settlement
                .reported_model
                .as_deref()
                .is_some_and(|model| model != self.upstream_model)
        {
            return ("model_mismatch", None);
        }
        if settlement.state != "final" {
            return ("awaiting_usage", None);
        }
        match tariff.quote(settlement) {
            Some(amount) => ("priced", Some(amount)),
            None => ("awaiting_usage", None),
        }
    }
}

impl Tariff {
    fn quote(&self, settlement: &Settlement) -> Option<Amount> {
        let mut input = i128::from(settlement.input_tokens?);
        let mut output = i128::from(settlement.output_tokens?);
        if settlement.upstream_endpoint == "messages" {
            input = input
                .checked_add(i128::from(settlement.cached_input_tokens?))?
                .checked_add(i128::from(settlement.cache_write_input_tokens?))?;
        }
        if settlement.upstream_endpoint == "streamGenerateContent"
            || settlement.upstream_endpoint == "generateContent"
        {
            output = output.checked_add(i128::from(settlement.reasoning_tokens?))?;
        }
        let read = rate_component(&mut input, self.cache_read, settlement.cached_input_tokens)?;
        let write = rate_component(
            &mut input,
            self.cache_write,
            settlement.cache_write_input_tokens,
        )?;
        let reasoning = rate_component(&mut output, self.reasoning, settlement.reasoning_tokens)?;
        let numerator = input
            .checked_mul(self.input.0)?
            .checked_add(output.checked_mul(self.output.0)?)?
            .checked_add(read)?
            .checked_add(write)?
            .checked_add(reasoning)?;
        let quotient = numerator / PER_MILLION;
        let remainder = numerator % PER_MILLION;
        let round_up =
            remainder > PER_MILLION / 2 || (remainder == PER_MILLION / 2 && quotient % 2 != 0);
        Some(Amount(quotient + i128::from(round_up)))
    }
}

fn rate_component(total: &mut i128, rate: Option<Amount>, count: Option<i64>) -> Option<i128> {
    let Some(rate) = rate else { return Some(0) };
    let count = i128::from(count?);
    *total = total.checked_sub(count)?;
    if *total < 0 {
        return None;
    }
    rate.0.checked_mul(count)
}

#[derive(Serialize)]
pub(crate) struct ChargeRecord {
    pub(crate) attempt_id: String,
    pub(crate) direction: String,
    version: String,
    currency: Option<String>,
    tariff: Option<serde_json::Value>,
    amount: Option<String>,
    state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) actual: Option<crate::reconciliation::ActualCost>,
}

pub(crate) fn decode(row: &Row) -> DatabaseResult<ChargeRecord> {
    let text = |column| crate::managed_keys::text(row, column);
    let optional_text = |column| match row.get(column) {
        Some(Value::Text(text)) => Ok(Some(text.clone())),
        Some(Value::Null) => Ok(None),
        _ => Err(DatabaseError::InvalidScalar),
    };
    Ok(ChargeRecord {
        attempt_id: text("attempt_id")?,
        direction: text("direction")?,
        version: text("version")?,
        currency: optional_text("currency")?,
        tariff: optional_text("tariff")?
            .map(|tariff| serde_json::from_str(&tariff))
            .transpose()
            .map_err(|_| DatabaseError::InvalidScalar)?,
        amount: optional_text("amount")?,
        state: text("state")?,
        actual: None,
    })
}
