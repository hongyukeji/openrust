# 测试替身

## 替换容器实例 {#mocking-objects}

在测试里实现应用服务的 trait，再用 `TestApp::instance` 绑定替身。该方法引导测试应用并替换其后续解析使用的绑定。在被测代码解析服务前安装替身。其他测试应用各自保留自己的绑定。

```rust
use std::sync::Arc;
use openrust::testing::TestApp;

#[tokio::test]
async fn replaces_a_service() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut app = TestApp::create(|builder| builder)?;
    app.instance(Arc::new(String::from("test value"))).await?;

    assert_eq!(app.app().make::<String>().await?.as_str(), "test value");
    Ok(())
}
```

事件断言见[事件测试](../digging-deeper/events.md#testing)。
