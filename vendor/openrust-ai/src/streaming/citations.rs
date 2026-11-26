use serde::{Deserialize, Serialize, Serializer, ser::SerializeStruct};
use serde_json::Value;

/// 网页来源以及上游报告的文本位置。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UrlCitation {
    pub url: String,
    pub title: Option<String>,
    pub start_index: Option<u64>,
    pub end_index: Option<u64>,
}

impl UrlCitation {
    pub(crate) fn from_annotations(annotations: &Value) -> impl Iterator<Item = Self> {
        annotations
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Self::from_annotation)
    }

    pub(crate) fn from_annotation(value: &Value) -> Option<Self> {
        if value["type"] != "url_citation" {
            return None;
        }
        let url = value["url"].as_str()?;
        let parsed = url::Url::parse(url).ok()?;
        if !matches!(parsed.scheme(), "http" | "https") || parsed.host().is_none() {
            return None;
        }
        Some(Self {
            url: url.into(),
            title: value["title"].as_str().map(str::to_owned),
            start_index: value["start_index"].as_u64(),
            end_index: value["end_index"].as_u64(),
        })
    }
}

pub(super) fn serialize_event<S: Serializer>(
    citation: &UrlCitation,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let mut fields = serializer.serialize_struct("Citation", 2)?;
    fields.serialize_field("title", &citation.title)?;
    fields.serialize_field("url", &citation.url)?;
    fields.end()
}
