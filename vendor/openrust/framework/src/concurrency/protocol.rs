use aws_lc_rs::hmac;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::log::context::DehydratedContext;

use super::{Error, Result, Task};

pub(super) const COMMAND: &str = "invoke-serialized-closure";
pub(super) const INPUT_LIMIT: usize = 8 * 1024 * 1024;
const MARKER: &str = "@@CONCURRENCY-V1@@";

#[derive(Deserialize, Serialize)]
pub(super) struct Invocation {
    pub(super) task: Task,
    pub(super) context: DehydratedContext,
    pub(super) key: [u8; 32],
}

#[derive(Deserialize, Serialize)]
pub(super) enum Outcome {
    Success(Value),
    Failed(String),
}

pub(super) fn encode(outcome: &Outcome, key: &[u8; 32]) -> Result<String> {
    let payload = STANDARD.encode(serde_json::to_vec(outcome).map_err(Error::Encoding)?);
    let tag = hmac::sign(&hmac::Key::new(hmac::HMAC_SHA256, key), payload.as_bytes());
    Ok(format!(
        "{MARKER}{payload}.{}\n",
        STANDARD.encode(tag.as_ref())
    ))
}

pub(super) fn decode(output: &[u8], key: &[u8; 32]) -> Result<Outcome> {
    let key = hmac::Key::new(hmac::HMAC_SHA256, key);
    let output = String::from_utf8_lossy(output);
    for frame in output.rsplit(MARKER) {
        let line = frame.split(['\r', '\n']).next().unwrap_or_default();
        let Some((payload, signature)) = line.split_once('.') else {
            continue;
        };
        let Ok(tag) = STANDARD.decode(signature) else {
            continue;
        };
        if hmac::verify(&key, payload.as_bytes(), &tag).is_ok() {
            let payload = STANDARD
                .decode(payload)
                .map_err(|_| Error::InvalidEnvelope)?;
            return serde_json::from_slice(&payload).map_err(|_| Error::InvalidEnvelope);
        }
    }
    Err(Error::InvalidEnvelope)
}
