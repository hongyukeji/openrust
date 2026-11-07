//! 外包验证借用型默认setter、Send future及实际自定义覆盖；不读取内部状态。
#[cfg(test)]
mod tests {
    use openrust::{
        Result,
        database::eloquent::{AttributeSetter, ModelAttributes, ModelMetadata},
        support::BoxFuture,
    };
    use serde_json::{Map, Value, json};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    struct BorrowedSetter<'a> {
        calls: &'a AtomicUsize,
        prefix: &'a str,
    }
    impl AttributeSetter for BorrowedSetter<'_> {
        fn set(
            &self,
            value: Value,
            _raw: Map<String, Value>,
        ) -> BoxFuture<'_, Result<Map<String, Value>>> {
            Box::pin(async move {
                self.calls.fetch_add(1, Ordering::SeqCst);
                tokio::task::yield_now().await;
                Ok(Map::from_iter([(
                    "name".into(),
                    json!(format!(
                        "{}:{}",
                        self.prefix,
                        value.as_str().unwrap_or_default()
                    )),
                )]))
            })
        }
    }
    struct Reject;
    impl AttributeSetter for Reject {
        fn set(
            &self,
            _value: Value,
            _raw: Map<String, Value>,
        ) -> BoxFuture<'_, Result<Map<String, Value>>> {
            Box::pin(async {
                Err(openrust::Error::custom(
                    "consumer_rejected",
                    "业务setter拒绝",
                ))
            })
        }
    }
    fn send_future<T: Send>(future: T) -> T {
        future
    }

    #[tokio::test]
    async fn borrowed_default_is_send_and_custom_failure_bypasses_it() -> Result<()> {
        let attributes = ModelAttributes::new(Map::from_iter([("name".into(), json!("old"))]));
        let metadata = ModelMetadata::new("records", "Record");
        let calls = AtomicUsize::new(0);
        let prefix = String::from("default");
        let fallback = BorrowedSetter {
            calls: &calls,
            prefix: &prefix,
        };
        send_future(attributes.set_attribute_with_default_setter(
            &metadata,
            "name",
            json!("new"),
            &fallback,
        ))
        .await?;
        assert_eq!(
            attributes.raw_attribute("name")?,
            Some(json!("default:new"))
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        attributes.register_setter("name", Arc::new(Reject))?;
        let error = attributes
            .set_attribute_with_default_setter(&metadata, "name", json!("rejected"), &fallback)
            .await;
        assert!(matches!(error, Err(error) if error.code() == "consumer_rejected"));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            attributes.raw_attribute("name")?,
            Some(json!("default:new"))
        );
        Ok(())
    }
}
