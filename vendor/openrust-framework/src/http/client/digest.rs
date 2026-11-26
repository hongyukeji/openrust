use digest_auth::{AlgorithmType, AuthContext, WwwAuthenticateHeader};
use http::{HeaderMap, HeaderValue, header};
use reqwest::Request;

pub(super) struct DigestCredentials {
    pub username: String,
    pub password: String,
}

impl DigestCredentials {
    pub(super) fn authenticate(
        &self,
        mut request: Request,
        headers: &HeaderMap,
        stale_only: bool,
    ) -> Option<Request> {
        let mut challenge = headers
            .get_all(header::WWW_AUTHENTICATE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .flat_map(challenges)
            .filter_map(|challenge| {
                let (scheme, params) = challenge.trim_start().split_once(char::is_whitespace)?;
                if !scheme.eq_ignore_ascii_case("digest") {
                    return None;
                }
                digest_auth::parse(params).ok()
            })
            .filter(|challenge| !stale_only || challenge.stale)
            .max_by_key(strength)?;

        let uri = &request.url()[url::Position::BeforePath..url::Position::AfterQuery];
        let body = request
            .body()
            .and_then(reqwest::Body::as_bytes)
            .unwrap_or_default();
        let context = AuthContext::new_with_method(
            self.username.as_str(),
            self.password.as_str(),
            uri,
            Some(body),
            request.method().as_str().into(),
        );
        let response = challenge.respond(&context).ok()?;
        let mut header = HeaderValue::from_str(&response.to_string()).ok()?;
        header.set_sensitive(true);
        request.headers_mut().insert(header::AUTHORIZATION, header);
        Some(request)
    }
}

fn strength(challenge: &WwwAuthenticateHeader) -> u8 {
    match challenge.algorithm.algo {
        AlgorithmType::MD5 => 0,
        AlgorithmType::SHA2_256 => 1,
        AlgorithmType::SHA2_512_256 => 2,
    }
}

/// Split challenges at an unquoted comma followed by a new authentication scheme.
fn challenges(value: &str) -> impl Iterator<Item = &str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut quoted = false;
    let mut escaped = false;
    for (index, byte) in value.bytes().enumerate() {
        match (byte, quoted, escaped) {
            (_, true, true) => escaped = false,
            (b'\\', true, false) => escaped = true,
            (b'"', _, false) => quoted = !quoted,
            (b',', false, false) => {
                let next = value[index + 1..].trim_start();
                let token_end = next.find(|c: char| c.is_ascii_whitespace() || c == '=');
                if let Some(end) = token_end
                    && end > 0
                    && !next[end..].trim_start().starts_with('=')
                {
                    parts.push(&value[start..index]);
                    start = index + 1;
                }
            }
            _ => {}
        }
    }
    parts.push(&value[start..]);
    parts.into_iter()
}
