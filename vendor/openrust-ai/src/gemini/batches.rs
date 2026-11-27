use openrust::http::{HttpFuture, Method, client::Response};
use serde_json::{Value, json};
use url::Url;

use crate::{
    AiError, AiResult, FileInfo, FileProvider, FileUpload, GeminiBatchGateway, GeminiBatchRequest,
    files::{GeminiFileProvider, validate_gemini_id},
    gemini_batches::{batch_name, valid_cursor},
    text::transport_error,
};

use super::{GeminiTextProvider, validated_model_id};

impl GeminiBatchGateway for GeminiTextProvider {
    fn batch_scope(&self) -> AiResult<String> {
        self.0.request_namespace("gemini-batches")
    }

    fn upload_batch_input(&self, file: FileUpload) -> HttpFuture<'_, AiResult<FileInfo>> {
        Box::pin(async move { GeminiFileProvider(self.0.clone()).put_file(file).await })
    }

    fn send_batch<'a>(
        &'a self,
        operation: GeminiBatchRequest<'a>,
    ) -> HttpFuture<'a, AiResult<Response>> {
        Box::pin(async move {
            let request = self.0.request(None)?;
            let response = match operation {
                GeminiBatchRequest::Generate { model, batch } => {
                    let path = create_path(model, batch, "batchGenerateContent")?;
                    request.post(&path, &json!({"batch":batch})).await
                }
                GeminiBatchRequest::Embed { model, batch } => {
                    let path = create_path(model, batch, "asyncBatchEmbedContent")?;
                    request.post(&path, &json!({"batch":batch})).await
                }
                GeminiBatchRequest::Get { name } => request.get(batch_name(name)?).await,
                GeminiBatchRequest::List { limit, cursor } => {
                    let mut query = Vec::new();
                    if let Some(limit) = limit {
                        if limit == 0 {
                            return Err(AiError::InvalidPrompt);
                        }
                        query.push(("pageSize".into(), limit.to_string()));
                    }
                    if let Some(cursor) = cursor {
                        if !valid_cursor(cursor) {
                            return Err(AiError::InvalidPrompt);
                        }
                        query.push(("pageToken".into(), cursor.into()));
                    }
                    request.with_query_parameters(query).get("batches").await
                }
                GeminiBatchRequest::Cancel { name } => {
                    request
                        .post(&format!("{}:cancel", batch_name(name)?), &json!({}))
                        .await
                }
                GeminiBatchRequest::Delete { name } => {
                    request.send(Method::DELETE, batch_name(name)?).await
                }
                GeminiBatchRequest::Download { file } => {
                    let url = download_url(self.0.base_url(), file)?;
                    request
                        .with_query_parameters([("alt".into(), "media".into())])
                        .stream()
                        .get(url.as_str())
                        .await
                }
            };
            response.map_err(transport_error)
        })
    }
}

fn create_path(model: &str, batch: &Value, action: &str) -> AiResult<String> {
    if !batch.is_object() {
        return Err(AiError::InvalidPrompt);
    }
    let model = validated_model_id(model)?;
    if let Some(body_model) = batch.get("model")
        && body_model
            .as_str()
            .and_then(|model| validated_model_id(model).ok())
            != Some(model)
    {
        return Err(AiError::InvalidPrompt);
    }
    Ok(format!("models/{model}:{action}"))
}

fn download_url(base: &str, file: &str) -> AiResult<Url> {
    let id = file.strip_prefix("files/").ok_or(AiError::InvalidFile)?;
    validate_gemini_id(id)?;
    let mut url =
        Url::parse(base).map_err(|_| AiError::InvalidConfiguration("providers.<name>.url"))?;
    let (prefix, version) = url
        .path()
        .trim_end_matches('/')
        .rsplit_once('/')
        .filter(|(_, version)| matches!(*version, "v1" | "v1beta"))
        .ok_or(AiError::InvalidConfiguration("providers.<name>.url"))?;
    let path = format!("{prefix}/download/{version}/{file}:download");
    url.set_path(&path);
    Ok(url)
}
