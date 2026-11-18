use serde_json::Value;

use crate::http::{HttpError, Request};

#[allow(non_snake_case, reason = "Matches Laravel's Request flash data API")]
impl Request {
    /// Retrieves old input from the current session.
    pub fn old(&self, key: Option<&str>) -> Option<&Value> {
        self.session.as_ref()?.getOldInput(key)
    }

    /// Flashes the current input for the next request.
    pub async fn flash(&mut self) -> Result<(), HttpError> {
        self.session()?;
        let input = self.input(None).await?.unwrap_or_else(empty);
        Ok(self.session_mut()?.flashInput(input)?)
    }

    /// Flashes only the selected input paths.
    pub async fn flashOnly(&mut self, keys: &[&str]) -> Result<(), HttpError> {
        self.session()?;
        let input = self.only_input(keys).await?;
        Ok(self.session_mut()?.flashInput(input)?)
    }

    /// Flashes the current input except for the supplied paths.
    pub async fn flashExcept(&mut self, keys: &[&str]) -> Result<(), HttpError> {
        self.session()?;
        let input = self.except_input(keys).await?;
        Ok(self.session_mut()?.flashInput(input)?)
    }

    /// Replaces the old input with an empty flash value.
    pub fn flush(&mut self) -> Result<(), HttpError> {
        Ok(self.session_mut()?.flashInput(empty())?)
    }
}

fn empty() -> Value {
    Value::Array(Vec::new())
}
