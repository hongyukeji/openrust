# 扩展最终本地组合（2026-11-21）

## 范围和结论

按照 operations-ecosystem 路线完成 O01 Activitylog、O02 Horizon、O03 Telescope原生诊断范围、O04 Pennant、O05 Pulse及O06 Scout首个引擎范围，并补齐本轮发现的必要核心接口。联网来源及语言适配见各包审计和extension-parity，不把测试通过等同Laravel全部API、所有外部引擎或完整PHP前端逐项等价。

本轮独立执行，没有联系其他对话、没有委派子代理；结果写入项目记录。保留共享工作区WIP，未暂存、提交、推送、打tag或部署。所有快照、Cargo target、临时文件、日志、浏览器证据均在项目tmp/；使用Rust1.94.0、正常增量／并发，未查询账户额度。

## 组合验收

已验源码为 tmp/openrust-pennant-20261121/source，独占 target 为 tmp/openrust-core-ecosystem-20261121/target，TMPDIR 为项目 tmp/openrust-scout-20261121/temp 的绝对路径。以下日志相对 tmp/openrust-scout-20261121，Telescope补齐日志单列其目录。

| 验证 | 实际结果与证据 |
|---|---|
| 六新扩展全部features／targets | extensions-combined-final.log：103通过，10项需要Redis夹具而明确ignored；以下独立实机10项补齐，不计跳过为成功 |
| Horizon Redis8.10.3 | horizon-redis-final.log：runtime6；horizon-tags-final.log：标签／通知锁2，均专用隔离Redis |
| Telescope Redis8.10.3 | ../openrust-telescope-followup-20261121/redis-final.log：1项；隔离URL显式映射，不使用开发连接 |
| Pulse Redis8.10.3 | pulse-redis-final.log：1项；改为默认ignored并要求OPENRUST_TEST_REDIS_URL，避免测试隐式连接localhost服务 |
| 根全部features发现／配置／骨架及既有扩展 | root-combined-final.log：17通过，覆盖6新扩展、Cashier／Pay、发布／缓存／SourceInstallation与应用生命周期；默认骨架未预装可选Provider／config |
| 关闭Scout | configuration-off.log：命令禁用1＋骨架2通过 |
| Scout三库 | postgres-final.log／mysql-final.log各6，SQLite包含包6；根入口configuration-pgsql.log／configuration-mysql.log各1；实际版本PG18.7／MySQL8.4.13 |
| Telescope新增三库 | ../openrust-telescope-followup-20261121/watchers-postgres.log／watchers-mysql.log各2；SQLite包含组合。每函数独立数据库，实际测试不因缺变量提前返回 |
| 核心观察／契约／分页 | ../openrust-telescope-followup-20261121/core-tests.log：HTTP观察4、扩展契约3，SQLite和手工分页；MySQL条件函数缺连接时返回不计服务验收，Scout实际三库另有证据 |
| 实际响应链路 | ../openrust-telescope-followup-20261121/http-response-regression.log：23个真实HTTP测试，覆盖输入／结构化响应、缓存验证、文件准备、流、事件及下载 |
| Horizon当前核心真实进程＋浏览器 | horizon-process-final.log：实际父子树、Worker控制、崩溃替换／孤儿排空、信号／维护force、重试forget、资源退出／listen等通过；HORIZON_TEST_BROWSER=1，Chrome桌面手机授权／详情／标签写入／指标／CSP刷新／布局通过 |
| Telescope真实服务器＋浏览器 | ../openrust-telescope-followup-20261121/http-browser-final.log：桌面手机授权／CSRF／XSS／详情／监控／暂停恢复／布局、自定义path／domain／disabled通过 |
| Pulse真实服务器＋浏览器 | pulse-browser-final.log：桌面手机10卡片／3meters／SVG、4periods、刷新／XSS／授权／非法参数及Vapor退出通过 |
| 严格检查 | clippy-workspace-final.log：全workspace／all-features／all-targets -D warnings通过；fmt-final.log通过。第三方补丁依赖原有warning仍存在 |
| 三语指南 | ../openrust-telescope-followup-20261121/docs-build.log；专项Scout12＋Telescope12／通用12／导航6通过；导航支持状态19页动态计算，三语一致。Horizon专项12另见docs-horizon-final.log |
| 当前WIP API | api-final.log：全workspace／all-features／no-deps，RUSTDOCFLAGS=-D warnings通过；api-browser-final.log：19库入口＋7关键新接口锚点通过；api-wip-info.json保存已验源码指纹 |

Horizon包与此前完整进程验收快照逐文件一致；与当时核心相比Worker／事件／Artisan已有后续变更，故用本轮重编译程序实际复验，不沿用旧核心结果。Activitylog／Pennant／Pulse的三库与缓存全驱动矩阵见各包审计；没有因本轮只改诊断入口而虚称重复执行全部外部服务矩阵。

## API与Git边界

当前API仅为共享WIP源码验证，基准HEAD记录在api-wip-info.json，不将该HEAD冒充包含未提交源码的正式revision。没有调用要求干净提交的正式build_api.py导出流程，也没有覆盖旧正式API产物。发布用可追溯API导出仍须用户授权后的干净提交；当前Rustdoc入口在专属target/doc，可供本地查阅，不能作为正式发布包。

共享改动保持原样。最终核对范围内Rust源码与验收快照一致；若其他协作者之后修改源码，应依据指纹重验受影响行为。共享.env.example存在尾部空行差异，不属于本轮扩展代码；未改写或暂存该文件。Git核对不得以清理／reset覆盖其他工作。

## 已知支持边界

- Scout约定交付默认collection、null和泛型Engine扩展，不内置database／Algolia／Meilisearch／Typesense／Turbopuffer及外部语义服务；未知驱动和未支持语义调用明确失败。原生数据库全文入口另有指南。
- Telescope使用原生安全诊断UI；没有PHP任意字段／异步调用栈反射、Query／Gate来源过滤、multipart元数据或avatar。已知缓冲响应和路由字段已齐；未知Body／文件／流保持不读取。普通CLI表格／时间戳和PHP装饰不同。限制见三语正文，没有删除账本来冒充完整等价。
- Horizon的显式标签、transport ID桥接、轮询listen、resident memory及Unix信号适配已写入包审计。PHP参考fixture不等同完整PHPUnit；无Linux／Windows实机或真实收件人证据。
- 真实商户、外部托管服务、其他平台、正式发布矩阵和部署需要各自条件与授权，未作为本地模拟通过。未开发应用级设置后台或独立Dusk包，原路线已明确后置。

缓存继续保留复用；收尾实测可用76,419,448KiB（78.254GB／72.879GiB），高于32GB阈值，未触发清理。未残留本轮Cargo／HTTP／Horizon进程或标签容器，不删除数据库／数据卷。

## 本地提交核对（2026-11-21）

用户本轮明确要求提交文件。核定范围包括六个扩展、必要框架接口与回归、Cargo装配、三语指南、专项文档检查及对应计划／审计；源码逐文件与上述已验快照一致，沿用已通过的组合与严格检查结果。README及两份包审计的API指纹差异属于验收后的文档记录更新，Rust源码与Cargo输入未漂移。当前交接仅暂存扩展完成与本次提交条目，保留其他未提交交接内容。Docker配置／部署文档、Reverb独立修改、构建策略脚本与其他共享规则记录留在工作区，不纳入本次提交。本次仅本地提交，未推送；正式API导出与发布仍未执行。上文“未暂存／提交”为验收完成时的历史状态。
