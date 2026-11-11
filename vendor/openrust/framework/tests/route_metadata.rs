use openrust::{
    routing::{RouteGroup, Router},
    view::view,
};

#[test]
fn route_metadata_reflects_nested_group_names_and_replaced_routes() {
    let mut router = Router::new();
    router.get("/outside", || view("welcome")).unwrap();
    router
        .group(RouteGroup::new().prefix("admin").name("admin."), |router| {
            router.group(
                RouteGroup::new()
                    .prefix("users")
                    .name("users.")
                    .middleware_name("auth"),
                |router| {
                    router.get("/{id?}", || view("welcome"))?.name("show");
                    router.get("/{id?}", || view("welcome"))?.name("current");
                    Ok(())
                },
            )
        })
        .unwrap();
    let routes = router.getRoutes();
    assert_eq!(routes.len(), 2);
    assert_eq!(routes[0].name, None);
    assert_eq!(routes[1].uri, "admin/users/{id?}");
    assert_eq!(routes[1].method, "GET|HEAD");
    assert_eq!(routes[1].name.as_deref(), Some("admin.users.current"));
    assert_eq!(routes[1].middleware, ["auth"]);
}
