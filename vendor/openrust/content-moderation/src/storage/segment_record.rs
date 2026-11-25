use serde::{Deserialize, Serialize};

use crate::{ModerationError, ModerationResult, StoredMedia, segment_plan::SegmentPlan};

const MAX_BYTES: usize = 4096;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Record {
    format: u8,
    disk: String,
    key: String,
    version: String,
    duration_ms: u32,
    start_offsets: Vec<u32>,
}

pub(super) fn encode(plan: &SegmentPlan) -> ModerationResult<String> {
    let record = Record {
        format: 1,
        disk: plan.original.disk().to_owned(),
        key: plan.original.key().to_owned(),
        version: plan.original.version().to_owned(),
        duration_ms: plan
            .original
            .video_duration_ms()
            .ok_or(ModerationError::ReviewStorage)?,
        start_offsets: plan.start_offsets.clone(),
    };
    serde_json::to_string(&record)
        .ok()
        .filter(|encoded| encoded.len() <= MAX_BYTES)
        .ok_or(ModerationError::ReviewStorage)
}

pub(super) fn decode(value: &str) -> ModerationResult<SegmentPlan> {
    if value.len() > MAX_BYTES {
        return Err(ModerationError::ReviewStorage);
    }
    let record: Record = serde_json::from_str(value).map_err(|_| ModerationError::ReviewStorage)?;
    if record.format != 1 {
        return Err(ModerationError::ReviewStorage);
    }
    let original = StoredMedia::new(record.disk, record.key, record.version)
        .and_then(|media| media.with_video_duration_ms(record.duration_ms))
        .map_err(|_| ModerationError::ReviewStorage)?;
    Ok(SegmentPlan {
        original,
        start_offsets: record.start_offsets,
    })
}
