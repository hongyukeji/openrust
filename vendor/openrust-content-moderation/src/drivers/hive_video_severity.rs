use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;

use super::{
    hive::{PatternMatches, ScoredClass, category, severity_scores},
    hive_ocr::Frame as OcrFrame,
    video::{VideoEvidence, build_assessment, seconds_to_ms},
};
use crate::{
    Confidence, Coverage, Decision, EvidenceSpan, Finding, Modality, ModerationError,
    ModerationResult, TaskId, TaskStatus,
};

#[derive(Deserialize)]
pub(super) struct SpeechChunk {
    classifications: Vec<Classification>,
}

#[derive(Deserialize)]
struct Classification {
    classes: Vec<ScoredClass>,
    #[serde(flatten)]
    patterns: PatternMatches,
}

pub(super) struct SeverityEvidence<'a> {
    classes: &'a BTreeSet<String>,
    threshold: i8,
    modality: Modality,
    peaks: BTreeMap<&'a str, (i8, Option<EvidenceSpan>)>,
    patterns: BTreeMap<String, VideoEvidence>,
    last_frame: Option<u64>,
    observed: bool,
}

impl<'a> SeverityEvidence<'a> {
    pub(super) fn new(classes: &'a BTreeSet<String>, threshold: i8, modality: Modality) -> Self {
        Self {
            classes,
            threshold,
            modality,
            peaks: BTreeMap::new(),
            patterns: BTreeMap::new(),
            last_frame: None,
            observed: false,
        }
    }

    pub(super) fn ocr(&mut self, frame: OcrFrame) -> ModerationResult<()> {
        let millis = seconds_to_ms(frame.time.ok_or(ModerationError::Protocol)?)?;
        if self.last_frame.is_some_and(|value| millis < value) {
            return Err(ModerationError::Protocol);
        }
        self.last_frame = Some(millis);
        self.observed = true;
        let span = Some(EvidenceSpan::new(millis, millis)?);
        for block in frame.frame_results {
            self.classification(&block.classes, &block.patterns, span)?;
        }
        Ok(())
    }

    pub(super) fn speech(&mut self, chunk: SpeechChunk) -> ModerationResult<()> {
        for section in chunk.classifications {
            // 上游句级类别关联字符区间，词时间戳不能直接当作句级时间。
            self.classification(&section.classes, &section.patterns, None)?;
        }
        Ok(())
    }

    fn classification(
        &mut self,
        scored: &[ScoredClass],
        patterns: &PatternMatches,
        span: Option<EvidenceSpan>,
    ) -> ModerationResult<()> {
        let scores = severity_scores(scored)?;
        for class in self.classes {
            if let Some(&score) = scores.get(class.as_str()).filter(|value| **value >= 0) {
                self.observed = true;
                let peak = self.peaks.entry(class.as_str()).or_insert((score, span));
                if score > peak.0 {
                    *peak = (score, span);
                }
            }
        }
        for finding in patterns.findings(self.modality) {
            self.observed = true;
            self.patterns
                .entry(finding.provider_label.clone())
                .or_insert(VideoEvidence {
                    finding,
                    span,
                    score: None,
                });
        }
        Ok(())
    }

    pub(super) fn finish(self, task: &TaskId) -> ModerationResult<TaskStatus> {
        let mut evidence = self
            .peaks
            .into_iter()
            .filter(|(_, (score, _))| *score >= self.threshold)
            .map(|(class, (score, span))| {
                Ok(VideoEvidence {
                    finding: Finding {
                        modality: self.modality,
                        category: category(class).to_owned(),
                        provider_label: class.to_owned(),
                    },
                    span,
                    score: Some(Confidence::new(f64::from(score), 3.0)?),
                })
            })
            .collect::<ModerationResult<Vec<_>>>()?;
        evidence.extend(self.patterns.into_values());
        let decision = if evidence.is_empty() {
            Decision::Allow
        } else {
            Decision::Review
        };
        let coverage = if self.observed {
            Coverage::Partial
        } else {
            Coverage::Unavailable
        };
        Ok(TaskStatus::Succeeded(build_assessment(
            decision,
            BTreeMap::from([(self.modality, coverage)]),
            Some(task.as_str().to_owned()),
            evidence,
        )?))
    }
}
