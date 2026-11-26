use axum::{extract::State, handler::Handler as _};
use openrust::{
    contracts::{
        http::Kernel as _,
        validation::{Factory as _, ParameterizedRule, Validator as _},
    },
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    routing::Router,
    validation::{
        Data, Factory, Rule, RuleBuilder, RuleSet,
        rules::{ArrayRule, Date, Distinct, IpAddress, Numeric, StringRule, Timezone, Url, Uuid},
    },
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, net::SocketAddr, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    time::timeout,
};
fn specification(name: &str) -> Rule {
    match name {
        "string_csv" => StringRule::new().starts_with(["a,b"]).into(),
        "array_csv" => ArrayRule::new(["a,b"]).into(),
        "string_size" => StringRule::new()
            .min(2)
            .max(5)
            .between(2, 5)
            .exactly(3)
            .into(),
        "string_alpha" => Rule::string().alpha(false),
        "string_alpha_ascii" => StringRule::new().alpha(true).into(),
        "string_dash" => StringRule::new().alpha_dash(true).into(),
        "string_num" => StringRule::new().alpha_numeric(false).into(),
        "string_ascii" => StringRule::new().ascii().into(),
        "string_lower" => StringRule::new().lowercase().into(),
        "string_upper" => StringRule::new().uppercase().into(),
        "string_affix" => Rule::string()
            .starts_with(["A", "B"])
            .ends_with(["!"])
            .doesnt_start_with(["BAD"])
            .doesnt_end_with(["?"]),
        "array_size" => Rule::array().min(1).max(3).between(1, 3).exactly(2),
        "array_keys" => ArrayRule::new(["name", "email"]).min(1).into(),
        "numeric_strict" => Rule::numeric(true).into(),
        "numeric_size" => Numeric::new(false)
            .between(-2, 5)
            .min(-2)
            .max(5)
            .exactly(2)
            .into(),
        "numeric_decimal" => Numeric::default().decimal(2, Some(3)).into(),
        "numeric_integer" => Numeric::default().integer(true).into(),
        "numeric_digits" => Numeric::default()
            .digits(3)
            .digits_between(2, 4)
            .min_digits(3)
            .max_digits(3)
            .into(),
        "numeric_multiple" => Numeric::default().multiple_of(3).into(),
        "numeric_compare" => Numeric::default()
            .greater_than("lower")
            .greater_than_or_equal_to("low")
            .less_than("upper")
            .less_than_or_equal_to("high")
            .different("other")
            .same("same")
            .into(),
        "date_format" => Date::new().format("Y-m-d").into(),
        "date_window" => Rule::date().between("2024-01-01", "2025-01-01").into(),
        "date_inclusive" => Rule::date()
            .between_or_equal("2024-01-01", "2025-01-01")
            .into(),
        "date_equals" => Date::new().equals("2024-01-01").into(),
        "date_before_today" => Date::new().before_today().into(),
        "date_after_today" => Date::new().after_today().into(),
        "date_today_before" => Date::new().today_or_before().into(),
        "date_today_after" => Date::new().today_or_after().into(),
        "date_past" => Date::new().past().into(),
        "date_future" => Date::new().future().into(),
        "date_now_past" => Date::new().now_or_past().into(),
        "date_now_future" => Date::new().now_or_future().into(),
        "url" => Rule::url().protocols(["https"]).into(),
        "uuid4" => Rule::uuid().version(4).into(),
        "uuid_nil" => Rule::uuid().version("nil").into(),
        "uuid_max" => Rule::uuid().version("max").into(),
        "ip4" => Rule::ip().version(4).unwrap().into(),
        "ip6" => Rule::ip().version(6).unwrap().into(),
        "timezone" => Rule::timezone(["per_country", "US"]).into(),
        _ => panic!("未声明夹具{name}"),
    }
}
fn conditional_rule(kind: &str, condition: openrust::validation::RuleCondition) -> Rule {
    match kind {
        "required_if" => Rule::required_if(condition).into(),
        "required_unless" => Rule::required_unless(condition).into(),
        "exclude_if" => Rule::exclude_if(condition).into(),
        "exclude_unless" => Rule::exclude_unless(condition).into(),
        "prohibited_if" => Rule::prohibited_if(condition).into(),
        "prohibited_unless" => Rule::prohibited_unless(condition).into(),
        _ => panic!("未声明条件{kind}"),
    }
}
fn conditions(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let data: Value = serde_json::from_slice(&request.body_bytes().await?)?;
        let condition = data["flag"].as_bool().unwrap().into();
        let rule = conditional_rule(data["kind"].as_str().unwrap(), condition);
        Response::json(
            StatusCode::OK,
            request
                .validate(RuleSet::new().field("value", [rule])?)
                .await?,
        )
    })
}
fn check(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let data: Value = serde_json::from_slice(&request.body_bytes().await?)?;
        let rule = specification(data["name"].as_str().unwrap());
        let rules = RuleSet::new().field("value", [Rule::required(), rule])?;
        Response::json(StatusCode::OK, request.validate(rules).await?)
    })
}
fn distinct(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let rules =
            RuleSet::new().field("rows.*", [Rule::distinct().strict().ignore_case().into()])?;
        Response::json(StatusCode::OK, request.validate(rules).await?)
    })
}
async fn transport(
    State(kernel): State<Arc<Kernel>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    kernel.handle_transport(request).await
}
async fn send(address: SocketAddr, path: &str, body: &str, content: &str) -> (u16, Value) {
    timeout(Duration::from_secs(5),async{
 let mut stream=TcpStream::connect(address).await.unwrap();stream.write_all(format!("POST {path} HTTP/1.1\r\nHost: {address}\r\nAccept: application/json\r\nContent-Type: {content}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();let mut response=String::new();stream.read_to_string(&mut response).await.unwrap();let status=response.split_whitespace().nth(1).unwrap().parse().unwrap();let body=response.split_once("\r\n\r\n").unwrap().1;(status,serde_json::from_str(body).unwrap())
}).await.unwrap()
}
#[tokio::test]
async fn fluent_objects_and_chaining_match_fixed_rules_over_real_http() {
    let root = tempfile::tempdir().unwrap();
    let app = Application::configure(root.path().to_owned())
        .with_config(|_, _| {
            Ok(BTreeMap::from([(
                "app".into(),
                json!({"locale":"en","supported_locales":[],"timezone":"UTC"}),
            )]))
        })
        .with_routing(Routing::new(|_| {
            let mut r = Router::new();
            r.post("/check", check)?;
            r.post("/distinct", distinct)?;
            r.post("/conditions", conditions)?;
            Ok(r)
        }))
        .create()
        .unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            transport.with_state(Arc::new(kernel)).into_make_service(),
        )
        .await
        .unwrap()
    });
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("reference/validation-fluent.json")).unwrap();
    for case in cases {
        let body=json!({"name":case["name"],"value":case["value"],"lower":1,"low":3,"upper":5,"high":3,"other":4,"same":3}).to_string();
        let (status, data) = send(address, "/check", &body, "application/json").await;
        assert_eq!(
            status,
            if case["passes"] == true { 200 } else { 422 },
            "{case}: {data}"
        );
        if status == 200 {
            assert_eq!(data, case["validated"], "{case}");
        } else {
            let keys = data["errors"]
                .as_object()
                .unwrap()
                .keys()
                .collect::<Vec<_>>();
            assert_eq!(
                keys,
                case["errors"]
                    .as_object()
                    .unwrap()
                    .keys()
                    .collect::<Vec<_>>(),
                "{case}"
            );
        }
    }
    assert_eq!(
        send(
            address,
            "/distinct",
            r#"{"rows":["a","A"]}"#,
            "application/json"
        )
        .await
        .0,
        422
    );
    assert_eq!(
        send(
            address,
            "/distinct",
            r#"{"rows":["a","b"]}"#,
            "application/json"
        )
        .await
        .0,
        200
    );
    assert_eq!(
        send(
            address,
            "/distinct",
            r#"{"rows":[1,"1"]}"#,
            "application/json"
        )
        .await
        .0,
        422
    );
    let conditional_cases: Vec<Value> =
        serde_json::from_str(include_str!("reference/validation-fluent-conditions.json")).unwrap();
    for case in conditional_cases
        .iter()
        .filter(|case| case.get("wildcard").is_none())
    {
        let mut data = case["input"].clone();
        data["kind"] = case["kind"].clone();
        data["flag"] = case["flag"].clone();
        let (status, value) = send(
            address,
            "/conditions",
            &data.to_string(),
            "application/json",
        )
        .await;
        assert_eq!(
            status,
            if case["passes"] == true { 200 } else { 422 },
            "{case}: {value}"
        );
        if status == 200 {
            if case["validated"] == json!([]) {
                assert_eq!(value, json!({}));
            } else {
                let mut expected = case["validated"].clone();
                // 真实HTTP默认经过ConvertEmptyStringsToNull，工厂对照保留原始值。
                if expected.get("value") == Some(&json!("")) {
                    expected["value"] = Value::Null;
                }
                assert_eq!(value, expected);
            }
        }
    }
    server.abort();
    let _ = server.await;
}
#[tokio::test]
async fn builder_contracts_conditions_and_standard_parser_agree() {
    assert!(IpAddress::new().version(5).is_err());
    assert_eq!(
        Url::new().protocols(["http", "https"]).to_array(),
        vec!["url", "http", "https"]
    );
    assert_eq!(Uuid::new().version(4).to_array(), vec!["uuid", "4"]);
    assert_eq!(
        Distinct::new().strict().strict().ignore_case().to_array(),
        vec!["distinct", "strict", "ignore_case"]
    );
    assert_eq!(
        Timezone::new(["per_country", "US"]).to_array(),
        vec!["timezone", "per_country", "US"]
    );
    struct Strict;
    impl ParameterizedRule for Strict {
        fn to_array(&self) -> Vec<String> {
            vec!["numeric".into(), "strict".into()]
        }
    }
    let factory = Factory::default();
    for (value, expected) in [(json!(1), true), (json!("1"), false), (json!(false), false)] {
        for rule in [
            Rule::parameterized(Strict),
            Rule::parse("numeric:strict").unwrap(),
            Numeric::new(true).into(),
        ] {
            let mut v = factory
                .make(
                    Data::new(json!({"v":value})).unwrap(),
                    RuleSet::new().field("v", [rule]).unwrap(),
                    Default::default(),
                    Default::default(),
                )
                .unwrap();
            assert_eq!(v.validate().await.is_ok(), expected);
        }
    }
    for (rule, value, expected) in [
        (Rule::boolean_strict(), json!(true), true),
        (Rule::boolean_strict(), json!(1), false),
        (Rule::integer_strict(), json!(2), true),
        (Rule::integer_strict(), json!("2"), false),
        (
            Rule::date_time().format("Y-m-d").into(),
            json!("2024-01-01"),
            true,
        ),
        (
            ArrayRule::default()
                .min(1)
                .max(3)
                .between(1, 3)
                .exactly(2)
                .into(),
            json!([1, 2]),
            true,
        ),
        (
            ArrayRule::default()
                .min(1)
                .max(3)
                .between(1, 3)
                .exactly(2)
                .into(),
            json!([1]),
            false,
        ),
        (
            StringRule::new().starts_with(["a,b"]).into(),
            json!("apple"),
            true,
        ),
        (ArrayRule::new(["a,b"]).into(), json!({"a":1}), true),
        (ArrayRule::new(["a,b"]).into(), json!({"a,b":1}), false),
    ] {
        let mut v = factory
            .make(
                Data::new(json!({"v":value})).unwrap(),
                RuleSet::new().field("v", [rule]).unwrap(),
                Default::default(),
                Default::default(),
            )
            .unwrap();
        assert_eq!(v.validate().await.is_ok(), expected, "{value}");
    }
    assert_eq!(
        StringRule::new().alpha(false).min(2).max(3).to_string(),
        "string|alpha|min:2|max:3"
    );
    assert_eq!(
        Date::new().format("Y-m-d").before("2025-01-01").to_string(),
        "date_format:Y-m-d|before:2025-01-01"
    );
    assert_eq!(
        Numeric::new(false).digits(3).exactly(123).to_string(),
        "numeric|integer|digits:3|size:123"
    );
    let rule = StringRule::new()
        .when(false, |r| r.min(99))
        .unless(false, |r| r.min(2))
        .when_else(true, |r| r.max(3), |r| r.max(1));
    let mut validator = factory
        .make(
            Data::new(json!({"v":"abc"})).unwrap(),
            RuleSet::new().field("v", [rule.into()]).unwrap(),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    assert!(validator.validate().await.is_ok());
    let date = openrust::support::Date::from_utc(
        "2024-06-01T12:00:00Z".parse().unwrap(),
        openrust::support::Date::timezone("UTC").unwrap(),
    );
    let rule = Date::new().format("Y-m-d").before(date);
    let mut validator = factory
        .make(
            Data::new(json!({"v":"2024-01-01"})).unwrap(),
            RuleSet::new().field("v", [rule.into()]).unwrap(),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    assert!(validator.validate().await.is_ok());
    let mut plain = factory
        .make(
            Data::new(json!({"v":"abc"})).unwrap(),
            RuleSet::new()
                .field("v", [Rule::string().min(2).max(3)])
                .unwrap(),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    assert!(plain.validate().await.is_ok());
    assert_eq!(plain.get_rules().unwrap()[0].1.len(), 3);
}

#[tokio::test]
async fn conditional_rules_compile_at_the_original_phase_and_recompile_on_data_change() {
    use openrust::validation::{ConditionalRules, RuleCondition};
    use std::sync::atomic::{AtomicUsize, Ordering};
    let factory = Factory::default();
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("reference/validation-fluent-conditions.json")).unwrap();
    for case in cases {
        let calls = Arc::new(AtomicUsize::new(0));
        let capture = calls.clone();
        let flag = case["flag"].as_bool().unwrap_or(true);
        let condition = RuleCondition::lazy(move || {
            capture.fetch_add(1, Ordering::SeqCst);
            flag
        });
        let wildcard = case["wildcard"] == true;
        let rule = if case["kind"] == "when" {
            Rule::when(condition, vec![Rule::required()], Vec::new()).into()
        } else {
            conditional_rule(case["kind"].as_str().unwrap(), condition)
        };
        let input = if wildcard {
            json!({"rows":[1,2,3]})
        } else {
            case["input"].clone()
        };
        let rules = RuleSet::new()
            .field(if wildcard { "rows.*" } else { "value" }, [rule])
            .unwrap();
        let mut validator = factory
            .make(
                Data::new(input).unwrap(),
                rules,
                Default::default(),
                Default::default(),
            )
            .unwrap();
        assert_eq!(
            calls.load(Ordering::SeqCst),
            case["calls_at_make"].as_u64().unwrap() as usize,
            "{case}"
        );
        let first = validator.validate().await;
        let second = validator.validate().await;
        assert_eq!(first.is_ok(), second.is_ok());
        if let Ok(input) = &first
            && !wildcard
        {
            if case["validated"] == json!([]) {
                assert_eq!(input.all(), &json!({}));
            } else {
                assert_eq!(input.all(), &case["validated"]);
            }
        }
        if !wildcard {
            assert_eq!(first.is_ok(), case["passes"].as_bool().unwrap(), "{case}");
        }
        assert_eq!(
            calls.load(Ordering::SeqCst),
            case["calls"].as_u64().unwrap() as usize,
            "{case}"
        );
    }
    let calls = Arc::new(AtomicUsize::new(0));
    let capture = calls.clone();
    let condition = RuleCondition::callback(move |data| {
        capture.fetch_add(1, Ordering::SeqCst);
        Ok(data.as_value()["enabled"] == true)
    });
    let rules = RuleSet::new()
        .field(
            "value",
            [
                ConditionalRules::new(condition, vec![Rule::required()], vec![Rule::nullable()])
                    .into(),
            ],
        )
        .unwrap();
    let mut v = factory
        .make(
            Data::new(json!({"enabled":false})).unwrap(),
            rules,
            Default::default(),
            Default::default(),
        )
        .unwrap();
    assert!(v.validate().await.is_ok());
    v.set_data(Data::new(json!({"enabled":true})).unwrap());
    assert!(v.validate().await.is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let rule = Rule::unless(true, vec![Rule::required()], vec![Rule::string()]);
    let mut v = factory
        .make(
            Data::new(json!({"v":"A"})).unwrap(),
            RuleSet::new().field("v", [rule.into()]).unwrap(),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    assert!(v.validate().await.is_ok());
}
