use std::collections::HashSet;

use serde::Serialize;
use serde_json::{Value, json};

use crate::{AiError, AiResult};

pub enum Question {
    Boolean(Boolean),
    Choice(Choice),
    Score(Score),
}

impl From<Boolean> for Question {
    fn from(question: Boolean) -> Self {
        Self::Boolean(question)
    }
}
impl From<Choice> for Question {
    fn from(question: Choice) -> Self {
        Self::Choice(question)
    }
}
impl From<Score> for Question {
    fn from(question: Score) -> Self {
        Self::Score(question)
    }
}

pub struct Boolean {
    instructions: String,
    criteria: [Option<String>; 2],
}

impl Boolean {
    pub fn instructions(&self) -> &str {
        &self.instructions
    }

    pub fn criteria(&self) -> [Option<&str>; 2] {
        [self.criteria[0].as_deref(), self.criteria[1].as_deref()]
    }

    pub fn new(instructions: impl Into<String>) -> Self {
        Self {
            instructions: instructions.into(),
            criteria: [None, None],
        }
    }

    pub fn when_true(mut self, criteria: impl Into<String>) -> Self {
        self.criteria[0] = Some(criteria.into());
        self
    }

    pub fn when_false(mut self, criteria: impl Into<String>) -> Self {
        self.criteria[1] = Some(criteria.into());
        self
    }
}

pub struct Choice {
    instructions: String,
    pub(super) options: Vec<ChoiceOption>,
}

impl Choice {
    pub fn instructions(&self) -> &str {
        &self.instructions
    }

    pub fn options(&self) -> &[ChoiceOption] {
        &self.options
    }

    pub fn new(
        instructions: impl Into<String>,
        options: impl IntoIterator<Item = impl Into<ChoiceOption>>,
    ) -> Self {
        Self {
            instructions: instructions.into(),
            options: options.into_iter().map(Into::into).collect(),
        }
    }

    pub(super) fn contains(&self, value: &str) -> bool {
        self.options.iter().any(|option| option.value == value)
    }
}

#[derive(Clone, Serialize)]
pub struct ChoiceOption {
    pub value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl ChoiceOption {
    pub fn new(value: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            description: Some(description.into()),
        }
    }
}

impl From<&str> for ChoiceOption {
    fn from(value: &str) -> Self {
        value.to_owned().into()
    }
}

impl From<String> for ChoiceOption {
    fn from(value: String) -> Self {
        Self {
            value,
            description: None,
        }
    }
}

pub struct Score {
    instructions: String,
    pub(super) levels: Vec<ScoreLevel>,
}

impl Score {
    pub fn instructions(&self) -> &str {
        &self.instructions
    }

    pub fn levels(&self) -> &[ScoreLevel] {
        &self.levels
    }

    pub fn new(
        instructions: impl Into<String>,
        levels: impl IntoIterator<Item = impl Into<ScoreLevel>>,
    ) -> Self {
        Self {
            instructions: instructions.into(),
            levels: levels.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ScoreLevel {
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl ScoreLevel {
    pub fn new(label: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            description: Some(description.into()),
        }
    }
}

impl From<&str> for ScoreLevel {
    fn from(label: &str) -> Self {
        label.to_owned().into()
    }
}

impl From<String> for ScoreLevel {
    fn from(label: String) -> Self {
        Self {
            label,
            description: None,
        }
    }
}

impl Question {
    pub(super) fn definition(&self, name: &str) -> AiResult<Value> {
        if name.trim().is_empty() {
            return Err(AiError::InvalidClassification);
        }
        match self {
            Self::Boolean(question) if !question.instructions.trim().is_empty() => {
                let mut instructions = question.instructions.clone();
                for (case, criteria) in ["True", "False"].into_iter().zip(&question.criteria) {
                    if let Some(criteria) = criteria {
                        instructions.push_str("\n\n");
                        instructions.push_str(case);
                        instructions.push_str(": ");
                        instructions.push_str(criteria);
                    }
                }
                Ok(json!({"type":"predicate","name":name,"instructions":instructions}))
            }
            Self::Choice(question)
                if !question.instructions.trim().is_empty() && question.options.len() >= 2 =>
            {
                let mut seen = HashSet::new();
                if question
                    .options
                    .iter()
                    .any(|option| option.value.trim().is_empty() || !seen.insert(&option.value))
                {
                    return Err(AiError::InvalidClassification);
                }
                Ok(
                    json!({"type":"choice","name":name,"instructions":question.instructions,"choices":question.options}),
                )
            }
            Self::Score(question)
                if !question.instructions.trim().is_empty()
                    && question.levels.len() >= 2
                    && question
                        .levels
                        .iter()
                        .all(|level| !level.label.trim().is_empty()) =>
            {
                Ok(
                    json!({"type":"score","name":name,"instructions":question.instructions,"levels":question.levels}),
                )
            }
            _ => Err(AiError::InvalidClassification),
        }
    }
}
