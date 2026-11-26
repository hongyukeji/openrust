use std::{collections::BTreeMap, time::Duration};

use openrust::{
    http::HttpFuture,
    support::{ArrayKey, Collection, Str, Stringable},
};
use serde_json::Value;

use crate::{AiError, AiResult};

use super::{Answer, Boolean, Choice, ChoiceOption, Classification, PendingClassification};

const DECISION: &str = "decision";

#[derive(Clone, Copy, Default)]
pub struct DecideOptions<'a> {
    pub threshold: Option<f64>,
    pub provider: Option<&'a str>,
    pub model: Option<&'a str>,
    pub timeout: Option<Duration>,
}

impl DecideOptions<'_> {
    fn validate(self) -> AiResult<()> {
        if self
            .threshold
            .is_some_and(|threshold| !(0.0..=1.0).contains(&threshold))
        {
            return Err(AiError::InvalidClassification);
        }
        Ok(())
    }

    fn configure(self, pending: PendingClassification) -> PendingClassification {
        match self.timeout {
            Some(timeout) => pending.timeout(timeout),
            None => pending,
        }
    }
}

pub trait Decides {
    fn decide(&self, question: impl Into<Boolean>) -> HttpFuture<'_, AiResult<bool>> {
        self.decide_with(question, DecideOptions::default())
    }

    fn decide_with<'a>(
        &'a self,
        question: impl Into<Boolean>,
        options: DecideOptions<'a>,
    ) -> HttpFuture<'a, AiResult<bool>>;
}

impl Decides for str {
    fn decide_with<'a>(
        &'a self,
        question: impl Into<Boolean>,
        options: DecideOptions<'a>,
    ) -> HttpFuture<'a, AiResult<bool>> {
        let question = question.into();
        Box::pin(async move {
            options.validate()?;
            let pending = Classification::of(self)?.question(DECISION, question);
            let result = options
                .configure(pending)
                .classify_with(options.provider, options.model)
                .await?;
            match result.answer(DECISION) {
                Some(Answer::Boolean(answer)) => {
                    Ok(answer.is_true_at(options.threshold.unwrap_or(0.5)))
                }
                _ => Err(AiError::InvalidResponse),
            }
        })
    }
}

impl Decides for Stringable {
    fn decide_with<'a>(
        &'a self,
        question: impl Into<Boolean>,
        options: DecideOptions<'a>,
    ) -> HttpFuture<'a, AiResult<bool>> {
        self.value().decide_with(question, options)
    }
}

pub trait DecidesText {
    fn decide(value: &str, question: impl Into<Boolean>) -> HttpFuture<'_, AiResult<bool>> {
        Self::decide_with(value, question, DecideOptions::default())
    }

    fn decide_with<'a>(
        value: &'a str,
        question: impl Into<Boolean>,
        options: DecideOptions<'a>,
    ) -> HttpFuture<'a, AiResult<bool>>;
}

impl DecidesText for Str {
    fn decide_with<'a>(
        value: &'a str,
        question: impl Into<Boolean>,
        options: DecideOptions<'a>,
    ) -> HttpFuture<'a, AiResult<bool>> {
        value.decide_with(question, options)
    }
}

pub trait DecidesBetween<T: Sync> {
    fn decide<'a>(
        &'a self,
        question: impl Into<String>,
        state: impl Into<Value>,
    ) -> HttpFuture<'a, AiResult<Option<&'a T>>>
    where
        T: AsRef<str>,
    {
        self.decide_by(
            question,
            state,
            |value| value.as_ref().into(),
            DecideOptions::default(),
        )
    }

    fn decide_by<'a>(
        &'a self,
        question: impl Into<String>,
        state: impl Into<Value>,
        by: impl FnMut(&T) -> ChoiceOption,
        options: DecideOptions<'a>,
    ) -> HttpFuture<'a, AiResult<Option<&'a T>>>;

    fn decide_key<'a>(
        &'a self,
        question: impl Into<String>,
        state: impl Into<Value>,
        options: DecideOptions<'a>,
    ) -> HttpFuture<'a, AiResult<Option<String>>>
    where
        T: AsRef<str>;
}

impl<T: Sync> DecidesBetween<T> for Collection<T> {
    fn decide_by<'a>(
        &'a self,
        question: impl Into<String>,
        state: impl Into<Value>,
        mut by: impl FnMut(&T) -> ChoiceOption,
        options: DecideOptions<'a>,
    ) -> HttpFuture<'a, AiResult<Option<&'a T>>> {
        let mut items = BTreeMap::new();
        let choices: Vec<_> = self
            .all()
            .values()
            .map(|item| {
                let option = by(item);
                items.insert(option.value.clone(), item);
                option
            })
            .collect();
        let pending = Classification::of(state)
            .map(|pending| pending.question(DECISION, Choice::new(question, choices)));
        Box::pin(async move {
            let choice = choose(pending?, options).await?;
            Ok(choice.and_then(|choice| items.get(&choice).copied()))
        })
    }

    fn decide_key<'a>(
        &'a self,
        question: impl Into<String>,
        state: impl Into<Value>,
        options: DecideOptions<'a>,
    ) -> HttpFuture<'a, AiResult<Option<String>>>
    where
        T: AsRef<str>,
    {
        let choices: AiResult<Vec<_>> = self
            .all()
            .iter()
            .map(|(key, description)| match key {
                ArrayKey::String(key) => Ok(ChoiceOption::new(key, description.as_ref())),
                ArrayKey::Integer(_) => Err(AiError::InvalidClassification),
            })
            .collect();
        let question = question.into();
        let pending = Classification::of(state);
        Box::pin(async move {
            choose(
                pending?.question(DECISION, Choice::new(question, choices?)),
                options,
            )
            .await
        })
    }
}

async fn choose(
    pending: PendingClassification,
    options: DecideOptions<'_>,
) -> AiResult<Option<String>> {
    options.validate()?;
    let mut result = options
        .configure(pending)
        .classify_with(options.provider, options.model)
        .await?;
    match result.answers.remove(DECISION) {
        Some(Answer::Choice(answer)) => {
            let probability = answer.probability_of(&answer.choice);
            Ok(options
                .threshold
                .is_none_or(|threshold| probability >= threshold)
                .then_some(answer.choice))
        }
        _ => Err(AiError::InvalidResponse),
    }
}
