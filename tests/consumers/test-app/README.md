# TestApp 独立应用示例

`tests/exception_handler.rs` 只依赖公开 `BootContext::exceptions`，以两个独立应用实例展示具体异常 report/render、原生 Response 尾处理、默认 HTML/JSON 协商及非 debug 来源隐藏。本测试代码仍待最终统一运行，不把编写视为通过。

此 consumer 使用公开 `ApplicationDefinition`、`ServiceProvider` 和 `openrust::testing::TestApp`，展示同进程两个应用的服务覆盖、移除业务 Provider 后的 404 与保留健康路由，并显式关闭各应用。HTTP 请求为进程内调用，不代表 TCP 网络验收。

```sh
cargo test --manifest-path tests/consumers/test-app/Cargo.toml
```

`tests/model_attribute_state.rs` 只依赖公开ModelAttributes/ModelMetadata，覆盖原值点路径、cast感知dirty、changes/previous及fluent同步；并调用公开 `ModelMetadata::cast_attribute` 验证读阶段原始声明检查和null短路。无需数据库或新增feature，不将该属性状态验收等同完整Model save。
