use super::*;

#[test]
fn deprecated_delay_matches_unmodified_reference_worker_options() {
    let reference: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/queue_work_backoff.json")).unwrap();
    assert_eq!(
        reference["commit"],
        "1449bf241017087fbd38c118ecaabef3f6797fdc"
    );
    assert_eq!(reference["cases"].as_array().unwrap().len(), 50);
    let command = WorkCommand::new();
    for case in reference["cases"].as_array().unwrap() {
        let input = command
            .definition()
            .parse(&[
                format!("--backoff={}", case["backoff"].as_str().unwrap()).into(),
                format!("--delay={}", case["delay"]).into(),
            ])
            .unwrap();
        let actual: Vec<_> = backoff(input.matches())
            .into_iter()
            .map(|value| value.as_secs())
            .collect();
        assert_eq!(serde_json::json!(actual), case["expected"], "{case}");
    }
}
