use openrust::cookie::CookieValuePrefix;

#[test]
fn laravel_cookie_prefix_binds_name_and_accepts_previous_keys() {
    let current = [b'b'; 32];
    let previous = [b'a'; 32];
    let prefix = CookieValuePrefix::create("session", &previous);
    assert_eq!(&prefix, b"e94b27c918c924d66c45bd6347120e703562b3ba|");

    let body = b"\0hello\xff";
    let mut value = prefix.to_vec();
    value.extend_from_slice(body);
    let keys = [current.as_slice(), previous.as_slice()];

    assert_eq!(CookieValuePrefix::remove(&value), body);
    assert_eq!(
        CookieValuePrefix::validate("session", &value, keys),
        Some(body.as_slice())
    );
    assert_eq!(CookieValuePrefix::validate("other", &value, keys), None);
    assert_eq!(
        CookieValuePrefix::validate("session", &value, [current.as_slice()]),
        None
    );
    assert_eq!(
        CookieValuePrefix::validate("session", &value[..40], keys),
        None
    );
    value[40] = b':';
    assert_eq!(CookieValuePrefix::validate("session", &value, keys), None);
}
