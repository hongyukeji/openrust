#![allow(dead_code, reason = "Shared protocol and database fixtures")]
use aws_lc_rs::{aead, signature};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use openrust::{config::Repository, http::client::Factory};
use openrust_pay::{Amount, Order, PayManager};
use rsa::{
    PrivateKey, PublicKey,
    pkcs1::EncodeRsaPublicKey,
    pkcs8::DecodePublicKey,
    pkcs8::{DecodePrivateKey, EncodePrivateKey},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};
pub const MERCHANT: &str = include_str!("../fixtures/merchant.pem");
pub const PLATFORM: &str = include_str!("../fixtures/platform.pem");
pub const PUBLIC: &str = include_str!("../fixtures/platform-public.pem");
pub const SECRET: &str = "0123456789abcdef0123456789abcdef";
pub fn sign(pem: &str, message: &[u8]) -> String {
    let private = PrivateKey::from_pkcs8_pem(pem)
        .unwrap()
        .to_pkcs8_der()
        .unwrap();
    let key = signature::RsaKeyPair::from_pkcs8(private.as_bytes()).unwrap();
    let mut out = vec![0; key.public_modulus_len()];
    key.sign(
        &signature::RSA_PKCS1_SHA256,
        &aws_lc_rs::rand::SystemRandom::new(),
        message,
        &mut out,
    )
    .unwrap();
    STANDARD.encode(out)
}
pub fn verify(message: &[u8], signed: &str) {
    let key = PublicKey::from_public_key_pem(include_str!("../fixtures/merchant-public.pem"))
        .unwrap()
        .to_pkcs1_der()
        .unwrap();
    signature::UnparsedPublicKey::new(&signature::RSA_PKCS1_2048_8192_SHA256, key.as_bytes())
        .verify(message, &STANDARD.decode(signed).unwrap())
        .unwrap();
}
pub fn profile(driver: &str, endpoint: &str) -> Value {
    json!({"driver":driver,"app_id":"app123","mch_id":"1900001234","seller_id":"2088123456","serial_no":"ABCD1234","private_key":MERCHANT,"public_key":PUBLIC,"public_key_id":"PUB_KEY_ID_123","api_v3_key":SECRET,"endpoint":endpoint})
}
pub fn manager(driver: &str, endpoint: &str) -> (Arc<Repository>, PayManager) {
    let config = Arc::new(Repository::new(BTreeMap::from([(
        "pay".into(),
        json!({"default":"main","drivers":{"main":profile(driver,endpoint)}}),
    )])));
    let manager = PayManager::new(config.clone(), Arc::new(Factory::new().unwrap()));
    (config, manager)
}
pub fn order() -> Order {
    Order::new(
        "order_001",
        "测试订单 & coffee",
        Amount::fromFen(1234).unwrap(),
        "https://shop.example/pay/notify",
    )
}
pub fn expected() -> openrust_pay::ExpectedPayment {
    openrust_pay::ExpectedPayment {
        out_trade_no: "order_001".into(),
        amount: Amount::fromFen(1234).unwrap(),
    }
}
pub fn signed_headers(body: &[u8], offset: i64) -> http::HeaderMap {
    let timestamp = (jiff::Timestamp::now().as_second() + offset).to_string();
    let nonce = "fixture-nonce";
    let mut msg = format!("{timestamp}\n{nonce}\n").into_bytes();
    msg.extend_from_slice(body);
    msg.push(b'\n');
    let mut headers = http::HeaderMap::new();
    for (k, v) in [
        ("wechatpay-serial", "PUB_KEY_ID_123".to_owned()),
        ("wechatpay-timestamp", timestamp),
        ("wechatpay-nonce", nonce.to_owned()),
        ("wechatpay-signature", sign(PLATFORM, &msg)),
    ] {
        headers.insert(k, http::HeaderValue::from_str(&v).unwrap());
    }
    headers
}
pub fn wechat_callback(overrides: Value) -> (http::HeaderMap, Vec<u8>) {
    let mut payload = json!({"appid":"app123","mchid":"1900001234","out_trade_no":"order_001","transaction_id":"wx_tx_001","trade_state":"SUCCESS","amount":{"total":1234,"currency":"CNY"}});
    for (k, v) in overrides.as_object().unwrap() {
        payload[k] = v.clone();
    }
    let mut body = serde_json::to_vec(&payload).unwrap();
    let key = aead::LessSafeKey::new(
        aead::UnboundKey::new(&aead::AES_256_GCM, SECRET.as_bytes()).unwrap(),
    );
    let nonce = b"fixture-nonc";
    key.seal_in_place_append_tag(
        aead::Nonce::assume_unique_for_key(*nonce),
        aead::Aad::from(b"transaction"),
        &mut body,
    )
    .unwrap();
    let body=serde_json::to_vec(&json!({"id":"event_001","event_type":"TRANSACTION.SUCCESS","resource_type":"encrypt-resource","resource":{"algorithm":"AEAD_AES_256_GCM","original_type":"transaction","associated_data":"transaction","nonce":std::str::from_utf8(nonce).unwrap(),"ciphertext":STANDARD.encode(body)}})).unwrap();
    (signed_headers(&body, 0), body)
}
pub fn canonical(params: &BTreeMap<String, String>, notification: bool) -> String {
    params
        .iter()
        .filter(|(k, v)| {
            k.as_str() != "sign" && !(notification && k.as_str() == "sign_type") && !v.is_empty()
        })
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&")
}
pub fn alipay_callback(overrides: Value) -> (http::HeaderMap, Vec<u8>) {
    let mut params: BTreeMap<String, String> = [
        ("app_id", "app123"),
        ("seller_id", "2088123456"),
        ("out_trade_no", "order_001"),
        ("trade_no", "ali_tx_001"),
        ("total_amount", "12.34"),
        ("trade_status", "TRADE_SUCCESS"),
        ("sign_type", "RSA2"),
        ("subject", "订单 & coffee=1"),
    ]
    .into_iter()
    .map(|(k, v)| (k.into(), v.into()))
    .collect();
    for (k, v) in overrides.as_object().unwrap() {
        params.insert(k.clone(), v.as_str().unwrap().into());
    }
    params.insert(
        "sign".into(),
        sign(PLATFORM, canonical(&params, true).as_bytes()),
    );
    let body = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(params)
        .finish()
        .into_bytes();
    let mut headers = http::HeaderMap::new();
    headers.insert(
        "content-type",
        http::HeaderValue::from_static("application/x-www-form-urlencoded; charset=utf-8"),
    );
    (headers, body)
}
#[derive(Clone)]
pub struct ServerState {
    pub mode: Arc<AtomicUsize>,
    pub requests: Arc<Mutex<Vec<(String, Value)>>>,
}
pub struct Server {
    pub endpoint: String,
    pub state: ServerState,
    handle: tokio::task::JoinHandle<()>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.handle.abort();
    }
}
impl Server {
    pub async fn new() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/", listener.local_addr().unwrap());
        let state = ServerState {
            mode: Arc::new(AtomicUsize::new(0)),
            requests: Arc::default(),
        };
        let app = axum::Router::new()
            .fallback(handle)
            .with_state(state.clone());
        let handle = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self {
            endpoint,
            state,
            handle,
        }
    }
    pub fn mode(&self, mode: usize) {
        self.state.mode.store(mode, Ordering::SeqCst);
    }
    pub fn calls(&self) -> usize {
        self.state.requests.lock().unwrap().len()
    }
}
async fn handle(
    axum::extract::State(state): axum::extract::State<ServerState>,
    request: axum::extract::Request,
) -> http::Response<axum::body::Body> {
    let (parts, body) = request.into_parts();
    let body = axum::body::to_bytes(body, 1024 * 1024).await.unwrap();
    let path = parts.uri.path_and_query().unwrap().as_str();
    let mode = state.mode.load(Ordering::SeqCst);
    let mut response = if path.starts_with("/v3/") {
        let auth = parts.headers["authorization"]
            .to_str()
            .unwrap()
            .strip_prefix("WECHATPAY2-SHA256-RSA2048 ")
            .unwrap();
        let auth: BTreeMap<_, _> = auth
            .split(',')
            .map(|p| p.split_once('=').unwrap())
            .map(|(k, v)| (k, v.trim_matches('"')))
            .collect();
        assert_eq!(auth["mchid"], "1900001234");
        assert_eq!(auth["serial_no"], "ABCD1234");
        assert_eq!(parts.headers["wechatpay-serial"], "PUB_KEY_ID_123");
        let mut msg = format!(
            "{}\n{path}\n{}\n{}\n",
            parts.method, auth["timestamp"], auth["nonce_str"]
        )
        .into_bytes();
        msg.extend_from_slice(&body);
        msg.push(b'\n');
        verify(&msg, auth["signature"]);
        let payload: Value = if body.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&body).unwrap()
        };
        state
            .requests
            .lock()
            .unwrap()
            .push((path.into(), payload.clone()));
        let response = if path.ends_with("/native") {
            json!({"code_url":"weixin://fixture"})
        } else if path.ends_with("/jsapi") || path.ends_with("/app") {
            json!({"prepay_id":"wx-prepay"})
        } else if path.ends_with("/h5") {
            json!({"h5_url":"https://wx.tenpay.com/fixture"})
        } else if path.contains("refunds") {
            json!({"out_trade_no":"order_001","out_refund_no":"refund_001","refund_id":"refund_tx_001","status":"SUCCESS","amount":{"total":1234,"refund":100,"currency":"CNY"}})
        } else {
            json!({"appid":"app123","mchid":"1900001234","out_trade_no":"order_001","trade_state":"SUCCESS","transaction_id":"wx_tx_001","amount":{"total":1234,"currency":"CNY"}})
        };
        let body = if path.ends_with("/close") {
            Vec::new()
        } else {
            serde_json::to_vec(&response).unwrap()
        };
        let headers = signed_headers(&body, if mode == 3 { -301 } else { 0 });
        let mut response = http::Response::new(axum::body::Body::from(body));
        *response.headers_mut() = headers;
        if path.ends_with("/close") {
            *response.status_mut() = http::StatusCode::NO_CONTENT;
        }
        response
    } else {
        let params: BTreeMap<String, String> =
            url::form_urlencoded::parse(&body).into_owned().collect();
        verify(canonical(&params, false).as_bytes(), &params["sign"]);
        assert_eq!(params["app_id"], "app123");
        assert_eq!(params["sign_type"], "RSA2");
        let biz: Value = serde_json::from_str(&params["biz_content"]).unwrap();
        let method = &params["method"];
        state.requests.lock().unwrap().push((method.clone(), biz));
        let response = "{ \"code\": \"10000\", \"out_trade_no\": \"order_001\", \"qr_code\": \"https://qr.alipay.com/fixture\", \"out_request_no\": \"refund_001\", \"trade_no\": \"ali_tx_001\" }".to_owned();
        let signed = sign(PLATFORM, response.as_bytes());
        let body = format!(
            "{{\"{}_response\":{},\"sign\":{}}}",
            method.replace('.', "_"),
            response,
            serde_json::to_string(&signed).unwrap()
        );
        http::Response::new(axum::body::Body::from(body))
    };
    match mode {
        1 => {
            response
                .headers_mut()
                .insert("wechatpay-signature", http::HeaderValue::from_static("bad"));
            if !path.starts_with("/v3/") {
                *response.body_mut() =
                    axum::body::Body::from("{\"error_response\":{\"code\":\"40004\"}}");
            }
        }
        2 => {
            *response.status_mut() = http::StatusCode::FOUND;
            response.headers_mut().insert(
                "location",
                http::HeaderValue::from_static("http://127.0.0.1:1/leak"),
            );
        }
        _ => {}
    }
    if mode == 4 {
        tokio::time::sleep(std::time::Duration::from_secs(16)).await;
    }
    response
}
