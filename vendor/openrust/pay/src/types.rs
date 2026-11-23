use crate::{Error, Result};
use serde_json::Value;

/// An exact, positive CNY amount in fen. Floating point input is never accepted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Amount(i64);
impl Amount {
    pub fn from_fen(fen: i64) -> Result<Self> {
        if !(1..=99_999_999_999).contains(&fen) {
            return Err(Error::InvalidInput("amount"));
        }
        Ok(Self(fen))
    }
    pub fn fen(self) -> i64 {
        self.0
    }
    pub fn yuan(self) -> String {
        format!("{}.{:02}", self.0 / 100, self.0 % 100)
    }
    pub fn from_yuan(value: &str) -> Result<Self> {
        let (whole, fraction) = value.split_once('.').unwrap_or((value, ""));
        if whole.is_empty()
            || whole.len() > 10
            || !whole.bytes().all(|c| c.is_ascii_digit())
            || fraction.len() > 2
            || !fraction.bytes().all(|c| c.is_ascii_digit())
            || value.ends_with('.')
        {
            return Err(Error::InvalidInput("amount"));
        }
        let whole: i64 = whole.parse().map_err(|_| Error::InvalidInput("amount"))?;
        let fractional: i64 = if fraction.is_empty() {
            0
        } else {
            fraction
                .parse::<i64>()
                .map_err(|_| Error::InvalidInput("amount"))?
                * if fraction.len() == 1 { 10 } else { 1 }
        };
        Self::from_fen(whole * 100 + fractional)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scene {
    Scan,
    Web,
    Wap,
    App,
    Jsapi,
}
/// One business order. The merchant order number must remain stable across retries.
#[derive(Clone)]
pub struct Order {
    pub out_trade_no: String,
    pub description: String,
    pub amount: Amount,
    pub notify_url: String,
    pub return_url: Option<String>,
    pub openid: Option<String>,
    pub client_ip: Option<std::net::IpAddr>,
}
impl Order {
    pub fn new(
        out_trade_no: impl Into<String>,
        description: impl Into<String>,
        amount: Amount,
        notify_url: impl Into<String>,
    ) -> Self {
        Self {
            out_trade_no: out_trade_no.into(),
            description: description.into(),
            amount,
            notify_url: notify_url.into(),
            return_url: None,
            openid: None,
            client_ip: None,
        }
    }
    pub(crate) fn validate(&self) -> Result<()> {
        identifier(&self.out_trade_no)?;
        if self.description.trim().is_empty() || self.description.len() > 127 {
            return Err(Error::InvalidInput("description"));
        }
        crate::crypto::https_url(&self.notify_url)?;
        if let Some(url) = &self.return_url {
            crate::crypto::https_url(url)?;
        }
        Ok(())
    }
}
/// Refunds use the original total and a stable, unique merchant refund number.
#[derive(Clone)]
pub struct Refund {
    pub out_trade_no: String,
    pub out_refund_no: String,
    pub total: Amount,
    pub amount: Amount,
    pub reason: Option<String>,
}
impl Refund {
    pub(crate) fn validate(&self) -> Result<()> {
        identifier(&self.out_trade_no)?;
        identifier(&self.out_refund_no)?;
        if self.amount.0 > self.total.0 || self.reason.as_ref().is_some_and(|s| s.len() > 80) {
            return Err(Error::InvalidInput("refund"));
        }
        Ok(())
    }
}
/// Pass the payload to the matching payment UI; it is not proof of payment.
pub enum Payment {
    QrCode(String),
    Redirect(String),
    App(String),
    Jsapi(Value),
    WechatApp(Value),
}
impl std::fmt::Debug for Payment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Payment([REDACTED])")
    }
}
/// Load this from the trusted business order, never from callback parameters.
#[derive(Clone, Debug)]
pub struct ExpectedPayment {
    pub out_trade_no: String,
    pub amount: Amount,
}
/// A signature-verified payment-success event. Constructible only by a gateway.
/// Its fields have not yet been matched to a business order until `validate` succeeds.
#[derive(Clone)]
pub struct VerifiedNotification {
    pub(crate) identity: String,
    pub(crate) out_trade_no: String,
    pub(crate) transaction_id: String,
    pub(crate) amount: Amount,
    pub(crate) provider: &'static str,
}
impl std::fmt::Debug for VerifiedNotification {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("VerifiedNotification([REDACTED])")
    }
}
impl VerifiedNotification {
    pub fn out_trade_no(&self) -> &str {
        &self.out_trade_no
    }
    pub fn transaction_id(&self) -> &str {
        &self.transaction_id
    }
    pub fn amount(&self) -> Amount {
        self.amount
    }
    pub fn provider(&self) -> &'static str {
        self.provider
    }
    pub fn validate(&self, expected: &ExpectedPayment) -> Result<()> {
        if self.out_trade_no != expected.out_trade_no || self.amount != expected.amount {
            return Err(Error::OrderMismatch);
        }
        Ok(())
    }
    /// Send only after the business transaction commits (including the duplicate path).
    pub fn success(&self) -> http::Response<String> {
        if self.provider == "alipay" {
            http::Response::new("success".to_owned())
        } else {
            let mut response = http::Response::new(String::new());
            *response.status_mut() = http::StatusCode::NO_CONTENT;
            response
        }
    }
    pub(crate) fn key(&self) -> String {
        crate::crypto::hash(self.identity.as_bytes())
    }
    pub(crate) fn fingerprint(&self) -> String {
        crate::crypto::hash(
            &serde_json::to_vec(&(
                &self.provider,
                &self.out_trade_no,
                &self.transaction_id,
                self.amount.0,
            ))
            .expect("scalar payment notification"),
        )
    }
}
pub(crate) fn identifier(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 32
        || !value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_'))
    {
        return Err(Error::InvalidInput("merchant order/refund number"));
    }
    Ok(())
}
