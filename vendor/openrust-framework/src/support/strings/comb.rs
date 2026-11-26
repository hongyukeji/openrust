use std::time::{SystemTime, UNIX_EPOCH};

use super::Str;

/// An error when generating a timestamp-first COMB identifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrderedUuidError {
    TimestampOutOfRange,
    RandomUnavailable,
}

impl std::fmt::Display for OrderedUuidError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::TimestampOutOfRange => "COMB timestamp is outside the 48-bit Unix range",
            Self::RandomUnavailable => "system randomness is unavailable",
        })
    }
}

impl std::error::Error for OrderedUuidError {}

pub(super) fn generate(time: SystemTime) -> Result<uuid::Uuid, OrderedUuidError> {
    let elapsed = time
        .duration_since(UNIX_EPOCH)
        .map_err(|_| OrderedUuidError::TimestampOutOfRange)?;
    let ticks = elapsed.as_micros() / 10;
    if ticks >= 1 << 48 {
        return Err(OrderedUuidError::TimestampOutOfRange);
    }
    let stamp = u64::try_from(ticks)
        .expect("checked 48-bit timestamp")
        .to_be_bytes();
    let mut bytes = Str::uuid()
        .map_err(|_| OrderedUuidError::RandomUnavailable)?
        .into_bytes();
    // TimestampFirstCombCodec places the six timestamp bytes before the untouched
    // UUID version and variant, followed by the remaining random bytes.
    bytes[..6].copy_from_slice(&stamp[2..]);
    Ok(uuid::Uuid::from_bytes(bytes))
}
