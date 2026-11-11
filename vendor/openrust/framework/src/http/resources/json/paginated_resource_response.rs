use serde::{Serializer as _, ser::SerializeMap as _};
use serde_json::{Map, Value};

use crate::http::HttpError;

use super::{PaginationInformation, PaginationLinks, ResourceItems, ResourceResult};

/// Extracts resource navigation and metadata from the original paginator.
pub struct PaginatedResourceResponse;

impl PaginatedResourceResponse {
    pub fn information<T>(
        items: &ResourceItems<T>,
    ) -> ResourceResult<Option<PaginationInformation>> {
        let mut fields = serde_json::value::Serializer
            .serialize_map(None)
            .map_err(internal)?;
        match items {
            ResourceItems::Collection(_) => return Ok(None),
            ResourceItems::Paginator(page) => page.serialize_metadata(&mut fields),
            ResourceItems::LengthAwarePaginator(page) => page.serialize_metadata(&mut fields),
            ResourceItems::CursorPaginator(page) => page.serialize_metadata(&mut fields),
        }
        .map_err(internal)?;
        let Value::Object(mut meta) = fields.end().map_err(internal)? else {
            unreachable!("serialize_map constructs an object");
        };
        let links = PaginationLinks {
            first: take_link(&mut meta, "first_page_url"),
            last: take_link(&mut meta, "last_page_url"),
            prev: take_link(&mut meta, "prev_page_url"),
            next: take_link(&mut meta, "next_page_url"),
        };
        Ok(Some(PaginationInformation { links, meta }))
    }

    pub(super) fn fields(info: PaginationInformation) -> ResourceResult<Map<String, Value>> {
        Ok(Map::from_iter([
            (
                "links".into(),
                serde_json::to_value(info.links).map_err(internal)?,
            ),
            ("meta".into(), Value::Object(info.meta)),
        ]))
    }
}

fn take_link(meta: &mut Map<String, Value>, name: &str) -> Option<String> {
    match meta.shift_remove(name) {
        Some(Value::String(value)) => Some(value),
        _ => None,
    }
}

fn internal(source: serde_json::Error) -> HttpError {
    HttpError::Internal {
        source: Box::new(source),
    }
}
