# `openrust-translation-database`

> 受众：AI Agent
> 摘要：显式数据库翻译来源扩展；完整翻译接口边界见[翻译事实](i18n.md)。

## 机制

- 独立包依赖框架公开 `TranslationLoader`、Provider、配置、数据库与迁移机制；框架不反向依赖此包，不建立第二套 Translator。
- 文件是默认来源；数据库与文件组合均显式选择。数据库 Provider 贡献具体 Loader、`translation_database` 配置及标准迁移，不自动替换默认译器或启动后台任务。
- 首次安装保留文件模式并启用 `install_database`，标准迁移成功后再选择数据库或组合来源；显式来源捕获失败不静默回退。命名连接与表前缀沿同一数据库配置，迁移命令显式选择连接。
- 来源捕获会话构造不可变快照；同步翻译不逐次访问 SQL。修订、提交后失效提示和双消费者游标与旧快照隔离。写入回执区分提交结果、通知交付和刷新错误，不将提交后失败冒充回滚。
- 普通共享译器保留 Laravel 查找策略；错误、验证及明确需要增强的示例命令使用共享目录的 Catalog 视图。数据库空值属于合法来源值，查找策略决定最终返回键还是原值。
- 唯一示例通过 `example:translation` 命令展示读取、修改、禁用、删除与刷新，不提供匿名 HTTP 写入口。

## 边界

schema、修订与失效提示是已授权来源增强，不宣称 Laravel 内置同一数据库表。日常真实服务范围是 MySQL 8.4；其他数据库、最低版本和 Windows 文件来源运行边界留发布前验收。完整 `get`、`choice`、Loader 与 deferred 生命周期仍开放，不能以来源扩展验收关闭。

计划见[统一来源](../plans/localization-sources.md)，验收以[测试基线](../memory/testing.md)与实际 Git 提交为准。
