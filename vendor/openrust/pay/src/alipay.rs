use crate::{
    Amount, Error, Order, Payment, Refund, Result, Scene, VerifiedNotification,
    crypto::{self, SigningKey, required},
};
use http::HeaderMap;
use openrust::http::client::Factory;
use serde_json::{Value, json, value::RawValue};
use std::{collections::BTreeMap, sync::Arc, time::Duration};

pub(crate) struct Alipay {
    app_id: String,
    seller_id: String,
    key: SigningKey,
    public_key: Vec<u8>,
    endpoint: url::Url,
    http: Arc<Factory>,
}
impl Alipay {
    pub fn new(profile: &Value, http: Arc<Factory>) -> Result<Self> {
        Ok(Self {
            app_id: required(profile, "app_id")?.to_owned(),
            seller_id: required(profile, "seller_id")?.to_owned(),
            key: SigningKey::new(&crypto::pem(profile, "private_key", "private_key_path")?)?,
            public_key: crypto::public_key(&crypto::pem(
                profile,
                "public_key",
                "public_key_path",
            )?)?,
            endpoint: crypto::endpoint(profile, "https://openapi.alipay.com/gateway.do")?,
            http,
        })
    }
    fn parameters(
        &self,
        method: &str,
        biz: &Value,
        order: Option<&Order>,
    ) -> Result<BTreeMap<String, String>> {
        let timestamp = jiff::Timestamp::now()
            .to_zoned(
                openrust::support::Date::timezone("Asia/Shanghai")
                    .map_err(|_| Error::Cryptography)?,
            )
            .strftime("%Y-%m-%d %H:%M:%S")
            .to_string();
        let mut params: BTreeMap<String, String> = [
            ("app_id", self.app_id.clone()),
            ("method", method.to_owned()),
            ("format", "JSON".into()),
            ("charset", "utf-8".into()),
            ("sign_type", "RSA2".into()),
            ("timestamp", timestamp),
            ("version", "1.0".into()),
            (
                "biz_content",
                serde_json::to_string(biz).map_err(|_| Error::Protocol)?,
            ),
        ]
        .into_iter()
        .map(|(k, v)| (k.into(), v))
        .collect();
        if let Some(order) = order {
            params.insert("notify_url".into(), order.notify_url.clone());
            if let Some(url) = &order.return_url {
                params.insert("return_url".into(), url.clone());
            }
        }
        let signature = self.key.sign(canonical(&params, false).as_bytes())?;
        params.insert("sign".into(), signature);
        Ok(params)
    }
    async fn request(&self, method: &str, biz: &Value, order: Option<&Order>) -> Result<Value> {
        let params = self.parameters(method, biz, order)?;
        let response = self
            .http
            .pending_request()
            .redact_payload()
            .as_form()
            .accept_json()
            .without_redirecting()
            .timeout(Duration::from_secs(15))
            .max_response_bytes(1024 * 1024)
            .post(self.endpoint.as_str(), &params)
            .await
            .map_err(|_| Error::OutcomeUnknown)?;
        if !response.successful() {
            return Err(Error::OutcomeUnknown);
        }
        // RawValue preserves the exact UTF-8 JSON bytes covered by Alipay's signature.
        let body = response.body().map_err(|_| Error::OutcomeUnknown)?;
        let fields: BTreeMap<String, &RawValue> =
            serde_json::from_slice(&body).map_err(|_| Error::OutcomeUnknown)?;
        let name = method.replace('.', "_") + "_response";
        let raw = fields.get(&name).ok_or(Error::OutcomeUnknown)?;
        let signed: String =
            serde_json::from_str(fields.get("sign").ok_or(Error::OutcomeUnknown)?.get())
                .map_err(|_| Error::OutcomeUnknown)?;
        crypto::verify(&self.public_key, raw.get().as_bytes(), &signed)
            .map_err(|_| Error::OutcomeUnknown)?;
        let result: Value = serde_json::from_str(raw.get()).map_err(|_| Error::OutcomeUnknown)?;
        if result["code"] != "10000" {
            return Err(Error::Rejected);
        }
        Ok(result)
    }
    pub async fn pay(&self, scene: Scene, order: &Order) -> Result<Payment> {
        order.validate()?;
        let (method, product) = match scene {
            Scene::Scan => ("alipay.trade.precreate", None),
            Scene::Web => ("alipay.trade.page.pay", Some("FAST_INSTANT_TRADE_PAY")),
            Scene::Wap => ("alipay.trade.wap.pay", Some("QUICK_WAP_WAY")),
            Scene::App => ("alipay.trade.app.pay", Some("QUICK_MSECURITY_PAY")),
            _ => return Err(Error::InvalidInput("Alipay payment scene")),
        };
        let mut biz = json!({"out_trade_no":order.out_trade_no,"subject":order.description,"total_amount":order.amount.yuan()});
        if let Some(product) = product {
            biz["product_code"] = json!(product);
        }
        if scene == Scene::Scan {
            let result = self.request(method, &biz, Some(order)).await?;
            if result["out_trade_no"] != order.out_trade_no {
                return Err(Error::OutcomeUnknown);
            }
            return Ok(Payment::QrCode(
                result["qr_code"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .ok_or(Error::OutcomeUnknown)?
                    .to_owned(),
            ));
        }
        let params = self.parameters(method, &biz, Some(order))?;
        let query = url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(&params)
            .finish();
        if scene == Scene::App {
            Ok(Payment::App(query))
        } else {
            let mut url = self.endpoint.clone();
            url.set_query(Some(&query));
            Ok(Payment::Redirect(url.into()))
        }
    }
    pub async fn find(&self, order: &str) -> Result<Value> {
        crate::types::identifier(order)?;
        let value = self
            .request("alipay.trade.query", &json!({"out_trade_no":order}), None)
            .await?;
        self.check_order(&value, order)?;
        Ok(value)
    }
    pub async fn close(&self, order: &str) -> Result<Value> {
        crate::types::identifier(order)?;
        let value = self
            .request("alipay.trade.close", &json!({"out_trade_no":order}), None)
            .await?;
        self.check_order(&value, order)?;
        Ok(value)
    }
    pub async fn refund(&self, refund: &Refund) -> Result<Value> {
        refund.validate()?;
        let mut biz = json!({"out_trade_no":refund.out_trade_no,"out_request_no":refund.out_refund_no,"refund_amount":refund.amount.yuan()});
        if let Some(reason) = &refund.reason {
            biz["refund_reason"] = json!(reason);
        }
        let value = self.request("alipay.trade.refund", &biz, None).await?;
        self.check_order(&value, &refund.out_trade_no)?;
        Ok(value)
    }
    pub async fn find_refund(&self, order: &str, refund: &str) -> Result<Value> {
        crate::types::identifier(order)?;
        crate::types::identifier(refund)?;
        let value = self
            .request(
                "alipay.trade.fastpay.refund.query",
                &json!({"out_trade_no":order,"out_request_no":refund}),
                None,
            )
            .await?;
        self.check_order(&value, order)?;
        if value["out_request_no"] != refund {
            return Err(Error::OutcomeUnknown);
        }
        Ok(value)
    }
    fn check_order(&self, value: &Value, order: &str) -> Result<()> {
        if value["out_trade_no"] != order {
            Err(Error::OutcomeUnknown)
        } else {
            Ok(())
        }
    }
    pub fn callback(&self, headers: &HeaderMap, body: &[u8]) -> Result<VerifiedNotification> {
        if body.len() > 1024 * 1024 {
            return Err(Error::Protocol);
        }
        let content = headers
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .split(';')
            .next()
            .unwrap_or("")
            .trim();
        if !content.eq_ignore_ascii_case("application/x-www-form-urlencoded") {
            return Err(Error::Protocol);
        }
        let text = std::str::from_utf8(body).map_err(|_| Error::Protocol)?;
        // Validate percent escapes and decoded UTF-8 before the forgiving URL decoder.
        let mut params = BTreeMap::new();
        for pair in text.split('&') {
            let (key, value) = pair.split_once('=').ok_or(Error::Protocol)?;
            let key = decode(key)?;
            let value = decode(value)?;
            if params.insert(key, value).is_some() {
                return Err(Error::Protocol);
            }
        }
        if params.get("sign_type").map(String::as_str) != Some("RSA2") {
            return Err(Error::Verification);
        }
        crypto::verify(
            &self.public_key,
            canonical(&params, true).as_bytes(),
            params.get("sign").ok_or(Error::Verification)?,
        )?;
        let get = |key| {
            params
                .get(key)
                .filter(|v| !v.is_empty())
                .map(String::as_str)
                .ok_or(Error::Protocol)
        };
        if get("app_id")? != self.app_id || get("seller_id")? != self.seller_id {
            return Err(Error::OrderMismatch);
        }
        if !matches!(get("trade_status")?, "TRADE_SUCCESS" | "TRADE_FINISHED")
            || params.contains_key("refund_fee")
        {
            return Err(Error::Protocol);
        }
        let out_trade_no = get("out_trade_no")?.to_owned();
        crate::types::identifier(&out_trade_no)?;
        let transaction_id = get("trade_no")?.to_owned();
        Ok(VerifiedNotification {
            identity: format!("alipay:{}:{}:{transaction_id}", self.seller_id, self.app_id),
            out_trade_no,
            transaction_id,
            amount: Amount::from_yuan(get("total_amount")?)?,
            provider: "alipay",
            acknowledgment: None,
        })
    }
}
fn canonical(params: &BTreeMap<String, String>, notification: bool) -> String {
    params
        .iter()
        .filter(|(k, v)| {
            k.as_str() != "sign" && !(notification && k.as_str() == "sign_type") && !v.is_empty()
        })
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&")
}
fn decode(input: &str) -> Result<String> {
    let mut bytes = Vec::new();
    let mut chars = input.as_bytes().iter().copied();
    while let Some(c) = chars.next() {
        match c {
            b'+' => bytes.push(b' '),
            b'%' => {
                let h = chars
                    .next()
                    .and_then(|c| (c as char).to_digit(16))
                    .ok_or(Error::Protocol)?;
                let l = chars
                    .next()
                    .and_then(|c| (c as char).to_digit(16))
                    .ok_or(Error::Protocol)?;
                bytes.push((h * 16 + l) as u8);
            }
            c => bytes.push(c),
        }
    }
    String::from_utf8(bytes).map_err(|_| Error::Protocol)
}
