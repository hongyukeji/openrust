use crate::{Error, Result};
use aws_lc_rs::{aead, signature};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use rsa::{
    PrivateKey, PublicKey,
    pkcs1::{DecodeRsaPrivateKey, DecodeRsaPublicKey, EncodeRsaPublicKey},
    pkcs8::{DecodePrivateKey, DecodePublicKey, EncodePrivateKey},
};
use serde_json::Value;
use zeroize::Zeroizing;

pub(crate) struct SigningKey(signature::RsaKeyPair);
impl SigningKey {
    pub fn new(pem: &str) -> Result<Self> {
        let key = PrivateKey::from_pkcs8_pem(pem)
            .or_else(|_| PrivateKey::from_pkcs1_pem(pem))
            .map_err(|_| Error::Configuration("private_key"))?;
        key.validate()
            .map_err(|_| Error::Configuration("private_key"))?;
        let der = key.to_pkcs8_der().map_err(|_| Error::Cryptography)?;
        let key = signature::RsaKeyPair::from_pkcs8(der.as_bytes())
            .map_err(|_| Error::Configuration("private_key"))?;
        if key.public_modulus_len() != 256 {
            return Err(Error::Configuration("RSA keys must be 2048 bits"));
        }
        Ok(Self(key))
    }
    pub fn sign(&self, message: &[u8]) -> Result<String> {
        let mut signature = vec![0; self.0.public_modulus_len()];
        self.0
            .sign(
                &signature::RSA_PKCS1_SHA256,
                &aws_lc_rs::rand::SystemRandom::new(),
                message,
                &mut signature,
            )
            .map_err(|_| Error::Cryptography)?;
        Ok(STANDARD.encode(signature))
    }
}
pub(crate) fn public_key(pem: &str) -> Result<Vec<u8>> {
    let key = PublicKey::from_public_key_pem(pem)
        .or_else(|_| PublicKey::from_pkcs1_pem(pem))
        .map_err(|_| Error::Configuration("public_key"))?;
    key.to_pkcs1_der()
        .map(|der| der.as_bytes().to_vec())
        .map_err(|_| Error::Cryptography)
}
pub(crate) fn verify(key: &[u8], message: &[u8], signed: &str) -> Result<()> {
    let signed = STANDARD.decode(signed).map_err(|_| Error::Verification)?;
    signature::UnparsedPublicKey::new(&signature::RSA_PKCS1_2048_8192_SHA256, key)
        .verify(message, &signed)
        .map_err(|_| Error::Verification)
}
pub(crate) fn decrypt(
    key: &[u8],
    nonce: &str,
    aad: &str,
    ciphertext: &str,
) -> Result<Zeroizing<Vec<u8>>> {
    let key = aead::UnboundKey::new(&aead::AES_256_GCM, key)
        .map(aead::LessSafeKey::new)
        .map_err(|_| Error::Cryptography)?;
    let nonce: [u8; 12] = nonce
        .as_bytes()
        .try_into()
        .map_err(|_| Error::Verification)?;
    let mut bytes = Zeroizing::new(
        STANDARD
            .decode(ciphertext)
            .map_err(|_| Error::Verification)?,
    );
    let length = key
        .open_in_place(
            aead::Nonce::assume_unique_for_key(nonce),
            aead::Aad::from(aad.as_bytes()),
            &mut bytes,
        )
        .map_err(|_| Error::Verification)?
        .len();
    bytes.truncate(length);
    Ok(bytes)
}
pub(crate) fn nonce() -> Result<String> {
    let mut bytes = [0u8; 16];
    aws_lc_rs::rand::fill(&mut bytes).map_err(|_| Error::Cryptography)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
pub(crate) fn hash(bytes: &[u8]) -> String {
    aws_lc_rs::digest::digest(&aws_lc_rs::digest::SHA256, bytes)
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
pub(crate) fn required<'a>(profile: &'a Value, field: &'static str) -> Result<&'a str> {
    profile[field]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .ok_or(Error::Configuration(field))
}
pub(crate) fn pem(
    profile: &Value,
    field: &'static str,
    path: &'static str,
) -> Result<Zeroizing<String>> {
    match (
        profile[field].as_str().filter(|s| !s.is_empty()),
        profile[path].as_str().filter(|s| !s.is_empty()),
    ) {
        (Some(value), None) => Ok(Zeroizing::new(value.to_owned())),
        (None, Some(path)) => std::fs::read_to_string(path)
            .map(Zeroizing::new)
            .map_err(|_| Error::Configuration(field)),
        _ => Err(Error::Configuration(field)),
    }
}
pub(crate) fn endpoint(profile: &Value, default: &str) -> Result<url::Url> {
    let value = match profile.get("endpoint") {
        None | Some(Value::Null) => default,
        Some(Value::String(s)) => s,
        _ => return Err(Error::Configuration("endpoint")),
    };
    let url = url::Url::parse(value).map_err(|_| Error::Configuration("endpoint"))?;
    let local = matches!(url.host_str(), Some("127.0.0.1" | "[::1]" | "localhost"));
    if (url.scheme() != "https" && !(url.scheme() == "http" && local))
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(Error::Configuration("endpoint"));
    }
    Ok(url)
}
pub(crate) fn https_url(value: &str) -> Result<()> {
    let url = url::Url::parse(value).map_err(|_| Error::InvalidInput("callback URL"))?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(Error::InvalidInput("callback URL"));
    }
    Ok(())
}
