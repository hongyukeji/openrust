use std::collections::HashSet;

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
        let mut citation = Self::from_url(value["url"].as_str()?, value["title"].as_str())?;
        citation.start_index = value["start_index"].as_u64();
        citation.end_index = value["end_index"].as_u64();
        Some(citation)
    }

    pub(crate) fn unique_sources<'a>(
        sources: impl IntoIterator<Item = (&'a Value, &'a Value)>,
    ) -> Vec<Self> {
        let mut seen = HashSet::new();
        sources
            .into_iter()
            .filter_map(|(url, title)| {
                let url = url.as_str()?;
                seen.insert(url).then_some(())?;
                Self::from_url(url, title.as_str())
            })
            .collect()
    }

    fn from_url(url: &str, title: Option<&str>) -> Option<Self> {
        let parsed = url::Url::parse(url).ok()?;
        if !matches!(parsed.scheme(), "http" | "https") || parsed.host().is_none() {
            return None;
        }
        Some(Self {
            url: url.into(),
            title: title.map(str::to_owned),
            start_index: None,
            end_index: None,
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
