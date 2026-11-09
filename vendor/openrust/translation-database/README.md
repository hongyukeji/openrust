# 数据库翻译来源

`openrust-translation-database` 是显式选择的来源扩展，复用框架唯一 `TranslationLoader`、语言选择和格式化机制。登记 `DatabaseTranslationServiceProvider` 只贡献具体 Loader、`translation_database` 配置及标准迁移，不自动改变应用翻译来源，不启动后台守护。

应用首先保留文件或静态目录来源，注册数据库 Provider 后运行标准 `migrate` 完成表结构，再通过 `TranslationServiceProvider::from_loader_factory` 选择数据库或 `CompositeTranslationLoader`。此安装顺序避免让尚未建表的数据库来源阻塞首次迁移；来源初始捕获失败明确报错，不静默回退到文件。命名连接来自同一 `Databases` 配置，不另传URL或凭据；迁移时须显式使用 `migrate --database <连接名>`，选择 Loader 的连接配置不会自动改写命令目标。

消息更新、修订和提交共用同一数据库事务；提交成功后通过公开 `Events::dispatch` 与共享缓存发送失效提示，尊重应用显式登记的直接或队列监听器；不隐式启动队列或worker。通知失败可重试，不回滚已提交数据。队列通知在原事务提交后入列，不与原写入再次原子；成功回执只表示通道接受提示，不表示worker或远端完成，重试可能重复，消费者按修订处理幂等。普通翻译只读已发布快照，有界显式刷新或修订检查才访问SQL；每个消费者保存自己的成功发布游标，其他消费者捕获不能消费本方进度。

数据库schema、修订及失效提示属于已授权来源增强，并非Laravel内置表结构。日常真实服务范围为 MySQL 8.4；其他数据库及最低支持版本的运行兼容性须在发布前验证。Windows文件来源的原子打开边界仍由核心来源计划保留。


## 显式翻译来源（Source）

框架默认 `TranslationServiceProvider` 仍不代表 Laravel 默认 FileLoader、locale/deferred 生命周期已经等价；官方 `get`/`choice`/Loader 完整接口继续保留对应缺口。应用安装器只选择来源，复用唯一 Container `Translator` 并保留普通 Laravel 查找默认；Files、Database 或 Composite 都不会隐式切换查找策略。明确业务需要跨语言全局 JSON 回退或原样空值时，使用该译器的 `clone().with_lookup(TranslationLookup::Catalog)` 视图；错误渲染与验证也复用同一目录。

根应用和新生成应用在 `config/translation_sources.rs` 编辑 `SourceSettings`。默认 `files` 读取 `lang`，无需数据库扩展、Redis 或后台任务。文件和显式数据库来源共用 `app.locale` 与 `app.fallback_locale`。显式数据库初始捕获失败会阻止启动，不静默当成文件来源成功。

启用 `translation-database` 和相应 `db-mysql` 后，首次安装先保持 `mode: Files` 并设 `install_database: true`，运行标准 `migrate`；成功后改 `Database` 或 `Composite`。命名连接使用 `translation_database.connection` 与同一 `Databases`，迁移显式传 `migrate --database <连接名>`，Loader 连接不会改写命令目标。不要在新库尚未建表时直接选择数据库来源。

组合顺序固定为 `[files, database]`：同一 locale/resource/key 后者覆盖前者；空串是有效资源值，禁用/删除表示该来源缺失。以下平面 JSON 返回示例使用显式 Catalog 业务视图；普通 Laravel 查找仍遵循其 JSON falsey 和 group 回退规则。先在请求的精确语言查全部来源，再按既有语言回退顺序查询。例如 `fr` 文件为 `Bonjour`、`en` 数据库为 `Hello` 时，显式 `fr` 返回 `Bonjour`。`en` 文件 `label=file`、数据库 `label=db` 返回 `db`；数据库值改空串返回空串；禁用/删除后返回 `file`。模块文件别名仅补缺，不能覆盖已捕获 Source 的精确全局键或 `namespace::key`；数据库删除并发布下一快照后恢复模块文件。JSON 字面键优先规则保持原有公开行为。

唯一模块示例使用命名空间 `example-source`，配置 `namespaces` 映射到 `modules/example/lang/source-overrides`；编译数据库扩展并登记来源后可运行 `example:translation show [locale]`、`set locale text`、`disable locale`、`delete locale`、`refresh`。该命令是运维入口，没有匿名 HTTP 写入口。写操作回执区分已提交修订、通知交付和提交后刷新错误，刷新失败不伪装数据库已回滚。

同步翻译仅访问已发布快照；HTTP `response_context` 在取得请求槽后、共享绝对期限及取消信号内核对刷新，再按原始请求头协商一次，后台任务显式刷新。批量捕获、校验和原子发布失败保留上次有效快照并记录脱敏固定错误码；通知是提示，有界修订核对负责最终收敛，不承诺全节点即时一致。外部 SQL 必须按契约推进修订或显式刷新。Windows 原子文件打开边界与完整 Laravel Loader 对应仍是公开缺口。

应用安装器在唯一 `default_providers()` 清单移出原默认 Translation 后追加到默认末尾，在 package register 前首次登记来源工厂；不是 application Provider 列表中晚 force 替换。合法 package register 提前解析与 boot 后解析复用同一个 Arc/Source 目录。JWT 生成继续仅有一个 default_providers 函数，保留显式 Fortify 替换与 Cookie/Session 排除。

`TranslationSourceChanged` 实现公开 `Event`，稳定名为 `OpenRust\TranslationDatabase\Events\TranslationSourceChanged`。typed 与同名字符串派发共用 exact/wildcard 选择及响应；跨类型通配回调用 `PayloadRef` 安全读取真实对象。该名字标识来源增强，不表示 Laravel 内置数据库翻译事件。
