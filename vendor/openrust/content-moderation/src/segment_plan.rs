use std::collections::BTreeSet;

use crate::StoredMedia;

pub(crate) const MAX_VIDEO_SEGMENTS: usize = 64;
const SHORT_VIDEO_LIMIT_MS: u32 = 60_000;

/// 已由应用核实并上传的原素材片段。起点使用原素材时间线。
pub struct VideoReviewSegment {
    pub(crate) start_ms: u32,
    pub(crate) source: StoredMedia,
}

impl VideoReviewSegment {
    pub fn new(start_ms: u32, source: StoredMedia) -> Self {
        Self { start_ms, source }
    }
}

#[derive(Eq, PartialEq)]
pub(crate) struct SegmentPlan {
    pub(crate) original: StoredMedia,
    pub(crate) start_offsets: Vec<u32>,
}

impl SegmentPlan {
    pub(crate) fn covers<'a>(
        &self,
        sources: impl ExactSizeIterator<Item = &'a StoredMedia>,
    ) -> bool {
        let Some(duration) = self.original.video_duration_ms() else {
            return false;
        };
        if self.original.version() == "null"
            || !(2..=MAX_VIDEO_SEGMENTS).contains(&sources.len())
            || sources.len() != self.start_offsets.len()
        {
            return false;
        }
        let mut seen = BTreeSet::new();
        let mut frontier = 0;
        let mut previous_start = None;
        for (&start, source) in self.start_offsets.iter().zip(sources) {
            let Some(length) = source
                .video_duration_ms()
                .filter(|&ms| ms < SHORT_VIDEO_LIMIT_MS)
            else {
                return false;
            };
            let Some(end) = start.checked_add(length) else {
                return false;
            };
            let reference = object(source);
            if start > frontier
                || end <= frontier
                || end > duration
                || previous_start.is_some_and(|previous| start <= previous)
                || source.version() == "null"
                || reference == object(&self.original)
                || !seen.insert(reference)
            {
                return false;
            }
            frontier = end;
            previous_start = Some(start);
        }
        frontier == duration
    }
}

fn object(source: &StoredMedia) -> (&str, &str, &str) {
    (source.disk(), source.key(), source.version())
}
