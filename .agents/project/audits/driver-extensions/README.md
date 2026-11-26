# 自定义驱动扩展验收

## 范围与依赖

2026-11-25，持续目标从顺序2开始。顺序1由其他对话负责；不纳入其未提交改动。首批处理 DRV-01 存储接缝与 DRV-05 既有短信扩展消费者；其后按 Webhook 接收／发送、机器翻译、Pay、Lang Loader、四家内容审核、Reverb 登录、Cashier 范围与 New Relic 的依赖顺序继续。此记录不代表全部持续目标完成，供应商账号／合同／实际投递／发布权限单列，不伪造通过。

## 调研与决定

- 固定 Laravel `v13.40.0`，提交 `1449bf241017087fbd38c118ecaabef3f6797fdc`：[FilesystemManager](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Filesystem/FilesystemManager.php)。采用实例内 `extend` 工厂、按配置选择驱动、具名缓存、显式遗忘缓存的职责划分；自定义工厂优先于内置驱动，返回完整适配器。
- [Flysystem 3.30.2 的适配契约](https://github.com/thephpleague/flysystem/blob/3.30.2/src/FilesystemAdapter.php)用于核对流式读写、文件／目录存在性、元数据、复制／移动的职责。既有 Rust `Filesystem`／`Cloud` 已能承载这些职责，复用而不引入另一个存储引擎或依赖。PHP 包本身不嵌入 Rust 运行时。
- 已查看同机 `openrust-v0.x` 的文件存储契约、驱动注册和回归。沿用外部后端与框架包装层分工；旧 TOML、租户前缀与旧公开类型不覆盖当前 Laravel 式配置及现有返回类型。
- Rust 工厂接收当前 `FilesystemManager` 和原始磁盘配置，应用依赖可由 Provider 捕获。保留强类型 `build(LocalDiskConfig)`／`build_s3`，另提供 `build_from_config`，避免破坏现有调用。
- 不增加全局注册表。每个具名磁盘的初始化槽保证并发只构造一次，执行工厂不持有全局缓存锁，允许解析另一磁盘；循环解析自身不支持。未知名称不留下缓存槽。替换工厂不修改旧句柄；`forget_disk` 后重建，失败可重试。

## 实现与消费者

- `FilesystemManager::extend`、`Storage::extend`、两者的 `build_from_config`；外部通过 `FilesystemAdapter::custom(Arc<dyn Cloud>, throw, report, url)` 进入既有适配器。`Filesystem` 新增的文件／目录存在性和 MIME 方法提供默认“不支持”，不要求既有实现补齐才能编译。
- 保留路径校验、分隔符归一化、throw／report、读取／下载、写入、目录、可见性、元数据、复制／移动、流式校验和与临时 URL 回调。无能力时返回明确错误；自定义后端不自动得到本地签名路由或 S3 SDK 专属 Builder。后端自身负责原子发布、权限及所连接服务的并发契约。
- `custom_filesystem` 将公开分块契约接回 AsyncRead，不整文件缓冲。上传块64 KiB、队列容量1；无后台任务。后端提前结束时取消尚在等待源数据的读取，避免只关闭接收端而仍等待源读取；失败／取消的发布保证仍由后端契约负责，本地测试后端已复验原文件保留。
- 独立消费 crate 的测试实现位于 `tests/support/driver_extensions/storage.rs`：只使用公开契约，以本地服务代替真实供应商，不依赖修改内部 Driver 枚举。单元入口 `vendor/openrust-framework/tests/filesystem_custom.rs`；独立应用入口 `tests/support/driver_extensions/app.rs`。
- 短信复用已有 SmsManager／SmsDriver，不重复实现 Manager 或另加发送通道。三语新增完整 Provider、HTTP 驱动和具名调用示例，明确纯文本范围、凭据配置、Worker 自行注册、快照与 `forget_drivers`。
- `tests/driver_extensions_docs.py` 提取三语正文的12段 Rust 示例，生成隔离快照中的6个编译消费者；不把只编译示例说成运行了供应商调用。实际调用由 HTTP／Artisan 与既有独立 Worker 夹具验证。

## 验收记录

隔离源码位于 `target/tmp/driver-extensions-20261125/source`，复用独占 `target/tmp/driver-extensions/cargo-target`；全部构建经 `tests/build_guard.py`，Rust 1.94.0、locked、正常增量与并发。首轮完成后从 `6cb7cc2dbd351ad0528e64499636cb9f685637dd` 重建最新已提交源码快照，再仅叠加本任务文件进行集成复验；其他对话 WIP 不进入验收结论。

已执行：

- 外部存储4项：公开读写／流／下载／元数据／URL／目录／权限、非法路径、并发12线程只构造一次、失败重试／替换缓存／多应用隔离，以及读取失败不发布、后端提前返回与永不就绪的源读取取消。
- 既有本地存储10项，包括真实 HTTP 挂载、路径／链接／硬链接保护、Unix 权限、流失败与取消；短信 Manager 6项生命周期回归。
- 独立 HTTP／Artisan 通过4次真实外部存储与短信调用：配置缓存前后工厂均由新进程注册，缓存后修改环境端点为不可达地址仍使用缓存中原端点。HTTP 与 Artisan 共用所配置后端；不把该本机服务说成远程集群存储验收。
- PostgreSQL 18.7＋database缓存、MySQL 8.4.13＋Redis缓存、SQLite＋file缓存，分别复用短信 `check_backend` 的 database／Redis 两种队列。共6组，各含默认／显式配置直接发送、4个入队配置和4个独立 Artisan Worker。派发端凭据／端点不入队，Worker 使用自己的配置，即使默认配置反向切换仍送往入队时选定的名称。Redis 8 专属容器先核 `INFO server` 和 `PING`，所有批次容器均已回收，保留原有数据库／Redis容器与数据卷。
- 三语12段正文示例编译、全仓 fmt 和 workspace/all-targets 严格 Clippy首轮通过；三语内容／命令入口检查及 VitePress 构建通过。站点保留既有大块体积提示，不通过修改阈值屏蔽。

最终集成已通过：`final-tests.log` 的20项定向行为回归＋6项正文编译消费者，以及两个独立应用入口构建；`final-http-artisan.log` 的缓存前后4次调用；`final-workers-{postgres,mysql,sqlite}.log` 的三库双队列全组；`final-clippy.log` 的全workspace/all-targets严格Clippy与`final-fmt.log`；`final-docs-build.log` 的三语构建。浏览器新增12项自定义驱动页面、既有12项桌面／移动端与根／子目录全导航支持范围检查均通过。固定最终代码后新增的多配置测试还确认同一外部工厂可以指向不同后端根，数据互不混用。

集成途中触及32 GB停线，守卫安全停止自己的构建，发现其他占用时保留锁；取得锁后清理全部项目编译产物，净释放79,840,591,872字节，保留源码、日志和数据。被中断的检查未计通过，随后冷构建并完成上述最终复验。临时数据库、Redis与应用目录已回收，三语站点产物与本批一次性编辑脚本已清理；隔离源码／日志和仍用于后续开发的独占构建缓存保留。

## 闭合与下一批

DRV-01 自定义存储驱动接缝、DRV-05 外部短信消费者与三语示例按上述定向范围闭合，不扩称完整文件存储上游审计或真实供应商发布通过。持续目标保持进行中，下一批 WH-00／WH-01 固定公开契约清单与两个独立包的正式装配，随后接收可靠性 WH-02／WH-03，再推进发送／运维 WH-04至WH-07。已开始复读固定 Client 的 WebhookProcessor 和 Server 的 WebhookCall；须保留“签名→Profile→存储→派发→响应”职责，并逐项裁决同步／异步派发事件差异，不把后续实现预记为通过。

失败与边界：首次 HTTP 夹具忘记停用默认 Queue Runner，修正为只运行所需 HTTP；随后配置缓存固化监听端口，夹具错误地换端口而请求失败，改为缓存前后使用同一隔离端口。首次示例引用了非公开短信验证方法及缺少 trait 导入，改为由现有 Sender 执行验证并导入公开应用契约。首次 Clippy 报测试未使用导入，删除后复验通过。HTTP端口夹具失败与首次Clippy失败日志保留于上述批次目录；其他首轮编译诊断见本轮执行输出，此处保留原因与修复结果。

S3测试入口本轮缺端点时提前返回，不计真实对象存储通过；本批未修改S3协议。未验证真实云存储账号、短信供应商／运营商实际投递或远程多主机；不能用本地协议服务消除这些供应商／发布边界。Webhook／机器翻译／Pay等尚在持续目标后续，不因本批基础接缝通过而闭合。
