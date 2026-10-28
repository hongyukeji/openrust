# TestApp 独立应用示例

此 consumer 使用公开 `Application`、`ServiceProvider` 和 `openrust::testing::TestApp`，展示同进程两个应用的服务覆盖、移除业务 Provider 后的 404 与保留健康路由，并显式关闭各应用。HTTP 请求为进程内调用，不代表 TCP 网络验收。

```sh
cargo test --manifest-path tests/consumers/test-app/Cargo.toml
```
