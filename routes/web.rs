use openrust::{routing::RoutingError, support::facades::Route, view::view};

pub(crate) fn register() -> Result<(), RoutingError> {
    Route::get("/", || view("welcome"))?;
    Route::staticPublic()?;
    Route::staticDisk("public")?;
    Ok(())
}
