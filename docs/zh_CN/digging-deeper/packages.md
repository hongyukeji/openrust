# 扩展包开发

## 安装与服务提供者

将扩展包添加为 Cargo 依赖，再将其 Provider 加入 `bootstrap/providers.rs`。OpenRust 将 Provider 和资源注册函数编译进应用，由应用在装配时显式选择 Provider。

在 Provider 的 `register` 中绑定服务，并调用 `mergeConfigFrom(app, defaults, key)` 合并包默认值，应用配置覆盖第一层的同名键。用 `replaceConfigRecursivelyFrom(app, defaults, key)` 保留嵌套默认值，数组按索引由应用覆盖。默认值使用 JSON 对象或数组，配置键使用应用的点号路径。

## 资源

在 Provider 的 `boot` 中向应用注册资源。

| 方法 | 资源 |
| --- | --- |
| `loadRoutesFrom` | 使用 `Route` 声明路由的编译型函数。应用先注册自己的路由，再注册包路由。 |
| `loadViewsFrom` | 视图目录和命名空间。使用默认视图根目录时，`probe::welcome` 优先读取应用的 `resources/views/vendor/probe/welcome.html`，缺失时读取包视图。 |
| `loadTranslationsFrom` | 翻译目录和命名空间。`lang/vendor/probe/en/messages.json` 可覆盖 `probe::messages.title` 的英文文本，其余键继续回退到包内翻译。 |
| `loadMigrationsFrom` | 静态的编译型 `MigrationEntry` 切片。Artisan 使用应用的迁移记录执行和回滚包迁移。 |
| `commands` | 使用 `Artisan` 注册命令的编译型函数，由控制台内核启动时调用。 |

通过启动时加载的 Provider 注册包 HTTP 路由，确保路由装配时能够读取。部署时随应用分发视图和翻译文件，并将发布后的目录传给 Provider。可运行的宿主与 Provider 见[独立 crate 验收示例](https://github.com/hongyukeji/openrust/tree/master/vendor/openrust/framework/tests/fixtures/package)。

OpenRust 使用 Cargo 显式安装扩展包并注册 Provider。自动发现和 Artisan 资源发布待接入。
