use crate::{Error, Provider, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SubscriptionItem {
    pub id: String,
    pub price: String,
    pub quantity: Option<u64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Subscription {
    pub id: String,
    pub customer: String,
    pub name: String,
    pub created_at: i64,
    pub status: String,
    pub trial_ends_at: Option<i64>,
    pub ends_at: Option<i64>,
    pub paused_at: Option<i64>,
    pub items: Vec<SubscriptionItem>,
}
impl Subscription {
    pub fn on_trial(&self) -> bool {
        self.trial_ends_at
            .is_some_and(|t| t > jiff::Timestamp::now().as_second())
            && self.status == "trialing"
    }
    pub fn canceled(&self) -> bool {
        self.ends_at.is_some() || matches!(self.status.as_str(), "canceled" | "cancelled")
    }
    pub fn on_grace_period(&self) -> bool {
        self.ends_at
            .is_some_and(|t| t > jiff::Timestamp::now().as_second())
    }
    pub fn paused(&self) -> bool {
        self.status == "paused"
    }
    pub fn incomplete(&self) -> bool {
        self.status == "incomplete"
    }
    pub fn past_due(&self) -> bool {
        self.status == "past_due"
    }
    pub fn active(&self) -> bool {
        matches!(self.status.as_str(), "active" | "trialing")
            && self
                .ends_at
                .is_none_or(|t| t > jiff::Timestamp::now().as_second())
    }
    pub fn valid(&self) -> bool {
        self.active() || self.on_grace_period()
    }
    pub fn has_price(&self, price: &str) -> bool {
        self.items.iter().any(|i| i.price == price)
    }
    pub(crate) fn parse(provider: Provider, v: &Value) -> Result<Self> {
        let id = text(v, "id")?;
        let customer = match provider {
            Provider::Stripe => object_id(&v["customer"])?,
            Provider::Paddle => text(v, "customer_id")?,
        };
        let metadata = if provider == Provider::Stripe {
            &v["metadata"]
        } else {
            &v["custom_data"]
        };
        let name = metadata["cashier_type"]
            .as_str()
            .unwrap_or("default")
            .to_owned();
        crate::client::identifier(&name)?;
        let status = text(v, "status")?;
        let items = if provider == Provider::Stripe {
            &v["items"]["data"]
        } else {
            &v["items"]
        };
        let items = items
            .as_array()
            .ok_or(Error::OutcomeUnknown)?
            .iter()
            .map(|i| {
                let price = object_id(&i["price"])?;
                Ok(SubscriptionItem {
                    id: i["id"].as_str().unwrap_or(&price).into(),
                    price,
                    quantity: i["quantity"].as_u64(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let (trial_ends_at, ends_at, paused_at) = if provider == Provider::Stripe {
            let end = time(&v["cancel_at"])
                .or_else(|| {
                    if v["cancel_at_period_end"] == true {
                        time(&v["current_period_end"]).or_else(|| {
                            v["items"]["data"].as_array().and_then(|a| {
                                a.iter()
                                    .filter_map(|i| time(&i["current_period_end"]))
                                    .max()
                            })
                        })
                    } else {
                        None
                    }
                })
                .or_else(|| time(&v["ended_at"]));
            (time(&v["trial_end"]), end, None)
        } else {
            let change = &v["scheduled_change"];
            (
                v["items"].as_array().and_then(|a| {
                    a.iter()
                        .filter_map(|i| time(&i["trial_dates"]["ends_at"]))
                        .max()
                }),
                if change["action"] == "cancel" {
                    time(&change["effective_at"])
                } else {
                    time(&v["canceled_at"])
                },
                if change["action"] == "pause" {
                    time(&change["effective_at"])
                } else {
                    time(&v["paused_at"])
                },
            )
        };
        let created_at = time(&v["created"])
            .or_else(|| time(&v["created_at"]))
            .unwrap_or(0);
        Ok(Self {
            id,
            customer,
            name,
            created_at,
            status,
            trial_ends_at,
            ends_at,
            paused_at,
            items,
        })
    }
}
/// Prices and quantities are validated before sending; amounts remain provider catalog prices.
#[derive(Clone, Debug)]
pub struct Price {
    pub id: String,
    pub quantity: Option<u64>,
}
impl Price {
    pub fn new(id: impl Into<String>, quantity: u64) -> Result<Self> {
        let id = id.into();
        crate::client::identifier(&id)?;
        if quantity == 0 {
            return Err(Error::InvalidInput("quantity"));
        }
        Ok(Self {
            id,
            quantity: Some(quantity),
        })
    }
    pub fn metered(id: impl Into<String>) -> Result<Self> {
        let id = id.into();
        crate::client::identifier(&id)?;
        Ok(Self { id, quantity: None })
    }
}
#[derive(Clone, Copy, Debug)]
pub enum Proration {
    Prorate,
    Invoice,
    None,
}
impl Proration {
    pub(crate) fn value(self, provider: Provider) -> &'static str {
        match (provider, self) {
            (Provider::Stripe, Self::Prorate) => "create_prorations",
            (Provider::Stripe, Self::Invoice) => "always_invoice",
            (Provider::Stripe, Self::None) => "none",
            (Provider::Paddle, Self::Prorate) => "prorated_next_billing_period",
            (Provider::Paddle, Self::Invoice) => "prorated_immediately",
            (Provider::Paddle, Self::None) => "do_not_bill",
        }
    }
}
/// Safe local invoice/transaction projection; card data, addresses and client secrets are not stored.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Invoice {
    pub id: String,
    pub customer: String,
    pub status: String,
    pub currency: String,
    pub total: String,
    pub subscription: Option<String>,
}
impl Invoice {
    pub(crate) fn parse(provider: Provider, v: &Value) -> Result<Self> {
        let customer = match provider {
            Provider::Stripe => object_id(&v["customer"])?,
            Provider::Paddle => text(v, "customer_id")?,
        };
        let (currency, total, subscription) = if provider == Provider::Stripe {
            (
                text(v, "currency")?,
                v["total"]
                    .as_i64()
                    .map(|n| n.to_string())
                    .ok_or(Error::OutcomeUnknown)?,
                v.get("subscription")
                    .and_then(|s| object_id(s).ok())
                    .or_else(|| {
                        object_id(&v["parent"]["subscription_details"]["subscription"]).ok()
                    }),
            )
        } else {
            (
                text(v, "currency_code")?,
                v["details"]["totals"]["total"]
                    .as_str()
                    .ok_or(Error::OutcomeUnknown)?
                    .into(),
                object_id(&v["subscription_id"]).ok(),
            )
        };
        Ok(Self {
            id: text(v, "id")?,
            customer,
            status: text(v, "status")?,
            currency,
            total,
            subscription,
        })
    }
}
pub(crate) fn text(v: &Value, key: &str) -> Result<String> {
    v[key]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .ok_or(Error::OutcomeUnknown)
}
pub(crate) fn object_id(v: &Value) -> Result<String> {
    let id = v
        .as_str()
        .or_else(|| v["id"].as_str())
        .ok_or(Error::OutcomeUnknown)?;
    crate::client::identifier(id)?;
    Ok(id.into())
}
pub(crate) fn time(v: &Value) -> Option<i64> {
    v.as_i64().or_else(|| {
        v.as_str()?
            .parse::<jiff::Timestamp>()
            .ok()
            .map(|t| t.as_second())
    })
}
