use serde_json::{Map, Value};

use crate::{
    contracts::session::Session as _,
    routing::{UrlError, UrlResult},
    support::{Arr, MessageBag, data_forget},
};

use super::{HttpError, Request, Response};

/// An HTTP redirect with fluent session flashing.
pub struct RedirectResponse(Response);

#[allow(non_snake_case, reason = "Matches Laravel's RedirectResponse")]
impl RedirectResponse {
    pub(crate) fn new(response: Response) -> Self {
        Self(response)
    }

    /// Flashes a value to the next request's session.
    pub fn with(self, request: &mut Request, key: &str, value: Value) -> UrlResult<Self> {
        request
            .session_mut()
            .and_then(|session| session.flash(key, value))
            .map_err(UrlError::Session)?;
        Ok(self)
    }

    /// Flashes supplied input, or reads the current request's input when omitted.
    pub async fn withInput(
        self,
        request: &mut Request,
        input: Option<Value>,
    ) -> Result<Self, HttpError> {
        let input = match input {
            Some(input) => input,
            None => request_input(request).await?,
        };
        Ok(self.flash_input(request, input)?)
    }

    /// Flashes only the selected input paths.
    pub async fn onlyInput(self, request: &mut Request, keys: &[&str]) -> Result<Self, HttpError> {
        let input = request_input(request).await?;
        let mut selected = Value::Object(Map::new());
        for key in keys {
            if let Some(value) = Arr::get(&input, key) {
                Arr::set(&mut selected, key, value.clone())
                    .map_err(|_| HttpError::InvalidRequest)?;
            }
        }
        Ok(self.flash_input(request, selected)?)
    }

    /// Flashes input after removing the specified paths.
    pub async fn exceptInput(
        self,
        request: &mut Request,
        keys: &[&str],
    ) -> Result<Self, HttpError> {
        let mut input = request_input(request).await?;
        for key in keys {
            if let Some(fields) = input.as_object_mut() {
                fields.remove(*key);
            }
            data_forget(&mut input, key).map_err(|_| HttpError::InvalidRequest)?;
        }
        Ok(self.flash_input(request, input)?)
    }

    /// Flashes an error bag without replacing other named bags.
    pub fn withErrors(
        self,
        request: &mut Request,
        errors: &MessageBag,
        bag: &str,
    ) -> UrlResult<Self> {
        request
            .session_mut()
            .and_then(|session| session.flashErrors(bag, errors))
            .map_err(UrlError::Session)?;
        Ok(self)
    }

    fn flash_input(self, request: &mut Request, input: Value) -> UrlResult<Self> {
        request
            .session_mut()
            .and_then(|session| session.flashInput(input))
            .map_err(UrlError::Session)?;
        Ok(self)
    }
}

impl From<RedirectResponse> for Response {
    fn from(redirect: RedirectResponse) -> Self {
        redirect.0
    }
}

async fn request_input(request: &mut Request) -> Result<Value, HttpError> {
    Ok(request.input(None).await?.unwrap_or_default())
}
