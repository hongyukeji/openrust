use openrust::{routing::RoutingError, support::facades::Route};

use crate::app::http::controllers::{
    home_controller::HomeController, locale_controller::LocaleController,
};

pub(crate) fn register() -> Result<(), RoutingError> {
    Route::get_with("/", HomeController::index, |route| {
        route.name("home");
        Ok(())
    })?;
    Route::post("/locale", LocaleController::store)?;
    Route::static_disk("web")?;
    Route::static_disk("public")?;
    Ok(())
}
