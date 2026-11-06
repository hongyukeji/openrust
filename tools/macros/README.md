# macros 过程宏

`macros` package 提供 OpenRust 的过程宏；应用通过 framework package 的 `openrust` library 门面使用 `#[openrust::resource]`。

该宏标注固有 `impl`，读取其中的 `list`、`get`、`create`、`update`、`delete` 关联函数，并生成 `openrust::http::Resource` 实现。未识别的方法保留为普通辅助函数；不支持的参数、trait `impl`、接收者或动作方法泛型会产生编译错误。

框架保留 `macros` 独立 package 作为 proc-macro 编译边界，根 package 将其重新导出。根 server 集成测试验证生成的五种动作路由。`Job` 与 `Event` derive 宏尚未实现，现有运行时 trait 不代表 derive 已交付。

```rust,ignore
#[openrust::resource]
impl Posts {
    async fn list() -> Json<Vec<Post>> { /* ... */ }
    async fn get() -> Json<Post> { /* ... */ }
}
```

```bash
cargo test -p openrust-framework -p openrust
```
