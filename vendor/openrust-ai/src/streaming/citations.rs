use serde::{Serialize, Serializer, ser::SerializeStruct};
use serde_json::Value;

/// 流式响应中的网页来源及其在输出文本中的位置。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct UrlCitation {
    pub url: String,
    pub title: Option<String>,
    pub start_index: Option<u64>,
    pub end_index: Option<u64>,
}

impl UrlCitation {
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
