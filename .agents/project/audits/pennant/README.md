# Pennant

## 来源与适配

固定 `laravel/pennant` `9046466feb0146b6223db8b8e9c589aeba23f7f0`，源码含测试／MIT许可证位于 `tmp/openrust-ecosystem-20261121/upstream/laravel-pennant`。复用官方 Driver／Decorator／PendingScopedFeatureInteraction／FeatureManager、命令与中间件职责；不另建SQL或作用域容器。旧版 `6d1a81924451e15f595f68c8dfb520ad6d6c44cf` 无独立Pennant包。GitHub固定来源请求及本地完整归档对照，不据此宣称实时热门排名。

Rust静态类采用 `FeatureDefinition` 与异步工厂，显式名称与空作用域契约替代PHP反射；factory可调用原生容器，延迟创建。`before` 在缓存未命中时先于持久化读取执行，非空结果仅入生命周期缓存，不写回驱动。PHP源码扫描／动态类装载需以应用编译模块与工厂注册替代，不提供虚假的运行时PHP式扫描。作用域使用显式 `Scope`，模型具体类与morph别名分开，`FeatureScopeable`按store显式转换。

Provider单例保存定义，scoped manager保存值缓存／array状态；HTTP／Artisan／Worker依赖原生容器作用域，无全局清空竞态。数据库驱动复用原生schema/query/upsert，首次并发插入按唯一键重试；批量读取优先单次查询，all-scope更新仅作用于已有scope。仅严格false表示关闭，null／0／空字符串／数组均为开启。

## 最终单包验收（2026-11-21）

独立源码 `tmp/openrust-pennant-20261121/source`，独占复用 `tmp/openrust-core-ecosystem-20261121/target`；TMPDIR为本工作流temp目录。Rust1.94.0，未限制并发或关闭增量。所属格式化文件已回写活跃工作区；其他扩展WIP保留，未提交／推送。

- `cargo +1.94.0 test --locked -p openrust-pennant`：16个测试函数全部通过，无忽略；包含数组／数据库、异步类工厂与异构nameMap、before及空作用域、事件监听失败／写入顺序、可选驱动能力、loadMissing与定义顺序、真实模型认证／morph、HTTP与原生Worker生命周期隔离。
- 固定PHP8.5.12运行未修改上游ArrayDriver／PendingScopedFeatureInteraction，2401组聚合一致；未修改FeatureManager::serializeScope的23组标量／浮点scope一致。最小Collection／容器替身不等于完整Laravel PHPUnit环境。
- PostgreSQL18.7（180007）／MySQL8.4.13分别运行最终二进制两项database测试及根应用原生Artisan配置测试，均通过。SQLite包含在16项包测试和根应用测试中；三库覆盖迁移／批量读取／唯一冲突／12并发首次解析／持久化／before不写回，原生配置测试覆盖包发现、迁移、数据库purge、发布保留及配置缓存。日志为postgres-native-final.log／mysql-native-final.log。
- 根应用feature关闭1项、启用配置与application_skeleton共3项通过；框架extension_core_contracts共3项通过；features示例实际运行通过。
- 独立源码`cargo +1.94.0 fmt --all --check`及Pennant／framework所有target严格Clippy通过。vendored rav1e／imgalifier／sqlx-sqlite既有警告不作为包错误。具体日志为fmt-final.log／clippy-final.log／core-contracts.log／configuration-final.log／feature-off.log／example.log。
- 独立文档源码npm ci、三语构建通过；Pennant专项12组、通用12组、全部导航6组浏览器通过，覆盖根路径／docs子目录、三语与桌面手机。显式不支持章节为22页；日志为docs-build-isolated.log／docs-browser-isolated.log／docs-browser-common-isolated.log／docs-scope-isolated.log。

## 语言差异与剩余组合边界

PHP动态类发现由编译模块和显式异步factory注册替代；typed instance与nameMap保留可观察定义信息，值为JSON而非任意PHP对象。默认无Request的入口读取已解析认证快照，显式`forRequest`异步解析原生guard；中间件主动解析，三语正文已说明并由真实HTTP认证测试验证。自定义驱动stored／definedFeaturesForScope可选，未声明批量写入能力时按逐项写入→缓存→事件执行，失败即停。

O04本包交付关闭；与最终核心及Horizon／Telescope／Pulse／Scout的全feature组合、最终API导出仍须单独验收，不从本包结果推断全部扩展完成。

## 扩展审计修复（2026-11-21）

参考生成器在执行固定 ArrayDriver／PendingScopedFeatureInteraction 前校验全部 require 输入摘要与来源，复现原 fixture 并逐个拒绝篡改；未改生产功能。 统一命令、环境、结果和边界见[全量扩展修复记录](../extension-design-review.md#修复验收与交付边界)。
