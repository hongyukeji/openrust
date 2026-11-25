use openrust::{routing::RoutingError, support::facades::Route};

use crate::app::http::controllers::{
    index_controller::IndexController, welcome_controller::WelcomeController,
};

pub(crate) fn register() -> Result<(), RoutingError> {
    Route::get("/", IndexController::index)?;
    Route::post("/locale", IndexController::store_locale)?;
    Route::get("/welcome", WelcomeController::index)?;
    Route::static_disk("web")?;
    Route::static_disk("public")?;
    Ok(())
}
