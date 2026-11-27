use std::{collections::BTreeMap, io};

use openrust::database::{DatabaseError, DatabaseResult, Row, Transaction, Value, query::Record};
use serde::{Deserialize, Serialize};

use crate::usage::Settlement;

pub(crate) mod adjustments;

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
    pub(crate) const ZERO: Self = Self(0);

    pub(crate) fn from_usd_ticks(ticks: u64) -> Self {
        Self(i128::from(ticks) * (SCALE / 10_000_000_000))
    }

    pub(crate) fn from_atoms(atoms: i64) -> Self {
        Self(i128::from(atoms))
    }

    pub(crate) fn atoms(self) -> Option<i64> {
        self.0.try_into().ok()
    }

    pub(crate) fn ledger(text: &str) -> DatabaseResult<Self> {
        Self::parse_decimal(text, 27).map_err(|_| DatabaseError::InvalidScalar)
    }

    pub(crate) fn add(self, other: Self) -> DatabaseResult<Self> {
        self.0
            .checked_add(other.0)
            .map(Self)
            .ok_or(DatabaseError::InvalidScalar)
    }

    pub(crate) fn difference(self, other: Self) -> DatabaseResult<Self> {
        self.0
            .checked_sub(other.0)
            .map(Self)
            .ok_or(DatabaseError::InvalidScalar)
    }

    pub(crate) fn is_zero(self) -> bool {
        self.0 == 0
    }

    fn parse_decimal(text: &str, width: usize) -> Result<Self, &'static str> {
        let (whole, fraction) = text.split_once('.').unwrap_or((text, ""));
        if whole.is_empty()
            || whole.len() > width
            || fraction.len() > 12
            || !whole
                .bytes()
                .chain(fraction.bytes())
                .all(|byte| byte.is_ascii_digit())
        {
            return Err("费率必须是非负十进制金额，整数和小数各最多 12 位");
        }
        let whole = whole
            .parse::<i128>()
            .ok()
            .and_then(|whole| whole.checked_mul(SCALE))
            .ok_or("金额超出计费范围")?;
        let fraction = if fraction.is_empty() {
            0
        } else {
            fraction.parse::<i128>().map_err(|_| "无效费率")?
                * 10_i128.pow(12 - fraction.len() as u32)
        };
        whole
            .checked_add(fraction)
            .map(Self)
            .ok_or("金额超出计费范围")
    }
}

impl TryFrom<String> for Amount {
    type Error = &'static str;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        Self::parse_decimal(&text, 12)
    }
}

impl From<Amount> for String {
    fn from(value: Amount) -> Self {
        let sign = if value.0.is_negative() { "-" } else { "" };
        let magnitude = value.0.unsigned_abs();
        let fraction = format!("{:012}", magnitude % SCALE as u128);
        let fraction = fraction.trim_end_matches('0');
        let whole = magnitude / SCALE as u128;
        if fraction.is_empty() {
            format!("{sign}{whole}")
        } else {
            format!("{sign}{whole}.{fraction}")
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
    image: Option<Amount>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    image_tiers: BTreeMap<String, Amount>,
    #[serde(skip_deserializing, skip_serializing_if = "Option::is_none")]
    image_tier: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    input_token_limit: Option<i64>,
    #[serde(
        skip_deserializing,
        skip_serializing_if = "Option::is_none",
        rename = "rate_multiplier"
    )]
    multiplier: Option<Amount>,
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
        if self
            .upstream
            .values()
            .flatten()
            .map(|(_, tariff)| tariff)
            .chain(self.downstream.values())
            .any(|tariff| tariff.image_tiers.keys().any(|key| !valid_image_tier(key)))
        {
            return Err(invalid(
                "图片价格档须使用尺寸或宽高比以及质量，例如 1152x768/standard",
            ));
        }
        Ok(Some(self))
    }

    pub(crate) fn select(
        &self,
        provider: &str,
        upstream_model: &str,
        model: &str,
        multiplier: Option<Amount>,
        by_image: bool,
        image_tier: Option<&str>,
    ) -> ChargePlan {
        let mut downstream = self.downstream.get(model).cloned();
        if let Some(tariff) = &mut downstream {
            tariff.multiplier = multiplier;
        }
        let upstream = self
            .upstream
            .get(provider)
            .and_then(|models| models.get(upstream_model))
            .cloned();
        let selected = |tariff: Option<Tariff>| {
            tariff.map(|mut tariff| {
                if by_image {
                    tariff.image = image_tier
                        .and_then(|tier| tariff.image_tiers.get(tier).copied())
                        .or(tariff.image);
                    tariff.image_tier = image_tier.map(str::to_owned);
                }
                tariff.image_tiers.clear();
                tariff
            })
        };
        ChargePlan {
            version: self.version.clone(),
            by_image,
            upstream_model: upstream_model.to_owned(),
            upstream: selected(upstream),
            downstream: selected(downstream),
        }
    }
}

fn valid_image_tier(key: &str) -> bool {
    let Some((size, quality)) = key.split_once('/') else {
        return false;
    };
    let dimensions = match quality {
        "standard" | "premium" => size.split_once('x'),
        "default" => size.split_once(':'),
        _ => None,
    };
    dimensions.is_some_and(|(width, height)| {
        [width, height]
            .into_iter()
            .all(|value| value.parse::<u32>().is_ok_and(|value| value > 0))
    })
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

pub(crate) async fn close_undispatched(
    transaction: &mut Transaction,
    attempt: &str,
) -> DatabaseResult<()> {
    let mut changed = 0;
    for has_currency in [false, true] {
        let query = transaction
            .table(TABLE)
            .r#where("attempt_id", "=", attempt)
            .r#where("state", "=", "pending");
        let (query, amount) = if has_currency {
            (query.where_not_null("currency"), "0".into())
        } else {
            (query.where_null("currency"), Value::Null)
        };
        changed += query
            .update(Record::from([
                ("state".into(), "not_billable".into()),
                ("amount".into(), amount),
            ]))
            .await?;
    }
    if changed != 2 {
        return Err(DatabaseError::InvalidQuery(
            "AI 网关未出站的价格快照状态无效",
        ));
    }
    Ok(())
}

pub(crate) async fn close_interrupted_embedding(
    transaction: &mut Transaction,
    attempt: &str,
    dispatched: bool,
    input_tokens: Option<i64>,
) -> DatabaseResult<Option<Amount>> {
    let rows = transaction
        .table(TABLE)
        .select(["direction", "currency", "tariff"])
        .r#where("attempt_id", "=", attempt)
        .r#where("state", "=", "pending")
        .get()
        .await?;
    if rows.is_empty() {
        return Ok((!dispatched).then_some(Amount::ZERO));
    }
    if rows.len() != 2 {
        return Err(DatabaseError::InvalidQuery("AI 网关批量恢复价格快照不完整"));
    }
    let mut cost = None;
    for row in rows {
        let direction = crate::managed_keys::text(&row, "direction")?;
        let has_price = matches!(row.get("currency"), Some(Value::Text(_)));
        let (state, amount) = if !dispatched || direction == "receivable" {
            ("not_billable", has_price.then_some(Amount::ZERO))
        } else if !has_price {
            ("missing_price", None)
        } else if let Some(tokens) = input_tokens {
            let tariff: Tariff = serde_json::from_str(&crate::managed_keys::text(&row, "tariff")?)
                .map_err(|_| DatabaseError::InvalidScalar)?;
            let amount = i128::from(tokens)
                .checked_mul(tariff.input.0)
                .and_then(|value| tariff.amount(value, Rounding::HalfEven))
                .ok_or(DatabaseError::InvalidScalar)?;
            ("priced", Some(amount))
        } else {
            ("awaiting_usage", None)
        };
        if direction == "cost" {
            cost = amount;
        }
        if transaction
            .table(TABLE)
            .r#where("attempt_id", "=", attempt)
            .r#where("direction", "=", direction.as_str())
            .r#where("state", "=", "pending")
            .update(Record::from([
                ("state".into(), state.into()),
                (
                    "amount".into(),
                    amount.map_or(Value::Null, |amount| String::from(amount).into()),
                ),
            ]))
            .await?
            != 1
        {
            return Err(DatabaseError::InvalidQuery(
                "AI 网关批量恢复价格快照状态无效",
            ));
        }
    }
    Ok(if dispatched { cost } else { Some(Amount::ZERO) })
}

#[derive(Clone)]
pub(crate) struct ChargePlan {
    version: String,
    pub(crate) by_image: bool,
    upstream_model: String,
    upstream: Option<Tariff>,
    downstream: Option<Tariff>,
}

impl ChargePlan {
    pub(crate) async fn recover(
        transaction: &mut Transaction,
        attempt: &str,
        upstream_model: String,
    ) -> DatabaseResult<Option<Self>> {
        let rows = transaction
            .table(TABLE)
            .r#where("attempt_id", "=", attempt)
            .get()
            .await?;
        if rows.is_empty() {
            return Ok(None);
        }
        let invalid = || openrust::database::DatabaseError::InvalidScalar;
        let mut cost = None;
        let mut receivable = None;
        for row in &rows {
            let mut charge = decode(row)?;
            let tariff = charge
                .tariff
                .take()
                .map(|mut tariff| {
                    let object = tariff.as_object_mut().ok_or_else(invalid)?;
                    let multiplier = object
                        .remove("rate_multiplier")
                        .map(serde_json::from_value)
                        .transpose()
                        .map_err(|_| invalid())?;
                    let mut tariff: Tariff =
                        serde_json::from_value(tariff).map_err(|_| invalid())?;
                    tariff.multiplier = multiplier;
                    Ok::<_, openrust::database::DatabaseError>(tariff)
                })
                .transpose()?;
            match charge.direction.as_str() {
                "cost" if cost.is_none() => cost = Some((charge.version, tariff)),
                "receivable" if receivable.is_none() => receivable = Some((charge.version, tariff)),
                _ => return Err(invalid()),
            }
        }
        let ((version, upstream), (other, downstream)) =
            cost.zip(receivable).ok_or_else(invalid)?;
        if version != other {
            return Err(invalid());
        }
        Ok(Some(Self {
            version,
            upstream_model,
            upstream,
            downstream,
            by_image: false,
        }))
    }

    pub(crate) fn receivable_ceiling(
        &self,
        currency: &str,
        output: i64,
        items: i64,
    ) -> Option<i64> {
        self.ceiling(self.downstream.as_ref()?, currency, output, items)
    }

    pub(crate) fn cost_ceiling(&self, currency: &str, output: i64, items: i64) -> Option<i64> {
        self.ceiling(self.upstream.as_ref()?, currency, output, items)
    }

    fn ceiling(&self, tariff: &Tariff, currency: &str, output: i64, items: i64) -> Option<i64> {
        if tariff.currency != currency {
            return None;
        }
        if self.by_image {
            return tariff.image_amount(output, Rounding::Ceiling)?.atoms();
        }
        let cap = i128::from(self.upstream.as_ref()?.input_token_limit?)
            .checked_mul(i128::from(items))?;
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
            .checked_mul(cap)?
            .checked_add(output_rate.checked_mul(i128::from(output))?)?;
        tariff.amount(units, Rounding::Ceiling)?.atoms()
    }

    pub(crate) fn receivable(&self, settlement: &Settlement) -> Option<Amount> {
        self.charge("receivable", self.downstream.as_ref(), settlement)
            .1
    }

    pub(crate) fn cost(&self, settlement: &Settlement) -> Option<Amount> {
        if let Some(actual) = settlement.reported_cost_usd {
            return self
                .upstream
                .as_ref()
                .filter(|tariff| tariff.currency == "USD")
                .map(|_| actual);
        }
        self.charge("cost", self.upstream.as_ref(), settlement).1
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
            let mut update = Record::from([
                ("state".into(), state.into()),
                (
                    "amount".into(),
                    amount.map_or(Value::Null, |amount| String::from(amount).into()),
                ),
            ]);
            if direction == "cost"
                && let Some(actual) = settlement.reported_cost_usd
            {
                update.insert("reported_cost_usd".into(), String::from(actual).into());
            }
            let affected = transaction
                .table(TABLE)
                .r#where("attempt_id", "=", attempt)
                .r#where("direction", "=", direction)
                .r#where("state", "=", "pending")
                .update(update)
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
        let cache_lifecycle = crate::usage::gemini_caches::OPERATIONS
            .contains(&settlement.upstream_endpoint.as_str());
        if settlement.state == "not_dispatched"
            || settlement.upstream_endpoint == crate::usage::gemini_caches::DELETE
            || (direction == "receivable" && (cache_lifecycle || settlement.outcome != "completed"))
        {
            // 内部失败尝试不会形成下游应收。
            return ("not_billable", tariff.map(|_| Amount(0)));
        }
        if cache_lifecycle {
            // 创建及续期成本需上游账务证据，缓存 Token 数不能作为已结算金额。
            return ("awaiting_usage", None);
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
        if self.by_image && tariff.image.is_none() {
            return ("missing_price", None);
        }
        if settlement.state != "final" || (direction == "cost" && settlement.cost_pending) {
            return ("awaiting_usage", None);
        }
        let amount = if self.by_image {
            settlement
                .output_images
                .and_then(|count| tariff.image_amount(count, Rounding::HalfEven))
        } else {
            tariff.quote(settlement)
        };
        match amount {
            Some(amount) => ("priced", Some(amount)),
            None => ("awaiting_usage", None),
        }
    }
}

impl Tariff {
    fn image_amount(&self, count: i64, rounding: Rounding) -> Option<Amount> {
        let amount = self.image?.0.checked_mul(i128::from(count))?;
        self.amount(amount.checked_mul(PER_MILLION)?, rounding)
    }

    fn quote(&self, settlement: &Settlement) -> Option<Amount> {
        let mut input = i128::from(settlement.input_tokens?);
        if settlement.input_only {
            return self.amount(input.checked_mul(self.input.0)?, Rounding::HalfEven);
        }
        let mut output = i128::from(settlement.output_tokens?);
        if settlement.upstream_endpoint == "messages" {
            input = input
                .checked_add(i128::from(settlement.cached_input_tokens?))?
                .checked_add(i128::from(settlement.cache_write_input_tokens?))?;
        }
        if settlement.upstream_endpoint == "streamGenerateContent"
            || settlement.upstream_endpoint == "generateContent"
            || settlement.upstream_endpoint == "batchGenerateContent"
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
        self.amount(numerator, Rounding::HalfEven)
    }

    fn amount(&self, numerator: i128, rounding: Rounding) -> Option<Amount> {
        let (quotient, remainder, divisor) = if let Some(multiplier) = self.multiplier {
            let divisor = PER_MILLION * SCALE;
            // 分解两侧整除项，避免在最终金额可表示时中间乘积溢出。
            let fraction = numerator % divisor;
            let tail = fraction * (multiplier.0 % divisor);
            let quotient = (numerator / divisor)
                .checked_mul(multiplier.0)?
                .checked_add(fraction * (multiplier.0 / divisor))?
                .checked_add(tail / divisor)?;
            (quotient, tail % divisor, divisor)
        } else {
            (
                numerator / PER_MILLION,
                numerator % PER_MILLION,
                PER_MILLION,
            )
        };
        let round_up = match rounding {
            Rounding::Ceiling => remainder != 0,
            Rounding::HalfEven => {
                remainder > divisor / 2 || (remainder == divisor / 2 && quotient % 2 != 0)
            }
        };
        quotient.checked_add(i128::from(round_up)).map(Amount)
    }
}

enum Rounding {
    Ceiling,
    HalfEven,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    reported_cost_usd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    adjustments: Option<adjustments::Summary>,
}

impl ChargeRecord {
    pub(crate) fn attach_adjustments(&mut self, rows: &[Row]) -> DatabaseResult<()> {
        let base = self
            .actual
            .as_ref()
            .map_or(self.amount.as_deref(), |actual| actual.amount.as_deref());
        self.adjustments = adjustments::summarize(rows, base)?;
        Ok(())
    }
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
        reported_cost_usd: optional_text("reported_cost_usd")?,
        adjustments: None,
    })
}
