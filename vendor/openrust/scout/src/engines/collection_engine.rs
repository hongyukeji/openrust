use super::{Engine, SearchResults};
use crate::{Builder, ScoutFuture, Searchable};
use serde_json::json;
#[derive(Default)]
pub struct CollectionEngine;
impl CollectionEngine {
    async fn searchModels<M: Searchable>(&self, builder: &Builder<M>) -> crate::Result<Vec<M>> {
        let query = builder.collectionQuery()?;
        let models = M::makeSearchableUsing(query.get().await?.into_iter().collect()).await?;
        let needle = builder.query.to_lowercase();
        let mut results = Vec::new();
        for model in models {
            if !model.shouldBeSearchable() {
                continue;
            }
            if needle.is_empty()
                || model.toSearchableArray()?.values().any(|value| {
                    let text = match value {
                        serde_json::Value::String(s) => s.clone(),
                        serde_json::Value::Null => "null".into(),
                        serde_json::Value::Bool(b) => if *b { "1" } else { "" }.into(),
                        serde_json::Value::Number(n) if n.is_f64() => {
                            openrust::support::php_float_string(n.as_f64().expect("JSON float"))
                        }
                        serde_json::Value::Number(n) => n.to_string(),
                        value => phpJson(value),
                    };
                    text.to_lowercase().contains(&needle)
                })
            {
                results.push(model);
            }
        }
        Ok(results)
    }
    fn results<M: Searchable>(models: &[M], total: u64) -> crate::Result<SearchResults> {
        Ok(SearchResults {
            ids: models.iter().filter_map(Searchable::getScoutKey).collect(),
            total,
            raw: json!({"results":models.iter().map(|m|m.to_array()).collect::<openrust::database::DatabaseResult<Vec<_>>>()?, "total":total}),
        })
    }
}
impl<M: Searchable> Engine<M> for CollectionEngine {
    fn update<'a>(&'a self, _: &'a [M]) -> ScoutFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
    fn delete<'a>(&'a self, _: &'a [M]) -> ScoutFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
    fn search<'a>(&'a self, builder: &'a Builder<M>) -> ScoutFuture<'a, SearchResults> {
        Box::pin(async move {
            let mut models = self.searchModels(builder).await?;
            if let Some(limit) = builder.limit {
                models.truncate(limit);
            }
            Self::results(&models, models.len() as u64)
        })
    }
    fn paginate<'a>(
        &'a self,
        builder: &'a Builder<M>,
        perPage: u64,
        page: u64,
    ) -> ScoutFuture<'a, SearchResults> {
        Box::pin(async move {
            let models = self.searchModels(builder).await?;
            let offset = page.saturating_sub(1).saturating_mul(perPage);
            let offset = usize::try_from(offset)
                .unwrap_or(usize::MAX)
                .min(models.len());
            let end = offset
                .saturating_add(usize::try_from(perPage).unwrap_or(usize::MAX))
                .min(models.len());
            Self::results(&models[offset..end], models.len() as u64)
        })
    }
    fn flush<'a>(&'a self, _: &'a M) -> ScoutFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
    fn createIndex<'a>(&'a self, _: &'a str, _: &'a serde_json::Value) -> ScoutFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
    fn deleteIndex<'a>(&'a self, _: &'a str) -> ScoutFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
}

// CollectionEngine searches PHP json_encode output for non-scalar attributes.
fn phpJson(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Array(values) => format!(
            "[{}]",
            values.iter().map(phpJson).collect::<Vec<_>>().join(",")
        ),
        serde_json::Value::Object(values) => format!(
            "{{{}}}",
            values
                .iter()
                .map(|(key, value)| format!("{}:{}", phpString(key), phpJson(value)))
                .collect::<Vec<_>>()
                .join(",")
        ),
        serde_json::Value::String(s) => phpString(s),
        serde_json::Value::Number(n) if n.is_f64() => {
            openrust::support::php_numeric_json_string(&n.to_string(), false)
                .expect("finite JSON number")
        }
        value => value.to_string(),
    }
}
fn phpString(value: &str) -> String {
    use std::fmt::Write as _;
    let json = serde_json::to_string(value).expect("valid string");
    let mut output = String::new();
    for character in json.chars() {
        if character == '/' {
            output.push_str("\\/");
        } else if !character.is_ascii() {
            for unit in character.encode_utf16(&mut [0; 2]) {
                write!(&mut output, "\\u{unit:04x}").expect("write to String");
            }
        } else {
            output.push(character);
        }
    }
    output
}
