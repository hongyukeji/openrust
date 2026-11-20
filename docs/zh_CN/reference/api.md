# API 参考

使用指南介绍应用行为。Rustdoc 列出构建快照中公开的 Rust 模块、类型、方法、feature 条件及源码链接。

`openrust-translation-database` 提供 `DatabaseLoader`，可显式将数据库翻译加载到现有翻译器。装配、文件回退及数据库支持范围见[数据库翻译](../digging-deeper/localization.md#database-translation-loader)。

`openrust-settings` 提供 `Settings<T>`、`SettingsGroup` 和 `SettingsServiceProvider`，用于类型化数据库设置。注册、迁移、加密及事务与作用域行为见[数据库设置](../packages/settings.md)。

## 构建源码快照

使用 Python 3.11 或更新版本、Rust 1.94.0 和已提交的干净源码快照。在仓库根目录选择新的输出目录和独占 Cargo 产物目录：

```sh
python3 -B docs/scripts/build_api.py --output /tmp/openrust-api --target-dir /tmp/openrust-api-target
```

没有 Git 元数据的源码归档须加上 `--revision`，传入该归档完整的 40 位提交 SHA。命令使用锁文件，以全部 feature 构建每个 workspace 库的文档，拒绝损坏及指向私有项目的文档链接。每次重建前清理专属 target 的 Rustdoc 页面，防止已删除 API 残留；Cargo 依赖编译缓存保留。已有导出目录不会被覆盖。根应用二进制不属于库 API。

导出面向构建主机，`build-info.json` 记录目标三元组；平台专属 API 按该目标的条件编译显示。脚本使用主机目标，不采用 `CARGO_BUILD_TARGET`。全部 feature 用于展示可选 API；应用仍须在 Cargo.toml 按需启用功能。外部依赖 API 链接到对应 Rustdoc，不随产物打包。导出包含本地搜索、源码页和 `build-info.json`，记录源码提交、Rust 版本、包清单、锁文件哈希及源码指纹。

## 将 API 加入文档站

```sh
cd docs
DOCS_API_DIR=/tmp/openrust-api npm run build
```

站点先核对当前锁文件及源码指纹，再复制至 `api/`。提供导出目录时，侧栏增加 Rustdoc 入口；支持站点根路径及 `/docs/` 等部署子路径。普通构建无需 API 产物，仍可阅读本指南。单独预览导出时，用本地 HTTP 服务托管其目录并打开 `index.html`。

生成 API 和站点不会发布版本。库源码、workspace 清单或锁文件变化后须重新生成。发布产物应对应记录的源码提交，参见[版本说明](../prologue/releases.md)和[部署](../getting-started/deployment.md)。
