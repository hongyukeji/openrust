use openrust::json_schema::{
    Deserializer, JsonSchema, JsonSchemaTypeFactory, Serializer, types::*,
};
use serde_json::{Value, json};

#[test]
fn deserialization_matches_fixed_upstream_cases() {
    let fixture: Value = serde_json::from_str(include_str!("reference/json-schema.json")).unwrap();
    for (index, case) in fixture["cases"].as_array().unwrap().iter().enumerate() {
        let result =
            JsonSchema::fromArray(case["schema"].clone()).and_then(|schema| schema.to_array());
        if case.get("error") == Some(&Value::Bool(true)) {
            assert!(
                result.is_err(),
                "用例 {index} 应拒绝：{case}; 实际 {result:?}"
            );
        } else {
            assert_eq!(result.unwrap(), case["expected"], "用例 {index}：{case}");
        }
    }
}

#[test]
fn typed_factory_retains_constraints_and_required_parent_semantics() {
    let factory = JsonSchemaTypeFactory;
    let schema = factory
        .object([
            (
                "name",
                factory
                    .string()
                    .min(1)
                    .max(20)
                    .pattern("[a-z]+")
                    .format("hostname")
                    .title("名称")
                    .description("描述")
                    .default("a")
                    .required(true)
                    .into(),
            ),
            (
                "score",
                factory
                    .number()
                    .min(0)
                    .max(10.5)
                    .multiple_of(0.5)
                    .default(1)
                    .nullable(true)
                    .into(),
            ),
            (
                "roles",
                factory
                    .array()
                    .items(factory.string().r#enum([json!("admin"), json!("user")]))
                    .min(1)
                    .max(3)
                    .unique(true)
                    .default(vec![])
                    .into(),
            ),
            ("enabled", factory.boolean().default(false).into()),
        ])
        .withoutAdditionalProperties();
    let value = schema.to_array().unwrap();
    assert_eq!(value["required"], json!(["name"]));
    assert_eq!(
        value["properties"]["score"]["type"],
        json!(["number", "null"])
    );
    assert_eq!(value["properties"]["roles"]["uniqueItems"], true);
    assert_eq!(value["properties"]["enabled"]["default"], false);
    assert!(value["properties"]["name"].get("required").is_none());
    assert_eq!(
        JsonSchema::fromArray(value.clone())
            .unwrap()
            .to_array()
            .unwrap(),
        value
    );
    assert_eq!(
        Serializer::serialize(&schema).unwrap(),
        serde_json::to_value(&schema).unwrap()
    );
    assert!(schema.to_string().unwrap().contains('\n'));
}

#[test]
fn combinations_can_be_nested_and_disabled_flags_are_omitted() {
    let any = JsonSchema::any_of([
        JsonSchema::string().into(),
        JsonSchema::integer()
            .min(0)
            .max(5)
            .multiple_of(1)
            .default(2)
            .into(),
    ])
    .nullable(true)
    .required(true);
    assert_eq!(any.schemas().len(), 2);
    assert_eq!(
        any.to_array().unwrap(),
        json!({"anyOf":[{"type":"string"},{"type":"integer","minimum":0,"maximum":5,"multipleOf":1,"default":2},{"type":"null"}]})
    );
    let union = JsonSchema::union(["integer", "null", "integer", "string"]).unwrap();
    assert_eq!(union.types(), ["integer", "string"]);
    assert_eq!(
        union.to_array().unwrap(),
        json!({"type":["integer","string","null"]})
    );
    assert!(JsonSchema::union(["bogus"]).is_err());
    assert_eq!(
        JsonSchema::array()
            .unique(true)
            .unique(false)
            .nullable(false)
            .required(false)
            .to_array()
            .unwrap(),
        json!({"type":"array"})
    );
    assert_eq!(
        JsonSchema::object(Vec::<(String, Type)>::new())
            .to_array()
            .unwrap(),
        json!({"type":"object"})
    );
}

#[test]
fn invalid_numeric_and_reference_inputs_fail_explicitly() {
    assert!(JsonSchema::number().min(f64::INFINITY).to_array().is_err());
    assert!(JsonSchema::number().default(f64::NAN).to_string().is_err());
    assert!(JsonSchema::fromArray(json!({"$ref":"#/$defs/a","$defs":{"a":42}})).is_err());
    assert!(
        JsonSchema::fromArray(json!({"type":"object","properties":{"a":{"$ref":"#"}}})).is_err()
    );
    let mut nested = json!({"type":"string"});
    for _ in 0..=Deserializer::MAX_DEPTH {
        nested = json!({"type":"array","items":nested});
    }
    assert!(JsonSchema::fromArray(nested).is_err());
}

#[test]
fn node_budget_and_repeated_references_are_checked_independently() {
    let mut properties = serde_json::Map::new();
    for n in 0..Deserializer::MAX_NODES {
        properties.insert(n.to_string(), json!({"type":"string"}));
    }
    assert!(JsonSchema::fromArray(json!({"type":"object","properties":properties})).is_err());
    let value = json!({"type":"object","properties":{"a":{"$ref":"#/$defs/s"},"b":{"$ref":"#/$defs/s"}},"$defs":{"s":{"type":"string"}}});
    let result = JsonSchema::fromArray(value).unwrap().to_array().unwrap();
    assert_eq!(result["properties"]["a"], result["properties"]["b"]);
}

#[test]
fn public_contract_builds_typed_properties_without_concrete_factory_dependency() {
    fn schema(factory: &impl openrust::contracts::json_schema::JsonSchema) -> Type {
        factory
            .object([("name", factory.string().required(true).into())])
            .into()
    }
    assert_eq!(
        schema(&JsonSchemaTypeFactory).to_array().unwrap(),
        json!({"type":"object","properties":{"name":{"type":"string"}},"required":["name"]})
    );
}
