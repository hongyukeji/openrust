use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use http::{HeaderMap, HeaderValue, header::AUTHORIZATION};
use openrust::{
    config::Environment,
    filesystem::FilesystemManager,
    http::client::{Factory, Part, Response},
};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};
use zeroize::Zeroizing;

use super::{Endpoint, hive_image::ImageService, http_status, secret};
use crate::{
    Assessment, Confidence, Coverage, Decision, Finding, ImageInput, Modality, ModerationDriver,
    ModerationError, ModerationFuture, ModerationResult, Operation, TextInput,
};

pub(super) struct Hive {
    text: Option<TextService>,
    image: Option<ImageService>,
}

impl Hive {
    pub(super) fn new(
        http: Arc<Factory>,
        env: &Environment,
        storage: Arc<FilesystemManager>,
        profile: &Value,
    ) -> ModerationResult<Self> {
        let text = TextService::configured(http.clone(), env, profile)?;
        let image = ImageService::configured(http, env, storage, profile)?;
        if text.is_none() && image.is_none() {
            return Err(ModerationError::InvalidConfiguration("hive services"));
        }
        Ok(Self { text, image })
    }
}

pub(super) struct Model {
    endpoint: Endpoint,
    authorization: HeaderValue,
    project: u64,
    model_type: String,
}

impl Model {
    pub(super) fn new(
        http: Arc<Factory>,
        env: &Environment,
        profile: &Value,
        token_key: &'static str,
        project_key: &'static str,
        model_type: &str,
    ) -> ModerationResult<Self> {
        let mut endpoint = Endpoint::new(http, profile, "https://api.thehive.ai")?;
        if endpoint.url.path() != "/" {
            return Err(ModerationError::InvalidConfiguration("base_url"));
        }
        endpoint.url.set_path("/api/v2/task/sync");
        let token = secret(env, profile, token_key)?
            .ok_or(ModerationError::InvalidConfiguration(token_key))?;
        let authorization = Zeroizing::new(format!("Token {}", token.as_str()));
        let mut authorization = HeaderValue::from_str(&authorization)
            .map_err(|_| ModerationError::InvalidConfiguration(token_key))?;
        authorization.set_sensitive(true);
        let project = profile
            .get(project_key)
            .and_then(Value::as_u64)
            .filter(|value| *value != 0)
            .ok_or(ModerationError::InvalidConfiguration(project_key))?;
        Ok(Self {
            endpoint,
            authorization,
            project,
            model_type: model_type.to_owned(),
        })
    }

    fn headers(&self) -> HeaderMap {
        HeaderMap::from_iter([(AUTHORIZATION, self.authorization.clone())])
    }

    pub(super) async fn submit<T: DeserializeOwned>(
        &self,
        body: String,
    ) -> ModerationResult<(String, T)> {
        let response = self
            .endpoint
            .post_encoded(self.headers(), body, "application/x-www-form-urlencoded")
            .await?;
        self.validate(response)
    }

    pub(super) async fn submit_parts<T: DeserializeOwned>(
        &self,
        parts: Vec<(&'static str, Part)>,
    ) -> ModerationResult<(String, T)> {
        let response = self.endpoint.post_parts(self.headers(), parts).await?;
        self.validate(response)
    }

    fn validate<T: DeserializeOwned>(&self, response: Response) -> ModerationResult<(String, T)> {
        http_status(response.status().as_u16())?;
        let task: TaskOutput<T> = response.json().map_err(|_| ModerationError::Protocol)?;
        http_status(task.code)?;
        if task.project_id != self.project {
            return Err(ModerationError::Protocol);
        }
        let job = match task.status {
            Jobs::Single(job) => job,
            Jobs::List(mut jobs) if jobs.len() == 1 => {
                jobs.pop().ok_or(ModerationError::Protocol)?
            }
            Jobs::List(_) => return Err(ModerationError::Protocol),
        };
        if !matches!(&job.status.code, StatusCode::Text(code) if code == "0")
            && !matches!(job.status.code, StatusCode::Number(0))
        {
            return Err(ModerationError::Rejected);
        }
        let output = job.response.ok_or(ModerationError::Protocol)?;
        if output.input.model_type != self.model_type
            || output.input.project_id != self.project
            || output.input.id != task.id
        {
            return Err(ModerationError::Protocol);
        }
        Ok((task.id, output.result))
    }
}

struct TextService {
    model: Model,
    classes: BTreeSet<String>,
    languages: BTreeSet<String>,
    review_severity: i8,
}

impl TextService {
    fn configured(
        http: Arc<Factory>,
        env: &Environment,
        profile: &Value,
    ) -> ModerationResult<Option<Self>> {
        if profile.get("text_project").is_none() {
            return Ok(None);
        }
        let model = Model::new(
            http,
            env,
            profile,
            "token_env",
            "text_project",
            "TEXT_CLASSIFICATION",
        )?;
        let classes = configured_set(profile, "text_classes", provider_class)?;
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
        Ok(Some(Self {
            model,
            classes,
            languages,
            review_severity: review_severity as i8,
        }))
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
        let (id, result) = self.model.submit(body).await?;
        self.assessment(id, result, language)
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
        match operation {
            Operation::Text => self.text.is_some(),
            Operation::Image => self.image.is_some(),
            _ => false,
        }
    }

    fn text<'a>(&'a self, input: &'a TextInput) -> ModerationFuture<'a, Assessment> {
        Box::pin(async move {
            let text = self
                .text
                .as_ref()
                .ok_or(ModerationError::Unsupported(Operation::Text))?;
            text.classify(input).await
        })
    }

    fn image<'a>(&'a self, input: &'a ImageInput) -> ModerationFuture<'a, Assessment> {
        Box::pin(async move {
            let image = self
                .image
                .as_ref()
                .ok_or(ModerationError::Unsupported(Operation::Image))?;
            image.assess(input).await
        })
    }
}

#[derive(Deserialize)]
struct TaskOutput<T> {
    id: String,
    code: u16,
    project_id: u64,
    status: Jobs<T>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Jobs<T> {
    List(Vec<JobOutput<T>>),
    Single(JobOutput<T>),
}

#[derive(Deserialize)]
struct JobOutput<T> {
    status: JobCode,
    response: Option<JobResponse<T>>,
}

#[derive(Deserialize)]
struct JobCode {
    code: StatusCode,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum StatusCode {
    Text(String),
    Number(u16),
}

#[derive(Deserialize)]
struct JobResponse<T> {
    input: JobInput,
    #[serde(flatten)]
    result: T,
}

#[derive(Deserialize)]
struct TextOutput {
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

pub(super) fn configured_set(
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

pub(super) fn provider_class(class: &str) -> bool {
    !class.is_empty()
        && class.len() <= 64
        && class
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
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
