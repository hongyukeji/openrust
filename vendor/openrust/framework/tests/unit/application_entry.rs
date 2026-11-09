use super::parse_console_args;
use super::prelude::*;

#[test]
fn prelude_json_is_a_response() {
    let response = Json("ok").into_response();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "application/json");
}

#[test]
fn console_environment_argument_is_removed_before_command_dispatch() {
    let parse =
        |args: &[&str]| parse_console_args(args.iter().map(|arg| (*arg).to_owned()), "serve");
    assert_eq!(parse(&[]), ("serve".into(), Vec::new(), None));
    assert_eq!(
        parse_console_args(["--env=local".to_owned()], "start"),
        ("start".into(), Vec::new(), Some("local".into()))
    );
    assert_eq!(
        parse(&["--env", "local", "config:cache"]),
        ("config:cache".into(), Vec::new(), Some("local".into()))
    );
    assert_eq!(
        parse(&["help", "--env=staging", "detail"]),
        ("help".into(), vec!["detail".into()], Some("staging".into()))
    );
    assert_eq!(
        parse(&["--envelope=mail", "--env"]),
        ("--envelope=mail".into(), Vec::new(), None)
    );
    assert_eq!(
        parse(&["-n", "--env=production", "migrate", "down"]),
        (
            "migrate".into(),
            vec!["down".into(), "--no-interaction".into()],
            Some("production".into()),
        )
    );
}
