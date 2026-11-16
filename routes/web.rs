use openrust::{routing::RoutingError, support::facades::Route};

use crate::app::http::controllers::welcome_controller::WelcomeController;

pub(crate) fn register() -> Result<(), RoutingError> {
    Route::get("/", WelcomeController::index)?;
    Route::staticPublic()?;
    Route::staticDisk("public")?;
    Ok(())
}
