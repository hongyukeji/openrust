use http::HeaderMap;
use openrust::support::Str;

pub(crate) struct ClientInvocation {
    pub(crate) id: String,
    pub(crate) model_turn: i64,
}

pub(crate) struct CallIdentity<'a> {
    pub(crate) request_id: &'a str,
    pub(crate) invocation: Option<&'a ClientInvocation>,
}

impl ClientInvocation {
    pub(crate) fn from_headers(headers: &HeaderMap) -> Option<Self> {
        let invocation = single_header(headers, openrust_ai::INVOCATION_HEADER)?;
        let turn = single_header(headers, openrust_ai::MODEL_TURN_HEADER)?;
        let model_turn: i64 = turn.parse().ok()?;
        if !Str::is_ulid(invocation) || model_turn < 0 || model_turn.to_string() != turn {
            return None;
        }
        Some(Self {
            id: invocation.to_ascii_uppercase(),
            model_turn,
        })
    }
}

fn single_header<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    let mut values = headers.get_all(name).iter();
    let value = values.next()?.to_str().ok()?;
    values.next().is_none().then_some(value)
}
