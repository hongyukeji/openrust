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
