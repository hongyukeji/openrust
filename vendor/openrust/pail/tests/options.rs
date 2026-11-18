use std::ffi::OsString;

use clap::{Arg, Command};
use openrust::console::CommandDefinition;
use openrust_pail::{
    Options,
    value_objects::{
        MessageLogged, Origin,
        origin::{Console, Http, Queue},
    },
};
use serde_json::{Value, json};

fn record(context: Value) -> MessageLogged {
    MessageLogged::fromJson(&json!({
        "message":"Import CRITICAL report 你好 🚀", "datetime":"2026-11-19T03:04:05.123456+08:00",
        "level_name":"CRITICAL", "context":context,
        "channel":"pail", "level":500, "extra":{}
    }).to_string()).unwrap()
}
fn http() -> MessageLogged {
    record(
        json!({"__pail":{"origin":{"type":"http","method":"GET","path":"reports/weekly",
      "auth_id":12,"auth_email":"user@example.test","trace":[{"file":"/app/routes/web.rs","line":17}]}},
      "exception":{"class":"ImportError","file":"/app/jobs/import.rs:9"},"userId":"internal","team":"North", "count":3}),
    )
}
fn options(
    auth: Option<&str>,
    level: Option<&str>,
    filter: Option<&str>,
    message: Option<&str>,
) -> Options {
    Options::new(
        3600,
        auth.map(Into::into),
        level.map(Into::into),
        filter.map(Into::into),
        message.map(Into::into),
    )
}
fn parsed(arguments: &[&str]) -> Options {
    let mut command = Command::new("pail");
    for name in ["auth", "user", "level", "filter", "message", "timeout"] {
        let mut arg = Arg::new(name)
            .long(name)
            .num_args(1)
            .allow_hyphen_values(true);
        if name == "timeout" {
            arg = arg.default_value("3600");
        }
        command = command.arg(arg);
    }
    let definition = CommandDefinition::new(command);
    let input = definition
        .parse(
            &arguments[1..]
                .iter()
                .map(OsString::from)
                .collect::<Vec<_>>(),
        )
        .unwrap();
    Options::fromCommand(&input).unwrap()
}

#[test]
fn parsed_records_preserve_dates_origins_and_public_context() {
    let event = http();
    assert_eq!(event.message(), "Import CRITICAL report 你好 🚀");
    assert_eq!(event.date().unwrap(), "2026-11-19 03:04:05");
    assert_eq!(event.time().unwrap(), "03:04:05");
    assert_eq!(event.level(), "CRITICAL");
    assert_eq!(event.classOrType(), "ImportError");
    assert_eq!(event.file(), Some("/app/jobs/import.rs:9"));
    assert_eq!(event.authId().as_deref(), Some("12"));
    assert_eq!(event.trace().unwrap().len(), 1);
    assert_eq!(
        event.context(),
        serde_json::Map::from_iter([("team".into(), json!("North")), ("count".into(), json!(3))])
    );
    assert_eq!(
        event.origin().unwrap(),
        Origin::Http(Http {
            method: "GET".into(),
            path: "reports/weekly".into(),
            authId: Some("12".into()),
            authEmail: Some("user@example.test".into())
        })
    );
    assert_eq!(
        Http::fromArray(&json!({"method":"POST","path":"/","auth_id":"0012","auth_email":null}))
            .unwrap()
            .authId
            .as_deref(),
        Some("0012")
    );
    assert_eq!(
        Console::fromArray(&json!({})).unwrap(),
        Console { command: None }
    );
    assert_eq!(
        Queue::fromArray(&json!({"queue":"default","job":"Import","command":"queue:work"}))
            .unwrap()
            .command
            .as_deref(),
        Some("queue:work")
    );
    assert!(Queue::fromArray(&json!({"job":"Import"})).is_err());
    assert!(event.to_string().contains(r"\u4f60\u597d \ud83d\ude80"));
    assert!(event.to_string().contains("reports/weekly"));
    assert!(!event.to_string().contains(r"\/"));
    let roundtrip = MessageLogged::fromJson(&event.to_string()).unwrap();
    assert_eq!(roundtrip.message(), event.message());
    assert!(
        serde_json::from_str::<Value>(&event.to_string())
            .unwrap()
            .get("channel")
            .is_none()
    );
}

#[test]
fn severity_origin_and_absent_fields_follow_pail() {
    for (level, color) in [
        ("DEBUG", "gray"),
        ("INFO", "blue"),
        ("NOTICE", "yellow"),
        ("WARNING", "yellow"),
        ("ERROR", "red"),
        ("CRITICAL", "red"),
        ("ALERT", "red"),
        ("EMERGENCY", "red"),
        ("error", "gray"),
        ("unknown", "gray"),
    ] {
        let event=MessageLogged::fromJson(&json!({"message":"ok","datetime":"2026-11-19T23:59:59.000000-05:00","level_name":level,"context":{"__pail":{"origin":{"type":"console","command":null}}}}).to_string()).unwrap();
        assert_eq!(event.color(), color);
        assert_eq!(event.classOrType(), level.to_ascii_uppercase());
        assert_eq!(
            event.origin().unwrap(),
            Origin::Console(Console { command: None })
        );
        assert!(event.file().is_none() && event.trace().is_none() && event.authId().is_none());
        assert_eq!(event.time().unwrap(), "23:59:59");
    }
    let event = record(
        json!({"__pail":{"origin":{"type":"queue","queue":"mail","job":"SendMail","command":null}}}),
    );
    assert_eq!(
        event.origin().unwrap(),
        Origin::Queue(Queue {
            queue: "mail".into(),
            job: "SendMail".into(),
            command: None
        })
    );
    assert!(MessageLogged::fromJson("not JSON").is_err());
    assert!(MessageLogged::fromJson("{}").is_err());
    assert!(
        MessageLogged::fromJson(
            r#"{"message":1,"datetime":"now","level_name":"INFO","context":{}}"#
        )
        .is_err()
    );
    let invalid = MessageLogged::fromJson(
        r#"{"message":"ok","datetime":"now","level_name":"INFO","context":{}}"#,
    )
    .unwrap();
    assert!(invalid.date().is_err() && invalid.time().is_err() && invalid.origin().is_err());
}

#[test]
fn filters_are_conjunctive_and_search_the_expected_payload() {
    let event = http();
    for accepted in [
        options(None, None, None, None),
        options(Some("12"), Some("cRiTiCaL"), Some("NoRtH"), Some("import")),
        options(None, None, Some("user@example.test"), Some("REPORT")),
        options(None, None, Some("ImportError"), None),
        options(None, None, Some(""), Some("")),
        options(None, None, Some(r"\u4f60"), Some("你好")),
    ] {
        assert!(accepted.accepts(&event));
    }
    for rejected in [
        options(Some("012"), None, None, None),
        options(Some("13"), None, None, None),
        options(None, Some("error"), None, None),
        options(None, None, Some("South"), None),
        options(None, None, None, Some("North")),
        options(None, None, Some("你好"), None),
        options(None, None, None, Some("İmport")),
    ] {
        assert!(!rejected.accepts(&event));
    }
    let guest = record(
        json!({"__pail":{"origin":{"type":"http","method":"GET","path":"/","auth_id":null,"auth_email":null}}}),
    );
    assert!(!options(Some(""), None, None, None).accepts(&guest));
    assert!(options(None, None, None, None).accepts(&guest));
}

#[test]
fn command_options_preserve_alias_priority_empty_values_and_timeout_casts() {
    let event = http();
    assert_eq!(parsed(&["pail"]).timeout(), 3600);
    assert!(parsed(&["pail", "--user=12"]).accepts(&event));
    assert!(parsed(&["pail", "--auth=12", "--user=13"]).accepts(&event));
    assert!(!parsed(&["pail", "--auth=", "--user=12"]).accepts(&event));
    assert!(
        parsed(&[
            "pail",
            "--level=critical",
            "--message=IMPORT",
            "--filter=north"
        ])
        .accepts(&event)
    );
    for (value, expected) in [
        ("0", 0),
        ("-2", -2),
        ("1.9", 1),
        ("1e2", 100),
        ("12junk", 12),
        ("1e2junk", 100),
        ("junk", 0),
        ("", 0),
        ("  +12rest", 12),
        ("-.5", 0),
        ("1e", 1),
        ("9223372036854775807", i64::MAX),
    ] {
        assert_eq!(
            parsed(&["pail", &format!("--timeout={value}")]).timeout(),
            expected,
            "{value}"
        );
    }
}

#[test]
fn pinned_php_options_and_serialization_oracle() {
    #[derive(serde::Deserialize)]
    struct Corpus {
        upstream: String,
        php_version: String,
        records: Vec<String>,
        strings: Vec<String>,
        filters: Vec<Filter>,
        timeouts: Vec<(String, i64)>,
    }
    type Filter = (
        usize,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        bool,
    );
    let corpus: Corpus = serde_json::from_str(include_str!("fixtures/pail-options.json")).unwrap();
    assert_eq!(corpus.upstream, "fdb73f5eacf03db576c710d5a00101ba185f2254");
    assert!(corpus.php_version.starts_with("8.4."));
    let records: Vec<_> = corpus
        .records
        .iter()
        .map(|json| MessageLogged::fromJson(json).unwrap())
        .collect();
    for (record, expected) in records.iter().zip(&corpus.strings) {
        assert_eq!(record.to_string(), *expected);
    }
    for (index, auth, level, filter, message, expected) in corpus.filters {
        assert_eq!(
            Options::new(3600, auth, level, filter, message).accepts(&records[index]),
            expected
        );
    }
    for (value, expected) in corpus.timeouts {
        assert_eq!(
            parsed(&["pail", &format!("--timeout={value}")]).timeout(),
            expected,
            "PHP {value:?}"
        );
    }
}

#[test]
fn trilingual_documentation_example_selects_error_messages() {
    fn select(line: &str) -> Result<bool, serde_json::Error> {
        let message = MessageLogged::fromJson(line)?;
        let options = Options::new(
            3600,
            None,
            Some("ERROR".into()),
            None,
            Some("payment".into()),
        );
        Ok(options.accepts(&message))
    }
    for (level, text, expected) in [
        ("ERROR", "Payment failed", true),
        ("INFO", "Payment accepted", false),
        ("ERROR", "Import failed", false),
    ] {
        let line=json!({"message":text,"level_name":level,"datetime":"2026-11-19T03:04:05.000000Z","context":{}}).to_string();
        assert_eq!(select(&line).unwrap(), expected);
    }
}
