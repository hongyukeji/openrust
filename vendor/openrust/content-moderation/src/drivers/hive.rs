use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use http::{HeaderMap, HeaderValue, header::AUTHORIZATION};
use openrust::{config::Environment, http::client::Factory};
use serde::Deserialize;
use serde_json::{Value, json};
use zeroize::Zeroizing;

use super::{Endpoint, http_status, secret};
use crate::{
    Assessment, Confidence, Coverage, Decision, Finding, Modality, ModerationDriver,
    ModerationError, ModerationFuture, ModerationResult, Operation, TextInput,
};

pub(super) struct Hive {
    endpoint: Endpoint,
    authorization: HeaderValue,
    project: u64,
    classes: BTreeSet<String>,
    languages: BTreeSet<String>,
    review_severity: i8,
}

impl Hive {
    pub(super) fn new(
        http: Arc<Factory>,
        env: &Environment,
        profile: &Value,
    ) -> ModerationResult<Self> {
        let mut endpoint = Endpoint::new(http, profile, "https://api.thehive.ai")?;
        if endpoint.url.path() != "/" {
            return Err(ModerationError::InvalidConfiguration("base_url"));
        }
        endpoint.url.set_path("/api/v2/task/sync");
        let token = secret(env, profile, "token_env")?
            .ok_or(ModerationError::InvalidConfiguration("token_env"))?;
        let authorization = Zeroizing::new(format!("Token {}", token.as_str()));
        let mut authorization = HeaderValue::from_str(&authorization)
            .map_err(|_| ModerationError::InvalidConfiguration("token_env"))?;
        authorization.set_sensitive(true);
        let project = profile
            .get("text_project")
            .and_then(Value::as_u64)
            .filter(|value| *value != 0)
            .ok_or(ModerationError::InvalidConfiguration("text_project"))?;
        let classes = configured_set(profile, "text_classes", |class| {
            !class.is_empty()
                && class.len() <= 64
                && class
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        })?;
        let languages = match profile.get("text_languages") {
            None => BTreeSet::new(),
            Some(_) => configured_set(profile, "text_languages", |language| {
                language.len() == 2
                    && language
                        .bytes()
                        .all(|character| character.is_ascii_lowercase())
            })?,
        };
        let review_severity = profile
            .get("text_review_severity")
            .and_then(Value::as_i64)
            .filter(|value| (1..=3).contains(value))
            .ok_or(ModerationError::InvalidConfiguration(
                "text_review_severity",
            ))?;
        Ok(Self {
            endpoint,
            authorization,
            project,
            classes,
            languages,
            review_severity: review_severity as i8,
        })
    }

    async fn classify(&self, input: &TextInput) -> ModerationResult<Assessment> {
        // 上游按 UTF-16 码元计数，非 BMP 字符占两个。
        if input.text().encode_utf16().count() > 1024 {
            return Err(ModerationError::InvalidInput("hive text length"));
        }
        let language = input
            .language()
            .map(|hint| {
                let language = hint
                    .split('-')
                    .next()
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                self.languages
                    .get(&language)
                    .map(String::as_str)
                    .ok_or(ModerationError::InvalidInput("hive text language"))
            })
            .transpose()?;
        let body = {
            let mut form = url::form_urlencoded::Serializer::new(String::new());
            form.append_pair("text_data", input.text());
            if let Some(language) = language {
                form.append_pair("options", &json!({"input_language": language}).to_string());
            }
            form.finish()
        };
        let response = self
            .endpoint
            .post_encoded(
                HeaderMap::from_iter([(AUTHORIZATION, self.authorization.clone())]),
                body,
                "application/x-www-form-urlencoded",
            )
            .await?;
        http_status(response.status().as_u16())?;
        let mut task: TaskOutput = response.json().map_err(|_| ModerationError::Protocol)?;
        http_status(task.code)?;
        if task.project_id != self.project || task.status.len() != 1 {
            return Err(ModerationError::Protocol);
        }
        let result = task.status.pop().ok_or(ModerationError::Protocol)?;
        if result.status.code != "0" {
            return Err(ModerationError::Rejected);
        }
        let output = result.response.ok_or(ModerationError::Protocol)?;
        if output.input.model_type != "TEXT_CLASSIFICATION"
            || output.input.project_id != self.project
            || output.input.id != task.id
        {
            return Err(ModerationError::Protocol);
        }
        self.assessment(task.id, output, language)
    }

    fn assessment(
        &self,
        id: String,
        result: TextOutput,
        requested_language: Option<&str>,
    ) -> ModerationResult<Assessment> {
        let mut complete = !result.output.is_empty()
            && result.language.len() == 2
            && result
                .language
                .bytes()
                .all(|value| value.is_ascii_alphabetic())
            && requested_language
                .is_none_or(|language| result.language.eq_ignore_ascii_case(language))
            && self
                .classes
                .iter()
                .all(|class| result.moderated_classes.contains(class));
        let mut maximums = BTreeMap::new();
        for section in &result.output {
            let mut parsed = BTreeMap::new();
            for entry in &section.classes {
                if !(-1..=3).contains(&entry.score)
                    || parsed.insert(&entry.class, entry.score).is_some()
                {
                    return Err(ModerationError::Protocol);
                }
            }
            for class in &self.classes {
                if let Some(score) = parsed.get(class).filter(|score| **score >= 0) {
                    let maximum = maximums.entry(class).or_insert(0);
                    *maximum = (*maximum).max(*score);
                } else {
                    complete = false;
                }
            }
        }
        let mut findings = Vec::new();
        let mut confidence = Vec::new();
        for (class, score) in maximums {
            if score >= self.review_severity {
                confidence.push((findings.len(), Confidence::new(f64::from(score), 3.0)?));
                findings.push(Finding {
                    modality: Modality::Text,
                    category: category(class).to_owned(),
                    provider_label: class.clone(),
                });
            }
        }
        // 词表证据只保留该类命中，避免暴露匹配词、个人信息或自定义词表名称。
        for (hits, label, category) in [
            (result.text_filters, "text_filters", "profanity"),
            (result.pii_entities, "pii_entities", "personal_information"),
            (result.custom_classes, "custom_classes", "custom"),
        ] {
            if hits.is_some_and(|hits| !hits.is_empty()) {
                findings.push(Finding {
                    modality: Modality::Text,
                    category: category.to_owned(),
                    provider_label: label.to_owned(),
                });
            }
        }
        let decision = if findings.is_empty() {
            Decision::Allow
        } else {
            Decision::Review
        };
        let coverage = if complete {
            Coverage::Complete
        } else {
            Coverage::Partial
        };
        let mut assessment = Assessment::new(
            decision,
            BTreeMap::from([(Modality::Text, coverage)]),
            findings,
        )
        .with_request_id(id)
        .map_err(|_| ModerationError::Protocol)?;
        for (index, score) in confidence {
            assessment = assessment.with_confidence(index, score)?;
        }
        Ok(assessment)
    }
}

impl ModerationDriver for Hive {
    fn supports(&self, operation: Operation) -> bool {
        operation == Operation::Text
    }

    fn text<'a>(&'a self, input: &'a TextInput) -> ModerationFuture<'a, Assessment> {
        Box::pin(self.classify(input))
    }
}

#[derive(Deserialize)]
struct TaskOutput {
    id: String,
    code: u16,
    project_id: u64,
    status: Vec<JobOutput>,
}

#[derive(Deserialize)]
struct JobOutput {
    status: JobCode,
    response: Option<TextOutput>,
}

#[derive(Deserialize)]
struct JobCode {
    code: String,
}

#[derive(Deserialize)]
struct TextOutput {
    input: JobInput,
    language: String,
    moderated_classes: BTreeSet<String>,
    output: Vec<Section>,
    text_filters: Option<Vec<serde::de::IgnoredAny>>,
    pii_entities: Option<Vec<serde::de::IgnoredAny>>,
    custom_classes: Option<Vec<serde::de::IgnoredAny>>,
}

#[derive(Deserialize)]
struct JobInput {
    model_type: String,
    project_id: u64,
    id: String,
}

#[derive(Deserialize)]
struct Section {
    classes: Vec<ScoredClass>,
}

#[derive(Deserialize)]
struct ScoredClass {
    class: String,
    score: i8,
}

fn configured_set(
    profile: &Value,
    name: &'static str,
    valid: impl Fn(&str) -> bool,
) -> ModerationResult<BTreeSet<String>> {
    let values = profile
        .get(name)
        .and_then(Value::as_array)
        .filter(|values| !values.is_empty() && values.len() <= 64)
        .ok_or(ModerationError::InvalidConfiguration(name))?;
    let mut set = BTreeSet::new();
    for value in values {
        let value = value
            .as_str()
            .filter(|value| valid(value))
            .ok_or(ModerationError::InvalidConfiguration(name))?;
        if !set.insert(value.to_owned()) {
            return Err(ModerationError::InvalidConfiguration(name));
        }
    }
    Ok(set)
}

fn category(class: &str) -> &str {
    match class {
        "hate" => "discriminatory",
        "bullying" => "harassment",
        "drug" | "drugs" | "weapons" => "contraband",
        "promotions" => "advertising",
        "phone_number" => "personal_information",
        _ => class,
    }
}
