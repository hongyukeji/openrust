use openrust::{routing::RoutingError, support::facades::Route};

use crate::app::http::controllers::api_controller::ApiController;

pub(crate) fn register() -> Result<(), RoutingError> {
    Route::get("/", ApiController::index)
}
