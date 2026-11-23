# Blade 应用与容器调用

日期：2026-11-23。用户授权实现、验收及本地提交，不包含推送或发布。承接 `47e48890e` 的四种 app 只读特例，采用提交前最新基线组合复验；其他协作者的错误转换、模型等在途改动单独保留。

## 固定依据与取舍

- Laravel Framework v13.40.0，提交 `1449bf241017087fbd38c118ecaabef3f6797fdc`：`src/Illuminate/Foundation/helpers.php` 的 `app($abstract = null, array $parameters = [])` 在 abstract 为 null 时返回当前容器，否则调用 `make`。`Foundation/Application.php` 核对语言、版本、环境、路径及容器解析；`tests/Container/ContainerTest.php` 核对参数覆盖、别名和服务生命周期。已联网读取固定原文，未升级基线。
- MiniJinja 2.12.0 的 [Object](https://github.com/mitsuhiko/minijinja/blob/2.12.0/minijinja/src/value/object.rs) 展示 Rust 模板对象显式方法分派；采用其对象边界思想，复用本项目表达式、ViewValue、类型投影和请求桥接，不引入其依赖或另一套模板／容器。
- 旧 `v0.x:vendor/openrust/framework/src/view.rs`、`src/view/README.md` 采用另一套视图结构，本次沿用当前可缓存 AST 和应用作用域，不迁入旧机制。
- Rust 的 `Any` 不提供 PHP 方法反射，不能由运行时字符串自动调用任意未声明 Rust 方法。通过 `ViewObject` 显式公开调用，再使用原有 `ClassProjection` 或 `dyn ViewObject` 容器绑定；无需逐服务修改编译器。此为具体静态语言适配，不将框架 Application 所有接口或任意 PHP 执行计为已对齐。

## 实际修改

1. app()／app(null) 返回作用域中的应用对象，app(service, parameters) 与 make／makeWith 走现有命名解析、别名、构造参数和生命周期；HTTP 异步解析走 Application::makeWithNamed，包括延迟 Provider。
2. 通用成员调用 AST 支持对象变量、链、子视图、组件、嵌套表达式；移除解析器的四个 app 方法特例。ViewValue::Instance 保存真实服务句柄而非序列化快照；对象不直接回显或序列化到 JSON／模板缓存，缓存版本45。
3. 核心应用对象暴露正文列出的语言、版本、环境、路径和容器查询；配置对象复用实际配置仓储。语言切换通过真实异步应用接口，保持事件更新顺序和错误传播。
4. 沿用 Facade 和容器请求隔离，新增弱作用域凭据，应用对象也弱持应用句柄，避免保留对象延长请求／应用生命周期；跨请求保留对象不能调用。不增加全局可变对象或服务管理器。
5. 三语 Blade 正文同步公开用法、类型参数、同步／异步支持范围及实际限制；欢迎页继续使用现有 app／config／request 写法。

## 验收

项目内材料：`target/tmp/blade-app-container-20261123/`；隔离源码：`target/tmp/locale-save-20261123/source`；同工作流独占复用缓存：`target/tmp/locale-save-20261123/cargo-target`。命令通过根 `tests/build_guard.py`，使用 Rust1.94.0 和项目 TMPDIR，未操作开发数据。

修改前，新消费者在仅含既有实现的快照返回500，错误为 Unsupported Blade expression，见 `baseline.log`；加入通用对象实现后进行正反回归。测试不借用其他协作者尚未提交的错误转换。

磁盘触线时守卫安全停止本次构建并取得各 profile 排他锁，清理约102.6 GB编译产物，清理后约134.1 GB可用，锁inode保留；退出75不计通过，恢复后重跑。具体事件见项目 `target/tmp/build-guard-events.jsonl`。


## 支持边界

- 自定义服务须显式实现 ViewObject 并绑定／投影；没有投影的已有服务会返回明确类型错误，不自动反射所有Rust接口。核心应用公开模板方法以三语正文清单为准，其余方法未宣称实现。
- 普通 Request／Config 函数助手的既有契约保留，不把本次 app 对象能力描述为完整 PHP Request 或配置表达式执行器。普通模板不执行 PHP 代码块。
- 同步离线渲染需要有效应用作用域；HTTP 管线提供异步工厂、callAsync及语言切换。未引入离线异步运行时。
- 未涉及共享数据库／缓存驱动，无需启动三库或外部缓存；完整workspace、其他平台和发布验收单列，不以局部回归代表全框架完整对齐。

### 最终结果

- `python3 -B tests/build_guard.py --workflow blade-app-container-20261123 --target-dir target/tmp/locale-save-20261123/cargo-target -- python3 -B target/tmp/blade-app-container-20261123/checks.py`：框架11个测试目标69项、应用3个测试目标53项，共122项通过，0失败、0忽略。框架目标为blade_app、blade_request_config、blade_expression_contracts、blade_arithmetic、blade_statements、blade_translations、blade_query_helpers、view_native_values、view_finder、application_locale、container_offsets；应用目标为welcome、configuration、http。
- 同一守卫／目录执行 `quality.py`：范围内Rust格式检查、框架库及上述测试目标严格Clippy、应用三个测试目标严格Clippy全部通过。均使用`--locked`，Clippy使用`-- -D warnings`；依赖包自身已有警告不等于本包lint失败。未执行完整workspace／all-features检查，不外推为通过。
- 同一守卫／目录执行 `browser-checks.py`：重新构建真实HTTP Kernel消费者，25组Chromium通过，覆盖三语自动／固定偏好、URL临时覆盖、CSRF表单、恢复自动、Cookie损坏／过期、自定义配置、桌面／手机、明暗主题及仓库链接。验收服务已正常退出。
- `python3 -B tests/build_guard.py --workflow blade-app-container-docs-20261123 --target-dir target/tmp/blade-app-container-20261123/docs-target -- python3 -B target/tmp/blade-app-container-20261123/docs.py`：三语正文内容检查、VitePress构建、12组双前缀浏览器和6组全导航检查通过。三语生命周期正文同时删除控制器传名称／版本的过时说明，与实际模板一致。
- 广域回归先发现两处缓存测试硬编码40；改为检查44失效、45重建，并保留输出及新产物版本断言。旧表达式测试把所有`$model->method()`判为编译错误；现验证语法可编译，而null／字符串／普通JSON对象的调用仍在运行时报错。实际ViewObject成功和失败由9项真实Kernel测试覆盖，未靠删除失败断言关闭问题。
- 技术交付内容逐字节与验收快照核对；共享文件只取本次对象桥接、导出和消费者修改，其他协作者的错误转换与模型等在途修改保留。最终提交索引与验收源码一致，明细留在忽略目录，不另在审计目录存源码哈希。

本任务范围无已知实现阻塞。`app()`对齐的是应用／现有容器访问及上述已公开的方法，不能据此宣称整个Blade、全部Foundation API或任意PHP方法完全等价。完整workspace、其他浏览器引擎／真实设备、Windows、Docker发布、性能及不受本次影响的外部服务未验。按本次授权仅本地提交，不推送或发布。

## 提交后复查（2026-11-23）

复核起点`c3f0b38f0`。检查应用适配器、命名解析与类型投影、成员表达式、比较、请求桥接、弱作用域、消费者及三语用法；本节不代替全Foundation／PHP公开API审计。

| 问题 | 修改前证据 | 实际修复 | 实际验收 |
| --- | --- | --- | --- |
| `environment([])`被当成无参数读取 | 新HTTP消费者期望`testing\|1\|1\|1`，实际空数组比较false失败 | 依据原始实参数量区分读取与空候选匹配；固定Application.php的environment实现对显式空数组执行空模式匹配 | 同时验证无参、空数组、不匹配模式和testing；新增回归通过 |
| 括号接收对象编译失败 | `(app())->currentLocale()`返回500 | 只剥离包住完整表达式的外层括号，保留成员接收对象；复用现有引号处理 | 单层／多层括号、对象链、配置对象、默认转义及原算术分组通过 |
| 相同实例`==`错误返回false | app对象及服务别名相等比较输出为空，`===`为1 | 松比较补齐Instance分支，复用已有类型及实例身份比较 | app自身、别名同实例、严格比较、!=与null均通过 |

三项修复前独立回归0通过／3失败（`before.log`），修复后真实Kernel回归12项全部通过。PHP同实例相等可保留；不同Rust实例的属性无法依赖Any自动反射，`==`／`===`均采用显式实例身份，不将不同实例的结构比较宣称等价。三语Blade正文明确此边界及空环境模式语义。没有新增服务管理器、依赖或公开API；AST存储结构未变，保持缓存版本45。

继续复用上一节固定上游来源及现有实现。参考PHP对象比较官方说明`https://www.php.net/manual/en/language.oop5.object-comparison.php`；未创建或执行PHP文件。

最终命令仍经`tests/build_guard.py --workflow blade-app-container-20261123 --target-dir target/tmp/locale-save-20261123/cargo-target`，执行`target/tmp/blade-app-review-20261123/checks.py`和`final-checks.py`：Rust1.94.0框架72项＋应用53项，共125项通过，0失败／忽略；范围fmt、框架库及原11个测试目标和应用3个测试目标严格Clippy通过。欢迎页消费者重新构建后25组HTTP Kernel／Chromium通过并正常退出。文档独立守卫执行同目录`docs.py`，三语内容、构建、12组双前缀浏览器及6组全导航通过。

本次开始完整复验时磁盘再次触线；守卫安全取得锁并清理约100.7GB本项目编译产物，恢复至约132.7GB可用，保留源码、在途改动、日志、依赖下载及锁inode；清理后重新冷编译，最终结果来自本轮源码。实际事件仍见`target/tmp/build-guard-events.jsonl`。隔离源码和独占目标沿用上一节，复查日志位于`target/tmp/blade-app-review-20261123/`。交付内容与本轮验收快照逐字节核对；共享文件排除其他任务的错误转换。

本次发现的三项问题已修复并验收；完整workspace／all-features、其他平台／引擎／实机、发布及不同Rust对象属性的PHP反射比较仍不计通过。沿用本任务用户已有本地提交授权，不推送或发布，保留其他协作WIP。
