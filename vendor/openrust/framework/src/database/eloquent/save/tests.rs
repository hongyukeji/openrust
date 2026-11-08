use super::*;

#[test]
fn generated_key_preserves_supported_return_value_types() -> Result<()> {
    use serde_json::json;

    assert_eq!(inserted_key(sea_orm::Value::BigInt(Some(-2)))?, json!(-2));
    assert_eq!(
        inserted_key(sea_orm::Value::from("ref-0042"))?,
        json!("ref-0042")
    );
    assert_eq!(
        inserted_key(sea_orm::Value::from(vec![0_u8, 255]))?,
        json!([0, 255])
    );
    assert_eq!(inserted_key(sea_orm::Value::String(None))?, Value::Null);
    Ok(())
}

#[test]
fn mutable_sql_dates_reject_updates_until_every_binding_is_released() -> Result<()> {
    use crate::database::eloquent::ModelDate;
    use crate::support::{CalendarInstant, InputDate, ResolvedTimezone};
    let date = ModelDate::new(
        InputDate::new(CalendarInstant::new(0, 0)?, ResolvedTimezone::utc()),
        false,
    );
    let first = date.pin_for_binding()?;
    let second = date.pin_for_binding()?;
    drop(first);
    let code = date.modify(Ok).err().map(|error| error.code().to_owned());
    assert_eq!(code.as_deref(), Some("model_date_binding_pending"));
    drop(second);
    let mut overlap = None;
    let code = date
        .modify(|value| {
            overlap = date.pin_for_binding()?;
            Ok(value)
        })
        .err()
        .map(|error| error.code().to_owned());
    assert_eq!(code.as_deref(), Some("model_date_binding_pending"));
    drop(overlap);
    assert!(date.modify(Ok)?.same_object(&date));
    Ok(())
}

#[cfg(feature = "db-postgres")]
#[tokio::test]
async fn postgres_model_date_binding_uses_local_whole_seconds_as_timestamp() -> Result<()> {
    use crate::support::{CalendarInstant, InputDate, ResolvedTimezone};

    let mut options = sea_orm::ConnectOptions::new("postgres://fixture@127.0.0.1:1/openrust");
    options.connect_lazy(true).min_connections(0);
    let db = crate::Db::default();
    db.initialize(
        sea_orm::Database::connect(options)
            .await
            .map_err(crate::Error::internal)?,
    )?;
    let query = db.table("records")?;
    let date = InputDate::new(
        CalendarInstant::new(0, 654_321)?,
        ResolvedTimezone::parse("+08:00")?,
    );
    let bound = query.bind_model_date(&date)?;
    let expected = chrono::NaiveDate::from_ymd_opt(1970, 1, 1)
        .and_then(|date| date.and_hms_opt(8, 0, 0))
        .ok_or_else(|| crate::Error::custom("test_date_fixture_invalid", "测试日期无效"))?;
    assert_eq!(bound, sea_orm::Value::from(expected));
    let wide_year = chrono::NaiveDate::from_ymd_opt(10_000, 1, 1)
        .and_then(|date| date.and_hms_opt(0, 0, 0))
        .ok_or_else(|| crate::Error::custom("test_date_fixture_invalid", "测试日期无效"))?;
    let date = InputDate::new(
        CalendarInstant::new(wide_year.and_utc().timestamp(), 0)?,
        ResolvedTimezone::utc(),
    );
    assert_eq!(
        query.bind_model_date(&date)?,
        sea_orm::Value::from(wide_year)
    );
    Ok(())
}

#[cfg(any(feature = "db-mysql", feature = "db-postgres", feature = "db-sqlite"))]
mod configured_connection {
    use super::*;
    use crate::database::eloquent::{is_unguarded, model_events::test_events};
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::sync::{Arc, Mutex};

    // 查询构造需要已配置的方言。本组用例不取得连接，且模型绑定拒绝执行写入。
    async fn databases_for_query_construction() -> Result<Databases> {
        let url = if cfg!(feature = "db-mysql") {
            "mysql://fixture@127.0.0.1:1/openrust"
        } else if cfg!(feature = "db-postgres") {
            "postgres://fixture@127.0.0.1:1/openrust"
        } else {
            "sqlite::memory:"
        };
        let mut options = sea_orm::ConnectOptions::new(url);
        options.connect_lazy(true).min_connections(0);
        let connection = sea_orm::Database::connect(options)
            .await
            .map_err(crate::Error::internal)?;
        let db = crate::Db::default();
        db.initialize(connection)?;
        Ok(Databases::new(db, BTreeMap::new()))
    }

    struct Record {
        metadata: ModelMetadata,
        attributes: ModelAttributes,
        exists: bool,
    }

    impl SaveModel for Record {
        fn metadata(&self) -> &ModelMetadata {
            &self.metadata
        }

        fn attributes(&self) -> &ModelAttributes {
            &self.attributes
        }

        fn attributes_mut(&mut self) -> &mut ModelAttributes {
            &mut self.attributes
        }

        fn exists(&self) -> bool {
            self.exists
        }

        fn mark_created(&mut self) {
            self.exists = true;
        }

        fn new_unique_id(&self) -> Result<Option<String>> {
            Ok(Some("generated".into()))
        }

        fn touch_owners<'a>(
            &'a self,
            _transaction: Option<&'a DatabaseTransaction>,
        ) -> BoxFuture<'a, Result<()>> {
            Box::pin(async {
                assert!(
                    self.attributes
                        .is_dirty(&self.metadata, &["changed_by_saved"])?
                );
                self.attributes
                    .set_raw_attribute("owner_touched", json!(true))?;
                Ok(())
            })
        }

        fn bind_attribute(&self, _name: &str, _value: &Value) -> Result<sea_orm::Value> {
            Err(crate::Error::custom(
                "unexpected_binding",
                "此回归不允许执行 SQL",
            ))
        }
    }

    #[tokio::test]
    async fn cancelled_update_and_clean_save_do_not_bind_or_execute_sql() -> Result<()> {
        let mut record = Record {
            metadata: ModelMetadata::new("records", "example::Record"),
            attributes: ModelAttributes::default(),
            exists: true,
        };
        let databases = databases_for_query_construction().await?;
        let calendar = CalendarContext::default();
        let trace = Arc::new(Mutex::new(Vec::new()));
        let mut provider = crate::events::EventsServiceProvider::new();
        for (stage, response) in [("saving", true), ("updating", false), ("saved", true)] {
            let trace = trace.clone();
            provider.listen_named_reply(
                format!("eloquent.{stage}: example::Record"),
                move |payload, _| {
                    let trace = trace.clone();
                    Box::pin(async move {
                        assert!(payload.downcast_ref::<Record>().is_some());
                        trace
                            .lock()
                            .map_err(|_| {
                                crate::Error::custom("save_test_lock", "回归记录锁不可用")
                            })?
                            .push(stage);
                        Ok(DataValue::Bool(response))
                    })
                },
            );
        }
        let events = test_events(provider).await?;
        let context = SaveContext {
            databases: &databases,
            events: Some(&events),
            calendar: &calendar,
            touch: true,
        };
        assert!(save(&mut record, &context).await?);
        record
            .attributes
            .set_raw_attribute("name", Value::String("new".into()))?;
        assert!(!save(&mut record, &context).await?);
        assert!(record.attributes.is_dirty(&record.metadata, &["name"])?);
        assert!(record.attributes.raw_attribute("updated_at")?.is_none());
        assert_eq!(
            *trace
                .lock()
                .map_err(|_| crate::Error::custom("save_test_lock", "回归记录锁不可用"))?,
            ["saving", "saved", "saving", "updating"]
        );
        Ok(())
    }

    #[tokio::test]
    async fn creating_can_cancel_after_unique_id_but_before_timestamps_and_sql() -> Result<()> {
        let mut record = Record {
            metadata: ModelMetadata::new("records", "example::Record"),
            attributes: ModelAttributes::default(),
            exists: false,
        };
        record.metadata.uses_unique_ids = true;
        record.metadata.unique_id_columns.push("id".into());
        let databases = databases_for_query_construction().await?;
        let calendar = CalendarContext::default();
        let mut provider = crate::events::EventsServiceProvider::new();
        provider.listen_named_reply("eloquent.creating: example::Record", |payload, _| {
            Box::pin(async move {
                let record = payload
                    .downcast_ref::<Record>()
                    .ok_or_else(|| crate::Error::custom("save_test_payload", "模型事件类型错误"))?;
                assert_eq!(
                    record.attributes.raw_attribute("id")?,
                    Some(Value::String("generated".into()))
                );
                assert!(record.attributes.raw_attribute("created_at")?.is_none());
                assert!(!record.exists());
                Ok(DataValue::Bool(false))
            })
        });
        let events = test_events(provider).await?;
        let context = SaveContext {
            databases: &databases,
            events: Some(&events),
            calendar: &calendar,
            touch: true,
        };
        assert!(!save(&mut record, &context).await?);
        assert!(!record.exists());
        assert!(record.attributes.get_raw_original_attributes()?.is_empty());
        assert!(record.attributes.raw_attribute("id")?.is_some());
        Ok(())
    }

    #[tokio::test]
    async fn empty_nonincrementing_model_completes_without_becoming_created() -> Result<()> {
        let mut record = Record {
            metadata: ModelMetadata::new("records", "example::Record"),
            attributes: ModelAttributes::default(),
            exists: false,
        };
        record.metadata.incrementing = false;
        record.metadata.timestamps = false;
        let databases = databases_for_query_construction().await?;
        let calendar = CalendarContext::default();
        let context = SaveContext {
            databases: &databases,
            events: None,
            calendar: &calendar,
            touch: true,
        };
        assert!(save(&mut record, &context).await?);
        assert!(!record.exists());
        Ok(())
    }

    #[tokio::test]
    async fn quiet_save_skips_model_events_and_restores_them_after_the_call() -> Result<()> {
        let mut record = Record {
            metadata: ModelMetadata::new("records", "example::Record"),
            attributes: ModelAttributes::default(),
            exists: true,
        };
        let databases = databases_for_query_construction().await?;
        let calendar = CalendarContext::default();
        let mut provider = crate::events::EventsServiceProvider::new();
        provider.listen_named_reply("eloquent.saving: example::Record", |_, _| {
            Box::pin(async { Ok(DataValue::Bool(false)) })
        });
        provider.listen_named_reply("unrelated", |_, _| {
            Box::pin(async { Ok(DataValue::Bool(false)) })
        });
        let events = test_events(provider).await?;
        let context = SaveContext {
            databases: &databases,
            events: Some(&events),
            calendar: &calendar,
            touch: true,
        };
        assert!(save_quietly(&mut record, &context).await?);
        assert!(!save(&mut record, &context).await?);
        assert!(matches!(
            without_model_events(events.until_named("unrelated", &())).await?,
            DataValue::Bool(false)
        ));
        Ok(())
    }

    #[tokio::test]
    async fn update_new_model_skips_fill_and_force_create_keeps_unguarded_until_cancelled()
    -> Result<()> {
        let new_model = || {
            Ok(Record {
                metadata: ModelMetadata::new("records", "example::Record"),
                attributes: ModelAttributes::default(),
                exists: false,
            })
        };
        let databases = databases_for_query_construction().await?;
        let calendar = CalendarContext::default();
        let mut provider = crate::events::EventsServiceProvider::new();
        provider.listen_named_reply("eloquent.creating: example::Record", |payload, _| {
            Box::pin(async move {
                assert!(is_unguarded());
                let record = payload
                    .downcast_ref::<Record>()
                    .ok_or_else(|| crate::Error::custom("save_test_payload", "模型事件类型错误"))?;
                assert_eq!(
                    record.attributes.raw_attribute("name")?,
                    Some(json!("draft"))
                );
                Ok(DataValue::Bool(false))
            })
        });
        let events = test_events(provider).await?;
        let context = SaveContext {
            databases: &databases,
            events: Some(&events),
            calendar: &calendar,
            touch: true,
        };
        let assignment = MassAssignment::new(&databases);
        let input = Map::from_iter([("name".into(), json!("draft"))]);
        let mut unsaved = new_model()?;
        assert!(!update(&mut unsaved, input.clone(), &assignment, &context).await?);
        assert!(unsaved.attributes.raw_attributes()?.is_empty());
        let cancelled = force_create(new_model, input, &assignment, &context).await?;
        assert!(!cancelled.exists());
        assert_eq!(
            cancelled.attributes.raw_attribute("name")?,
            Some(json!("draft"))
        );
        assert!(!is_unguarded());
        Ok(())
    }

    #[tokio::test]
    async fn saved_listener_changes_drive_touch_before_original_is_synced() -> Result<()> {
        let databases = databases_for_query_construction().await?;
        let calendar = CalendarContext::default();
        let mut provider = crate::events::EventsServiceProvider::new();
        provider.listen_named_reply("eloquent.saved: example::Record", |payload, _| {
            Box::pin(async move {
                let record = payload
                    .downcast_ref::<Record>()
                    .ok_or_else(|| crate::Error::custom("save_test_payload", "模型事件类型错误"))?;
                record
                    .attributes
                    .set_raw_attribute("changed_by_saved", json!(true))?;
                Ok(DataValue::Null)
            })
        });
        let events = test_events(provider).await?;
        for touch in [true, false] {
            let mut model = Record {
                metadata: ModelMetadata::new("records", "example::Record"),
                attributes: ModelAttributes::default(),
                exists: true,
            };
            let context = SaveContext {
                databases: &databases,
                events: Some(&events),
                calendar: &calendar,
                touch,
            };
            assert!(save(&mut model, &context).await?);
            assert_eq!(
                model.attributes.raw_attribute("owner_touched")?,
                touch.then(|| json!(true))
            );
            assert!(model.attributes.is_clean(&model.metadata, &[])?);
            assert_eq!(
                model.attributes.get_raw_original("changed_by_saved")?,
                Some(json!(true))
            );
        }
        Ok(())
    }
}
