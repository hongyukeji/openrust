use crate::resolver::internal;
use openrust::{
    contracts::session::Session as _,
    http::{ApiResult, HttpError, HttpFuture, Request, Response, StatusCode},
    support::{facades::Config, now, php_integer_less_than_value},
};
use serde_json::json;
pub struct ConfirmedPasswordStatusController;
impl ConfirmedPasswordStatusController {
    pub fn show<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            let now = now(None)
                .map_err(internal)?
                .as_date_time()
                .timestamp()
                .as_nanosecond()
                .div_euclid(1_000_000_000) as i64;
            let last = request
                .session()
                .map_err(internal)?
                .get("auth.password_confirmed_at")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            let elapsed = now.checked_sub(last).ok_or(HttpError::InvalidRequest)?;
            let default = Config::get("auth.password_timeout")
                .map_err(internal)?
                .unwrap_or(json!(900));
            let seconds = request.input(Some("seconds")).await?.unwrap_or(default);
            let confirmed = php_integer_less_than_value(elapsed, &seconds);
            let mut response = Response::json(
                StatusCode::OK,
                ApiResult::success(json!({"confirmed":confirmed})),
            )?;
            if confirmed && elapsed != 0 {
                response = response.header(
                    "x-retry-after".parse().map_err(internal)?,
                    elapsed.to_string().parse().map_err(internal)?,
                );
            }
            Ok(response)
        })
    }
}
