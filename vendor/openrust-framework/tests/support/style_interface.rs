use openrust::console::{ChoiceKey, ConsoleError, QuestionCallback, StyleInterface};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
fn record(steps: &mut Vec<Value>, name: &str, result: Result<Value, ConsoleError>) {
    match result {
        Ok(value) => steps.push(json!({"method":name,"value":value,"failed":false})),
        Err(error) => {
            if name == "title" {
                let mut cause: &(dyn std::error::Error + 'static) = &error;
                loop {
                    if cause
                        .downcast_ref::<openrust::console::ConsoleFormatError>()
                        .is_some()
                    {
                        break;
                    }
                    cause = cause.source().expect("格式来源链丢失");
                }
            }
            steps.push(json!({"method":name,"value":Value::Null,"failed":true}));
        }
    }
}
pub(crate) async fn exercise<S: StyleInterface + ?Sized>(
    style: &mut S,
    c: &Value,
    trace: Arc<Mutex<Vec<Value>>>,
) -> Vec<Value> {
    let mut steps = Vec::new();
    macro_rules! call {
        ($name:literal,$expr:expr) => {
            record(&mut steps, $name, $expr.map(|()| Value::Null))
        };
    }
    call!(
        "title",
        style.title(if c["variant"] == 1 {
            "<fg=bogus>Title</>"
        } else {
            "Title"
        })
    );
    call!("section", style.section("Section"));
    call!("listing", style.listing(&["first", "名"]));
    call!("text", style.text((&["alpha", "beta"]).into()));
    call!("success", style.success("Ready".into()));
    call!("error", style.error("Failure".into()));
    call!("warning", style.warning("Review".into()));
    call!("note", style.note((&["one", "two"]).into()));
    call!("caution", style.caution("Caution".into()));
    call!(
        "table",
        style.table(["K", "V"].into(), vec![["x", "y"].into()])
    );
    let variant = c["variant"].as_u64().unwrap();
    let log = trace.clone();
    let validator = QuestionCallback::sync(move |value| {
        log.lock().unwrap().push(json!(["ask", value]));
        if variant == 2 {
            return Err(ConsoleError::from_runtime_source(std::io::Error::other(
                "validator runtime",
            )));
        }
        Ok(json!({"name":value}))
    });
    record(
        &mut steps,
        "ask",
        style.ask("Name", Some("Guest"), Some(validator)).await,
    );
    let log = trace.clone();
    let hidden = QuestionCallback::sync(move |value| {
        log.lock().unwrap().push(json!(["hidden", value]));
        Ok(json!({"length":value.as_str().unwrap_or("").len()}))
    });
    record(
        &mut steps,
        "askHidden",
        style.ask_hidden("Token", Some(hidden)).await,
    );
    record(
        &mut steps,
        "confirm",
        style.confirm("Continue", true).await.map(|v| json!(v)),
    );
    let choices = if variant == 1 {
        vec![
            (ChoiceKey::from("r"), json!("red")),
            (ChoiceKey::from("g"), json!("green")),
        ]
    } else {
        vec![
            (ChoiceKey::from(0_i64), json!("red")),
            (ChoiceKey::from(1_i64), json!("green")),
        ]
    };
    record(
        &mut steps,
        "choice",
        style.choice("Color", choices, Some(json!("green"))).await,
    );
    call!("newLine", style.new_line(2));
    call!("progressFinish-before", style.progress_finish());
    call!("progressStart", style.progress_start(2));
    call!("progressAdvance", style.progress_advance(1));
    call!("progressFinish", style.progress_finish());
    call!("text-recovery", style.text("Recovered".into()));
    steps
}
