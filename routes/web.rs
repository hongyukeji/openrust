use openrust::{routing::RoutingError, support::facades::Route};

use crate::app::http::controllers::{
    index_controller::IndexController, locale_controller::LocaleController,
};

pub(crate) fn register() -> Result<(), RoutingError> {
    Route::get("/", IndexController::index)?;
    Route::post("/locale", LocaleController::store)?;
    Route::static_disk("web")?;
    Route::static_disk("public")?;
    Ok(())
}
