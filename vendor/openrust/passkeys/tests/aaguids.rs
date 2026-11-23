use openrust::database::{Value, eloquent::Model};
use openrust_passkeys::{Aaguids, Passkey};
use serde_json::json;
use std::sync::Arc;

#[test]
fn pinned_php_labels_icons_unknown_theme_and_flush_match() {
    let expected: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/aaguids.json")).unwrap();
    for case in expected["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        assert_eq!(
            serde_json::to_value(Aaguids::labelFor(id)).unwrap(),
            case["label"],
            "{id}"
        );
        for (index, theme) in ["light", "dark"].iter().enumerate() {
            let icon = Aaguids::iconFor(id, theme).unwrap();
            let digest = icon.as_ref().map(|icon| {
                aws_lc_rs::digest::digest(&aws_lc_rs::digest::SHA256, icon.as_bytes())
                    .as_ref()
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>()
            });
            assert_eq!(
                serde_json::to_value(digest).unwrap(),
                case["icons"][index],
                "{id} {theme}"
            );
        }
    }
    assert_eq!(
        Aaguids::iconFor("", "invalid").unwrap_err().to_string(),
        expected["invalid_theme"]
    );
    let borrowed = Aaguids::all();
    Aaguids::flush();
    let reloaded = Aaguids::all();
    assert!(!Arc::ptr_eq(&borrowed, &reloaded));
    assert_eq!(borrowed, reloaded);
}

#[test]
fn model_appends_label_but_icons_require_explicit_opt_in() {
    for id in [
        "adce0002-35bc-c60a-648b-0b25f1f05503",
        Aaguids::unknown(),
        "unrecognized",
    ] {
        let mut passkey = Passkey::new_instance(
            [
                ("name".into(), "Device".into()),
                ("credential".into(), Value::Json(json!({"aaguid":id}))),
            ]
            .into(),
        )
        .unwrap();
        let array = passkey.to_array().unwrap();
        assert_eq!(
            array.get("authenticator"),
            Some(&json!(Aaguids::labelFor(id)))
        );
        assert!(!array.contains_key("authenticator_icon_light"));
        assert!(!array.contains_key("authenticator_icon_dark"));
        passkey.append(&["authenticator_icon_light", "authenticator_icon_dark"]);
        let array = passkey.to_array().unwrap();
        assert_eq!(
            array["authenticator_icon_light"],
            json!(Aaguids::iconFor(id, "light").unwrap())
        );
        assert_eq!(
            array["authenticator_icon_dark"],
            json!(Aaguids::iconFor(id, "dark").unwrap())
        );
    }
    let absent = Passkey::default().to_array().unwrap();
    assert_eq!(absent["authenticator"], json!(null));
}
