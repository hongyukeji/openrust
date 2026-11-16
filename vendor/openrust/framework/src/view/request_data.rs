use std::{collections::BTreeMap, sync::Arc};

use crate::{contracts::session::Session as _, http::Request, log::context::Context};

use super::{ViewData, ViewValue};

#[derive(Default)]
pub(super) struct ViewRequestData {
    pub csrf_token: Option<Arc<str>>,
    session: BTreeMap<String, ViewValue>,
    context: Option<ViewData>,
}

impl ViewRequestData {
    pub fn capture(request: &Request) -> Self {
        let session = request.session().ok();
        Self {
            csrf_token: session.and_then(|session| session.token()).map(Arc::from),
            session: session.map_or_else(BTreeMap::new, |session| session.all().clone()),
            context: Context::all().ok(),
        }
    }

    pub fn session(&self, name: &str) -> Option<ViewValue> {
        crate::session::get_attribute(&self.session, name)
            .filter(|value| !value.is_null())
            .cloned()
    }

    pub fn context(&self, name: &str) -> Option<ViewValue> {
        match &self.context {
            Some(context) => context.get(name).cloned(),
            None => Context::get(name).ok().flatten(),
        }
    }
}
