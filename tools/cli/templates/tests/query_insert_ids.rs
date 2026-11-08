//! 验证生成应用的 Query Builder 插入 ID 与 Eloquent 保存。

use openrust::{
    Error, Result,
    database::{
        Db,
        sea_orm::{ConnectionTrait, DbBackend, TransactionTrait, Value},
        sea_orm_migration::{MigrationTrait, MigratorTrait},
    },
    testing::TestDatabase,
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

struct EmptyMigrator;
impl MigratorTrait for EmptyMigrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        Vec::new()
    }
}

fn check(condition: bool) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(Error::custom(
            "generated_insert_id_contract",
            "插入 ID 公开契约不符",
        ))
    }
}

struct ResetJsonCodec;
impl Drop for ResetJsonCodec {
    fn drop(&mut self) {
        use openrust::database::eloquent::Json;
        Json::encode_using(None);
        Json::decode_using(None);
    }
}

#[tokio::test]
async fn generated_application_insert_ids_preserve_execution_and_backend_contracts()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    // {{database_setup}}
    let outcome: Result<()> = async {
        let db = Db::default();
        db.initialize(database.connection()?)?;
        let connection = db.connection()?;
        let backend = db.backend()?;
        let ddl = match backend {
            DbBackend::Sqlite => "CREATE TABLE insert_items (id INTEGER PRIMARY KEY, label TEXT DEFAULT 'default')",
            DbBackend::MySql => "CREATE TABLE insert_items (id BIGINT AUTO_INCREMENT PRIMARY KEY, label VARCHAR(255) DEFAULT 'default')",
            DbBackend::Postgres => "CREATE TABLE insert_items (id BIGSERIAL PRIMARY KEY, label TEXT DEFAULT 'default')",
            _ => return Err(Error::custom("generated_insert_id_backend", "测试需要受支持的数据库")),
        };
        connection.execute_unprepared(ddl).await.map_err(Error::internal)?;
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&calls);
        let query = db.table("insert_items")?.before_query(move |_| {
            observed.fetch_add(1, Ordering::SeqCst);
            Ok(())
        });
        check(query.insert_get_id(Vec::new(), None).await? == Value::BigInt(Some(1)))?;
        let label = "bound ' value ? $1";
        check(query.insert_get_id(vec![("label", label.into())], Some("")).await? == Value::BigInt(Some(2)))?;
        check(calls.load(Ordering::SeqCst) == 1)?;
        check(query.count().await? == 2)?;
        check(db.table("insert_items")?.where_eq("id", 2_i64)?.value::<String>("label").await?.as_deref() == Some(label))?;
        check(db.table("insert_items")?.where_eq("id", 1_i64)?.value::<String>("label").await?.as_deref() == Some("default"))?;
        check(query.insert_get_id(Vec::new(), Some("0")).await? == Value::BigInt(Some(3)))?;
        check(calls.load(Ordering::SeqCst) == 1)?;
        rollback_insert(&db).await?;
        check_generated_enum_cast().await?;
        check_generated_decimal_cast().await?;
        check_generated_primitive_json_casts().await?;
        check_generated_as_fluent().await?;
        check_generated_as_stringable().await?;
        check_generated_as_html_string().await?;
        check_generated_date_string_format().await?;
        check_generated_json_codec().await?;
        check_generated_json_flags()?;
        check(query.count().await? == 3)?;
        backend_ids(&db).await?;
        check_generated_class_cast().await?;
        check_generated_collection_cast().await?;
        #[cfg(feature = "encryption")]
        check_generated_encrypted_collection().await?;
        generated_model_save(database.url()).await
    }.await;
    database.close().await?;
    outcome?;
    Ok(())
}

async fn check_generated_class_cast() -> Result<()> {
    use openrust::database::eloquent::{
        AttributeCast, AttributeMap, AttributeValue, ClassCast, ClassCastWrite, ClassCaster,
        ModelAttributes, ModelMetadata, ModelObject,
    };
    use serde_json::json;

    struct Label(String);
    impl openrust::support::DataObject for Label {
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
        fn json_projection(&self) -> Option<serde_json::Value> {
            Some(json!(self.0))
        }
    }
    struct Normalize;
    impl ClassCaster for Normalize {
        fn get_value(
            &self,
            _: &str,
            raw: Option<&AttributeValue>,
            _: &AttributeMap,
        ) -> Result<AttributeValue> {
            let text = raw
                .map(AttributeValue::as_json)
                .transpose()?
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            Ok(ModelObject::new(Label(text.into())).into())
        }

        fn set(&self, _: &str, input: AttributeValue, _: &AttributeMap) -> Result<ClassCastWrite> {
            let projected = if let Some(object) = input.as_object() {
                object
                    .snapshot()?
                    .json_projection()
                    .unwrap_or(serde_json::Value::Null)
            } else {
                input.try_into_json()?
            };
            let text = projected.as_str().ok_or_else(|| {
                Error::custom("generated_class_cast_input", "测试类转换要求字符串")
            })?;
            Ok(ClassCastWrite::Value(json!(text.trim()).into()))
        }
    }

    let mut metadata = ModelMetadata::new("insert_items", "GeneratedRecord");
    metadata.casts.insert(
        "label".into(),
        AttributeCast::Class(ClassCast::new(|| Ok(Normalize))),
    );
    let attributes = ModelAttributes::default();
    attributes
        .set_attribute(&metadata, "label", json!("  value  "))
        .await?;
    check(attributes.attributes_to_json(&metadata)?["label"] == json!("value"))?;
    let object = attributes
        .get_attribute_value(&metadata, "label")?
        .and_then(|value| value.as_object().cloned())
        .ok_or_else(|| Error::custom("generated_class_cast_object", "缺少类转换对象"))?;
    object.modify(|_: &Label| Ok(Label("updated".into())))?;
    check(attributes.raw_attribute("label")? == Some(json!("updated")))
}

async fn check_generated_collection_cast() -> Result<()> {
    use openrust::database::eloquent::{
        ArrayObject, AsArrayObject, AsCollection, ModelAttributes, ModelMetadata,
    };
    use serde_json::{Value, json};

    let mut metadata = ModelMetadata::new("insert_items", "GeneratedRecord");
    metadata
        .casts
        .insert("data".into(), AsArrayObject::nullable());
    let attributes = ModelAttributes::default();
    attributes
        .set_attribute(&metadata, "data", json!({"tag":"initial"}))
        .await?;
    let object = attributes
        .get_attribute_value(&metadata, "data")?
        .and_then(|value| value.as_object().cloned())
        .ok_or_else(|| Error::custom("generated_array_object_missing", "缺少数组对象"))?;
    object.modify(|value: &ArrayObject| {
        let mut next = value.clone();
        next.as_collection_mut().try_put("tag", json!("updated"))?;
        Ok(next)
    })?;
    check(attributes.attributes_to_json(&metadata)?["data"] == json!({"tag":"updated"}))?;
    attributes
        .set_attribute(&metadata, "data", Value::Null)
        .await?;
    check(attributes.raw_attribute("data")? == Some(Value::Null))?;
    metadata.casts.insert(
        "data".into(),
        AsCollection::of(|value, key| Ok((key.to_string(), value))).cast(),
    );
    attributes.set_raw_attribute("data", json!(r#"{"label":"test"}"#))?;
    check(
        attributes.attributes_to_array(&metadata)?["data"].as_json()?
            == &json!({"label":["label","test"]}),
    )
}

#[cfg(feature = "encryption")]
async fn check_generated_encrypted_collection() -> Result<()> {
    use figment::{Figment, providers::Serialized};
    use openrust::{
        database::eloquent::{
            AsEncryptedArrayObject, AttributeCast, EncryptedCast, EncryptedCastKind, Json,
            ModelAttributes, ModelMetadata,
        },
        encryption::{ApplicationEncrypter, EncryptionServiceProvider},
        foundation::ProviderBuilder,
    };
    use serde_json::json;

    let mut providers = ProviderBuilder::new();
    providers.provider(EncryptionServiceProvider);
    let runtime = providers
        .start(Figment::from(Serialized::defaults(json!({
            "app":{"key":"0123456789abcdef0123456789abcdef"}
        }))))
        .await?;
    let encrypter = runtime
        .container()
        .resolve::<dyn ApplicationEncrypter>()
        .await?;
    let mut metadata = ModelMetadata::new("insert_items", "GeneratedRecord");
    metadata.casts.insert(
        "data".into(),
        AsEncryptedArrayObject::cast(encrypter.clone()),
    );
    let attributes = ModelAttributes::default();
    attributes
        .set_attribute(&metadata, "data", json!({"tag":"encrypted"}))
        .await?;
    let raw = attributes.raw_attribute("data")?;
    let raw = raw
        .as_ref()
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| Error::custom("generated_encrypted_cast_missing", "缺少加密数组对象密文"))?;
    check(encrypter.decrypt_string(raw)? == br#"{"tag":"encrypted"}"#)?;
    attributes
        .set_attribute(&metadata, "data->settings->enabled", json!(true))
        .await?;
    let expected = json!({"tag":"encrypted","settings":{"enabled":true}});
    check(attributes.attributes_to_json(&metadata)?["data"] == expected)?;
    metadata.casts.insert(
        "native_data".into(),
        AttributeCast::Encrypted(EncryptedCast::new(EncryptedCastKind::Collection, encrypter.clone())),
    );
    attributes
        .set_attribute(&metadata, "native_data", json!(["native"]))
        .await?;
    attributes.sync_original()?;
    attributes
        .set_attribute(&metadata, "native_data", json!(["native"]))
        .await?;
    check(attributes.is_clean(&metadata, &["native_data"])?)?;
    check(attributes.attributes_to_json(&metadata)?["native_data"] == json!(["native"]))?;

    let _reset = ResetJsonCodec;
    let calls = Arc::new(AtomicUsize::new(0));
    let reads = Arc::clone(&calls);
    Json::decode_using(Some(Arc::new(move |_, _| {
        reads.fetch_add(1, Ordering::Relaxed);
        Ok(json!({"hook":true}))
    })));
    let codec = ModelAttributes::new(serde_json::Map::from_iter([
        ("data".into(), json!(encrypter.encrypt_string(b"old")?)),
        ("native_data".into(), json!(encrypter.encrypt_string(b"old")?)),
    ]));
    codec.set_raw_attribute("data", json!(encrypter.encrypt_string(b"new")?))?;
    codec.set_raw_attribute("native_data", json!(encrypter.encrypt_string(b"new")?))?;
    check(!codec.original_is_equivalent(&metadata, "data")?)?;
    check(calls.load(Ordering::Relaxed) == 0)?;
    check(codec.original_is_equivalent(&metadata, "native_data")?)?;
    check(calls.load(Ordering::Relaxed) == 2)?;
    codec.set_raw_attribute("data", json!(encrypter.encrypt_string(b"")?))?;
    check(codec.attributes_to_json(&metadata)?["data"] == json!({"hook":true}))?;

    Json::encode_using(Some(Arc::new(|_, _| Ok(serde_json::Value::Null))));
    let codec = ModelAttributes::default();
    codec.set_attribute(&metadata, "native_data", json!(["new"])).await?;
    check(codec.raw_attribute("native_data")? == Some(serde_json::Value::Null))?;
    let result = codec.set_attribute(&metadata, "data", json!(["new"])).await;
    check(matches!(result, Err(error) if error.code() == "model_json_encryption_value"))
}

async fn check_generated_enum_cast() -> Result<()> {
    use openrust::database::eloquent::{
        AsEnumArrayObject, AsEnumCollection, AttributeCast, EnumCast, ModelAttributes,
        ModelMetadata,
    };
    use serde_json::json;

    #[derive(PartialEq, Eq)]
    enum Visibility {
        Public,
        Private,
    }
    let cast = EnumCast::unit(
        |name| match name {
            "Public" => Some(Visibility::Public),
            "Private" => Some(Visibility::Private),
            _ => None,
        },
        |case| match case {
            Visibility::Public => "Public",
            Visibility::Private => "Private",
        },
    );
    let mut model = ModelMetadata::new("insert_items", "GeneratedRecord");
    model
        .casts
        .insert("visibility".into(), AttributeCast::Enum(cast.clone()));
    let attrs = ModelAttributes::default();
    attrs
        .set_attribute_value(&model, "visibility", cast.case(Visibility::Private)?.into())
        .await?;
    let typed = attrs.get_attribute_value(&model, "visibility")?;
    check(
        typed
            .as_ref()
            .and_then(|value| value.as_enum())
            .and_then(|case| case.downcast_ref::<Visibility>())
            == Some(&Visibility::Private),
    )?;
    check(attrs.attributes_to_json(&model)?["visibility"] == json!("Private"))?;
    for (declaration, expected) in [
        (AsEnumArrayObject::of(cast.clone()), json!(r#"["Private"]"#)),
        (AsEnumCollection::of(cast), json!(r#"{"key":"Private"}"#)),
    ] {
        let attributes = ModelAttributes::default();
        model.casts.insert("options".into(), declaration);
        attributes
            .set_attribute(&model, "options", json!({"key":"Private"}))
            .await?;
        check(attributes.raw_attribute("options")? == Some(expected))?;
    }
    Ok(())
}

async fn check_generated_decimal_cast() -> Result<()> {
    use openrust::database::eloquent::{AttributeCast, ModelAttributes, ModelMetadata};
    use serde_json::json;
    let mut model = ModelMetadata::new("insert_items", "GeneratedRecord");
    model
        .casts
        .insert("amount".into(), AttributeCast::decimal(2));
    let attributes = ModelAttributes::default();
    attributes
        .set_attribute(&model, "amount", json!("-1/8"))
        .await?;
    check(attributes.raw_attribute("amount")? == Some(json!("-1/8")))?;
    check(attributes.attributes_to_json(&model)?["amount"] == json!("-0.13"))?;
    attributes.sync_original()?;
    attributes
        .set_attribute(&model, "amount", json!("-0.129"))
        .await?;
    check(attributes.is_clean(&model, &["amount"])?)
}

async fn check_generated_primitive_json_casts() -> Result<()> {
    use openrust::database::eloquent::{AttributeCast, ModelAttributes, ModelMetadata};
    use serde_json::json;

    let mut model = ModelMetadata::new("insert_items", "GeneratedRecord");
    let attrs = ModelAttributes::default();
    let input = json!({"标签":{"0":"中文😀/"},"空":{}});
    model.casts.insert("settings".into(), AttributeCast::JsonUnicode);
    attrs.set_attribute(&model, "settings", input.clone()).await?;
    check(attrs.raw_attribute("settings")? == Some(json!(r#"{"标签":{"0":"中文😀\/"},"空":{}}"#)))?;
    let associative = json!({"标签":["中文😀/"],"空":[]});
    check(attrs.attributes_to_json(&model)?["settings"] == associative)?;
    model.casts.insert("settings".into(), AttributeCast::Object);
    check(attrs.attributes_to_json(&model)?["settings"] == input)?;
    model.casts.insert("settings".into(), AttributeCast::Array);
    check(attrs.get_attribute(&model, "settings")? == Some(associative.clone()))?;
    model.casts.insert("settings".into(), AttributeCast::Collection);
    check(attrs.get_attribute_value(&model, "settings")?.is_some_and(|v| v.as_object().is_some()))?;
    check(attrs.attributes_to_json(&model)?["settings"] == associative)
}

async fn check_generated_as_fluent() -> Result<()> {
    use openrust::{
        database::eloquent::{AsFluent, ModelAttributes, ModelMetadata, ModelObject},
        support::Fluent,
    };
    use serde_json::json;

    let mut model = ModelMetadata::new("insert_items", "GeneratedFluentRecord");
    model.casts.insert("data".into(), AsFluent::cast());
    let attrs = ModelAttributes::default();
    let value = ModelObject::new(Fluent::from_value(json!({"x":1}))?);
    attrs.set_attribute_value(&model, "data", value.clone().into()).await?;
    check(attrs.get_attribute_value(&model, "data")?.and_then(|read| read.as_object().cloned())
        .is_some_and(|read| read.same_object(&value)))?;
    value.modify::<Fluent>(|value| {
        let mut next = value.clone();
        next.set("x", json!(2))?;
        Ok(next)
    })?;
    attrs.set_attribute(&model, "data->path", json!(true)).await?;
    check(attrs.attributes_to_json(&model)?["data"] == json!({"x":2,"path":true}))?;
    check(attrs.raw_attribute("data")? == Some(json!(r#"{"x":2,"path":true}"#)))
}

async fn check_generated_as_html_string() -> Result<()> {
    use openrust::{
        database::eloquent::{
            AsHtmlString, AttributeValue, ModelAttributes, ModelHtmlString, ModelMetadata, ModelObject,
        },
        support::{HtmlString, Htmlable},
    };
    use serde_json::json;

    let mut metadata = ModelMetadata::new("insert_items", "HtmlItem");
    metadata.casts.insert("label".into(), AsHtmlString::cast());
    let attrs = ModelAttributes::default();
    let html = "<strong>标签</strong>";
    let value = ModelObject::new(HtmlString::new(html));
    attrs.set_attribute_value(&metadata, "label", value.clone().into()).await?;
    let typed = attrs.get_attribute_value(&metadata, "label")?;
    check(typed.as_ref().and_then(|field| field.as_object()).is_some_and(|field| field.same_object(&value)))?;
    check(Htmlable::to_html(&HtmlString::new(html)) == html)?;
    check(attrs.attributes_to_array(&metadata)?["label"].as_json()? == &json!(html))?;
    check(attrs.attributes_to_json(&metadata)?["label"] == json!(html))?;
    for raw in [json!(false), json!({"nested":[]})] {
        attrs.set_raw_attribute("label", raw.clone())?;
        let typed = attrs.get_attribute_value(&metadata, "label")?;
        let snapshot = typed.as_ref().and_then(AttributeValue::as_object).map(ModelObject::snapshot).transpose()?;
        let model_html = snapshot.as_deref().and_then(|value| value.as_any().downcast_ref::<ModelHtmlString>());
        check(model_html.is_some_and(|value| value.to_html().as_json().ok() == Some(&raw)))?;
        if raw.is_object() {
            check(attrs.attributes_to_array(&metadata).is_err())?;
            check(attrs.get_attribute_value(&metadata, "label").is_err())?;
        } else {
            check(attrs.attributes_to_json(&metadata)?["label"] == json!(""))?;
        }
    }
    Ok(())
}

async fn check_generated_date_string_format() -> Result<()> {
    use openrust::{
        database::eloquent::{AsHtmlString, ModelAttributes, ModelDate, ModelMetadata},
        support::{CalendarContext, DateConstruction, DateStringFormat},
    };
    use serde_json::json;

    let context = CalendarContext::default();
    context.set_date_string_format(Some(DateStringFormat::pattern("Y/m/d H:i")))?;
    let date = context.make_date(DateConstruction::Parse(&json!("2026-11-08T13:00:00+08:00")))?
        .ok_or_else(|| Error::custom("generated_date_string_missing", "缺少生成应用日期"))?;
    context.set_date_string_format(Some(DateStringFormat::pattern("d-m-Y H:i")))?;
    check(date.to_string_value()? == "2026/11/08 13:00")?;
    let date = date.with_string_format(None);
    let attrs = ModelAttributes::default();
    let mut metadata = ModelMetadata::new("insert_items", "DateLabel");
    metadata.casts.insert("label".into(), AsHtmlString::cast());
    attrs.set_raw_attribute_value("label", ModelDate::new(date, true).into())?;
    check(attrs.attributes_to_json(&metadata)?["label"] == json!("08-11-2026 13:00"))
}

async fn check_generated_as_stringable() -> Result<()> {
    use openrust::{
        database::eloquent::{AsStringable, ModelAttributes, ModelMetadata, ModelObject},
        support::{DataObject, Stringable},
    };
    use serde_json::json;
    struct Label;
    impl DataObject for Label {
        fn as_any(&self) -> &dyn std::any::Any { self }
        fn string_projection(&self) -> Option<String> { Some("custom".into()) }
    }
    let mut model = ModelMetadata::new("insert_items", "StringableRecord");
    model.casts.insert("label".into(), AsStringable::cast());
    let attrs = ModelAttributes::default();
    attrs.set_attribute_value(&model, "label", ModelObject::new(Label).into()).await?;
    check(attrs.raw_attribute("label")? == Some(json!("custom")))?;
    let value = ModelObject::new(Stringable::from("  generated  "));
    attrs.set_attribute_value(&model, "label", value.clone().into()).await?;
    check(attrs.attributes_to_array(&model)?["label"].as_object().is_some_and(|read| read.same_object(&value)))?;
    value.modify::<Stringable>(|text| Ok(text.clone().trim()))?;
    check(attrs.attributes_to_json(&model)?["label"] == json!("generated"))?;
    check(attrs.raw_attribute("label")? == Some(json!("generated")))
}

async fn check_generated_json_codec() -> Result<()> {
    use openrust::database::eloquent::{
        AsArrayObject, AsEnumArrayObject, AsFluent, AttributeCast, EnumCast, Json, ModelAttributes,
        ModelMetadata,
    };
    use serde_json::{Value as JsonValue, json};
    use std::sync::Mutex;

    let _reset = ResetJsonCodec;
    let calls = Arc::new(Mutex::new(Vec::new()));
    let writes = Arc::clone(&calls);
    Json::encode_using(Some(Arc::new(move |value, flags| {
        writes.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(json!(["encode", flags]));
        Json::encode_default(value, flags)
    })));
    let reads = Arc::clone(&calls);
    Json::decode_using(Some(Arc::new(move |value, associative| {
        reads.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(json!(["decode", value, associative]));
        Ok(json!({"seed":true}))
    })));
    let mut model = ModelMetadata::new("insert_items", "CodecRecord");
    model.casts.insert("data".into(), AttributeCast::JsonUnicode);
    model.casts.insert("data->label".into(), AttributeCast::Array);
    let attrs = ModelAttributes::default();
    attrs.set_raw_attribute("data", json!(r#"{"persisted":1}"#))?;
    attrs.set_attribute(&model, "data->label", json!("中文")).await?;
    check(attrs.raw_attribute("data")? == Some(json!(r#"{"seed":true,"label":"\"\\u4e2d\\u6587\""}"#)))?;
    check(calls.lock().unwrap_or_else(std::sync::PoisonError::into_inner).as_slice() == [
        json!(["encode", 0]), json!(["decode", r#"{"persisted":1}"#, true]), json!(["encode",256]),
    ])?;

    model.casts.insert("data".into(), AttributeCast::Object);
    attrs.set_raw_attribute("data", json!(true))?;
    check(attrs.get_attribute(&model, "data")? == Some(json!({"seed":true})))?;
    check(calls.lock().unwrap_or_else(std::sync::PoisonError::into_inner).last() == Some(&json!(["decode",true,false])))?;
    attrs.sync_original()?;
    attrs.set_raw_attribute("data", json!("different"))?;
    check(attrs.is_clean(&model, &["data"])?)?;
    attrs.set_raw_attribute("data", json!(""))?;
    check(attrs.get_attribute(&model, "data")? == Some(JsonValue::Null))?;
    model.casts.insert("data".into(), AsArrayObject::cast());
    check(attrs.get_attribute_value(&model, "data")?.is_some_and(|v| v.as_object().is_some()))?;
    check(calls.lock().unwrap_or_else(std::sync::PoisonError::into_inner).last() == Some(&json!(["decode","",true])))?;

    model.casts.insert("data".into(), AsFluent::cast());
    let fluent = ModelAttributes::default();
    fluent.set_raw_attribute("data", json!(""))?;
    check(fluent.attributes_to_json(&model)?["data"] == json!({"seed":true}))?;
    check(calls.lock().unwrap_or_else(std::sync::PoisonError::into_inner).last() == Some(&json!(["decode","",true])))?;

    Json::encode_using(Some(Arc::new(|_, _| Ok(JsonValue::Bool(false)))));
    let attrs = ModelAttributes::default();
    let result = attrs.set_attribute(&model, "data", json!([1])).await;
    check(matches!(result, Err(error) if error.code() == "model_json_encoding_failed"))?;
    let cases = EnumCast::unit(
        |name| (name == "Ready").then_some(()),
        |()| "Ready",
    );
    model.casts.insert("data".into(), AsEnumArrayObject::of(cases));
    attrs.set_attribute(&model, "data", json!(["Ready"])).await?;
    check(attrs.raw_attribute("data")? == Some(JsonValue::Bool(false)))?;
    Json::decode_using(Some(Arc::new(|_, _| {
        Json::decode_using(None);
        Ok(json!("first"))
    })));
    check(Json::decode(&json!(r#""default""#), Some(true))? == json!("first"))?;
    check(Json::decode(&json!(r#""default""#), Some(true))? == json!("default"))?;
    Ok(())
}

fn check_generated_json_flags() -> Result<()> {
    use openrust::database::eloquent::Json;
    use serde_json::json;

    let _reset = ResetJsonCodec;
    Json::encode_using(Some(Arc::new(Json::encode_default)));
    let value = json!({"01":["0042","/中文"," \" ",1.0]});
    let flags = Json::FORCE_OBJECT | Json::NUMERIC_CHECK | Json::PRETTY_PRINT
        | Json::UNESCAPED_UNICODE | Json::UNESCAPED_SLASHES | Json::PRESERVE_ZERO_FRACTION
        | Json::HEX_QUOT;
    let expected = r#"{
    "01": {
        "0": 42,
        "1": "/中文",
        "2": " \u0022 ",
        "3": 1.0
    }
}"#;
    check(Json::encode(&value, flags)? == json!(expected))
}

async fn rollback_insert(db: &Db) -> Result<()> {
    let transaction = db.connection()?.begin().await.map_err(Error::internal)?;
    let outcome: Result<()> = async {
        let query = db.table("insert_items")?.using_transaction(&transaction);
        check(
            query
                .insert_get_id(vec![("label", "rolled-back".into())], None)
                .await?
                == Value::BigInt(Some(4)),
        )?;
        check(query.count().await? == 4)
    }
    .await;
    transaction.rollback().await.map_err(Error::internal)?;
    outcome
}

async fn backend_ids(db: &Db) -> Result<()> {
    let connection = db.connection()?;
    match db.backend()? {
        DbBackend::Sqlite => {
            connection
                .execute_unprepared("CREATE TABLE signed_insert_items (id INTEGER PRIMARY KEY)")
                .await
                .map_err(Error::internal)?;
            let query = db.table("signed_insert_items")?;
            check(
                query
                    .insert_get_id(vec![("id", (-10_i64).into())], None)
                    .await?
                    == Value::BigInt(Some(-10)),
            )?;
            check(
                query
                    .insert_get_id(Vec::new(), Some("ignored_sequence"))
                    .await?
                    == Value::BigInt(Some(-9)),
            )
        }
        DbBackend::Postgres => {
            connection
                .execute_unprepared("CREATE TABLE text_insert_items (custom_key TEXT PRIMARY KEY)")
                .await
                .map_err(Error::internal)?;
            let query = db.table("text_insert_items")?;
            check(
                query
                    .insert_get_id(vec![("custom_key", "000123".into())], Some("custom_key"))
                    .await?
                    == Value::BigInt(Some(123)),
            )?;
            check(
                query
                    .insert_get_id(vec![("custom_key", "key:abc".into())], Some("custom_key"))
                    .await?
                    == Value::from("key:abc"),
            )
        }
        DbBackend::MySql => check(
            db.table("insert_items")?
                .insert_get_id(Vec::new(), Some("ignored_sequence"))
                .await?
                == Value::BigInt(Some(5)),
        ),
        _ => Err(Error::custom(
            "generated_insert_id_backend",
            "测试需要受支持的数据库",
        )),
    }
}

struct GeneratedModel {
    metadata: openrust::database::eloquent::ModelMetadata,
    attributes: openrust::database::eloquent::ModelAttributes,
    relations: openrust::database::eloquent::ModelRelations,
    exists: bool,
}

impl openrust::database::eloquent::SaveModel for GeneratedModel {
    fn metadata(&self) -> &openrust::database::eloquent::ModelMetadata {
        &self.metadata
    }

    fn attributes(&self) -> &openrust::database::eloquent::ModelAttributes {
        &self.attributes
    }

    fn attributes_mut(&mut self) -> &mut openrust::database::eloquent::ModelAttributes {
        &mut self.attributes
    }

    fn exists(&self) -> bool {
        self.exists
    }

    fn mark_created(&mut self) {
        self.exists = true;
    }

    fn bind_attribute(&self, name: &str, value: &serde_json::Value) -> Result<Value> {
        match name {
            "id" | "owner_id" | "parent_id" | "item_id" => value.as_i64().map(Value::from),
            "label" | "updated_at" | "tag" => value.as_str().map(Value::from),
            _ => None,
        }
        .ok_or_else(|| Error::custom("generated_model_binding", "生成应用模型列类型错误"))
    }
}

impl openrust::database::eloquent::CustomPivot for GeneratedModel {
    fn from_raw_pivot(
        attributes: openrust::database::eloquent::ModelAttributes,
        source: openrust::database::eloquent::PivotSource<'_>,
        exists: bool,
    ) -> Result<Self> {
        use openrust::database::eloquent::ModelMetadata;

        let mut metadata = ModelMetadata::new(source.keys.table, "generated::Membership");
        metadata.connection = source.parent.connection.clone();
        metadata.incrementing = false;
        metadata.timestamps = false;
        metadata.fillable(vec!["tag".into()]);
        Ok(Self {
            metadata,
            attributes,
            relations: Default::default(),
            exists,
        })
    }

    fn mark_pivot_deleted(&mut self) {
        self.exists = false;
    }

    fn raw_pivot_attribute(name: &str, value: Value) -> Result<serde_json::Value> {
        match (name, value) {
            ("parent_id" | "item_id", Value::BigInt(Some(value))) => Ok(value.into()),
            ("tag", Value::String(Some(value))) => Ok(value.into()),
            _ => Err(Error::custom(
                "generated_custom_pivot_input",
                "生成中间表列类型错误",
            )),
        }
    }
}

impl openrust::database::eloquent::HydrateModel for GeneratedModel {
    fn hydrate(
        &self,
        raw: serde_json::Map<String, serde_json::Value>,
        connection: Option<&str>,
    ) -> Result<Self> {
        let mut metadata = self.metadata.clone();
        metadata.connection = connection.map(str::to_owned);
        Ok(Self {
            metadata,
            attributes: openrust::database::eloquent::ModelAttributes::new(raw),
            relations: Default::default(),
            exists: true,
        })
    }

    fn load_relation<'a>(
        models: &'a mut [Self],
        relation: openrust::database::eloquent::RelationLoad<'a>,
        context: &'a openrust::database::eloquent::SaveContext<'_>,
        transaction: Option<&'a openrust::database::TransactionContext<'_>>,
    ) -> openrust::support::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            if relation.name != "items" {
                return Err(Error::custom(
                    "generated_relation_unavailable",
                    "生成夹具未声明该关系加载",
                ));
            }
            let Some(model) = models.first() else {
                return Ok(());
            };
            let mut related = model.hydrate(Default::default(), None)?;
            related.metadata.table = "relation_items".into();
            openrust::database::eloquent::HasMany::new(&related, "owner_id", "id")
                .load_with(models, relation, context, transaction, Ok)
                .await
        })
    }
}

impl openrust::database::eloquent::HasRelationships for GeneratedModel {
    fn relations(&self) -> &openrust::database::eloquent::ModelRelations {
        &self.relations
    }

    fn relations_mut(&mut self) -> &mut openrust::database::eloquent::ModelRelations {
        &mut self.relations
    }
}

async fn check_generated_has_many(
    db: &Db,
    context: &openrust::database::eloquent::SaveContext<'_>,
) -> Result<()> {
    use openrust::database::{
        SortDirection,
        eloquent::{HasMany, ModelAttributes, ModelMetadata},
    };
    use serde_json::{Map, json};

    let connection = db.connection()?;
    connection
        .execute_unprepared("CREATE TABLE relation_items (id BIGINT PRIMARY KEY, owner_id BIGINT, label VARCHAR(100), rank_value BIGINT NULL, updated_at VARCHAR(25) NULL)")
        .await
        .map_err(Error::internal)?;
    connection
        .execute_unprepared("INSERT INTO relation_items (id, owner_id, label) VALUES (1, 7, 'a'), (2, 7, 'b'), (3, 8, 'c'), (4, 20, 'other'), (5, NULL, 'none')")
        .await
        .map_err(Error::internal)?;
    let make_model = |table, class_name, key: serde_json::Value| GeneratedModel {
        metadata: ModelMetadata::new(table, class_name),
        attributes: ModelAttributes::new(Map::from_iter([("id".into(), key)])),
        relations: Default::default(),
        exists: true,
    };
    let related = make_model("relation_items", "generated::InsertItem", json!(null));
    let mut parents = [json!(7), json!(8), json!(7), json!(9), json!(null)]
        .map(|key| make_model("insert_items", "generated::Owner", key));
    for parent in &mut parents {
        parent.metadata.connection = Some("other".into());
    }
    let observed = Arc::new(AtomicUsize::new(0));
    let calls = Arc::clone(&observed);
    let has_many = HasMany::new(&related, "owner_id", "id");
    has_many
        .load(&mut parents, "items", context, move |query| {
            query.order_by("id", SortDirection::Desc).map(|query| {
                query.before_query(move |_| {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                })
            })
        })
        .await?;
    check(observed.load(Ordering::SeqCst) == 1)?;
    for (parent, expected) in parents.iter().zip([&[2, 1][..], &[3], &[2, 1], &[], &[]]) {
        let children = parent
            .relations
            .get_relation("items")
            .and_then(|value| value.downcast_ref::<Vec<Arc<GeneratedModel>>>())
            .ok_or_else(|| Error::custom("generated_has_many_unloaded", "关联集合未加载"))?;
        check(children.len() == expected.len())?;
        for (child, id) in children.iter().zip(expected) {
            check(child.exists && child.attributes.raw_attribute("id")? == Some(json!(id)))?;
            check(child.metadata.connection.as_deref() == Some("other"))?;
            check(child.attributes.raw_attribute("label")? == Some(json!("监听器读取")))?;
        }
    }
    check_generated_has_one(&related, &mut parents, context).await?;
    check_generated_one_of_many(db, &related, &mut parents, context).await?;
    check_generated_belongs_to_many(db, &related, &mut parents, context).await?;
    check_generated_belongs_to(&related, &mut parents, context).await
}

async fn check_generated_belongs_to_many(
    db: &Db,
    related: &GeneratedModel,
    parents: &mut [GeneratedModel],
    context: &openrust::database::eloquent::SaveContext<'_>,
) -> Result<()> {
    use openrust::database::eloquent::{
        BelongsToMany, LoadedRelation, Pivot, PivotKeys, RelationLoad,
    };
    use serde_json::json;

    let connection = db.connection()?;
    connection
        .execute_unprepared(
            "CREATE TABLE relation_links (parent_id BIGINT, item_id BIGINT, tag VARCHAR(100), linked_at VARCHAR(25) NULL, changed_at VARCHAR(25) NULL)",
        )
        .await
        .map_err(Error::internal)?;
    connection.execute_unprepared("INSERT INTO relation_links (parent_id, item_id, tag) VALUES (7, 1, 'first'), (8, 1, 'second')").await.map_err(Error::internal)?;
    let observed = Arc::new(AtomicUsize::new(0));
    let calls = Arc::clone(&observed);
    BelongsToMany::new(
        related,
        PivotKeys {
            table: "relation_links",
            parent_foreign_key: "parent_id",
            related_foreign_key: "item_id",
            parent_key: "id",
            related_key: "id",
        },
    )
    .with_pivot(&["tag"])
    .with_pivot_accessor("membership")
    .load(
        parents,
        RelationLoad {
            name: "linked_items",
            nested: &[],
        },
        context,
        move |query| {
            Ok(query.before_query(move |_| {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }))
        },
    )
    .await?;
    check(observed.load(Ordering::SeqCst) == 1)?;
    for (parent, expected) in
        parents
            .iter()
            .zip([Some("first"), Some("second"), Some("first"), None, None])
    {
        let items = parent
            .relations
            .get_relation("linked_items")
            .and_then(LoadedRelation::downcast_ref::<Vec<Arc<GeneratedModel>>>)
            .ok_or_else(|| Error::custom("generated_pivot_relation_missing", "多对多关系未加载"))?;
        if let Some(tag) = expected {
            check(items.len() == 1)?;
            let item = items
                .first()
                .ok_or_else(|| Error::custom("generated_pivot_model_missing", "缺少关联模型"))?;
            check(item.attributes.raw_attribute("id")? == Some(json!(1)))?;
            check(item.attributes.raw_attribute("pivot_tag")?.is_none())?;
            check(item.relations.refreshable_names().next().is_none())?;
            let pivot = item
                .relations
                .get_relation("membership")
                .and_then(LoadedRelation::downcast_ref::<Pivot>)
                .ok_or_else(|| Error::custom("generated_pivot_missing", "缺少中间表数据"))?;
            check(pivot.attributes().get("tag") == Some(&json!(tag)))?;
        } else {
            check(items.is_empty())?;
        }
    }
    check_generated_custom_pivot(related, parents, context).await?;
    check_generated_pivot_transaction(db, related, context).await?;
    check_generated_pivot_writes(db, related, context).await?;
    check_generated_pivot_sync(db, related, context).await?;
    check_generated_pivot_toggle(db, related, context).await?;
    check_generated_pivot_timestamps(db, related, context).await?;
    check_generated_pivot_touch(db, related, context).await?;
    check_generated_pivot_parent_touch(db, related, context).await?;
    check_generated_custom_pivot_insert(db, related, context).await?;
    check_generated_custom_pivot_update(db, related, context).await?;
    check_generated_custom_pivot_delete(db, related, context).await?;
    check_generated_pivot_value(db, related, context).await?;
    check_generated_pivot_touch_transaction_source(db, related, context).await
}

async fn check_generated_pivot_touch_transaction_source(
    db: &Db,
    related: &GeneratedModel,
    context: &openrust::database::eloquent::SaveContext<'_>,
) -> Result<()> {
    use openrust::{
        database::eloquent::{BelongsToMany, HydrateModel, PivotKeys, SaveContext},
        support::CalendarContext,
    };
    use serde_json::{Map, json};

    let mut parent = related.hydrate(Map::from_iter([("id".into(), json!(6))]), None)?;
    parent.metadata.timestamps = true;
    let related = related.hydrate(Map::new(), Some("other"))?;
    let databases = (*context.databases).clone();
    let other = related.metadata.resolve_connection(&databases, None)?;
    other.transaction_for_update_checked_context(move |transaction| {
        Box::pin(async move {
            let calendar = CalendarContext::default();
            let context = SaveContext::new(&databases, None, &calendar);
            let relation = BelongsToMany::new(&related, PivotKeys {
                table: "relation_links", parent_foreign_key: "parent_id", related_foreign_key: "item_id",
                parent_key: "id", related_key: "id",
            }).with_touching_parent();
            let result = relation.attach(&mut parent, &json!(4), Vec::new(), &context, Some(&transaction)).await;
            check(matches!(result, Err(error) if error.code() == "model_relation_transaction_source_mismatch"))
        })
    }, |_| Box::pin(async { Ok(()) })).await?;
    check(
        db.table("relation_links")?
            .where_eq("parent_id", 6_i64)?
            .where_eq("item_id", 4_i64)?
            .count()
            .await?
            == 0,
    )
}

async fn check_generated_pivot_value(
    db: &Db,
    related: &GeneratedModel,
    context: &openrust::database::eloquent::SaveContext<'_>,
) -> Result<()> {
    use openrust::database::eloquent::{
        BelongsToMany, HydrateModel, LoadedRelation, PivotKeys, RelationLoad,
    };
    use serde_json::{Map, json};
    let mut parent = related.hydrate(Map::from_iter([("id".into(), json!(6))]), None)?;
    let relation = BelongsToMany::new(
        related,
        PivotKeys {
            table: "relation_links",
            parent_foreign_key: "parent_id",
            related_foreign_key: "item_id",
            parent_key: "id",
            related_key: "id",
        },
    )
    .with_pivot_value("tag", "selected")?;
    relation
        .attach(&mut parent, &json!(4), Vec::new(), context, None)
        .await?;
    relation
        .attach(
            &mut parent,
            &json!(5),
            vec![("tag", "override".into())],
            context,
            None,
        )
        .await?;
    relation
        .load(
            std::slice::from_mut(&mut parent),
            RelationLoad {
                name: "selected",
                nested: &[],
            },
            context,
            Ok,
        )
        .await?;
    let items = parent
        .relations
        .get_relation("selected")
        .and_then(LoadedRelation::downcast_ref::<Vec<Arc<GeneratedModel>>>)
        .ok_or_else(|| Error::custom("generated_pivot_value_unloaded", "中间表约束关系未加载"))?;
    check(items.len() == 1 && items[0].attributes.raw_attribute("id")? == Some(json!(4)))?;
    check(relation.detach(&mut parent, None, context, None).await? == 1)?;
    check(
        db.table("relation_links")?
            .where_eq("parent_id", 6_i64)?
            .where_eq("item_id", 5_i64)?
            .value::<String>("tag")
            .await?
            .as_deref()
            == Some("override"),
    )
}

async fn check_generated_custom_pivot(
    related: &GeneratedModel,
    parents: &mut [GeneratedModel],
    context: &openrust::database::eloquent::SaveContext<'_>,
) -> Result<()> {
    use openrust::database::eloquent::{BelongsToMany, LoadedRelation, PivotKeys, RelationLoad};
    use serde_json::json;

    BelongsToMany::new(
        related,
        PivotKeys {
            table: "relation_links",
            parent_foreign_key: "parent_id",
            related_foreign_key: "item_id",
            parent_key: "id",
            related_key: "id",
        },
    )
    .using::<GeneratedModel>()
    .with_pivot(&["tag"])
    .with_pivot_accessor("membership")
    .load(
        parents,
        RelationLoad {
            name: "custom_linked_items",
            nested: &[],
        },
        context,
        Ok,
    )
    .await?;
    for (parent, tag) in parents.iter().zip(["first", "second"]) {
        let pivot = parent
            .relations
            .get_relation("custom_linked_items")
            .and_then(LoadedRelation::downcast_ref::<Vec<Arc<GeneratedModel>>>)
            .and_then(|items| items.first())
            .and_then(|item| item.relations.get_relation("membership"))
            .and_then(LoadedRelation::downcast_ref::<GeneratedModel>)
            .ok_or_else(|| {
                Error::custom("generated_custom_pivot_missing", "缺少自定义中间表模型")
            })?;
        check(pivot.exists && pivot.metadata.table == "relation_links")?;
        check(pivot.metadata.connection == parent.metadata.connection)?;
        check(pivot.attributes.raw_attribute("tag")? == Some(json!(tag)))?;
        check(!pivot.attributes.is_dirty(&pivot.metadata, &[])?)?;
    }
    Ok(())
}

async fn check_generated_custom_pivot_insert(
    db: &Db,
    related: &GeneratedModel,
    context: &openrust::database::eloquent::SaveContext<'_>,
) -> Result<()> {
    use openrust::database::eloquent::{BelongsToMany, HydrateModel, PivotKeys};
    use serde_json::{Map, json};

    let mut parent = related.hydrate(Map::from_iter([("id".into(), json!(6))]), None)?;
    let relation = BelongsToMany::new(
        related,
        PivotKeys {
            table: "relation_links",
            parent_foreign_key: "parent_id",
            related_foreign_key: "item_id",
            parent_key: "id",
            related_key: "id",
        },
    )
    .using::<GeneratedModel>();
    check(
        relation
            .attach(
                &mut parent,
                &json!(5),
                vec![("tag", "reject".into())],
                context,
                None,
            )
            .await?
            == 0,
    )?;
    check(
        db.table("relation_links")?
            .where_eq("parent_id", 6_i64)?
            .where_eq("item_id", 5_i64)?
            .count()
            .await?
            == 0,
    )?;
    check(
        relation
            .attach(
                &mut parent,
                &json!(2),
                vec![("tag", "raw".into())],
                context,
                None,
            )
            .await?
            == 1,
    )?;
    let row = db
        .table("relation_links")?
        .where_eq("parent_id", 6_i64)?
        .where_eq("item_id", 2_i64)?;
    check(row.value::<String>("tag").await?.as_deref() == Some("custom"))
}

async fn check_generated_custom_pivot_update(
    db: &Db,
    related: &GeneratedModel,
    context: &openrust::database::eloquent::SaveContext<'_>,
) -> Result<()> {
    use openrust::database::eloquent::{BelongsToMany, HydrateModel, PivotKeys};
    use serde_json::{Map, json};

    let mut parent = related.hydrate(Map::from_iter([("id".into(), json!(6))]), None)?;
    let relation = BelongsToMany::new(
        related,
        PivotKeys {
            table: "relation_links",
            parent_foreign_key: "parent_id",
            related_foreign_key: "item_id",
            parent_key: "id",
            related_key: "id",
        },
    )
    .using::<GeneratedModel>();
    let update = |tag: &'static str| vec![("tag", tag.into())];
    let row = |id| {
        db.table("relation_links")?
            .where_eq("parent_id", id)?
            .where_eq("item_id", 2_i64)
    };
    let other_before = row(7_i64)?.value::<String>("tag").await?;
    for (id, tag, expected) in [(2, "custom", 0), (5, "missing", 0), (2, "changed", 1)] {
        check(
            relation
                .update_existing_pivot(&mut parent, &json!(id), update(tag), context, None)
                .await?
                == expected,
        )?;
    }
    check(row(6_i64)?.value::<String>("tag").await?.as_deref() == Some("changed"))?;
    check(row(7_i64)?.value::<String>("tag").await? == other_before)
}

async fn check_generated_custom_pivot_delete(
    db: &Db,
    related: &GeneratedModel,
    context: &openrust::database::eloquent::SaveContext<'_>,
) -> Result<()> {
    use openrust::database::eloquent::{BelongsToMany, HydrateModel, PivotKeys};
    use serde_json::{Map, json};

    let mut parent = related.hydrate(Map::from_iter([("id".into(), json!(6))]), None)?;
    let relation = BelongsToMany::new(
        related,
        PivotKeys {
            table: "relation_links",
            parent_foreign_key: "parent_id",
            related_foreign_key: "item_id",
            parent_key: "id",
            related_key: "id",
        },
    )
    .using::<GeneratedModel>();
    relation
        .attach(
            &mut parent,
            &json!(3),
            vec![("tag", "keep".into())],
            context,
            None,
        )
        .await?;
    let row = |parent, item| {
        db.table("relation_links")?
            .where_eq("parent_id", parent)?
            .where_eq("item_id", item)
    };
    let other_before = row(7_i64, 2_i64)?.count().await?;
    check(
        relation
            .detach(&mut parent, Some(&[json!(2), json!(3)]), context, None)
            .await?
            == 1,
    )?;
    check(row(6_i64, 2_i64)?.count().await? == 0)?;
    check(row(6_i64, 3_i64)?.count().await? == 1)?;
    check(row(7_i64, 2_i64)?.count().await? == other_before)?;
    check(relation.detach(&mut parent, None, context, None).await? == 1)?;
    check(row(6_i64, 3_i64)?.count().await? == 1)
}

async fn check_generated_pivot_writes(
    db: &Db,
    related: &GeneratedModel,
    context: &openrust::database::eloquent::SaveContext<'_>,
) -> Result<()> {
    use openrust::database::eloquent::{BelongsToMany, HydrateModel, PivotKeys};
    use serde_json::{Map, json};

    let mut parent = related.hydrate(Map::from_iter([("id".into(), json!(7))]), None)?;
    let relation = BelongsToMany::new(
        related,
        PivotKeys {
            table: "relation_links",
            parent_foreign_key: "parent_id",
            related_foreign_key: "item_id",
            parent_key: "id",
            related_key: "id",
        },
    );
    let rejected = relation
        .attach(
            &mut parent,
            &json!(2),
            vec![("PARENT_ID", 8_i64.into())],
            context,
            None,
        )
        .await;
    check(matches!(rejected, Err(error) if error.code() == "model_pivot_key_override"))?;
    check(
        relation
            .attach(
                &mut parent,
                &json!(2),
                vec![("tag", "added".into())],
                context,
                None,
            )
            .await?
            == 1,
    )?;
    check(
        relation
            .update_existing_pivot(
                &mut parent,
                &json!(1),
                vec![("tag", "changed".into())],
                context,
                None,
            )
            .await?
            == 1,
    )?;
    let other = db
        .table("relation_links")?
        .where_eq("parent_id", 8_i64)?
        .where_eq("item_id", 1_i64)?;
    check(other.value::<String>("tag").await?.as_deref() == Some("second"))?;
    check(
        relation
            .detach(&mut parent, Some(&[]), context, None)
            .await?
            == 0,
    )?;
    check(
        relation
            .detach(&mut parent, Some(&[json!(2)]), context, None)
            .await?
            == 1,
    )?;
    check(
        db.table("relation_links")?
            .where_eq("parent_id", 7_i64)?
            .value::<String>("tag")
            .await?
            .as_deref()
            == Some("changed"),
    )?;
    check(relation.detach(&mut parent, None, context, None).await? == 1)?;
    check(other.count().await? == 1)
}

async fn check_generated_pivot_sync(
    db: &Db,
    related: &GeneratedModel,
    context: &openrust::database::eloquent::SaveContext<'_>,
) -> Result<()> {
    use openrust::{
        database::eloquent::{
            BelongsToMany, HydrateModel, PivotAttachment, PivotKeys, SaveContext,
        },
        support::CalendarContext,
    };
    use serde_json::{Map, json};

    let mut parent = related.hydrate(Map::from_iter([("id".into(), json!(7))]), None)?;
    let prototype = related.hydrate(Map::new(), None)?;
    let databases = (*context.databases).clone();
    db.transaction_for_update_checked_context(
        move |transaction| {
            Box::pin(async move {
                let calendar = CalendarContext::default();
                let context = SaveContext::new(&databases, None, &calendar);
                let relation = BelongsToMany::new(
                    &prototype,
                    PivotKeys {
                        table: "relation_links",
                        parent_foreign_key: "parent_id",
                        related_foreign_key: "item_id",
                        parent_key: "id",
                        related_key: "id",
                    },
                );
                let entry = |id, tag: &'static str| PivotAttachment {
                    related_key: json!(id),
                    attributes: vec![("tag", tag.into())],
                };
                let inserted = relation
                    .sync(
                        &mut parent,
                        vec![entry(1, "first"), entry(2, "second")],
                        true,
                        &context,
                        &transaction,
                    )
                    .await?;
                check(inserted.attached == [json!(1), json!(2)])?;
                let changed = relation
                    .sync(
                        &mut parent,
                        vec![entry(1, "updated"), entry(3, "new"), entry(3, "last")],
                        true,
                        &context,
                        &transaction,
                    )
                    .await?;
                check(
                    changed.attached == [json!(3)]
                        && changed.detached == [json!(2)]
                        && changed.updated == [json!(1)],
                )?;
                let kept = relation
                    .sync(
                        &mut parent,
                        vec![PivotAttachment {
                            related_key: json!(3),
                            attributes: Vec::new(),
                        }],
                        false,
                        &context,
                        &transaction,
                    )
                    .await?;
                check(
                    kept.attached.is_empty() && kept.detached.is_empty() && kept.updated.is_empty(),
                )
            })
        },
        |_| Box::pin(async { Ok(()) }),
    )
    .await?;
    let own = || db.table("relation_links")?.where_eq("parent_id", 7_i64);
    check(own()?.count().await? == 2)?;
    check(
        own()?
            .where_eq("item_id", 3_i64)?
            .value::<String>("tag")
            .await?
            .as_deref()
            == Some("last"),
    )?;
    check(
        db.table("relation_links")?
            .where_eq("parent_id", 8_i64)?
            .value::<String>("tag")
            .await?
            .as_deref()
            == Some("second"),
    )
}

async fn check_generated_pivot_toggle(
    db: &Db,
    related: &GeneratedModel,
    context: &openrust::database::eloquent::SaveContext<'_>,
) -> Result<()> {
    use openrust::{
        database::eloquent::{
            BelongsToMany, HydrateModel, PivotAttachment, PivotKeys, SaveContext,
        },
        support::CalendarContext,
    };
    use serde_json::{Map, json};

    let mut parent = related.hydrate(Map::from_iter([("id".into(), json!(7))]), None)?;
    let prototype = related.hydrate(Map::new(), None)?;
    let databases = (*context.databases).clone();
    db.transaction_for_update_checked_context(
        move |transaction| {
            Box::pin(async move {
                let calendar = CalendarContext::default();
                let context = SaveContext::new(&databases, None, &calendar);
                let relation = BelongsToMany::new(
                    &prototype,
                    PivotKeys {
                        table: "relation_links",
                        parent_foreign_key: "parent_id",
                        related_foreign_key: "item_id",
                        parent_key: "id",
                        related_key: "id",
                    },
                );
                let entry = |id, tag: &'static str| PivotAttachment {
                    related_key: json!(id),
                    attributes: vec![("tag", tag.into())],
                };
                let changed = relation
                    .toggle(
                        &mut parent,
                        vec![entry(1, "ignored"), entry(2, "earlier"), entry(2, "toggle")],
                        &context,
                        &transaction,
                    )
                    .await?;
                check(changed.attached == [json!(2)] && changed.detached == [json!(1)])
            })
        },
        |_| Box::pin(async { Ok(()) }),
    )
    .await?;
    let own = || db.table("relation_links")?.where_eq("parent_id", 7_i64);
    check(own()?.where_eq("item_id", 1_i64)?.count().await? == 0)?;
    check(
        own()?
            .where_eq("item_id", 2_i64)?
            .value::<String>("tag")
            .await?
            .as_deref()
            == Some("toggle"),
    )?;
    check(
        own()?
            .where_eq("item_id", 3_i64)?
            .value::<String>("tag")
            .await?
            .as_deref()
            == Some("last"),
    )?;
    check(
        db.table("relation_links")?
            .where_eq("parent_id", 8_i64)?
            .where_eq("item_id", 1_i64)?
            .count()
            .await?
            == 1,
    )
}

async fn check_generated_pivot_timestamps(
    db: &Db,
    related: &GeneratedModel,
    context: &openrust::database::eloquent::SaveContext<'_>,
) -> Result<()> {
    use openrust::{
        database::eloquent::{BelongsToMany, HydrateModel, PivotKeys, SaveContext},
        support::{CalendarContext, CalendarInstant, FixedClock, ResolvedTimezone},
    };
    use serde_json::{Map, json};

    let mut parent = related.hydrate(Map::from_iter([("id".into(), json!(7))]), None)?;
    let relation = BelongsToMany::new(
        related,
        PivotKeys {
            table: "relation_links",
            parent_foreign_key: "parent_id",
            related_foreign_key: "item_id",
            parent_key: "id",
            related_key: "id",
        },
    )
    .with_timestamps("linked_at", "changed_at")?;
    let first = CalendarContext::new(
        ResolvedTimezone::utc(),
        Arc::new(FixedClock::new(CalendarInstant::new(1_700_000_000, 0)?)),
    );
    let second = CalendarContext::new(
        ResolvedTimezone::utc(),
        Arc::new(FixedClock::new(CalendarInstant::new(1_700_000_060, 0)?)),
    );
    let create = SaveContext::new(context.databases, None, &first);
    let update = SaveContext::new(context.databases, None, &second);
    relation
        .attach(
            &mut parent,
            &json!(4),
            vec![("linked_at", "custom".into())],
            &create,
            None,
        )
        .await?;
    relation
        .attach(&mut parent, &json!(5), Vec::new(), &create, None)
        .await?;
    let row = |id| {
        db.table("relation_links")?
            .where_eq("parent_id", 7_i64)?
            .where_eq("item_id", id)
    };
    check(
        row(4_i64)?.value::<String>("changed_at").await?.as_deref() == Some("2023-11-14 22:13:20"),
    )?;
    check(
        row(5_i64)?.value::<String>("linked_at").await?.as_deref() == Some("2023-11-14 22:13:20"),
    )?;
    check(
        row(5_i64)?.value::<String>("changed_at").await?.as_deref() == Some("2023-11-14 22:13:20"),
    )?;
    relation
        .update_existing_pivot(
            &mut parent,
            &json!(4),
            vec![("changed_at", "ignored".into())],
            &update,
            None,
        )
        .await?;
    check(row(4_i64)?.value::<String>("linked_at").await?.as_deref() == Some("custom"))?;
    check(
        row(4_i64)?.value::<String>("changed_at").await?.as_deref() == Some("2023-11-14 22:14:20"),
    )?;
    relation
        .update_existing_pivot(&mut parent, &json!(5), Vec::new(), &update, None)
        .await?;
    check(
        row(5_i64)?.value::<String>("changed_at").await?.as_deref() == Some("2023-11-14 22:14:20"),
    )
}

async fn check_generated_pivot_touch(
    db: &Db,
    related: &GeneratedModel,
    context: &openrust::database::eloquent::SaveContext<'_>,
) -> Result<()> {
    use openrust::{
        database::eloquent::{BelongsToMany, HydrateModel, PivotKeys, SaveContext},
        support::{CalendarContext, CalendarInstant, FixedClock, ResolvedTimezone},
    };
    use serde_json::{Map, json};

    db.table("relation_items")?
        .insert(vec![("id", 6_i64.into())])
        .await?;
    let mut parent = related.hydrate(Map::from_iter([("id".into(), json!(7))]), None)?;
    let mut prototype = related.hydrate(Map::new(), None)?;
    prototype.metadata.timestamps = true;
    let relation = BelongsToMany::new(
        &prototype,
        PivotKeys {
            table: "relation_links",
            parent_foreign_key: "parent_id",
            related_foreign_key: "item_id",
            parent_key: "id",
            related_key: "id",
        },
    )
    .with_touching_related();
    let first = CalendarContext::new(
        ResolvedTimezone::utc(),
        Arc::new(FixedClock::new(CalendarInstant::new(1_700_000_000, 0)?)),
    );
    let second = CalendarContext::new(
        ResolvedTimezone::utc(),
        Arc::new(FixedClock::new(CalendarInstant::new(1_700_000_060, 0)?)),
    );
    let attach = SaveContext::new(context.databases, None, &first);
    let detach = SaveContext::new(context.databases, None, &second);
    relation
        .attach(&mut parent, &json!(1), Vec::new(), &attach, None)
        .await?;
    let row = |id| db.table("relation_items")?.where_eq("id", id);
    check(
        row(1_i64)?.value::<String>("updated_at").await?.as_deref() == Some("2023-11-14 22:13:20"),
    )?;
    check(
        row(2_i64)?.value::<String>("updated_at").await?.as_deref() == Some("2023-11-14 22:13:20"),
    )?;
    check(row(6_i64)?.value::<Option<String>>("updated_at").await? == Some(None))?;
    relation
        .detach(&mut parent, Some(&[json!(1)]), &detach, None)
        .await?;
    check(
        row(1_i64)?.value::<String>("updated_at").await?.as_deref() == Some("2023-11-14 22:13:20"),
    )?;
    check(
        row(2_i64)?.value::<String>("updated_at").await?.as_deref() == Some("2023-11-14 22:14:20"),
    )
}

async fn check_generated_pivot_parent_touch(
    db: &Db,
    related: &GeneratedModel,
    context: &openrust::database::eloquent::SaveContext<'_>,
) -> Result<()> {
    use openrust::{
        database::eloquent::{BelongsToMany, HydrateModel, PivotKeys, SaveContext, SaveModel},
        support::{CalendarContext, CalendarInstant, FixedClock, ResolvedTimezone},
    };
    use serde_json::{Map, json};

    let mut parent = related.hydrate(Map::from_iter([("id".into(), json!(6))]), None)?;
    parent.metadata.timestamps = true;
    let relation = BelongsToMany::new(
        related,
        PivotKeys {
            table: "relation_links",
            parent_foreign_key: "parent_id",
            related_foreign_key: "item_id",
            parent_key: "id",
            related_key: "id",
        },
    )
    .with_touching_parent();
    let clock = CalendarContext::new(
        ResolvedTimezone::utc(),
        Arc::new(FixedClock::new(CalendarInstant::new(1_700_000_120, 0)?)),
    );
    let context = SaveContext::new(context.databases, context.events, &clock);
    let related_row = || db.table("relation_items")?.where_eq("id", 1_i64);
    let related_before = related_row()?.value::<String>("updated_at").await?;
    relation
        .attach(&mut parent, &json!(1), Vec::new(), &context, None)
        .await?;
    let timestamp = "2023-11-14 22:15:20";
    let parent_row = || db.table("relation_items")?.where_eq("id", 6_i64);
    check(
        parent_row()?
            .value::<String>("updated_at")
            .await?
            .as_deref()
            == Some(timestamp),
    )?;
    check(
        parent
            .attributes()
            .get_attribute_value(parent.metadata(), "updated_at")?
            .as_ref()
            .and_then(openrust::database::eloquent::AttributeValue::as_date)
            .map(|date| date.snapshot()?.format("Y-m-d H:i:s"))
            .transpose()?
            .as_deref()
            == Some(timestamp),
    )?;
    check(!parent.attributes().is_dirty(parent.metadata(), &[])?)?;
    check(related_row()?.value::<String>("updated_at").await? == related_before)
}

async fn check_generated_pivot_transaction(
    db: &Db,
    related: &GeneratedModel,
    context: &openrust::database::eloquent::SaveContext<'_>,
) -> Result<()> {
    use openrust::{
        database::eloquent::{
            BelongsToMany, HydrateModel, Pivot, PivotKeys, RelationLoad, SaveContext,
        },
        support::CalendarContext,
    };
    use serde_json::{Map, json};

    let mut parent = related.hydrate(Map::from_iter([("id".into(), json!(7))]), Some("other"))?;
    let prototype = related.hydrate(Map::new(), None)?;
    let databases = (*context.databases).clone();
    let events = context.events.cloned();
    let result = db.transaction_for_update_checked_context(move |transaction| {
        Box::pin(async move {
            let calendar = CalendarContext::default();
            let context = SaveContext::new(&databases, events.as_ref(), &calendar);
            let relation = BelongsToMany::new(&prototype, PivotKeys {
                table: "relation_links", parent_foreign_key: "parent_id", related_foreign_key: "item_id",
                parent_key: "id", related_key: "id",
            }).with_pivot(&["tag"]);
            let load = RelationLoad { name: "linked", nested: &[] };
            let wrong = relation.load_in_transaction(std::slice::from_mut(&mut parent), load, &context, &transaction, Ok).await;
            check(matches!(wrong, Err(error) if error.code() == "model_relation_transaction_source_mismatch"))?;
            check(!parent.relations.relation_loaded("linked"))?;
            parent.metadata.connection = None;
            databases.default_database().table("relation_links")?
                .using_transaction(transaction.transaction())
                .insert(vec![("parent_id", 7_i64.into()), ("item_id", 2_i64.into()), ("tag", "uncommitted".into())])
                .await?;
            relation.load_in_transaction(std::slice::from_mut(&mut parent), load, &context, &transaction, Ok).await?;
            let items = parent.relations.get_relation("linked")
                .and_then(|value| value.downcast_ref::<Vec<Arc<GeneratedModel>>>())
                .ok_or_else(|| Error::custom("generated_pivot_not_loaded", "事务关系未加载"))?;
            check(items.len() == 2)?;
            let item = items.iter().find(|item| item.attributes.raw_attribute("id").is_ok_and(|id| id == Some(json!(2))))
                .ok_or_else(|| Error::custom("generated_pivot_uncommitted_missing", "未读到事务内中间表行"))?;
            check(item.attributes.raw_attribute("label")? == Some(json!("事务内监听器读取")))?;
            let pivot = item.relations.get_relation("pivot").and_then(|value| value.downcast_ref::<Pivot>())
                .ok_or_else(|| Error::custom("generated_pivot_missing", "缺少事务中间表数据"))?;
            check(pivot.attributes().get("tag") == Some(&json!("uncommitted")))?;
            Err::<(), _>(Error::custom("generated_pivot_rollback", "验证中间表事务回滚"))
        })
    }, |_| Box::pin(async { Ok(()) })).await;
    check(matches!(result, Err(error) if error.code() == "generated_pivot_rollback"))?;
    check(
        db.table("relation_links")?
            .where_eq("item_id", 2_i64)?
            .count()
            .await?
            == 0,
    )
}

async fn check_generated_belongs_to(
    related: &GeneratedModel,
    records: &mut [GeneratedModel],
    context: &openrust::database::eloquent::SaveContext<'_>,
) -> Result<()> {
    use openrust::database::{
        SortDirection,
        eloquent::{BelongsTo, LoadedRelation},
    };
    use serde_json::json;

    for (record, key) in records
        .iter()
        .zip([json!(1), json!(3), json!(1), json!(999), json!(null)])
    {
        record.attributes.set_raw_attribute("owner_id", key)?;
    }
    let observed = Arc::new(AtomicUsize::new(0));
    let calls = Arc::clone(&observed);
    BelongsTo::new(related, "owner_id", None)
        .load(records, "owner", context, move |query| {
            Ok(query.before_query(move |_| {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }))
        })
        .await?;
    check(observed.load(Ordering::SeqCst) == 1)?;
    for (record, expected) in records.iter().zip([Some(1), Some(3), Some(1), None, None]) {
        let owner = record
            .relations
            .get_relation("owner")
            .ok_or_else(|| Error::custom("generated_owner_unloaded", "所属模型关系未加载"))?;
        if let Some(id) = expected {
            let owner = owner
                .downcast_ref::<Arc<GeneratedModel>>()
                .ok_or_else(|| Error::custom("generated_owner_type", "所属模型类型不符"))?;
            check(owner.attributes.raw_attribute("id")? == Some(json!(id)))?;
            check(owner.metadata.connection.as_deref() == Some("other"))?;
        } else {
            check(matches!(owner, LoadedRelation::Null))?;
        }
    }
    check_generated_belongs_to_default(related, records, context).await?;
    let record = records
        .first_mut()
        .ok_or_else(|| Error::custom("generated_record_missing", "缺少测试模型"))?;
    record.attributes.set_raw_attribute("owner_id", json!(7))?;
    BelongsTo::new(related, "owner_id", Some("owner_id"))
        .load(
            std::slice::from_mut(record),
            "custom_owner",
            context,
            |query| query.order_by("id", SortDirection::Asc),
        )
        .await?;
    let owner = record
        .relations
        .get_relation("custom_owner")
        .and_then(|value| value.downcast_ref::<Arc<GeneratedModel>>())
        .ok_or_else(|| Error::custom("generated_custom_owner_missing", "未找到自定义所属键"))?;
    check(owner.attributes.raw_attribute("id")? == Some(json!(2)))?;
    let owner = Arc::clone(owner);
    check_belongs_to_mutation(related, record, owner).await
}

async fn check_generated_belongs_to_default(
    related: &GeneratedModel,
    records: &mut [GeneratedModel],
    context: &openrust::database::eloquent::SaveContext<'_>,
) -> Result<()> {
    use openrust::database::eloquent::{BelongsTo, HydrateModel, RelationLoad};
    use serde_json::{Map, json};

    let mut calls = 0;
    BelongsTo::new(related, "owner_id", None)
        .load_with_default(
            records,
            RelationLoad {
                name: "owner_or_default",
                nested: &[],
            },
            context,
            None,
            Ok,
            |prototype, _, connection| {
                calls += 1;
                let raw = Map::from_iter([("label".into(), json!("默认所属模型"))]);
                let mut owner = prototype.hydrate(raw, connection)?;
                owner.exists = false;
                Ok(owner)
            },
        )
        .await?;
    check(calls == records.len())?;
    for (record, expected) in records.iter().zip([Some(1), Some(3), Some(1), None, None]) {
        let owner = record
            .relations
            .get_relation("owner_or_default")
            .and_then(|value| value.downcast_ref::<Arc<GeneratedModel>>())
            .ok_or_else(|| Error::custom("generated_default_owner_missing", "缺少默认所属模型"))?;
        check(owner.metadata.connection.as_deref() == Some("other"))?;
        if let Some(id) = expected {
            check(owner.exists && owner.attributes.raw_attribute("id")? == Some(json!(id)))?;
            check(owner.attributes.raw_attribute("label")? == Some(json!("监听器读取")))?;
        } else {
            check(!owner.exists && owner.attributes.raw_attribute("id")?.is_none())?;
            check(owner.attributes.raw_attribute("owner_id")?.is_none())?;
            check(owner.attributes.raw_attribute("label")? == Some(json!("默认所属模型")))?;
        }
    }
    Ok(())
}

async fn check_belongs_to_mutation(
    related: &GeneratedModel,
    record: &mut GeneratedModel,
    owner: Arc<GeneratedModel>,
) -> Result<()> {
    use openrust::database::eloquent::{BelongsTo, LoadedRelation};
    use serde_json::json;

    let relation = BelongsTo::new(related, "owner_id", Some("owner_id"));
    let original = record.attributes.get_raw_original("owner_id")?;
    relation
        .associate(record, "owner", Arc::clone(&owner))
        .await?;
    check(record.attributes.raw_attribute("owner_id")? == Some(json!(7)))?;
    check(
        record
            .relations
            .get_relation("owner")
            .and_then(LoadedRelation::downcast_ref::<Arc<GeneratedModel>>)
            .is_some_and(|loaded| Arc::ptr_eq(loaded, &owner)),
    )?;
    relation.associate_key(record, "owner", json!(8)).await?;
    check(!record.relations.relation_loaded("owner"))?;
    check(record.attributes.raw_attribute("owner_id")? == Some(json!(8)))?;
    relation.disassociate(record, "owner").await?;
    check(record.attributes.raw_attribute("owner_id")? == Some(json!(null)))?;
    check(matches!(
        record.relations.get_relation("owner"),
        Some(LoadedRelation::Null)
    ))?;
    check(record.attributes.get_raw_original("owner_id")? == original)?;

    record
        .attributes
        .register_setter("owner_id", Arc::new(RejectOwnerKey))?;
    let denied = relation.associate(record, "owner", owner).await;
    check(matches!(denied, Err(error) if error.code() == "generated_owner_setter_rejected"))?;
    check(record.attributes.raw_attribute("owner_id")? == Some(json!(null)))?;
    check(matches!(
        record.relations.get_relation("owner"),
        Some(LoadedRelation::Null)
    ))
}

struct RejectOwnerKey;

impl openrust::database::eloquent::AttributeSetter for RejectOwnerKey {
    fn set(
        &self,
        _value: openrust::database::eloquent::AttributeValue,
        _raw_snapshot: openrust::database::eloquent::AttributeMap,
    ) -> openrust::support::BoxFuture<'_, Result<openrust::database::eloquent::AttributeMap>> {
        Box::pin(async {
            Err(Error::custom(
                "generated_owner_setter_rejected",
                "测试外键 setter 拒绝赋值",
            ))
        })
    }
}

async fn check_generated_one_of_many(
    db: &Db,
    related: &GeneratedModel,
    parents: &mut [GeneratedModel],
    context: &openrust::database::eloquent::SaveContext<'_>,
) -> Result<()> {
    use openrust::database::eloquent::{HasOne, LoadedRelation, RelationAggregate};

    db.table("relation_items")?
        .where_in("id", [1_i64, 2])?
        .update(vec![("rank_value", 10_i64.into())])
        .await?;
    let relation = || HasOne::new(related, "owner_id", "id");
    for (relation, winner) in [
        (relation().latest_of_many(Some("rank_value"))?, 2),
        (relation().oldest_of_many(Some("rank_value"))?, 2),
        (
            relation()
                .latest_of_many(Some("rank_value"))?
                .where_candidates(|rows| rows.where_eq("label", "a")?.or_where_eq("label", "c")),
            1,
        ),
        (
            relation().of_many_by("rank_value", RelationAggregate::Min),
            1,
        ),
        (
            relation().of_many(&[
                ("rank_value", RelationAggregate::Max),
                ("id", RelationAggregate::Min),
            ])?,
            1,
        ),
    ] {
        relation.load(parents, "ranked_item", context, Ok).await?;
        for (parent, expected) in parents
            .iter()
            .zip([Some(winner), None, Some(winner), None, None])
        {
            let result = parent
                .relations
                .get_relation("ranked_item")
                .ok_or_else(|| {
                    Error::custom("generated_aggregate_relation_unloaded", "聚合关系未加载")
                })?;
            if let Some(id) = expected {
                let child = result
                    .downcast_ref::<Arc<GeneratedModel>>()
                    .ok_or_else(|| {
                        Error::custom("generated_aggregate_relation_type", "聚合关系类型错误")
                    })?;
                check(child.attributes.raw_attribute("id")? == Some(serde_json::json!(id)))?;
            } else {
                check(matches!(result, LoadedRelation::Null))?;
            }
        }
    }
    Ok(())
}

async fn check_generated_has_one(
    related: &GeneratedModel,
    parents: &mut [GeneratedModel],
    context: &openrust::database::eloquent::SaveContext<'_>,
) -> Result<()> {
    use openrust::database::{
        SortDirection,
        eloquent::{HasOne, LoadedRelation},
    };
    use serde_json::json;

    let observed = Arc::new(AtomicUsize::new(0));
    let calls = Arc::clone(&observed);
    HasOne::new(related, "owner_id", "id")
        .load(parents, "first_item", context, move |query| {
            query.order_by("id", SortDirection::Desc).map(|query| {
                query.before_query(move |_| {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                })
            })
        })
        .await?;
    check(observed.load(Ordering::SeqCst) == 1)?;
    for (parent, expected) in parents.iter().zip([Some(2), Some(3), Some(2), None, None]) {
        let loaded = parent
            .relations
            .get_relation("first_item")
            .ok_or_else(|| Error::custom("generated_has_one_unloaded", "单值关系未加载"))?;
        if let Some(expected) = expected {
            let child = loaded
                .downcast_ref::<Arc<GeneratedModel>>()
                .ok_or_else(|| Error::custom("generated_has_one_type", "单值关系模型类型错误"))?;
            check(child.attributes.raw_attribute("id")? == Some(json!(expected)))?;
            check(child.attributes.raw_attribute("label")? == Some(json!("监听器读取")))?;
        } else {
            check(matches!(loaded, LoadedRelation::Null))?;
        }
    }
    check_generated_has_one_default(related, parents, context).await
}

async fn check_generated_has_one_default(
    related: &GeneratedModel,
    parents: &mut [GeneratedModel],
    context: &openrust::database::eloquent::SaveContext<'_>,
) -> Result<()> {
    use openrust::database::{
        SortDirection,
        eloquent::{HasOne, HydrateModel, RelationLoad},
    };
    use serde_json::json;

    let mut default_calls = 0;
    HasOne::new(related, "owner_id", "id")
        .load_with_default(
            parents,
            RelationLoad {
                name: "item_or_default",
                nested: &[],
            },
            context,
            None,
            |query| query.order_by("id", SortDirection::Desc),
            |prototype, parent, mut raw, connection| {
                default_calls += 1;
                check(raw.get("owner_id").cloned() == parent.attributes.raw_attribute("id")?)?;
                raw.insert("label".into(), json!("默认子模型"));
                let mut model = prototype.hydrate(raw, connection)?;
                model.exists = false;
                Ok(model)
            },
        )
        .await?;
    check(default_calls == parents.len())?;
    for (parent, expected) in parents.iter().zip([Some(2), Some(3), Some(2), None, None]) {
        let item = parent
            .relations
            .get_relation("item_or_default")
            .and_then(|value| value.downcast_ref::<Arc<GeneratedModel>>())
            .ok_or_else(|| {
                Error::custom("generated_default_item_missing", "缺少单值关系默认结果")
            })?;
        check(item.metadata.connection.as_deref() == Some("other"))?;
        if let Some(id) = expected {
            check(item.exists && item.attributes.raw_attribute("id")? == Some(json!(id)))?;
            check(item.attributes.raw_attribute("label")? == Some(json!("监听器读取")))?;
        } else {
            check(!item.exists && item.attributes.raw_attribute("id")?.is_none())?;
            check(
                item.attributes.raw_attribute("owner_id")?
                    == parent.attributes.raw_attribute("id")?,
            )?;
            check(item.attributes.raw_attribute("label")? == Some(json!("默认子模型")))?;
        }
    }
    Ok(())
}

async fn generated_model_save(url: &str) -> Result<()> {
    use openrust::database::{
        DatabaseServiceProvider,
        eloquent::{MassAssignment, ModelMetadata, SaveContext, save, save_or_fail},
    };
    use openrust::support::CalendarContext;
    use serde_json::{Map, json};

    let mut builder = openrust::foundation::ProviderBuilder::new();
    builder.provider(DatabaseServiceProvider);
    builder.provider(generated_retrieved_events());
    let source = figment::Figment::new()
        .merge(("database.url", url))
        .merge(("database.instances.other.url", url));
    let runtime = builder.start(source).await?;
    let outcome = async {
        let databases = runtime.container().resolve::<openrust::Databases>().await?;
        let db = databases.default_database();
        let calendar = CalendarContext::default();
        let events = runtime
            .container()
            .resolve::<openrust::events::Events>()
            .await?;
        let context = SaveContext::new(&databases, Some(&events), &calendar);
        let mut metadata = ModelMetadata::new("insert_items", "generated::InsertItem");
        metadata.timestamps = false;
        metadata.fillable(vec!["label".into()]);
        let mut model = GeneratedModel {
            metadata,
            attributes: Default::default(),
            relations: Default::default(),
            exists: false,
        };
        model
            .attributes
            .fill(
                &model.metadata,
                Map::from_iter([("label".into(), json!("Eloquent 创建"))]),
                &MassAssignment::new(&databases),
            )
            .await?;
        check(save(&mut model, &context).await? && model.exists)?;
        let original = model
            .attributes
            .raw_attribute("id")?
            .and_then(|value| value.as_i64())
            .ok_or_else(|| Error::custom("generated_model_id_missing", "保存未写回自增主键"))?;
        let replacement = original
            .checked_add(100)
            .ok_or_else(|| Error::custom("generated_model_id_range", "生成应用测试主键超出范围"))?;
        model
            .attributes
            .set_raw_attribute("id", json!(replacement))?;
        model
            .attributes
            .set_raw_attribute("label", json!("Eloquent 更新"))?;
        check(save_or_fail(&mut model, &context).await?)?;
        check(model.attributes.get_previous()?.get("id") == Some(&json!(original)))?;
        check(model.attributes.is_clean(&model.metadata, &[])?)?;
        check(
            db.table("insert_items")?
                .where_eq("id", original)?
                .count()
                .await?
                == 0,
        )?;
        check(
            db.table("insert_items")?
                .where_eq("id", replacement)?
                .value::<String>("label")
                .await?
                .as_deref()
                == Some("Eloquent 更新"),
        )?;
        check_generated_has_many(&db, &context).await?;
        check_generated_model_readback(
            databases.clone(),
            events.clone(),
            &context,
            model,
            original,
            replacement,
        )
        .await
    }
    .await;
    let shutdown = runtime
        .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
        .await;
    outcome?;
    shutdown
}

async fn check_generated_model_readback(
    databases: Arc<openrust::Databases>,
    events: Arc<openrust::events::Events>,
    context: &openrust::database::eloquent::SaveContext<'_>,
    mut model: GeneratedModel,
    original: i64,
    replacement: i64,
) -> Result<()> {
    use openrust::database::eloquent::refresh;
    use serde_json::{Map, json};

    let db = databases.default_database();
    let changes = model.attributes.get_changes()?;
    model
        .attributes
        .set_raw_attribute("label", json!("内存草稿"))?;
    db.table("insert_items")?
        .where_eq("id", replacement)?
        .update(vec![("label", "外部更新".into())])
        .await?;
    refresh(&mut model, context).await?;
    check(model.attributes.get_raw_original("label")? == Some(json!("监听器读取")))?;
    check(model.attributes.is_clean(&model.metadata, &[])?)?;
    check(model.attributes.get_changes()? == changes)?;
    check_model_relations(&mut model)?;
    check_fresh_model(&model, context).await?;
    check_failed_refresh(&db, &mut model, context, replacement).await?;
    model.relations.unset_relation("owner");
    check_readback_relations(&db, &mut model, context, replacement).await?;
    let mut absent = GeneratedModel {
        metadata: model.metadata.clone(),
        attributes: openrust::database::eloquent::ModelAttributes::new(Map::from_iter([
            ("id".into(), json!(original)),
            ("label".into(), json!("原基线")),
        ])),
        relations: Default::default(),
        exists: true,
    };
    absent
        .attributes
        .set_raw_attribute("label", json!("本地草稿"))?;
    generated_locked_model_refresh(databases, events, model).await?;
    let missing = refresh(&mut absent, context).await;
    check(matches!(missing, Err(error) if error.code() == "model_not_found"))?;
    check(absent.attributes.get_raw_original("label")? == Some(json!("原基线")))?;
    check(absent.attributes.raw_attribute("label")? == Some(json!("本地草稿")))
}

async fn generated_locked_model_refresh(
    databases: Arc<openrust::Databases>,
    events: Arc<openrust::events::Events>,
    mut model: GeneratedModel,
) -> Result<()> {
    use openrust::{
        database::eloquent::{SaveContext, refresh_for_update},
        support::CalendarContext,
    };
    use serde_json::json;

    let db = databases.default_database();
    model
        .attributes
        .set_raw_attribute("label", json!("锁定前草稿"))?;
    let result = db.transaction_for_update_checked_context(
        move |transaction| {
            Box::pin(async move {
                let calendar = CalendarContext::default();
                let context = SaveContext::new(&databases, Some(&events), &calendar);
                model.metadata.connection = Some("other".into());
                let wrong_source = refresh_for_update(&mut model, &context, &transaction).await;
                check(matches!(wrong_source, Err(error)
                    if error.code() == "model_refresh_transaction_source_mismatch"))?;
                check(model.attributes.raw_attribute("label")? == Some(json!("锁定前草稿")))?;
                let wrong_relation = <GeneratedModel as openrust::database::eloquent::HydrateModel>::load_relation(
                    std::slice::from_mut(&mut model),
                    openrust::database::eloquent::RelationLoad { name: "items", nested: &[] },
                    &context,
                    Some(&transaction),
                ).await;
                check(matches!(wrong_relation, Err(error)
                    if error.code() == "model_relation_transaction_source_mismatch"))?;
                model.metadata.connection = None;
                let id = model.attributes.raw_attribute("id")?.and_then(|id| id.as_i64())
                    .ok_or_else(|| Error::custom("generated_locked_key_missing", "锁定回读缺少主键"))?;
                databases.default_database().table("relation_items")?
                    .using_transaction(transaction.transaction())
                    .insert(vec![("id", 99_i64.into()), ("owner_id", id.into()), ("label", "未提交子行".into())])
                    .await?;
                refresh_for_update(&mut model, &context, &transaction).await?;
                check_locked_child(&model)?;
                check(
                    model.attributes.get_raw_original("label")? == Some(json!("事务内监听器读取")),
                )?;
                check(model.attributes.raw_attribute("label")? == Some(json!("事务内监听器读取")))?;
                check(model.attributes.is_clean(&model.metadata, &[])?)?;
                Err::<(), _>(Error::custom("generated_relation_rollback", "验证关系回读后回滚"))
            })
        },
        |_| Box::pin(async { Ok(()) }),
    )
    .await;
    check(matches!(result, Err(error) if error.code() == "generated_relation_rollback"))?;
    check(
        db.table("relation_items")?
            .where_eq("id", 99_i64)?
            .count()
            .await?
            == 0,
    )
}

fn check_locked_child(model: &GeneratedModel) -> Result<()> {
    use serde_json::json;

    let children = model
        .relations
        .get_relation("items")
        .and_then(|relation| relation.downcast_ref::<Vec<Arc<GeneratedModel>>>())
        .ok_or_else(|| Error::custom("generated_locked_relation_missing", "锁定回读未加载关系"))?;
    check(children.len() == 2)?;
    let child = children
        .iter()
        .find(|child| {
            child
                .attributes
                .raw_attribute("id")
                .is_ok_and(|id| id == Some(json!(99)))
        })
        .ok_or_else(|| {
            Error::custom("generated_uncommitted_child_missing", "未读到事务中的子行")
        })?;
    check(child.attributes.get_raw_original("label")? == Some(json!("未提交子行")))?;
    check(child.attributes.raw_attribute("label")? == Some(json!("事务内监听器读取")))
}

fn generated_retrieved_events() -> openrust::events::EventsServiceProvider {
    let mut provider = openrust::events::EventsServiceProvider::new();
    provider.listen_named_reply("eloquent.creating: generated::Membership", |payload, _| {
        Box::pin(async move {
            let pivot = payload.downcast_ref::<GeneratedModel>().ok_or_else(|| {
                Error::custom("generated_pivot_create_type", "创建事件模型类型不符")
            })?;
            check(!pivot.exists)?;
            match pivot
                .attributes
                .raw_attribute("tag")?
                .as_ref()
                .and_then(serde_json::Value::as_str)
            {
                Some("reject") => return Ok(openrust::support::DataValue::Bool(false)),
                Some("raw") => {
                    pivot
                        .attributes
                        .set_raw_attribute("tag", serde_json::json!("custom"))?;
                }
                _ => {}
            }
            Ok(openrust::support::DataValue::Null)
        })
    });
    provider.listen_named_reply(
        "eloquent.retrieved: generated::InsertItem",
        |payload, context| {
            Box::pin(async move {
                let retrieved = payload.downcast_ref::<GeneratedModel>().ok_or_else(|| {
                    Error::custom("generated_retrieved_type", "retrieved 事件模型类型不符")
                })?;
                let label = retrieved.attributes.get_raw_original("label")?;
                let transaction = context.transaction().is_some();
                if label == Some(serde_json::json!("监听器拒绝")) {
                    retrieved
                        .attributes
                        .set_raw_attribute("label", serde_json::json!("失败前的临时修改"))?;
                    return Err(Error::custom(
                        "generated_retrieved_failed",
                        "回读监听器失败",
                    ));
                }
                check(retrieved.exists)?;
                check(label == retrieved.attributes.raw_attribute("label")?)?;
                let loaded = if transaction {
                    "事务内监听器读取"
                } else {
                    "监听器读取"
                };
                retrieved
                    .attributes
                    .set_raw_attribute("label", serde_json::json!(loaded))?;
                Ok(openrust::support::DataValue::Null)
            })
        },
    );
    provider.listen_named_reply("eloquent.deleting: generated::Membership", |payload, _| {
        Box::pin(async move {
            let pivot = payload.downcast_ref::<GeneratedModel>().ok_or_else(|| {
                Error::custom("generated_pivot_delete_type", "删除事件模型类型不符")
            })?;
            check(pivot.exists)?;
            if pivot.attributes.raw_attribute("tag")? == Some(serde_json::json!("keep")) {
                return Ok(openrust::support::DataValue::Bool(false));
            }
            Ok(openrust::support::DataValue::Null)
        })
    });
    provider.listen_named_reply("eloquent.deleted: generated::Membership", |payload, _| {
        Box::pin(async move {
            let pivot = payload.downcast_ref::<GeneratedModel>().ok_or_else(|| {
                Error::custom("generated_pivot_delete_type", "删除事件模型类型不符")
            })?;
            check(!pivot.exists)?;
            Ok(openrust::support::DataValue::Null)
        })
    });
    provider
}

async fn check_failed_refresh(
    db: &Db,
    model: &mut GeneratedModel,
    context: &openrust::database::eloquent::SaveContext<'_>,
    key: i64,
) -> Result<()> {
    use openrust::database::eloquent::{fresh, refresh};
    use serde_json::json;

    let query = || db.table("insert_items")?.where_eq("id", key);
    check(query()?.value::<String>("label").await?.as_deref() == Some("外部更新"))?;
    query()?
        .update(vec![("label", "监听器拒绝".into())])
        .await?;
    let before = model.attributes.raw_attributes()?;
    let changes = model.attributes.get_changes()?;
    let previous = model.attributes.get_previous()?;
    let failed_refresh = refresh(model, context).await;
    check(matches!(failed_refresh, Err(error) if error.code() == "generated_retrieved_failed"))?;
    let failed_fresh = fresh(model, context).await;
    check(matches!(failed_fresh, Err(error) if error.code() == "generated_retrieved_failed"))?;
    check(model.attributes.raw_attributes()? == before)?;
    check(model.attributes.get_raw_original("label")? == Some(json!("监听器读取")))?;
    check(model.attributes.get_changes()? == changes)?;
    check(model.attributes.get_previous()? == previous)?;
    check(query()?.value::<String>("label").await?.as_deref() == Some("监听器拒绝"))?;
    query()?.update(vec![("label", "外部更新".into())]).await?;
    Ok(())
}

async fn check_readback_relations(
    db: &Db,
    model: &mut GeneratedModel,
    context: &openrust::database::eloquent::SaveContext<'_>,
    key: i64,
) -> Result<()> {
    use openrust::database::eloquent::{HasRelationships, LoadedRelation, fresh_with, refresh};
    use serde_json::json;

    db.table("relation_items")?
        .where_eq("id", 1_i64)?
        .update(vec![("owner_id", key.into())])
        .await?;
    db.table("relation_items")?
        .insert(vec![
            ("id", 11_i64.into()),
            ("owner_id", 1_i64.into()),
            ("label", "孙行".into()),
        ])
        .await?;
    let fresh = fresh_with(model, context, &["items", "items.items", "items.items"])
        .await?
        .ok_or_else(|| Error::custom("generated_related_fresh_missing", "回读未找到模型"))?;
    check_nested_items(&fresh)?;
    let serialized = fresh.json_serialize()?;
    check(serialized["items"][0]["items"][0]["id"] == json!(11))?;
    check(!model.relations.relation_loaded("items"))?;

    model.relations.set_relation(
        "items",
        LoadedRelation::Related(Box::new(Vec::<Arc<GeneratedModel>>::new())),
    );
    refresh(model, context).await?;
    check(
        model
            .relations
            .get_relation("items")
            .and_then(LoadedRelation::downcast_ref::<Vec<Arc<GeneratedModel>>>)
            .is_some_and(|items| items.len() == 1),
    )?;
    check(model.relations.relation_loaded("membership"))?;
    model
        .attributes
        .set_raw_attribute("label", json!("旧基线"))?;
    model.attributes.sync_original()?;
    model
        .relations
        .set_relation("unknown", LoadedRelation::Null);
    let failed = refresh(model, context).await;
    check(matches!(failed, Err(error) if error.code() == "generated_relation_unavailable"))?;
    check(model.attributes.raw_attribute("label")? == Some(json!("监听器读取")))?;
    check(model.attributes.get_raw_original("label")? == Some(json!("旧基线")))?;
    model.relations.unset_relation("unknown");
    Ok(())
}

fn check_nested_items(model: &GeneratedModel) -> Result<()> {
    use serde_json::json;

    let items = |model: &GeneratedModel| -> Result<Vec<Arc<GeneratedModel>>> {
        model
            .relations
            .get_relation("items")
            .and_then(|value| value.downcast_ref::<Vec<Arc<GeneratedModel>>>())
            .cloned()
            .ok_or_else(|| Error::custom("generated_nested_unloaded", "嵌套关系未加载"))
    };
    let children = items(model)?;
    check(children.len() == 1)?;
    let child = children
        .first()
        .ok_or_else(|| Error::custom("generated_child_missing", "缺少子模型"))?;
    let grandchildren = items(child)?;
    check(grandchildren.len() == 1)?;
    let grandchild = grandchildren
        .first()
        .ok_or_else(|| Error::custom("generated_grandchild_missing", "缺少孙模型"))?;
    check(grandchild.attributes.raw_attribute("id")? == Some(json!(11)))
}

fn check_model_relations(model: &mut GeneratedModel) -> Result<()> {
    use openrust::database::eloquent::LoadedRelation;

    let relations = &mut model.relations;
    check(!relations.relation_loaded("owner"))?;
    relations.set_relation("owner", LoadedRelation::Null);
    relations.set_relation(
        "items",
        LoadedRelation::Related(Box::new(Vec::<i64>::new())),
    );
    relations.set_relation("membership", LoadedRelation::Pivot(Box::new(7_i64)));
    check(matches!(
        relations.get_relation("owner"),
        Some(LoadedRelation::Null)
    ))?;
    check(relations.relation_loaded("items"))?;
    check(
        relations
            .get_relation("items")
            .and_then(LoadedRelation::downcast_ref::<Vec<i64>>)
            .is_some_and(Vec::is_empty),
    )?;
    check(relations.refreshable_names().eq(["owner", "items"]))?;
    relations.set_relation("owner", LoadedRelation::Related(Box::new(9_i64)));
    check(relations.refreshable_names().eq(["owner", "items"]))?;
    relations.unset_relation("items");
    check(!relations.relation_loaded("items"))?;
    check(relations.refreshable_names().eq(["owner"]))?;
    check(
        relations
            .get_relation("membership")
            .and_then(LoadedRelation::downcast_ref::<i64>)
            == Some(&7),
    )
}

async fn check_fresh_model(
    model: &GeneratedModel,
    context: &openrust::database::eloquent::SaveContext<'_>,
) -> Result<()> {
    use openrust::database::eloquent::fresh;
    use serde_json::json;

    model
        .attributes
        .set_raw_attribute("label", json!("原对象草稿"))?;
    let fresh = fresh(model, context)
        .await?
        .ok_or_else(|| Error::custom("generated_model_fresh_missing", "回读未找到模型"))?;
    check(fresh.exists)?;
    check(model.relations.relation_loaded("owner"))?;
    check(!fresh.relations.relation_loaded("owner"))?;
    check(fresh.attributes.get_raw_original("label")? == Some(json!("外部更新")))?;
    check(fresh.attributes.raw_attribute("label")? == Some(json!("监听器读取")))?;
    check(model.attributes.raw_attribute("label")? == Some(json!("原对象草稿")))
}
