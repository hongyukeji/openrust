use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value;

use crate::{AiError, AiResult};

use super::{Question, ScoreLevel};

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Answer {
    Boolean(BooleanAnswer),
    Choice(ChoiceAnswer),
    Score(ScoreAnswer),
    Refusal,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct BooleanAnswer {
    pub probability: f64,
}

impl BooleanAnswer {
    pub fn is_true(&self) -> bool {
        self.is_true_at(0.5)
    }

    pub fn is_true_at(&self, threshold: f64) -> bool {
        self.probability >= threshold
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ChoiceAnswer {
    pub choice: String,
    pub probabilities: BTreeMap<String, f64>,
    pub confidence: Option<f64>,
}

impl ChoiceAnswer {
    pub fn probability_of(&self, option: &str) -> f64 {
        self.probabilities.get(option).copied().unwrap_or_default()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ScoreAnswer {
    pub score: f64,
    pub probabilities: BTreeMap<usize, f64>,
    pub legend: Vec<ScoreLevel>,
    pub confidence: Option<f64>,
}

impl ScoreAnswer {
    pub fn level(&self) -> usize {
        self.probabilities
            .iter()
            .max_by(|(left, l), (right, r)| l.total_cmp(r).then_with(|| right.cmp(left)))
            .map(|(index, _)| *index)
            .unwrap_or_else(|| self.score.round() as usize)
    }

    pub fn label(&self) -> Option<&str> {
        self.legend
            .get(self.level())
            .map(|level| level.label.as_str())
    }

    pub fn normalized(&self) -> f64 {
        let highest = self.legend.len().saturating_sub(1);
        if highest == 0 {
            0.0
        } else {
            self.score / highest as f64
        }
    }
}

pub(super) fn decode(question: &Question, data: &Value) -> AiResult<Answer> {
    let confidence = || {
        data.get("confidence")
            .filter(|value| !value.is_null())
            .map(probability)
            .transpose()
    };
    match (question, data["type"].as_str()) {
        (_, Some("refusal")) => Ok(Answer::Refusal),
        (Question::Boolean(_), Some("predicate")) => Ok(Answer::Boolean(BooleanAnswer {
            probability: probability(&data["probability"])?,
        })),
        (Question::Choice(question), Some("choice")) => {
            let choice = data["choice"]
                .as_str()
                .filter(|choice| question.contains(choice))
                .ok_or(AiError::InvalidResponse)?;
            let mut probabilities = BTreeMap::new();
            for entry in probabilities_data(&data["probabilities"])? {
                let name = entry["value"]
                    .as_str()
                    .filter(|choice| question.contains(choice))
                    .ok_or(AiError::InvalidResponse)?;
                if probabilities
                    .insert(name.into(), probability(&entry["probability"])?)
                    .is_some()
                {
                    return Err(AiError::InvalidResponse);
                }
            }
            Ok(Answer::Choice(ChoiceAnswer {
                choice: choice.into(),
                probabilities,
                confidence: confidence()?,
            }))
        }
        (Question::Score(question), Some("score")) => {
            let score = data["score"]
                .as_f64()
                .filter(|score| (0.0..=(question.levels.len() - 1) as f64).contains(score))
                .ok_or(AiError::InvalidResponse)?;
            let mut probabilities = BTreeMap::new();
            for entry in probabilities_data(&data["probabilities"])? {
                let index = entry["value"]
                    .as_u64()
                    .and_then(|value| usize::try_from(value).ok())
                    .filter(|index| *index < question.levels.len())
                    .ok_or(AiError::InvalidResponse)?;
                if probabilities
                    .insert(index, probability(&entry["probability"])?)
                    .is_some()
                {
                    return Err(AiError::InvalidResponse);
                }
            }
            Ok(Answer::Score(ScoreAnswer {
                score,
                probabilities,
                legend: question.levels.clone(),
                confidence: confidence()?,
            }))
        }
        _ => Err(AiError::InvalidResponse),
    }
}

fn probabilities_data(value: &Value) -> AiResult<&[Value]> {
    if value.is_null() {
        return Ok(&[]);
    }
    value
        .as_array()
        .map(Vec::as_slice)
        .ok_or(AiError::InvalidResponse)
}

fn probability(value: &Value) -> AiResult<f64> {
    value
        .as_f64()
        .filter(|value| (0.0..=1.0).contains(value))
        .ok_or(AiError::InvalidResponse)
}
