use crate::{Amount, Error, Order, Payment, Refund, Result, Scene, VerifiedNotification, types};
use http::{HeaderMap, Response};
use openrust::http::HttpFuture;
use serde_json::Value;

/// 由应用注册的支付渠道。实现须自行验证服务商响应及原始通知的签名和商户身份。
pub trait PayDriver: Send + Sync + 'static {
    fn pay<'a>(&'a self, scene: Scene, order: &'a Order) -> HttpFuture<'a, Result<Payment>>;
    fn find<'a>(&'a self, out_trade_no: &'a str) -> HttpFuture<'a, Result<Value>>;
    fn close<'a>(&'a self, out_trade_no: &'a str) -> HttpFuture<'a, Result<Value>>;
    fn refund<'a>(&'a self, refund: &'a Refund) -> HttpFuture<'a, Result<Value>>;
    fn find_refund<'a>(
        &'a self,
        out_trade_no: &'a str,
        out_refund_no: &'a str,
    ) -> HttpFuture<'a, Result<Value>>;
    fn callback(&self, headers: &HeaderMap, body: &[u8]) -> Result<DriverNotification>;
}

/// 受信渠道在验证成功后提供的支付字段及回执。去重身份由 Gateway 按渠道和商户生成。
pub struct DriverNotification {
    merchant: String,
    out_trade_no: String,
    transaction_id: String,
    amount: Amount,
    acknowledgment: Response<String>,
}

impl DriverNotification {
    /// `merchant` 必须是在渠道内稳定的已验证商户身份，不能取配置名或未验签的请求字段。
    pub fn new(
        merchant: impl Into<String>,
        out_trade_no: impl Into<String>,
        transaction_id: impl Into<String>,
        amount: Amount,
        acknowledgment: Response<String>,
    ) -> Result<Self> {
        let merchant = merchant.into();
        let out_trade_no = out_trade_no.into();
        let transaction_id = transaction_id.into();
        types::identifier(&out_trade_no)?;
        for value in [&merchant, &transaction_id] {
            if value.is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
                return Err(Error::InvalidInput("driver notification identity"));
            }
        }
        if !acknowledgment.status().is_success() {
            return Err(Error::InvalidInput("driver notification acknowledgment"));
        }
        Ok(Self {
            merchant,
            out_trade_no,
            transaction_id,
            amount,
            acknowledgment,
        })
    }

    pub(crate) fn into_verified(self, provider: &'static str) -> VerifiedNotification {
        let identity = serde_json::to_string(&(provider, &self.merchant, &self.transaction_id))
            .expect("scalar payment notification identity");
        VerifiedNotification {
            identity: format!("custom:{identity}"),
            out_trade_no: self.out_trade_no,
            transaction_id: self.transaction_id,
            amount: self.amount,
            provider,
            acknowledgment: Some(self.acknowledgment),
        }
    }
}
