# Passport-02 客户端 scope 与显式明文追加

2026-11-19，按固定 Passport 13.10.0 `f5f5e9f64559dd52339daa0d6102cb1c30e4a0d7` 的 Client::hasScope、ResolvesInheritedScopes 与 plainSecret 访问器补齐。接续[连接回退](connection-fallback.md)，整单元仍保持未闭合。旧版 client.rs 的 has_scope 已核对，复用原始 null／缺失无约束和同应用 scope 状态原则，接入当前真实模型与唯一 PHP 比较桥。

## 实现

ClientModel 与 ClientRecord 提供 hasScope。原始 scopes 未设置或 null 时不加载应用服务，允许所有 scope；其余值通过现有 Eloquent JSON cast 读取数组并检查成员。错误 JSON 或标量数组结果正常传播错误。PHP 非严格比较复用框架现有桥，不另建比较内核。

Passport::withInheritedScopes 是应用实例的 AtomicBool，默认 false。启用后按冒号分段检查父级与完整 scope，保留空段、Unicode、顺序和 PHP in_array 行为。没有额外的星号通配符分支。依赖当前请求或 Artisan 的容器上下文，两个应用的开关互不污染。

Eloquent 追加访问器通过实例 mutateAttributeForArray 执行。默认实现保持原 getter(None, raw attributes) 行为和错误。Client 仅对 plain_secret／plainSecret 读取自己的临时字段，其余委托既有属性逻辑。显式 append 后才输出明文，仍经过 hidden／visible 筛选；重新水合的模型追加 null，不把临时明文写回持久属性。

## 固定 PHP 对照

[生成器](../../../../vendor/openrust-passport/tests/reference/inherited_scopes.php)实际 require 固定 Passport.php 与 ResolvesInheritedScopes.php；加载前校验两份 SHA，错误源码立即拒绝。ScopeProbe 只暴露真实 trait 的 protected 方法，不重新实现该算法。固定镜像 `php@sha256:50956f094e90a716d43bea28087d447555ad80510e386ab8c9c10d8c70d781a9` 实际为 PHP 8.5.12，容器禁用网络，只读挂载源码和脚本。

[312 行结果](../../../../vendor/openrust-passport/tests/fixtures/inherited_scopes.json)覆盖继承开关、父子 scope、连续／首尾冒号、空字符串、Unicode、数字／布尔／null 成员、关联键和嵌套数组。Rust 双应用真实 TCP HTTP 逐行对照，每应用重复请求，验证本应用开关保持。最终带源码 SHA 校验的生成器再次逐字重现相同 fixture，改动 Passport.php 后的负例被拒绝。

这些是 scope trait 的 PHP 实际观测，未冒充完整 Client／数据库 PHP 执行；Scope JSON 编码选项、setter 失败顺序、数组其他边界和浮点字符串转换继续保留后续项。PHP 只用于测试对照，生产实现和 Artisan 均为 Rust。

## 验收

基线 `7720f142e5a236cd2a50e332bad8b58bf481410c` 的 git archive 加本批 12 份源码／测试／fixture／正文，未叠加共享 init、ViewData、Session、Encryption、SMTP 或 Cargo 在途改动。[源码 SHA](scope-source-sha256.json)与隔离副本一致。目录 `/private/tmp/openrust-passport-02-scopes-20261119/` 保留回执、日志和本批覆盖包。

| 范围 | 结果 |
| --- | --- |
| 原生 Client 9 项 | 通过，含显式明文追加／水合 null／hidden 与 visible、无应用的 null scope、既有静态 Eloquent 追加访问器的原始值与错误回归 |
| 既有 lifecycle 10 项 | 通过，真实应用 Console Kernel 执行 passport:keys 及配置／密钥／HTTP 生命周期，未重编根 CLI 二进制 |
| client_lookup 10 项 | MySQL 8.4.13 夹具全部通过，0 忽略；其中 scope oracle 与非法 ID 检查不执行 SQL，其余验证实际客户端模型生命周期及 TCP HTTP |
| scope PHP oracle | 312 个不同输入全部一致；独立重复用例不累加为测试函数数量 |
| 源码检查 | Rust 1.94 全仓 fmt 与 features pail,passport 全工作区全目标严格 Clippy 通过 |
| 三语正文 | 最终构建、12 项通用与 6 项 Passport 浏览器检查通过；根及 /docs/、三语、桌面与移动无脚本错误或横向溢出，保留已有 Vite 大 chunk 提示 |

本批为 29 个不同测试函数。默认追加访问器只变更调度入口，用独立原生模型验证其行为，未重跑未受影响的整个 SQL／其他数据库矩阵。

```sh
cargo +1.94.0 test --offline --locked -p openrust-passport --test client --test lifecycle
python3 -B tests/mysql_acceptance.py -- cargo +1.94.0 test --offline --locked -p openrust-passport --test client_lookup -- --test-threads=1
cargo +1.94.0 fmt --all --check
cargo +1.94.0 clippy --workspace --all-targets --offline --locked --features pail,passport -- -D warnings
```

## 收尾与后继

本批无活动构建／浏览器进程，MySQL 夹具标签均无残留。保存 accepted-overlay.tgz、SHA 与成功日志后，直接删除专属 source／target／browser，逻辑 3.329 GB，df 观测增加 3.340 GB。共享工作区并行活动影响总可用空间，cleanup.json 记录清理时点；两次 PHP 容器均已退出，专属 PHP 镜像在固定源码正负例复验后移除。源码／证据和共享缓存、其他 WIP、数据库及全部卷保留。

Passport-02 尚缺完整关系／工厂、UUID 运行配置、Token／RefreshToken 生命周期撤销、完整 passport:client、固定 PHP 其余 oracle 和完整管理流程。维持 V06 部分验收，未推进 Passport-03。本批仅核定范围本地提交，不推送。
