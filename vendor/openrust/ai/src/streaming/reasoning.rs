use serde_json::Value;

use super::{StreamEventData, TextMessages};

/// 供应商公开的推理摘要片段。
pub enum ReasoningPart {
    Delta {
        part: (String, u64),
        delta: String,
    },
    End {
        part: (String, u64),
        text: Option<String>,
    },
}

impl ReasoningPart {
    pub(crate) fn from_openai_frame(value: &Value) -> Option<Self> {
        let kind = value["type"].as_str()?;
        let part = (
            value["item_id"].as_str()?.to_owned(),
            value["summary_index"].as_u64()?,
        );
        match kind {
            "response.reasoning_summary_text.delta" => {
                let delta = value["delta"].as_str().filter(|delta| !delta.is_empty())?;
                Some(Self::Delta {
                    part,
                    delta: delta.into(),
                })
            }
            "response.reasoning_summary_text.done" | "response.reasoning_summary_part.done" => {
                let text = value["text"]
                    .as_str()
                    .or_else(|| value["part"]["text"].as_str())
                    .filter(|text| !text.is_empty())
                    .map(str::to_owned);
                Some(Self::End { part, text })
            }
            _ => None,
        }
    }
}

impl TextMessages {
    pub(crate) fn reasoning(&mut self, part: ReasoningPart) -> [Option<StreamEventData>; 3] {
        match part {
            ReasoningPart::Delta { part, delta } => {
                let (reasoning_id, starts) = self.open(Some(&part));
                let start = starts.then(|| StreamEventData::ReasoningStart {
                    reasoning_id: reasoning_id.clone(),
                });
                [
                    start,
                    Some(StreamEventData::ReasoningDelta {
                        reasoning_id,
                        delta,
                    }),
                    None,
                ]
            }
            ReasoningPart::End { part, text } => {
                if !self.indexed.contains_key(&part)
                    && let Some(delta) = text
                {
                    let reasoning_id = self.get(Some(&part)).id.clone();
                    return [
                        Some(StreamEventData::ReasoningStart {
                            reasoning_id: reasoning_id.clone(),
                        }),
                        Some(StreamEventData::ReasoningDelta {
                            reasoning_id: reasoning_id.clone(),
                            delta,
                        }),
                        Some(StreamEventData::ReasoningEnd { reasoning_id }),
                    ];
                }
                [
                    self.close(Some(&part))
                        .map(|reasoning_id| StreamEventData::ReasoningEnd { reasoning_id }),
                    None,
                    None,
                ]
            }
        }
    }
}
