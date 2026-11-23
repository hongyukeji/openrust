use serde_json::Value;

use crate::http::{HttpError, Request};

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
impl Request {
    /// Retrieves old input from the current session.
    pub fn old(&self, key: Option<&str>) -> Option<&Value> {
        self.session.as_ref()?.get_old_input(key)
    }

    /// Flashes the current input for the next request.
    pub async fn flash(&mut self) -> Result<(), HttpError> {
        self.session()?;
        let input = self.input(None).await?.unwrap_or_else(empty);
        Ok(self.session_mut()?.flash_input(input)?)
    }

    /// Flashes only the selected input paths.
    pub async fn flash_only(&mut self, keys: &[&str]) -> Result<(), HttpError> {
        self.session()?;
        let input = self.only_input(keys).await?;
        Ok(self.session_mut()?.flash_input(input)?)
    }

    /// Flashes the current input except for the supplied paths.
    pub async fn flash_except(&mut self, keys: &[&str]) -> Result<(), HttpError> {
        self.session()?;
        let input = self.except_input(keys).await?;
        Ok(self.session_mut()?.flash_input(input)?)
    }

    /// Replaces the old input with an empty flash value.
    pub fn flush(&mut self) -> Result<(), HttpError> {
        Ok(self.session_mut()?.flash_input(empty())?)
    }
}

fn empty() -> Value {
    Value::Array(Vec::new())
}
