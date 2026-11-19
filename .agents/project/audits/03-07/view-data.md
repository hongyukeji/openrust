# V02 原生 ViewData 边界验收

2026-11-20 接续 2026-11-19 的暂停实现，本批 V02 闭合。只覆盖原生视图值与 JSON 边界，不代表 V03–V18 或整个框架完成。固定基线与三语正文分别见[Laravel 基线](../../plans/laravel-rebuild-baseline.md)、[Views](../../../../docs/zh_CN/the-basics/views.md#native-view-values)及[Blade](../../../../docs/zh_CN/the-basics/blade.md#rendering-json)。

## 原文、旧版与实现

固定 Laravel 13.40.0 的 View::with／gatherData、CompilesIncludes 和组件传递原生 PHP 值，不经过 JSON。Support\Js 在实际编码时拒绝 INF／NAN。原先 ViewData／ViewValue 直接别名到 serde_json，造成算术结果一进入数组、子视图或组件就失败。

前批已检查 `v0.x` 的 support/data_value.rs：有序树与显式投影可借鉴，但 Number 仍是有限 JSON 数，不能作为非有限数能力直接复用。沿用当前表达式、视图生命周期和消费者结构，未恢复旧版公开 API。

- `view::ViewValue` 保存 Null／Bool／Number／String／Array／Object；`ViewNumber` 保留 i64／u64、f64 非有限数与负零，`ViewData` 为有序 IndexMap。已有 JSON 输入显式 `.into()`，不以标记字符串代替数字。
- 数组、布局、include／each、props／aware、匿名与类组件、注册指令参数，以及共享／合并数据和循环元数据直接传递原生值。请求中的 Session／Context 仍在原 JSON 契约处转换，不改变其存储协议。
- `to_json()` 和 Serde 序列化递归拒绝非有限数；`@js`／`Js::from` 返回 RenderFailed，遵循应用的异常响应策略。整数值浮点数与负零的 JS 输出分别为 `2`／`-0`。普通回显保持 PHP 的 INF／NAN 字符串规则。
- 修复原生数组内 INF 与字符串的宽松比较，避免仅顶层算术比较正确。缺失值的 `??` 回退不改变已存在的非有限数。
- 迁移邮件、通知、分页、维护页面、错误共享及 Gate 参数消费者；类型化 Gate 参数保留原路径。仅开启既有 indexmap 2.14.2 的 serde 特性，不升级依赖。
- 编译缓存版本 33，使版本 32 的表达式制品重新编译。两处缓存测试规范化 macOS 临时目录的符号链接路径，核对实际制品及旧版本失效。

Rust 语言适配：公开数据使用枚举／有序映射和 `Result`，不承诺任意 PHP 对象、引用或可执行 PHP。模板回调接收 ViewValue；JSON 已提前丢失的非有限值无法恢复。三语正文同步给出原生构造、JSON 转换和错误处理示例。

## 固定 PHP 对照

参考脚本：`vendor/openrust/framework/tests/reference/blade_native_values.php`；Composer 依赖沿用同目录 `blade_arithmetic.composer.json` 的固定 13.40.0 集合。前批保留的实际锁文件位于 `/private/tmp/openrust-view-data-20261119/composer.lock`，哈希见[源码核对](view-data-sha256.json)。没有把重新解析最新依赖当作固定基线。

在已有远程夹具上使用固定 Composer 镜像、`--network none` 和 PHP 8.4.14 执行，脚本核对每个 Illuminate 包版本为 v13.40.0。最终 108 例连续两次输出逐字节相同：96 成功、12 JSON 非有限数错误。包括数组／对象嵌套、include、each、layout、props 默认值与 aware、指令参数、truth／比较、负零、缺失值回退，以及 @js 和 Js::from。Rust 对照验证输出或实际 RenderFailed 的错误源，不把任意解析错误当作预期编码错误。自定义 Rust 类组件另有原生回归。

初轮 Js::from 的两例有限值因独立 PHP 容器未注册 `Js` 类别名而失败；补齐 Laravel 默认别名后重新生成两次，旧失败日志保留。没有修改 Rust 预期去接受该夹具错误。

## 实际验收

证据根目录：`/private/tmp/openrust-v02-20261120/`。从该目录 `baseline.txt` 指定 HEAD 建立独立源码，并叠加当时工作区；本批 32 份源码／测试／正文与验收快照 SHA 相同。开发中初始化任务独立提交 75491cbb1；本批不改它的授权与记录。同期 Passport 在途改动未纳入此快照，本批全目标检查不代表那些后续改动已验收。

| 验收 | 实际结果与日志 |
| --- | --- |
| Rust 1.94.0 格式 | `cargo +1.94.0 fmt --all --check` 通过，fmt-final.log |
| 全目标严格 Clippy | `cargo +1.94.0 clippy --offline --locked --workspace --all-targets --features pail,passport -- -D warnings` 通过，clippy.log／clippy-final.log |
| Blade／View | 定向 view_native_values、view_finder、blade_arithmetic、blade_statements，共 22 个不同测试函数通过；最后增强错误源断言的 6 个 native 测试再过。view-tests-complete.log／native-accepted.log。含既有 962 组算术与 198 组预处理固定语料，新增 108 例不重复计为测试函数 |
| 邮件消费者 | 精确运行 mail_composes_views_mime_headers_and_keeps_application_capture_isolated，通过 1 项，mail-view-test.log。仅视图／MIME 与双应用隔离；不执行数据库或投递队列 |
| 分页消费者 | 官方 mysql_acceptance.py 建立 MySQL 8.4.13，映射 ELOQUENT 专用 URL 并设置 REQUIRE 后，精确执行 pagination_on_mysql_and_http，1 项通过，pagination-mysql-accepted.log |
| 实际根二进制 | 独立构建 main 后运行 tests/blade_arithmetic_http.py：原生数组／include／匿名组件、JSON 与 HTML 500、失败恢复、HEAD、health、view:cache／clear、inspire、原始 ./artisan inspire、SIGINT 退出 0 与端口释放通过，root-http-artisan.log |
| 三语正文 | 同一原生值示例已由 Rust 测试执行；隔离 docs 的 npm run build 通过，docs-build-final.log |
| 浏览器 | 既有 check_browser.mjs 的 `/`／`/docs/`、三语桌面／移动 12 项通过；增强 check_view_contracts.mjs 的 6 项原生值锚点／代码／Blade 到 Views 链接检查通过，docs-browser.log／docs-native-browser.log |

首轮旧缓存测试因 `/var` 与 `/private/var` 制品路径及旧版本断言失败，修正夹具后复验通过。首次分页测试未映射其专用环境变量，因此未实际查询，不计验收；最终 REQUIRE 强制执行。一次 npm 从仓库根目录误启动失败，最终只采用隔离 docs 构建结果。增强错误源断言时误读外层通用 Display 的失败日志保留，最终匹配实际 RenderFailed 源。没有运行无筛选 workspace 测试或 SQLite 数据库测试，也未重新执行 SMTP／Redis／队列、Windows／Linux 平台矩阵；这些不是本批新增已过范围。

## 交付边界与资源

本批仅提交原生值容器、必要消费者、测试、三语 Views／Blade、浏览器检查及 V02 账本／交接；不纳入 Passport、Eloquent factories、历史 patch、core/、translation-database/ 等协作者工作。提交前复核文件清单及 SHA；不推送、不改发布版本、不打标签。

资源收尾结果在本节追加。保留本批源码与验收日志、共享 root target／Cargo registry／docs/node_modules／现有 Chromium，以及数据库与全部数据卷。远程只删除本批新增脚本，既有前批夹具与镜像保留。

本批已退出的 target、浏览器依赖与隔离 docs dist 直接删除，逻辑共 9456200956 字节；df 可用由 60527079424 增至 69599420416 字节（并行任务也可能影响 df）。本批源码与全部日志保留在证据目录，cleanup.json 记录明细；未清理共享缓存或数据卷。
