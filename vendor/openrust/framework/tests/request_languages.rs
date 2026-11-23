use openrust::{
    foundation::configuration::Routing,
    http::{Body, HttpError, HttpFuture, Request, Response, StatusCode},
    support::facades::{App, Route},
    testing::TestApp,
};
use serde_json::json;

fn request(headers: &[&str]) -> Request {
    let mut request = http::Request::builder().uri("/");
    for header in headers {
        request = request.header("accept-language", *header);
    }
    let (parts, body) = request.body(Body::empty()).unwrap().into_parts();
    Request::from_parts(parts, body)
}

#[test]
fn languages_preserve_priority_ties_and_all_header_lines() {
    let request = request(&["en-US;q=0.4, fr;q=0.8", "zh-Hant;q=0.8, de"]);
    assert_eq!(request.getLanguages(), ["de", "fr", "zh-Hant", "en-US"]);
    assert_eq!(
        request.getPreferredLanguage(None).unwrap().as_deref(),
        Some("de")
    );
}

#[test]
fn languages_use_existing_bcp47_canonicalization_without_chinese_policy() {
    let request = request(&["EN-us-u-nu-latn, zh-CN, zh-Hant-HK, iw-IL, es-419"]);
    assert_eq!(
        request.getLanguages(),
        ["en-US-u-nu-latn", "zh-CN", "zh-Hant-HK", "he-IL", "es-419"]
    );
}

#[test]
fn duplicate_languages_keep_highest_weight_and_first_equal_position() {
    let request = request(&["en;q=0.2, fr;q=0.7, EN;q=0.9, de;q=0.9, en;q=0.9"]);
    assert_eq!(request.getLanguages(), ["en", "de", "fr"]);
}

#[test]
fn malformed_or_excluded_ranges_do_not_override_valid_preferences() {
    for header in [
        "en;q=0",
        "en;q=NaN",
        "en;q=1.1",
        "en;q=-1",
        "en;q=0.1234",
        "en;q=1;q=0",
        "en;broken",
        "en;q=1;extra=x",
        "../en",
        "en_US",
        "123",
        "x-demo",
        "i-cherokee",
    ] {
        let input = format!("{header}, fr;q=0.5");
        assert_eq!(request(&[&input]).getLanguages(), ["fr"], "{header}");
    }
    assert_eq!(
        request(&["en;q=1.000, fr;q=0.999, de;q=0.001"]).getLanguages(),
        ["en", "fr", "de"]
    );
}

#[test]
fn absent_empty_and_invalid_text_headers_have_no_preference() {
    for headers in [vec![], vec![""], vec![" , ; "]] {
        let request = request(&headers);
        assert!(request.getLanguages().is_empty());
        assert!(request.getPreferredLanguage(None).unwrap().is_none());
        assert!(request.getPreferredLanguage(Some(&[])).unwrap().is_none());
        assert_eq!(
            request
                .getPreferredLanguage(Some(&["fr", "en"]))
                .unwrap()
                .as_deref(),
            Some("fr")
        );
    }
    let mut request = request(&[]);
    request.parts_mut().headers.append(
        "accept-language",
        http::HeaderValue::from_bytes(b"\xff").unwrap(),
    );
    request
        .parts_mut()
        .headers
        .append("accept-language", "fr".parse().unwrap());
    assert_eq!(request.getLanguages(), ["fr"]);
}

#[test]
fn upstream_valid_preference_and_dialect_cases_use_bcp47_results() {
    // 来自固定 HttpFoundation RequestTest 的有效标签用例，公开标识改为 BCP 47。
    for (header, locales, expected) in [
        ("es-pa, en-us;q=0.8, en;q=0.6", vec![], "es-PA"),
        ("zh, en-us;q=0.8, en;q=0.6", vec!["en", "en-US"], "en-US"),
        (
            "ja-JP,fr-CA;q=0.7,fr;q=0.5,en-US;q=0.3",
            vec!["en-US", "fr-FR"],
            "fr-FR",
        ),
        ("zh, en-us;q=0.8", vec!["fr", "en"], "en"),
        ("ja-JP,fr-CA;q=0.7", vec!["en-US", "fr-FR"], "fr-FR"),
        ("fr-fr,hi-Latn-IN;q=0.5", vec!["hi-IN", "en-US"], "hi-IN"),
        (
            "fr-fr,hi-IN;q=0.5",
            vec!["hi-Latn-IN", "en-US"],
            "hi-Latn-IN",
        ),
        (
            "zh-hans-tw, zh-hant-tw",
            vec!["zh-Hans", "zh-TW"],
            "zh-Hans",
        ),
        (
            "en-Latn-US-fonipa-u-nu-latn-x-private,fr;q=0.5",
            vec!["fr-FR", "en-US"],
            "en-US",
        ),
    ] {
        assert_eq!(
            request(&[header])
                .getPreferredLanguage(Some(&locales))
                .unwrap()
                .as_deref(),
            Some(expected),
            "{header}"
        );
    }
}

#[test]
fn preferred_language_uses_ordered_fallback_and_wildcard() {
    for header in ["de", "*", "fr;q=0, *;q=1"] {
        assert_eq!(
            request(&[header])
                .getPreferredLanguage(Some(&["fr", "en"]))
                .unwrap()
                .as_deref(),
            Some("fr")
        );
    }
    assert_eq!(
        request(&["en;q=0.9, *;q=0.5"])
            .getPreferredLanguage(Some(&["fr", "en"]))
            .unwrap()
            .as_deref(),
        Some("en")
    );
    assert_eq!(
        request(&["*"])
            .getPreferredLanguage(None)
            .unwrap()
            .as_deref(),
        Some("*")
    );
}

#[test]
fn invalid_candidates_fail_even_if_an_earlier_candidate_matches() {
    for candidate in ["en_US", "", "../fr", "*"] {
        assert!(
            request(&["en"])
                .getPreferredLanguage(Some(&["en", candidate]))
                .is_err(),
            "{candidate}"
        );
    }
}

#[test]
fn mutating_request_parts_invalidates_the_per_request_language_cache() {
    let mut current = request(&["en"]);
    assert_eq!(current.getLanguages(), ["en"]);
    current
        .parts_mut()
        .headers
        .insert("accept-language", "zh-Hant".parse().unwrap());
    assert_eq!(current.getLanguages(), ["zh-Hant"]);
    current.parts_mut().headers.remove("accept-language");
    assert!(current.getLanguages().is_empty());
    assert_eq!(request(&["fr"]).getLanguages(), ["fr"]);
}

#[test]
fn generic_matching_does_not_guess_chinese_script_from_region() {
    assert_eq!(
        request(&["zh-TW"])
            .getPreferredLanguage(Some(&["zh-Hans", "zh-Hant"]))
            .unwrap()
            .as_deref(),
        Some("zh-Hans")
    );
    assert_eq!(
        request(&["zh-Hant-TW"])
            .getPreferredLanguage(Some(&["zh-Hans", "zh-Hant"]))
            .unwrap()
            .as_deref(),
        Some("zh-Hant")
    );
    assert_eq!(request(&["zh-1901"]).getLanguages(), ["zh-1901"]);
}

fn inspect(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let preferred = request.getPreferredLanguage(Some(&["en", "fr-FR"]))?;
        Response::json(
            StatusCode::OK,
            json!({
                "languages":request.getLanguages(),
                "preferred":preferred,
                "application_locale":App::current_locale().unwrap(),
            }),
        )
    })
}

#[tokio::test]
async fn actual_kernel_consumers_negotiate_without_changing_application_locale() {
    let mut app = TestApp::create(|builder| {
        builder
            .with_config(|_, _| {
                Ok([("app".into(), json!({"locale":"en","fallback_locale":"en"}))].into())
            })
            .with_routing(Routing::default().web(|| Route::get("/languages", inspect)))
    })
    .unwrap();
    let response = app
        .call(
            http::Request::builder()
                .uri("http://example.test/languages")
                .header("accept-language", "fr-CA, en;q=0.5")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    response
        .assertOk()
        .assertJsonPath("languages", &json!(["fr-CA", "en"]))
        .assertJsonPath("preferred", &json!("fr-FR"))
        .assertJsonPath("application_locale", &json!("en"));
    app.get("http://example.test/languages")
        .await
        .unwrap()
        .assertOk()
        .assertJsonPath("languages", &json!([]))
        .assertJsonPath("preferred", &json!("en"));
}
