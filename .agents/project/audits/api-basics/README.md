# 默认 API 基础与按需认证安装

日期：2026-11-24。用户确认默认预置普通 API 基础，Sanctum 仍由命令安装，并要求清理刚执行 install:api 生成的应用内容。这是明确的项目默认骨架选择，替代此前 API 文件也按需生成的决定；不称作 Laravel 默认文件布局或 Rust 语言限制。保留[固定 ApiInstallCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Console/ApiInstallCommand.php)的安装入口、自定义路由保留、force及迁移确认边界。

## 实际清理与保留

- 移除本次插入的 Cargo 默认 sanctum feature、User 的 HasApiTokens 条件实现、认证模型的 Sanctum 分支，以及配置加载器的 sanctum 模块／映射。User 同时发生其他协作者的邮箱验证改动，逐片段清理并保留该改动，没有整文件还原或提交它。清理过程中对方旧文本曾带回令牌trait，再次仅移除该片段，最终检查无残留。
- 直接删除本次未跟踪的 config/sanctum.rs 与 database/migrations/2026_11_23_203652_create_personal_access_tokens_table.rs，不经过废纸篓。前者与本次生成配置内容、后者令牌表定义已先核对；原始修改摘要与文件指纹保留在 target/tmp/api-basics/before.diff 和 before-sha256.json。
- 保留并纳入默认骨架：routes/api.rs 的空 register、config/api.rs 的 API_PREFIX 默认 /api、routes/mod.rs 与 config/mod.rs 的模块／加载，以及 bootstrap/app.rs 的 .api 注册；环境示例添加 API_PREFIX=/api。默认不含 /user、Sanctum 应用条件编译接线、专属配置或令牌迁移。
- 框架 Sanctum 实现、可选 feature 声明及安装资源继续保留；原有 CORS 可匹配 sanctum/csrf-cookie 的默认路径不是本次安装插入，不删除，也不会自行启用认证。没有修改私有 .env、查询或回滚现有数据库。若用户此前确认运行过令牌迁移，库内表及旧台账仍保留；后续重新迁移前需核对已有台账，不能用删除源码代替数据库回滚。

## 安装衔接与文档

安装器识别与默认空骨架一致的 API 文件，允许普通安装直接补入认证路由；逐非空行去首尾空白比较，兼容CRLF，不把文件存在误判为认证已安装。实际业务内容不同则继续保留，使用既有提示，由开发者手动追加受保护入口或显式force覆盖。原配置始终保留，force只影响既有约定的路由覆盖。共享路径也用于Passport转换，但本轮没有重跑完整Passport／OAuth安装矩阵，不扩大其发布验收结论。

三语路由、Sanctum、Passport章节同步默认基础／按需认证的生命周期；上下文与计划同步本轮决定，历史验收不回写。未修改其他协作者的请求门面、模型持久化、文件系统或教程在途内容。

## 实际验收

使用当前提交归档加本轮文件构成隔离快照 target/tmp/api-basics/source，排除其他协作者在途实现；Cargo target／TMPDIR均在本项目target/tmp/api-basics内。实际Rust应用在本机macOS运行，不把CRLF专项称作Windows实机。

| 路径 | 结果 |
| --- | --- |
| 未启用Sanctum、默认 /api 与 API_PREFIX=api/admin | 隔离项目临时增加公开探针，HTTP200；POST返回JSON405；/user返回404；旧前缀不误匹配 |
| 空基础骨架执行原生 Artisan install:api | 无需force生成认证/user、Sanctum配置、模型接线、默认feature与单份令牌迁移；自定义API配置保留 |
| 原生Cargo重建后的 /api/user 与 /api/admin/user | 无凭据真实HTTP401，JSON响应；应用正常退出并验证端口释放 |
| 重复安装及自定义业务路由 | 配置与业务内容字节保持，迁移文件仍为一份 |
| 显式force | 按约定替换路由，仍保留API配置与已有迁移 |
| 根启动与默认骨架回归 | startup全28项、application_skeleton全2项通过，含真实TCP前缀／JSON／CORS与源生成回归 |
| CRLF空骨架 | 原生安装器新增专项通过，能自动补认证且不提示已有业务路由 |
| 静态与文档门禁 | 受影响根binary／startup／skeleton严格Clippy、范围格式、diff及三语内容／构建通过 |

所有安装调用都显式使用 --without-migration-prompt／--no-interaction；独占空SQLite文件经只读查询确认未创建personal_access_tokens，没有执行迁移。该文件只用于隔离验证，用户数据库未操作。公开探针与安装后的文件仅在隔离快照使用，验收后已将快照恢复默认基础；根应用未添加探针或业务示例。

证据保留于target/tmp/api-basics/：verify.py、results.json、http-install.log、startup.log、regression.log、crlf.log、clippy-final.log、docs.log、installed-files.json。verify.py通过真实编译／重建与HTTP执行断言，非字符串检查代替运行；全局工作区尚有其他在途改动，不宣称整仓合并验收完成。

## 资源收尾

可用空间接近32GB时，本项目统一构建清理流程先行回收了包含本批target和文档dist在内的多个工作流产物，原始事件另存space-guard-event.json：净回收 62,557,171,712 字节，不能全部归为本批贡献。验收均在此之前完成，通过日志与源码保留。

随后确认本批剩余产物无人占用并取得Cargo锁，直接删除初始测试二进制副本及残留产物；锁inode保留，净增加 148,779,008 字节，测量后可用 94,514,159,616 字节（约 94.51 GB），详见cleanup.json。保留源码、日志、下载依赖、现有数据库和数据卷；未新建Docker资源。未推送。


## ApiResponse 三语教程补齐（2026-11-24）

- 三语 HTTP 响应章节统一使用 business-responses 显式锚点，补齐默认启用／环境变量覆盖、自动异常及普通 Resource 范围、JSON:API 和原生响应边界、状态与空值、翻译及字段覆盖顺序；异常和资源章节双向链接。未修改框架实现或 .env.example。
- 从三语正文提取全部三组 Rust 示例及 JSON，核对内容一致与 JSON 可解析。使用已有 Rust 1.94.0 编译依赖直接编译独立消费者，无新增 Cargo 构建目录或依赖；编译库来源快照的 api_response.rs、异常渲染和 Resource 响应源码与当前文件逐字相同。实际本地 HTTP 验证五个请求的状态和完整 JSON：分页200、创建201、字段错误422、自定义字段200、zh-Hans 参数翻译200。三项现有字段／空值／状态消费者另行通过，不提交其他协作者的测试改动。
- npm --prefix docs run build 通过（含入口规则、三语内容和链接检查）；浏览器脚本在 / 与 /docs/ 下三语桌面／移动共12组通过。原始日志和提取消费者保留于 target/tmp/api-response-docs/。默认 rustc 版本不匹配、首次读取时依赖库正重编译及临时消费者 PathBuf 参数错误均修正后重跑，不计失败尝试为通过。VitePress 仍有既有包体积提示，不影响构建。
- 本次验收对应当前工作区组合，不代表其他在途响应实现已提交或完整框架闭合。验收后删除本批独占可执行文件，源码与日志保留；不清理并行任务的构建缓存、文档依赖及数据库。


## API 前缀根路径与异常选择（2026-11-24）

- Request::is_api_path 同时匹配 API 前缀本身及带斜杠边界的子路径，保留空前缀不匹配。bootstrap/app.rs 移除未提交的 ApiResponse::enabled_for 重复条件，恢复仅由 is_api_path／expects_json 决定响应类型；该文件恢复到已有 HEAD 内容，因此本提交不产生该文件差异。业务封装开关继续由异常及 Resource 内部决定。
- 新增 api_prefix_boundary 集成测试，两项通过，共31个真实本地HTTP请求。默认及多级前缀分别覆盖封装开／关、根路径与子路径401、缺失路径404、带查询根路径、相似前缀与非前置路径不误匹配、前缀外Accept JSON，以及空前缀不把全站归为API。关闭封装仍为JSON且没有success/code；开启则保留业务字段。
- 使用当前工作区框架源码、已有只读依赖和Rust1.94.0在本批独立输出目录编译，再编译并执行实际集成测试；没有写入协作者的Cargo目标目录或提交其在途响应实现。范围rustfmt及diff检查通过。最初捕获编译参数丢失check-cfg引号，修正后重跑，未算通过。
- 证据及重跑脚本保存 target/tmp/api-prefix-boundary/（build.log、compile-test.log、tests.log、build.py、run.py）；临时env及运行服务由测试回收。本批独占编译目录及测试可执行文件验收后直接删除，源码、日志、共享依赖和数据库保留，清理测量见cleanup.json。未推送。
