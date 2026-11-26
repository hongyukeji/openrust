use std::{collections::BTreeMap, sync::Arc};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use openrust::{filesystem::FilesystemManager, http::client::Factory};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    Endpoint, field,
    media::VersionedMediaLinks,
    tencent::{Tencent, Verdict, rpc_endpoint},
};
use crate::{
    Assessment, Confidence, Coverage, Decision, ImageInput, Modality, ModerationError,
    ModerationResult,
};

pub(super) struct ImageService {
    endpoint: Endpoint,
    biz_type: String,
    media: Option<VersionedMediaLinks>,
}

impl ImageService {
    pub(super) fn configured(
        http: Arc<Factory>,
        storage: Arc<FilesystemManager>,
        profile: &Value,
    ) -> ModerationResult<Option<Self>> {
        let Some(biz_type) = field(profile, "image_biz_type")? else {
            return Ok(None);
        };
        let endpoint_profile = json!({
            "base_url": field(profile, "image_base_url")?,
            "timeout_ms": profile.get("timeout_ms").cloned().unwrap_or(json!(10_000)),
        });
        let endpoint = rpc_endpoint(http, &endpoint_profile, "https://ims.tencentcloudapi.com")?;
        let media = match (
            field(profile, "media_disk")?,
            field(profile, "media_origin")?,
        ) {
            (None, None) => None,
            _ => Some(VersionedMediaLinks::new(storage, profile)?),
        };
        Ok(Some(Self {
            endpoint,
            biz_type: biz_type.to_owned(),
            media,
        }))
    }

    pub(super) async fn assess(
        &self,
        driver: &Tencent,
        input: &ImageInput,
    ) -> ModerationResult<Assessment> {
        let mut body = json!({"BizType": self.biz_type, "Type": "IMAGE"});
        if let Some(bytes) = input.as_bytes() {
            if bytes.len() < 16 || bytes.len().div_ceil(3) * 4 >= 10_000_000 {
                return Err(ModerationError::InvalidInput("tencent image length"));
            }
            body["FileContent"] = json!(STANDARD.encode(bytes));
        } else if let (Some(media), Some(source)) = (&self.media, input.as_stored()) {
            body["FileUrl"] = json!(media.create(source).await?.as_str());
        } else {
            return Err(ModerationError::InvalidInput("tencent stored image"));
        }
        let result = driver
            .request(
                &self.endpoint,
                "ims",
                "ImageModeration",
                "2020-12-29",
                body,
                &mut None,
            )
            .await?;
        read_image(result, &self.biz_type)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ImageOutput {
    request_id: String,
    biz_type: String,
    #[serde(flatten)]
    summary: Verdict,
    label_results: Option<Vec<Verdict>>,
    object_results: Option<Vec<Verdict>>,
    lib_results: Option<Vec<Verdict>>,
    ocr_results: Option<Vec<Verdict>>,
}

fn read_image(output: ImageOutput, biz_type: &str) -> ModerationResult<Assessment> {
    if output.biz_type != biz_type {
        return Err(ModerationError::Protocol);
    }
    // 同步响应没有原图帧数或采样证明，OCR 也不能用是否存在命中来证明完整覆盖。
    let mut coverage = BTreeMap::from([(Modality::Visual, Coverage::Partial)]);
    if output
        .ocr_results
        .as_ref()
        .is_some_and(|results| !results.is_empty())
    {
        coverage.insert(Modality::Ocr, Coverage::Partial);
    }
    let verdicts = [
        (Modality::Visual, vec![output.summary]),
        (Modality::Visual, output.label_results.unwrap_or_default()),
        (Modality::Visual, output.object_results.unwrap_or_default()),
        (Modality::Visual, output.lib_results.unwrap_or_default()),
        (Modality::Ocr, output.ocr_results.unwrap_or_default()),
    ];
    let mut decision = Decision::Allow;
    let mut evidence = Vec::new();
    for (modality, group) in verdicts {
        for verdict in group {
            match verdict.decision()? {
                Decision::Allow if verdict.label == "Normal" => continue,
                Decision::Block => decision = Decision::Block,
                _ if decision == Decision::Allow => decision = Decision::Review,
                _ => {}
            }
            let finding = verdict.finding(modality);
            if evidence.first() != Some(&finding) {
                evidence.push(finding);
            }
        }
    }
    let (findings, scores): (Vec<_>, Vec<_>) = evidence.into_iter().unzip();
    let mut assessment = Assessment::new(decision, coverage, findings)
        .with_request_id(output.request_id)
        .map_err(|_| ModerationError::Protocol)?;
    for (index, score) in scores.into_iter().enumerate() {
        if let Some(score) = score {
            assessment = assessment.with_confidence(index, Confidence::new(score, 100.0)?)?;
        }
    }
    Ok(assessment)
}
