//! 测试用ES256签名器：真实校验WebAuthn签名，不代表硬件或生物识别验收。
use aws_lc_rs::{
    digest,
    rand::SystemRandom,
    signature::{ECDSA_P256_SHA256_ASN1_SIGNING, EcdsaKeyPair, KeyPair},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde_cbor_2::Value as Cbor;
use serde_json::{Value, json};
pub struct Authenticator {
    key: EcdsaKeyPair,
    id: Vec<u8>,
    origin: String,
}
impl Authenticator {
    pub fn new(id: u8) -> Self {
        Self::with_origin(id, "http://localhost")
    }
    pub fn with_origin(id: u8, origin: &str) -> Self {
        let rng = SystemRandom::new();
        let document = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, &rng).unwrap();
        Self {
            key: EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, document.as_ref())
                .unwrap(),
            id: vec![id; 32],
            origin: origin.into(),
        }
    }
    fn client(&self, options: &Value, kind: &str) -> Vec<u8> {
        serde_json::to_vec(&json!({"type":kind,"challenge":options["challenge"],"origin":self.origin,"crossOrigin":false})).unwrap()
    }
    fn auth_data(flags: u8, counter: u32) -> Vec<u8> {
        let mut bytes = digest::digest(&digest::SHA256, b"localhost")
            .as_ref()
            .to_vec();
        bytes.push(flags);
        bytes.extend(counter.to_be_bytes());
        bytes
    }
    pub fn register(&self, options: &Value) -> Value {
        let public = self.key.public_key().as_ref();
        let cose = Cbor::Map(
            [
                (Cbor::Integer(1), Cbor::Integer(2)),
                (Cbor::Integer(3), Cbor::Integer(-7)),
                (Cbor::Integer(-1), Cbor::Integer(1)),
                (Cbor::Integer(-2), Cbor::Bytes(public[1..33].into())),
                (Cbor::Integer(-3), Cbor::Bytes(public[33..65].into())),
            ]
            .into(),
        );
        let mut data = Self::auth_data(0x45, 0);
        data.extend([0u8; 16]);
        data.extend((self.id.len() as u16).to_be_bytes());
        data.extend(&self.id);
        data.extend(serde_cbor_2::to_vec(&cose).unwrap());
        let attestation = Cbor::Map(
            [
                (Cbor::Text("fmt".into()), Cbor::Text("none".into())),
                (Cbor::Text("attStmt".into()), Cbor::Map(Default::default())),
                (Cbor::Text("authData".into()), Cbor::Bytes(data)),
            ]
            .into(),
        );
        json!({"id":URL_SAFE_NO_PAD.encode(&self.id),"rawId":URL_SAFE_NO_PAD.encode(&self.id),"type":"public-key","authenticatorAttachment":"platform","clientExtensionResults":{},"response":{"clientDataJSON":URL_SAFE_NO_PAD.encode(self.client(options,"webauthn.create")),"attestationObject":URL_SAFE_NO_PAD.encode(serde_cbor_2::to_vec(&attestation).unwrap()),"transports":["internal"]}})
    }
    pub fn assertion(&self, options: &Value, handle: &str, counter: u32) -> Value {
        let client = self.client(options, "webauthn.get");
        let data = Self::auth_data(0x05, counter);
        let mut signed = data.clone();
        signed.extend(digest::digest(&digest::SHA256, &client).as_ref());
        let signature = self.key.sign(&SystemRandom::new(), &signed).unwrap();
        json!({"id":URL_SAFE_NO_PAD.encode(&self.id),"rawId":URL_SAFE_NO_PAD.encode(&self.id),"type":"public-key","authenticatorAttachment":"platform","clientExtensionResults":{},"response":{"clientDataJSON":URL_SAFE_NO_PAD.encode(client),"authenticatorData":URL_SAFE_NO_PAD.encode(data),"signature":URL_SAFE_NO_PAD.encode(signature.as_ref()),"userHandle":URL_SAFE_NO_PAD.encode(handle.as_bytes())}})
    }
}
