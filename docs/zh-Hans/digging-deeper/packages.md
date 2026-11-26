# 扩展包开发

## 安装与服务提供者

将扩展包添加为 Cargo 依赖。OpenRust 按当前目标和特性发现已声明的包。需要显式装配时，将 Provider 加入 `bootstrap/providers.rs`。

在 Provider 的 `register` 中绑定服务，并调用 `merge_config_from(app, defaults, key)` 合并包默认值，应用配置覆盖第一层的同名键。用 `replace_config_recursively_from(app, defaults, key)` 保留嵌套默认值，数组按索引由应用覆盖。默认值使用 JSON 对象或数组，配置键使用应用的点号路径。

## 包自动发现 {#package-discovery}

扩展包通过 Cargo 清单声明发现信息，并导出 `openrust_providers() -> Vec<Box<dyn openrust::support::ServiceProvider>>`。下例也导出了一个类型化 Rust 别名。

```toml
[package.metadata.openrust.discovery]
version = 1
providers = true
aliases = { ExampleFacade = "facades::Example" }
```

OpenRust 应用在 `build.rs` 调用 `openrust_package_discovery::generate()`，通过 `with_packages` 装配生成的清单。Provider 工厂和别名都经过编译。部署后的可执行文件可以直接使用已链接的清单启动。

`bootstrap/packages.rs` 通过 `manifest()` 提供编译后的包清单，应用入口调用 `.with_packages(super::packages::manifest())`。应用自身的 Provider 仍在 `bootstrap/providers.rs` 登记；无需编辑生成文件或手动列出自动发现的包。 编译后的 Rust 别名通过 `crate::bootstrap::packages::package_aliases` 访问，可用项随启用的 feature 和包发现排除规则变化。

在应用的 `[package.metadata.openrust.discovery]` 下设置 `version = 1` 和 `dont-discover = ["example-package"]`。`["*"]` 禁用全部自动发现。包也可声明需要排除的包名。排除清单中的包仍可显式注册 Provider。

HTTP 和 Artisan 启动时直接使用已链接的清单。要检查当前构建包含哪些包，运行 `./artisan package:discover` 列出包名，并将清单导出到 `bootstrap/cache/packages.json`。用 `APP_PACKAGES_CACHE` 指定其他导出路径。运行 `./artisan clear-compiled` 可清除导出文件。依赖、特性或排除规则变化后需重新编译应用。

应用解析环境后，`get_cached_packages_path()` 返回导出路径。延迟服务提供者的目录随二进制链接。

## 资源

在 Provider 的 `boot` 中向应用注册资源。

| 方法 | 资源 |
| --- | --- |
| `load_routes_from` | 使用 `Route` 声明路由的编译型函数。应用先注册自己的路由，再注册包路由。 |
| `load_views_from` | 视图目录和命名空间。使用默认视图根目录时，`probe::welcome` 优先读取应用的 `resources/views/vendor/probe/welcome.html`，缺失时读取包视图。 |
| `load_translations_from` | 翻译目录和命名空间。`lang/vendor/probe/en/messages.json` 可覆盖 `probe::messages.title` 的英文文本，其余键继续回退到包内翻译。 |
| `load_json_translations_from` | 匿名 JSON 翻译目录。应用对应语言的 JSON 可覆盖包内同名翻译。 |
| `load_view_components_as` | 为编译型组件登记前缀与别名。例如前缀 `probe` 和别名 `badge` 渲染为 `<x-probe-badge />`。包的 `views/components` 中的匿名组件可通过 `<x-probe::badge />` 使用。 |
| `load_migrations_from` | 静态的编译型 `MigrationEntry` 切片。Artisan 使用应用的迁移记录执行和回滚包迁移。 |
| `commands` | 使用 `Artisan` 注册命令的编译型函数，由控制台内核启动时调用。 |

通过启动时加载的 Provider 注册包 HTTP 路由，确保路由装配时能够读取。部署时随应用分发视图和翻译文件，并将发布后的目录传给 Provider。可运行的宿主与 Provider 见[独立 crate 示例](https://github.com/hongyukeji/openrust/tree/master/vendor/openrust-framework/tests/fixtures/package)。

## 发布资源

嵌入文件使用 `publishes`，目标路径相对于应用根目录。本地文件或目录使用 `publishes_paths`，目标支持应用内相对路径及明确配置的绝对路径，例如 `app.lang_path("vendor/package")`。运行 `./artisan vendor:publish` 后，键入文字即可筛选 Provider、标签或全部资源。用 ↑ 和 ↓ 移动，回车确认，Esc 取消。也可用 `--provider`、`--tag` 或 `--all` 直接选择。发布目录时保留文件的相对路径。默认保留已有文件，`--force` 覆盖已有文件，`--existing` 只更新已存在的文件。

可组合 `--provider` 与一个或多个 `--tag`，按源路径筛选资源。`--all` 跨 Provider 和标签发布。`--existing` 和 `--force` 同用时也只更新已存在的文件。相对源路径从应用根目录解析，安装在其他位置的包可显式提供绝对源路径。应用内的源路径和目标路径均拒绝符号链接，迁移文件名查找也遵守此规则。
