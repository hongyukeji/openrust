# 配置

应用默认配置放在根 `config/*.rs`，部署值放在 `.env` 或进程环境。`config/mod.rs` 显式声明 Rust 模块，并在 `ConfigManifest` 中登记配置函数；`Application::config()` 把这份清单交给框架。

配置函数收到同一次环境快照的 `ConfigContext`，可用 `env_or` 读取字符串、用 `bool_or` 严格解析布尔值，用 `require::<T>` 读取已求值的配置。被依赖区段须先登记，例如 `app` 可读取先完成的 `server.port`。错误中不应输出部署值。

```rust
pub fn config(ctx: &openrust::config::ConfigContext)
    -> Result<openrust::config::ApiConfig, openrust::config::ConfigError>
{
    Ok(openrust::config::ApiConfig {
        prefix: ctx.env_or("API_PREFIX", "/api"),
        http_status: openrust::config::HttpStatusMode::Semantic,
    })
}
```

默认值、代码配置、部署覆盖最终进入同一 `ConfigRepository`。进程变量优先于 dotenv；配置函数不会修改进程环境。`APP_DEBUG` 默认关闭，非法布尔值会阻止启动，不应用开启调试来暴露服务端异常细节。

从应用目录执行 `openrust env`，或在根应用执行 `cargo run -- env`，可查看本次启动选定的环境，例如 `The application environment is [production].`。命令沿应用正常启动流程读取既定环境；运行期修改配置仓储不会改变这一启动身份。`env` 不接收本地参数。

TOML 是显式可选来源。只有在清单选择 `.with_toml()` 等公开来源接口时才参与加载，默认启动不扫描 `config/*.toml`。Rust 代码配置不能运行时扫描执行，修改后需重新编译。

配置缓存保存最终可序列化快照，命中后跳过 dotenv 和配置函数。不可序列化 typed 值或工厂必须明确拒绝缓存，不能静默丢失。运行期仓储写入不会自动重算配置函数或重建已解析客户端。

接口细节见仓库 `src/config/README.md`。本章只介绍配置入口与当前边界，不代表完整 Laravel Config 行为已闭合。

对应固定 Laravel 13.x 官方章节：[configuration](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/configuration.md)。本章仅说明已落地的当前入口及边界，尚未覆盖该官方章节全部能力。
