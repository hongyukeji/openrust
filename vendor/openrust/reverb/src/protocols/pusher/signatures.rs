use std::collections::BTreeMap;

use aws_lc_rs::hmac;
use md5::{Digest as _, Md5};

const HEX: &[u8; 16] = b"0123456789abcdef";

/// Signs a private or presence channel authorization for this socket.
pub fn sign_channel(
    secret: &[u8],
    socket: &str,
    channel: &str,
    channel_data: Option<&str>,
) -> String {
    sign(secret, &channel_message(socket, channel, channel_data))
}

/// Verifies the exact channel data bytes sent by the authorization endpoint.
pub fn verify_channel(
    secret: &[u8],
    socket: &str,
    channel: &str,
    channel_data: Option<&str>,
    signature: &str,
) -> bool {
    verify(
        secret,
        &channel_message(socket, channel, channel_data),
        signature,
    )
}

/// Signs a Pusher API request using its application-relative path and raw body.
pub fn sign_request(
    secret: &[u8],
    method: &str,
    path: &str,
    query: &BTreeMap<String, String>,
    body: &[u8],
) -> String {
    sign(secret, &request_message(method, path, query, body))
}

/// Verifies the API request signature against the method, route, query, and body.
pub fn verify_request(
    secret: &[u8],
    method: &str,
    path: &str,
    query: &BTreeMap<String, String>,
    body: &[u8],
) -> bool {
    query.get("auth_signature").is_some_and(|signature| {
        verify(
            secret,
            &request_message(method, path, query, body),
            signature,
        )
    })
}

/// The hexadecimal MD5 body checksum required by the Pusher request protocol.
pub fn body_checksum(body: &[u8]) -> String {
    hexadecimal(&Md5::digest(body))
}

fn channel_message(socket: &str, channel: &str, channel_data: Option<&str>) -> String {
    let mut message = format!("{socket}:{channel}");
    if let Some(data) = channel_data {
        message.push(':');
        message.push_str(data);
    }
    message
}

fn request_message(
    method: &str,
    path: &str,
    query: &BTreeMap<String, String>,
    body: &[u8],
) -> String {
    let mut parameters: BTreeMap<&str, &str> = query
        .iter()
        .filter(|(key, _)| {
            !matches!(
                key.as_str(),
                "auth_signature" | "body_md5" | "appId" | "appKey" | "channelName"
            )
        })
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect();
    let checksum;
    if !body.is_empty() {
        checksum = body_checksum(body);
        parameters.insert("body_md5", &checksum);
    }
    let parameters = parameters
        .into_iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join("&");
    format!("{method}\n{path}\n{parameters}")
}

fn sign(secret: &[u8], message: &str) -> String {
    let key = hmac::Key::new(hmac::HMAC_SHA256, secret);
    hexadecimal(hmac::sign(&key, message.as_bytes()).as_ref())
}

fn verify(secret: &[u8], message: &str, signature: &str) -> bool {
    let Some(signature) = decode_signature(signature) else {
        return false;
    };
    let key = hmac::Key::new(hmac::HMAC_SHA256, secret);
    hmac::verify(&key, message.as_bytes(), &signature).is_ok()
}

fn hexadecimal(bytes: &[u8]) -> String {
    let mut hex = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        hex.push(char::from(HEX[usize::from(byte >> 4)]));
        hex.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    hex
}

fn decode_signature(signature: &str) -> Option<[u8; 32]> {
    if signature.len() != 64 {
        return None;
    }
    let mut result = [0; 32];
    for (byte, digits) in result.iter_mut().zip(signature.as_bytes().chunks_exact(2)) {
        let high = HEX.iter().position(|digit| *digit == digits[0])?;
        let low = HEX.iter().position(|digit| *digit == digits[1])?;
        *byte = ((high << 4) | low).try_into().ok()?;
    }
    Some(result)
}
