use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::{
    support::facades::{Lang, View as ViewFacade},
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
];

pub(super) const DEFAULT_VIEW: &str = "pagination::tailwind";
pub(super) const DEFAULT_SIMPLE_VIEW: &str = "pagination::simple-tailwind";

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
    data.insert("paginator".into(), paginator);
    if let Some(elements) = elements {
        data.insert("elements".into(), elements);
    }
    ViewFacade::make(name.to_owned(), data)
}
