use serde_json::Value;

use crate::{
    http::{HttpError, Request, RequestValue},
    support::Arr,
};

use super::{InputSource, as_fields, empty, input_is_empty, lookup};

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
impl Request {
    /// Tests whether all keys exist, including explicit nulls and uploaded files.
    pub async fn has(&mut self, keys: &[&str]) -> Result<bool, HttpError> {
        let data = self.all().await?;
        Ok(keys.iter().all(|key| data.get(key).is_some()))
    }

    pub async fn exists(&mut self, keys: &[&str]) -> Result<bool, HttpError> {
        self.has(keys).await
    }

    pub async fn hasAny(&mut self, keys: &[&str]) -> Result<bool, HttpError> {
        let data = self.all().await?;
        Ok(keys.iter().any(|key| data.get(key).is_some()))
    }

    pub async fn missing(&mut self, keys: &[&str]) -> Result<bool, HttpError> {
        Ok(!self.has(keys).await?)
    }

    /// Tests whether every input value is present and nonblank.
    pub async fn filled(&mut self, keys: &[&str]) -> Result<bool, HttpError> {
        let data = self.input(None).await?.unwrap_or_else(empty);
        Ok(keys.iter().all(|key| !blank(&data, key)))
    }

    /// Tests whether every input value is missing, null or a blank string.
    pub async fn isNotFilled(&mut self, keys: &[&str]) -> Result<bool, HttpError> {
        let data = self.input(None).await?.unwrap_or_else(empty);
        Ok(keys.iter().all(|key| blank(&data, key)))
    }

    pub async fn anyFilled(&mut self, keys: &[&str]) -> Result<bool, HttpError> {
        let data = self.input(None).await?.unwrap_or_else(empty);
        Ok(keys.iter().any(|key| !blank(&data, key)))
    }

    /// Calls the callback for a present key with its combined input or file value.
    pub async fn whenHas<T>(
        &mut self,
        key: &str,
        callback: impl AsyncFnOnce(&mut Self, Option<RequestValue>) -> T,
    ) -> Result<Option<T>, HttpError> {
        let data = self.all().await?;
        if data.get(key).is_none() {
            return Ok(None);
        }
        let value = data.data(key);
        Ok(Some(callback(self, value).await))
    }

    pub async fn whenFilled<T>(
        &mut self,
        key: &str,
        callback: impl AsyncFnOnce(&mut Self, Option<RequestValue>) -> T,
    ) -> Result<Option<T>, HttpError> {
        let value = self.input(Some(key)).await?;
        if value.as_ref().is_none_or(input_is_empty) {
            return Ok(None);
        }
        let value = self.all().await?.data(key);
        Ok(Some(callback(self, value).await))
    }

    pub async fn whenMissing<T>(
        &mut self,
        key: &str,
        callback: impl AsyncFnOnce(&mut Self) -> T,
    ) -> Result<Option<T>, HttpError> {
        if self.missing(&[key]).await? {
            return Ok(Some(callback(self).await));
        }
        Ok(None)
    }

    /// Sets each supplied dot path in the active input source.
    pub async fn merge(&mut self, input: Value) -> Result<&mut Self, HttpError> {
        self.merge_input(input, false).await
    }

    /// Merges paths that are absent from the original input and upload snapshot.
    pub async fn mergeIfMissing(&mut self, input: Value) -> Result<&mut Self, HttpError> {
        self.merge_input(input, true).await
    }

    /// Replaces the active input source, preserving the original body and uploads.
    pub async fn replace(&mut self, input: Value) -> Result<&mut Self, HttpError> {
        if !Arr::accessible(&input) {
            return Err(HttpError::InvalidRequest);
        }
        let source = self.input_source()?;
        self.source_values(source).await?;
        let candidate = Arr::with_paths(input, []).map_err(|_| HttpError::InvalidRequest)?;
        self.install_input(source, candidate);
        Ok(self)
    }

    async fn merge_input(
        &mut self,
        input: Value,
        missing_only: bool,
    ) -> Result<&mut Self, HttpError> {
        if !Arr::accessible(&input) {
            return Err(HttpError::InvalidRequest);
        }
        let original = if missing_only {
            Some(self.all().await?)
        } else {
            None
        };
        let source = self.input_source()?;
        let fields = self.source_values(source).await?.clone();
        let changes = as_fields(input)
            .into_iter()
            .filter(|(key, _)| original.as_ref().is_none_or(|data| data.get(key).is_none()));
        let candidate = Arr::with_paths(fields, changes).map_err(|_| HttpError::InvalidRequest)?;
        self.install_input(source, candidate);
        Ok(self)
    }

    fn install_input(&mut self, source: InputSource, input: Value) {
        match source {
            InputSource::Json => self.json_input = Some(Ok(input)),
            InputSource::Query => self.query_input = Some(input),
            InputSource::Form => self.form_input = Some(input),
        }
    }
}

fn blank(data: &Value, key: &str) -> bool {
    lookup(data, &key.split('.').collect::<Vec<_>>())
        .as_ref()
        .is_none_or(input_is_empty)
}
