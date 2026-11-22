use std::{collections::BTreeMap, sync::RwLock};

use serde_json::{Value, json};

use crate::{
    container::ServiceKey,
    support::facades::{Facade, Lang},
    view::{View, ViewData, ViewResult},
};

pub(crate) const VIEWS: &[(&str, &str)] = &[
    (
        "tailwind",
        include_str!("resources/views/tailwind.blade.html"),
    ),
    (
        "simple-tailwind",
        include_str!("resources/views/simple-tailwind.blade.html"),
    ),
    (
        "bootstrap-3",
        include_str!("resources/views/bootstrap-3.blade.html"),
    ),
    (
        "bootstrap-4",
        include_str!("resources/views/bootstrap-4.blade.html"),
    ),
    (
        "bootstrap-5",
        include_str!("resources/views/bootstrap-5.blade.html"),
    ),
    (
        "simple-bootstrap-3",
        include_str!("resources/views/simple-bootstrap-3.blade.html"),
    ),
    (
        "simple-bootstrap-4",
        include_str!("resources/views/simple-bootstrap-4.blade.html"),
    ),
    (
        "simple-bootstrap-5",
        include_str!("resources/views/simple-bootstrap-5.blade.html"),
    ),
    (
        "semantic-ui",
        include_str!("resources/views/semantic-ui.blade.html"),
    ),
];

pub(super) const DEFAULT_VIEW: &str = "pagination::tailwind";
pub(super) const DEFAULT_SIMPLE_VIEW: &str = "pagination::simple-tailwind";

pub(super) struct DefaultViews(RwLock<(String, String)>);

impl Default for DefaultViews {
    fn default() -> Self {
        Self(RwLock::new((
            DEFAULT_VIEW.to_owned(),
            DEFAULT_SIMPLE_VIEW.to_owned(),
        )))
    }
}

impl DefaultViews {
    pub(super) fn get(simple: bool) -> ViewResult<String> {
        let settings = ViewDefaults::getFacadeRoot()?;
        let views = settings.0.read().expect("pagination view defaults lock");
        Ok(if simple {
            views.1.clone()
        } else {
            views.0.clone()
        })
    }

    pub(super) fn configure(view: Option<String>, simple: Option<String>) -> ViewResult<()> {
        let settings = ViewDefaults::getFacadeRoot()?;
        let mut views = settings.0.write().expect("pagination view defaults lock");
        if let Some(view) = view {
            views.0 = view;
        }
        if let Some(simple) = simple {
            views.1 = simple;
        }
        Ok(())
    }
}

struct ViewDefaults;

impl Facade for ViewDefaults {
    type Service = DefaultViews;

    #[allow(non_snake_case, reason = "Matches the Facade contract")]
    fn getFacadeAccessor() -> ServiceKey {
        ServiceKey::typed::<DefaultViews>()
    }
}

pub(super) fn navigation_label(previous: bool) -> String {
    let (key, default) = if previous {
        ("pagination.previous", "&laquo; Previous")
    } else {
        ("pagination.next", "Next &raquo;")
    };
    // Missing language lines resolve to their key, including before lang:publish.
    label(key, default)
}

fn label(key: &str, default: &str) -> String {
    Lang::get(key, &BTreeMap::new(), None)
        .ok()
        .filter(|value| value != key)
        .unwrap_or_else(|| default.to_owned())
}

pub(super) fn paginator_data(
    has_pages: bool,
    previous: Option<String>,
    next: Option<String>,
) -> Value {
    json!({
        "has_pages": has_pages,
        "prev_page_url": previous,
        "next_page_url": next,
        "previous": navigation_label(true).replace("&laquo;", "«").replace("&raquo;", "»"),
        "next": navigation_label(false).replace("&laquo;", "«").replace("&raquo;", "»"),
        "navigation": label("Pagination Navigation", "Pagination Navigation"),
        "showing": label("Showing", "Showing"),
        "to": label("to", "to"),
        "of": label("of", "of"),
        "results": label("results", "results"),
    })
}

pub(super) fn make(
    name: &str,
    mut data: ViewData,
    paginator: Value,
    elements: Option<Value>,
) -> ViewResult<View> {
    data.insert("paginator".into(), paginator.into());
    if let Some(elements) = elements {
        data.insert("elements".into(), elements.into());
    }
    super::Paginator::viewFactory()?.make(name.to_owned(), data)
}
