use std::{collections::BTreeMap, sync::Arc};

use openrust::{config::Environment, http::client::Factory};
use serde::Deserialize;
use serde_json::Value;
use zeroize::Zeroizing;

use super::{Endpoint, field, http_status, secret};
use crate::{
    Assessment, Confidence, Coverage, Decision, Finding, Modality, ModerationDriver,
    ModerationError, ModerationFuture, ModerationResult, Operation, TextInput,
};

const RULE_LANGUAGES: &[&str] = &[
    "en", "zh", "da", "nl", "fi", "fr", "de", "it", "ko", "no", "pl", "pt", "ro", "ru", "es", "sv",
    "tl", "tr",
];
const ML_LANGUAGES: &[&str] = &["en", "fr", "it", "pt", "es", "ru", "tr"];
const RULE_CATEGORIES: &[&str] = &[
    "profanity",
    "personal",
    "link",
    "drug",
    "weapon",
    "spam",
    "content-trade",
    "money-transaction",
    "extremism",
    "violence",
    "self-harm",
    "medical",
];
const ML_MODELS: &[&str] = &["general", "self-harm"];
const GENERAL_CLASSES: &[&str] = &["sexual", "discriminatory", "insulting", "violent", "toxic"];

pub(super) struct Sightengine {
    endpoint: Endpoint,
    user: Zeroizing<String>,
    secret: Zeroizing<String>,
    languages: Vec<String>,
    mode: TextMode,
}

enum TextMode {
    Rules(Vec<String>),
    Ml {
        models: Vec<String>,
        review_score: f64,
    },
}

impl Sightengine {
    pub(super) fn new(
        http: Arc<Factory>,
        env: &Environment,
        profile: &Value,
    ) -> ModerationResult<Self> {
        let (mode, supported_languages) = match field(profile, "text_mode")? {
            Some("rules") => (
                TextMode::Rules(options(profile, "text_categories", RULE_CATEGORIES)?),
                RULE_LANGUAGES,
            ),
            Some("ml") => {
                let models = options(profile, "text_models", ML_MODELS)?;
                let review_score = profile
                    .get("text_review_score")
                    .and_then(Value::as_f64)
                    .filter(|score| (0.0..=1.0).contains(score))
                    .ok_or(ModerationError::InvalidConfiguration("text_review_score"))?;
                (
                    TextMode::Ml {
                        models,
                        review_score,
                    },
                    ML_LANGUAGES,
                )
            }
            _ => return Err(ModerationError::InvalidConfiguration("text_mode")),
        };
        let languages = options(profile, "text_languages", supported_languages)?;
        if matches!(&mode, TextMode::Ml { models, .. } if models.iter().any(|model| model == "self-harm"))
            && languages != ["en"]
        {
            return Err(ModerationError::InvalidConfiguration("text_languages"));
        }
        let mut endpoint = Endpoint::new(http, profile, "https://api.sightengine.com")?;
        if endpoint.url.path() != "/" {
            return Err(ModerationError::InvalidConfiguration("base_url"));
        }
        endpoint.url.set_path("/1.0/text/check.json");
        Ok(Self {
            endpoint,
            user: secret(env, profile, "api_user_env")?
                .ok_or(ModerationError::InvalidConfiguration("api_user_env"))?,
            secret: secret(env, profile, "api_secret_env")?
                .ok_or(ModerationError::InvalidConfiguration("api_secret_env"))?,
            languages,
            mode,
        })
    }

    fn languages(&self, hint: Option<&str>) -> ModerationResult<String> {
        let Some(hint) = hint else {
            return Ok(self.languages.join(","));
        };
        let language = hint
            .split('-')
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        if !self.languages.contains(&language) {
            return Err(ModerationError::InvalidInput("sightengine text language"));
        }
        Ok(language)
    }
}

impl ModerationDriver for Sightengine {
    fn supports(&self, operation: Operation) -> bool {
        operation == Operation::Text
    }

    fn text<'a>(&'a self, input: &'a TextInput) -> ModerationFuture<'a, Assessment> {
        Box::pin(async move {
            if input.text().chars().count() > 10_000 {
                return Err(ModerationError::InvalidInput("sightengine text length"));
            }
            let lang = self.languages(input.language())?;
            let body = {
                let mut form = url::form_urlencoded::Serializer::new(String::new());
                form.extend_pairs([
                    ("text", input.text()),
                    ("lang", &lang),
                    ("api_user", self.user.as_str()),
                    ("api_secret", self.secret.as_str()),
                ]);
                match &self.mode {
                    TextMode::Rules(categories) => {
                        form.append_pair("mode", "rules")
                            .append_pair("categories", &categories.join(","));
                    }
                    TextMode::Ml { models, .. } => {
                        form.append_pair("mode", "ml")
                            .append_pair("models", &models.join(","));
                    }
                }
                form.finish()
            };
            let response = self
                .endpoint
                .post_encoded(
                    http::HeaderMap::new(),
                    body,
                    "application/x-www-form-urlencoded",
                )
                .await?;
            http_status(response.status().as_u16())?;
            let output: TextOutput = response.json().map_err(|_| ModerationError::Protocol)?;
            if output.status != "success" {
                return Err(ModerationError::Protocol);
            }
            finish(output, &self.mode)
        })
    }
}

fn options(
    profile: &Value,
    field: &'static str,
    allowed: &[&str],
) -> ModerationResult<Vec<String>> {
    let values = profile
        .get(field)
        .and_then(Value::as_array)
        .filter(|values| !values.is_empty() && values.len() <= allowed.len())
        .ok_or(ModerationError::InvalidConfiguration(field))?;
    let mut settings = Vec::with_capacity(values.len());
    for value in values {
        let value = value
            .as_str()
            .filter(|value| allowed.contains(value))
            .ok_or(ModerationError::InvalidConfiguration(field))?;
        if settings.iter().any(|setting| setting == value) {
            return Err(ModerationError::InvalidConfiguration(field));
        }
        settings.push(value.to_owned());
    }
    Ok(settings)
}

#[derive(Deserialize)]
struct TextOutput {
    status: String,
    request: RequestMetadata,
    #[serde(flatten)]
    checks: BTreeMap<String, Value>,
}

#[derive(Deserialize)]
struct RequestMetadata {
    id: String,
}

#[derive(Deserialize)]
struct RuleCheck {
    matches: Vec<RuleMatch>,
}

#[derive(Deserialize)]
struct RuleMatch {
    #[serde(rename = "type")]
    kind: Option<String>,
}

#[derive(Deserialize)]
struct ClassScores {
    available: Vec<String>,
    #[serde(flatten)]
    scores: BTreeMap<String, f64>,
}

fn finish(output: TextOutput, mode: &TextMode) -> ModerationResult<Assessment> {
    let mut findings = Vec::new();
    let mut scores = Vec::new();
    let mut coverage = Coverage::Complete;
    match mode {
        TextMode::Rules(categories) => {
            for category in categories {
                let Some(check) = output.checks.get(category) else {
                    coverage = Coverage::Partial;
                    continue;
                };
                let check: RuleCheck =
                    serde_json::from_value(check.clone()).map_err(|_| ModerationError::Protocol)?;
                for matched in check.matches {
                    let label = match matched.kind.as_deref() {
                        Some(kind) if provider_tag(kind) => format!("{category}/{kind}"),
                        Some(_) => return Err(ModerationError::Protocol),
                        None => category.clone(),
                    };
                    findings.push(finding(category, label));
                }
            }
        }
        TextMode::Ml {
            models,
            review_score,
        } => {
            let mut expected = Vec::new();
            for model in models {
                match model.as_str() {
                    "general" => expected.extend_from_slice(GENERAL_CLASSES),
                    "self-harm" => expected.push("self-harm"),
                    _ => unreachable!("配置已校验"),
                }
            }
            if let Some(classes) = output.checks.get("moderation_classes") {
                let classes: ClassScores = serde_json::from_value(classes.clone())
                    .map_err(|_| ModerationError::Protocol)?;
                for &class in &expected {
                    let Some(score) = classes
                        .scores
                        .get(class)
                        .filter(|_| classes.available.iter().any(|available| available == class))
                    else {
                        coverage = Coverage::Partial;
                        continue;
                    };
                    let confidence =
                        Confidence::new(*score, 1.0).map_err(|_| ModerationError::Protocol)?;
                    if *score >= *review_score {
                        scores.push((findings.len(), confidence));
                        findings.push(finding(class, class.to_owned()));
                    }
                }
                if classes
                    .available
                    .iter()
                    .any(|class| !expected.contains(&class.as_str()))
                {
                    coverage = Coverage::Partial;
                }
            } else {
                coverage = Coverage::Partial;
            }
        }
    }
    let decision = if findings.is_empty() {
        Decision::Allow
    } else {
        Decision::Review
    };
    let mut result = Assessment::new(
        decision,
        BTreeMap::from([(Modality::Text, coverage)]),
        findings,
    )
    .with_request_id(output.request.id)
    .map_err(|_| ModerationError::Protocol)?;
    for (index, confidence) in scores {
        result = result.with_confidence(index, confidence)?;
    }
    Ok(result)
}

fn provider_tag(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn finding(class: &str, provider_label: String) -> Finding {
    let category = match class {
        "profanity" => "profanity",
        "personal" => "personal_information",
        "drug" | "weapon" => "contraband",
        "violent" | "violence" | "extremism" => "violence",
        "discriminatory" => "discriminatory",
        "insulting" | "toxic" => "harassment",
        "self-harm" => "self_harm",
        _ => class,
    };
    Finding {
        modality: Modality::Text,
        category: category.to_owned(),
        provider_label,
    }
}
