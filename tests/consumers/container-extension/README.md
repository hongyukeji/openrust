# Container extend 独立消费者

此独立 Cargo package 只依赖 `openrust` 的公开 Container API，验证外部 `dyn Trait` factory 在先登记装饰后绑定时仍执行原 factory、异步解析依赖、运行期装饰与 rebinding，以及子 scope 装饰不影响兄弟或未来 scope。

从仓库根目录运行：

```sh
cargo test --manifest-path tests/consumers/container-extension/Cargo.toml
```

它不属于根 workspace；唯一综合业务示例仍位于 `modules/example`。

## 第三方 SessionStore 装饰

启用独立 consumer 的 `session` feature 后，`original_custom_session_store_is_extended_through_managed_cookie_roundtrip` 使用公开 `SessionServiceProvider::with_store` 安装自定义原始 store。应用 Provider 对框架**原有** `SessionStoreHandle` 绑定调用 `extend`，从 `old.store()` 克隆原 `Arc<dyn SessionStore>` 并以委托型 wrapper 包装；不另建并行 store 绑定，也不重建内置 driver 枚举。wrapper 委托保证分类、认证声明、请求绑定、gc、load、write 和 delete。

测试通过受管理的 Session HTTP 路由连续发送写入请求与携带返回 Cookie 的读取请求，断言原始与装饰层的 load/write 计数一致、装饰器仅运行一次、原 store 在应用存活时保持同一实例，并在应用关闭且释放后恰好析构一次。`OriginalStore::new` 在 Provider 构造前执行；`created == 1` 只计原实例构造，**不直接计框架 singleton factory 的调用次数**。原 store 在测试中以内存后端保存记录；这是进程内 HTTP 证据，不替代真实 TCP 或 PostgreSQL/MySQL/Redis 验收。`store()` 是受信装饰器的底层逃生口；直接 I/O 绕过受管理 Session 状态/容量检查与句柄删除的 ServerSide 保证检查，调用者须自行遵守原契约，客户端快照不会因此获得全局撤销能力。

从仓库根目录运行定向测试：

```sh
cargo test --manifest-path tests/consumers/container-extension/Cargo.toml --features session original_custom_session_store_is_extended_through_managed_cookie_roundtrip
```

## 新生成应用的公开 Provider 验收

CLI 故意生成空模块清单，不复制 `modules/example`。本目录的 fixture 把一个普通应用 Provider 装入**临时生成项目**，不修改框架源码或 CLI 模板。Provider 的原 factory 返回 `"from original factory"`，`register` 的 `extend` 包装同一服务；`boot` 解析并持有最终实例，HTTP `Inject` 再解析它。

从框架仓库根目录执行；以下路径由总控选定尚不存在的临时目录：

```sh
cargo run -p cli -- new /tmp/openrust-r2x-generated-fixture
python3 tests/consumers/container-extension/fixtures/install-generated-provider.py /tmp/openrust-r2x-generated-fixture
cd /tmp/openrust-r2x-generated-fixture
cargo check
cargo test --test container_extension
cargo test
```

`generated_provider_extends_original_factory_in_boot_and_http` 对 `/api/generated/container-extension` 断言 `data.message == "from original factory"`、`data.extended == true`、`data.same_boot_instance == true`。另可在生成应用目录运行 `SERVER_PORT=3000 cargo run`，再从另一终端 `curl --noproxy '*' http://127.0.0.1:3000/api/generated/container-extension` 核对真实 TCP 响应中的同三项；默认服务配置位于可编辑的 `config/server.rs`，部署时可用 `SERVER_PORT` 指定监听端口。

安装脚本在该临时应用的 `app/providers/mod.rs` 导出 fixture Provider，并在 `bootstrap/providers.rs` 的应用清单中登记；复制 fixture 到 `app/providers/container_extension.rs` 与 `tests/container_extension.rs`。测试入口显式编译同一 `app`、`bootstrap`、`config`、`modules` 和 `routes`，经生产 Provider 清单验证原 factory、boot 与 HTTP 同实例。脚本遇到已修改的同名 fixture 文件、部分或重复登记会拒绝覆盖，重复安装相同内容不会重复注册。它只适用于当前生成骨架的固定锚点；骨架变化时显式失败。
