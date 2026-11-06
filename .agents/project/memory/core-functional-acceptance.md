# Laravel 核心功能验收清单

> 受众：开发与验收 Agent；核对日期：2026-11-06。
> 基线：已提交 main `501e4a2543eb50521e26bcb984455b0673b0361e`；清单版本：1。
> 本次是既有源码、Git及真实验收记录审计，没有重新运行功能测试。AR-N01等未提交源码不增加已验收分子。

## 统计口径

- 完成率＝已验收功能单元数÷本清单核心功能单元总数。每个单元等权；部分完成与待开发均不计分，不用代码量、测试数或提交数折算。
- 本版188个开发者功能单元覆盖固定导航中的71个核心相关章节；官方基线见[固定来源](../plans/laravel-rebuild-baseline.md)。L编号是回溯锚点，一章可映射多个单元，一单元可跨章，不再用749个混合文档二级节作分母。
- 这衡量明确清单的验收完成率，不是工时完成率，也不是全部API方法逐个计数后的覆盖率。每行只承诺“验收边界”列；一行已验不关闭所属整章。宽泛剩余API、默认与生命周期另列未闭合，不能用一句“完整能力待做”隐藏已知具体缺口。
- 已验收：限定行为有main已提交的生产、调用方、示例/适用生成、文档及匹配验证证据；历史验收按实际未变输入继承，不声称在当前WIP重新测试通过。部分完成：已有实现/专项或必要配套，官方语义或完整验收仍缺；待开发：没有可计交付或关键机制仍待实现。
- 本版ID固定，后续新增/拆分/合并功能必须说明范围变化并更新清单版本，保留旧版于Git；不能通过增列已验小方法、删除困难项或移动扩展来制造增长。此文件维护功能验收事实，执行排期只在[唯一待办](../todo.md)。
- 日常真实数据库验收仅MySQL8.4；其他后端和最低版本发布前完整验证。五后端兼容和SQLite剩余能力仍在分母，未执行项不记通过。
- 包体与框架职责分开：Vite在根应用的资产集成职责计核心，Vite工具自身列扩展；框架广播计核心，Reverb服务器/Echo客户端列扩展；Auth/Guard/PasswordBroker计核心，Fortify/Passport/JWT模式列扩展。

统计校验：总项=188；已验收=72；部分完成=86；待开发=30；完成率=38.3%。

## 领域汇总

| 核心领域 | 已验收 | 部分完成 | 待开发 | 总项 | 完成率 |
| --- | ---: | ---: | ---: | ---: | ---: |
| 启动与交付 | 2 | 1 | 2 | 5 | 40.0% |
| 容器与Facade | 3 | 2 | 2 | 7 | 42.9% |
| Provider与包机制 | 2 | 1 | 2 | 5 | 40.0% |
| 配置 | 5 | 0 | 1 | 6 | 83.3% |
| 路由与URL | 5 | 2 | 1 | 8 | 62.5% |
| 请求 | 4 | 2 | 0 | 6 | 66.7% |
| 响应与Resource | 3 | 3 | 0 | 6 | 50.0% |
| 验证 | 2 | 1 | 1 | 4 | 50.0% |
| Session与CSRF | 7 | 2 | 0 | 9 | 77.8% |
| 视图与前端 | 2 | 3 | 0 | 5 | 40.0% |
| 控制器与中间件 | 0 | 2 | 0 | 2 | 0.0% |
| 数据库连接 | 3 | 2 | 2 | 7 | 42.9% |
| 事务 | 2 | 1 | 1 | 4 | 50.0% |
| Schema与迁移 | 3 | 3 | 2 | 8 | 37.5% |
| Query Builder | 8 | 10 | 0 | 18 | 44.4% |
| Eloquent Model | 4 | 3 | 3 | 10 | 40.0% |
| 分页 | 0 | 1 | 1 | 2 | 0.0% |
| Factory与Seeder | 0 | 1 | 2 | 3 | 0.0% |
| SQLite | 2 | 1 | 0 | 3 | 66.7% |
| Hashing | 3 | 0 | 1 | 4 | 75.0% |
| 加密 | 0 | 2 | 0 | 2 | 0.0% |
| 缓存 | 0 | 6 | 0 | 6 | 0.0% |
| 文件存储 | 1 | 4 | 1 | 6 | 16.7% |
| 事件与广播 | 0 | 3 | 2 | 5 | 0.0% |
| 队列与Bus | 1 | 4 | 1 | 6 | 16.7% |
| 任务调度 | 0 | 3 | 0 | 3 | 0.0% |
| 邮件与通知 | 0 | 4 | 0 | 4 | 0.0% |
| 翻译 | 2 | 2 | 0 | 4 | 50.0% |
| 日志与Context | 0 | 2 | 0 | 2 | 0.0% |
| HTTP Client与Process | 0 | 1 | 2 | 3 | 0.0% |
| CLI与生成器 | 3 | 2 | 0 | 5 | 60.0% |
| 测试支持 | 1 | 2 | 1 | 4 | 25.0% |
| 集合与辅助函数 | 0 | 3 | 0 | 3 | 0.0% |
| 限流 | 0 | 1 | 0 | 1 | 0.0% |
| 图像 | 0 | 0 | 1 | 1 | 0.0% |
| 核心搜索 | 0 | 0 | 1 | 1 | 0.0% |
| Redis | 0 | 1 | 0 | 1 | 0.0% |
| 认证与授权 | 4 | 5 | 0 | 9 | 44.4% |

## 逐项验收记录

证据链接定位原有正式验收章节或当前领域事实；缺口记录不等于运行失败，0%表示本组尚无完整闭合单元，不表示没有任何代码。

| ID | 核心领域 | 固定导航 | 验收边界 | 状态 | 证据入口 | 剩余边界 |
| --- | --- | --- | --- | --- | --- | --- |
| C001 | 启动与交付 | L007/L010/L011 | 根应用薄入口、独立框架包及直接cargo run | 已验收 | [根应用与框架独立包限定验收](testing.md#根应用与框架独立包限定验收) | 仅本行限定行为已验；其他同章行为见未闭合项 |
| C002 | 启动与交付 | L004/L011/L027 | production+log默认启动及脱敏初始化诊断 | 已验收 | [默认生产日志驱动启动与安全诊断限定验收](testing.md#默认生产日志驱动启动与安全诊断限定验收) | 仅本行限定行为已验；其他同章行为见未闭合项 |
| C003 | 启动与交付 | L005.07 | maintenance/down/up完整工作流 | 待开发 | [core](../core/core.md) | 未取得完整main对应验收 |
| C004 | 启动与交付 | L010 | 生产部署缓存/维护模式/零停机与worker运维 | 部分完成 | [server](../core/server.md) | 配置缓存与信号专项已有，完整Laravel部署流程和集群门禁未闭合 |
| C005 | 启动与交付 | L001/L002 | 框架版本支持、升级与破坏变更指南 | 待开发 | [laravel-rebuild-matrix-1](../plans/laravel-rebuild-matrix-1.md) | 固定基线已登记，面向用户完整升级与版本支持工作流未闭合 |
| C006 | 容器与Facade | L012.02/L012.03 | bind/singleton/scoped及请求作用域 | 已验收 | [R2-B 统一 Provider 内核（验收证据）](testing.md#r2-b-统一-provider-内核验收证据)；[R2-C 入口 scope 与受管清理（验收证据）](testing.md#r2-c-入口-scope-与受管清理验收证据) | 仅本行限定行为已验；其他同章行为见未闭合项 |
| C007 | 容器与Facade | L012.02/L012.03 | 工厂失败、循环、并发发布及关闭 | 已验收 | [R2-B 统一 Provider 内核（验收证据）](testing.md#r2-b-统一-provider-内核验收证据)；[R2-C 入口 scope 与受管清理（验收证据）](testing.md#r2-c-入口-scope-与受管清理验收证据) | 仅本行限定行为已验；其他同章行为见未闭合项 |
| C008 | 容器与Facade | L012.03 | extend保留原factory、实例与scope | 已验收 | [R2-Q/X main 组合（验收通过）](testing.md#r2-qx-main-组合验收通过) | 仅本行限定行为已验；其他同章行为见未闭合项 |
| C009 | 容器与Facade | L012.02 | contextual binding完整公开契约 | 待开发 | [todo](../todo.md) | 明确缺口 |
| C010 | 容器与Facade | L012.02/L012.03 | tags、call与方法注入 | 待开发 | [todo](../todo.md) | 明确缺口 |
| C011 | 容器与Facade | L014 | Facade解析、mock/swap完整流程 | 部分完成 | [facade](../core/facade.md) | 不将re-export当完整Facade |
| C012 | 容器与Facade | L035 | 核心公共Contract完整目录、可替换实现与生命周期契约 | 部分完成 | [core](../core/core.md) | 部分trait已实现，完整Contract reference与调用方未逐项闭合 |
| C013 | Provider与包机制 | L013.02/L013.03 | 全量register先于boot及失败资源回收 | 已验收 | [R2-B 统一 Provider 内核（验收证据）](testing.md#r2-b-统一-provider-内核验收证据) | 仅本行限定行为已验；其他同章行为见未闭合项 |
| C014 | Provider与包机制 | L013.02/L013.03 | 运行期register/force与同内核嵌套注册 | 已验收 | [R2-P / S-DV main 限定组合验收](testing.md#r2-p--s-dv-main-限定组合验收) | 仅本行限定行为已验；其他同章行为见未闭合项 |
| C015 | Provider与包机制 | L013.04 | 完整deferred ProviderRepository | 待开发 | [todo](../todo.md) | eager注册不能替代deferred |
| C016 | Provider与包机制 | L044.02 | package discovery与编译发现清单 | 待开发 | [todo](../todo.md) | 显式descriptor不算包发现 |
| C017 | Provider与包机制 | L044.03/L044.04/L044.05/L044.06/L044.07 | 包资源加载、覆盖、发布与优化命令完整公共机制 | 部分完成 | [core](../core/core.md) | 已有模块资源机制不等于完整包开发契约 |
| C018 | 配置 | L005.01/L005.03 | config函数、dotenv后单次依赖求值 | 已验收 | [配置可编辑默认值与有序依赖限定验收](testing.md#配置可编辑默认值与有序依赖限定验收) | 仅本行限定行为已验；其他同章行为见未闭合项 |
| C019 | 配置 | L005.03 | raw/batch/typed读取与missing/null区分 | 已验收 | [R2 四簇 main 组合限定验收](testing.md#r2-四簇-main-组合限定验收)；[R2-Q/X main 组合（验收通过）](testing.md#r2-qx-main-组合验收通过) | 仅本行限定行为已验；其他同章行为见未闭合项 |
| C020 | 配置 | L005.03 | 同仓储运行期set/set_many与快照边界 | 已验收 | [R2-R01 配置运行期原始写入（限定验收与资源收尾通过，提交见Git）](testing.md#r2-r01-配置运行期原始写入限定验收与资源收尾通过提交见git) | 仅本行限定行为已验；其他同章行为见未闭合项 |
| C021 | 配置 | L005.01/L005.03 | 有序来源、basename命名空间与Provider浅合并 | 已验收 | [R2 四簇 main 组合限定验收](testing.md#r2-四簇-main-组合限定验收)；[R2-R02 配置来源顺序（限定验收通过）](testing.md#r2-r02-配置来源顺序限定验收通过) | 仅本行限定行为已验；其他同章行为见未闭合项 |
| C022 | 配置 | L005.04 | config:cache/clear及重启屏障 | 已验收 | [R2 四簇 main 组合限定验收](testing.md#r2-四簇-main-组合限定验收) | 仅本行限定行为已验；其他同章行为见未闭合项 |
| C023 | 配置 | L005.02 | env:encrypt/decrypt官方工作流 | 待开发 | [配置可编辑默认值与有序依赖限定验收](testing.md#配置可编辑默认值与有序依赖限定验收) | env加密未闭合 |
| C024 | 路由与URL | L015 | 显式方法/any、Route元数据及405/OPTIONS | 已验收 | [C04-D7 真实路由上下文（限定子范围已验收）](testing.md#c04-d7-真实路由上下文限定子范围已验收)；[http](../core/http.md) | 仅本行限定行为已验；其他同章行为见未闭合项 |
| C025 | 路由与URL | L015/L016 | prefix/name-prefix及middleware alias/group | 已验收 | [R3-B Laravel 路由组前缀与 middleware](testing.md#r3-b-laravel-路由组前缀与-middleware)；[R3-D Laravel middleware alias 与 group](testing.md#r3-d-laravel-middleware-alias-与-group) | 仅本行限定行为已验；其他同章行为见未闭合项 |
| C026 | 路由与URL | L015/L024 | 域路由、命名路由与domain/path/query URL | 已验收 | [R3-C Laravel 命名路由与具名 URL 生成](testing.md#r3-c-laravel-命名路由与具名-url-生成) | 仅本行限定行为已验；其他同章行为见未闭合项 |
| C027 | 路由与URL | L015 | fallback、声明覆盖及业务404/405终态 | 已验收 | [C04-D7 真实路由上下文（限定子范围已验收）](testing.md#c04-d7-真实路由上下文限定子范围已验收)；[http](../core/http.md) | 仅本行限定行为已验；其他同章行为见未闭合项 |
| C028 | 路由与URL | L024 | signed/temporary signed URL签发验签 | 已验收 | [R2 四簇 main 组合限定验收](testing.md#r2-四簇-main-组合限定验收) | 仅本行限定行为已验；其他同章行为见未闭合项 |
| C029 | 路由与URL | L015/L024 | 可选参数及模型/枚举隐式绑定完整链 | 部分完成 | [http](../core/http.md) | 可选段与完整绑定仍缺 |
| C030 | 路由与URL | L015 | route:cache/clear编译与失效流程 | 待开发 | [http](../core/http.md) | 无完整main闭合验收 |
| C031 | 路由与URL | L024/L020.02 | URL复杂/混合/位置/模型参数、relative与missing resolver | 部分完成 | [http](../core/http.md) | D10-G/H只关闭限定动作索引与分派，完整参数族未完成 |
| C032 | 请求 | L019 | query/json/form bag、trim/null与动态读写 | 已验收 | [当前 main 组合限定验收](testing.md#当前-main-组合限定验收) | 仅本行限定行为已验；其他同章行为见未闭合项 |
| C033 | 请求 | L019 | 合法输入的标量/日期/Collection/Enum便捷读取 | 已验收 | [C04-B2 类型便捷方法](testing.md#c04-b2-类型便捷方法) | PHP8.5数值溢出/非有限值错误边界单列C188，不由本行关闭 |
| C034 | 请求 | L019/L016 | URL/path/query/内容协商与可信代理上下文 | 已验收 | [C04-G TrustProxies main 限定验收](testing.md#c04-g-trustproxies-main-限定验收)；[C04-D6 Request URL（限定子范围已验收）](testing.md#c04-d6-request-url限定子范围已验收) | 仅本行限定行为已验；其他同章行为见未闭合项 |
| C035 | 请求 | L019 | 有界上传文件树、正文reader及暂存取消清理 | 已验收 | [当前 main 组合限定验收](testing.md#当前-main-组合限定验收) | 仅本行限定行为已验；其他同章行为见未闭合项 |
| C036 | 请求 | L019/L026 | 完整Fileinfo/MIME/文件验证及Windows取消 | 部分完成 | [当前 main 组合限定验收](testing.md#当前-main-组合限定验收) | 原生Fileinfo、全规则与Windows边界未闭合 |
| C037 | 响应与Resource | L020/L027 | 语义HTTP状态、解析错误与5xx脱敏 | 已验收 | [当前 main 组合限定验收](testing.md#当前-main-组合限定验收)；[R3-A Laravel 语义 HTTP 状态默认与统一 JSON 响应](testing.md#r3-a-laravel-语义-http-状态默认与统一-json-响应) | 仅本行限定行为已验；其他同章行为见未闭合项 |
| C038 | 响应与Resource | L020.02/L025 | 请求绑定redirect/input/errors与flash持久 | 已验收 | [C04-D10-C 请求局部视图错误共享（已验收，已提交784045c）](testing.md#c04-d10-c-请求局部视图错误共享已验收已提交784045c)；[C04-D10-B 请求绑定重定向与 Session 错误包（限定子范围已验收）](testing.md#c04-d10-b-请求绑定重定向与-session-错误包限定子范围已验收)；[C04-D9 main 组合（限定子范围验收通过）](testing.md#c04-d9-main-组合限定子范围验收通过) | 仅本行限定行为已验；其他同章行为见未闭合项 |
| C039 | 响应与Resource | L020.02/L024 | named/action/secure/guest/intended/refresh/away限定参数 | 已验收 | [http](../core/http.md) | 仅本行限定行为已验；其他同章行为见未闭合项 |
| C040 | 响应与Resource | L068 | 完整API Resource/Collection条件字段及分页 | 部分完成 | [http](../core/http.md) | 完整Resource公开契约与分页未完成 |
| C041 | 响应与Resource | L020 | stream/download/SSE完整响应工厂契约 | 部分完成 | [http](../core/http.md) | 已有实现未完成官方全量对应 |
| C042 | 响应与Resource | L027 | 完整异常report/render注册、日志级别/忽略/重复与生产安全默认 | 部分完成 | [core](../core/core.md) | 已有Handler和安全错误显示，完整官方公开扩展矩阵未闭合 |
| C043 | 验证 | L026/L019 | typed Json/Query规则执行、422与嵌套路径 | 已验收 | [当前 main 组合限定验收](testing.md#当前-main-组合限定验收) | 仅本行限定行为已验；其他同章行为见未闭合项 |
| C044 | 验证 | L026/L022/L025 | MessageBag/ViewErrorBag及Session错误共享 | 已验收 | [C04-D10-C 请求局部视图错误共享（已验收，已提交784045c）](testing.md#c04-d10-c-请求局部视图错误共享已验收已提交784045c)；[C04-D10-B 请求绑定重定向与 Session 错误包（限定子范围已验收）](testing.md#c04-d10-b-请求绑定重定向与-session-错误包限定子范围已验收)；[C04-D10-A 错误包（限定子范围已验收）](testing.md#c04-d10-a-错误包限定子范围已验收) | 仅本行限定行为已验；其他同章行为见未闭合项 |
| C045 | 验证 | L026.03/L026.05 | FormRequest prepare/authorize/validated与自动失败跳转 | 待开发 | [todo](../todo.md) | 手动flash不能替代生命周期 |
| C046 | 验证 | L026 | 官方完整规则集、文件及自定义规则扩展 | 部分完成 | [http](../core/http.md) | 完整规则集与文件验证仍缺 |
| C047 | Session与CSRF | L025 | 属性树、start/save及flash/now/reflash/keep | 已验收 | [R2-P / S-DV main 限定组合验收](testing.md#r2-p--s-dv-main-限定组合验收)；[C04-D1 Session Store/flash（限定子范围已验收）](testing.md#c04-d1-session-storeflash限定子范围已验收) | 仅本行限定行为已验；其他同章行为见未闭合项 |
| C048 | Session与CSRF | L025 | remember/push/pull/remove/increment等Store API | 已验收 | [C04-D2 Store公开API（限定子范围已验收）](testing.md#c04-d2-store公开api限定子范围已验收) | 仅本行限定行为已验；其他同章行为见未闭合项 |
| C049 | Session与CSRF | L025 | ID/handler更换、迁移销毁及请求driver绑定 | 已验收 | [C04-D3 Session扩展生命周期（限定子范围已验收）](testing.md#c04-d3-session扩展生命周期限定子范围已验收) | 仅本行限定行为已验；其他同章行为见未闭合项 |
| C050 | Session与CSRF | L025 | 内置Cookie driver与加密保护链 | 已验收 | [C04-D4 内置Cookie Session driver（限定子范围已验收）](testing.md#c04-d4-内置cookie-session-driver限定子范围已验收) | 仅本行限定行为已验；其他同章行为见未闭合项 |
| C051 | Session与CSRF | L025 | 原生数据库及Redis共享持久化/TTL/撤销 | 已验收 | [session](../core/session.md) | 仅本行限定行为已验；其他同章行为见未闭合项 |
| C052 | Session与CSRF | L025 | 请求级GC lottery及expiry索引 | 已验收 | [C04-D5 GC lottery（限定子范围已验收）](testing.md#c04-d5-gc-lottery限定子范围已验收) | 仅本行限定行为已验；其他同章行为见未闭合项 |
| C053 | Session与CSRF | L025.04 | blocking、cache/file及完整driver选择 | 部分完成 | [session](../core/session.md) | 数据库/Redis不能抵扣其他driver |
| C054 | Session与CSRF | L017 | 显式web组、默认边界与419拒绝子范围 | 已验收 | [当前 main 组合限定验收](testing.md#当前-main-组合限定验收) | 9035a000主线组合及todo行472明确已验；不代表完整CSRF族 |
| C055 | Session与CSRF | L017 | 完整Token生成轮换、豁免、Cookie传递与全入口行为对应 | 部分完成 | [session](../core/session.md) | 已有CSRF与默认Web资产，完整公开机制和矩阵未闭合 |
| C056 | 视图与前端 | L021/L022 | 文件render、转义、根站点与路径越界拒绝 | 已验收 | [view](../core/view.md)；[C04-D10-C 请求局部视图错误共享（已验收，已提交784045c）](testing.md#c04-d10-c-请求局部视图错误共享已验收已提交784045c) | 仅本行限定行为已验；其他同章行为见未闭合项 |
| C057 | 视图与前端 | L022/L026 | 请求私有errors及context覆盖/对象身份 | 已验收 | [C04-D10-C 请求局部视图错误共享（已验收，已提交784045c）](testing.md#c04-d10-c-请求局部视图错误共享已验收已提交784045c) | 仅本行限定行为已验；其他同章行为见未闭合项 |
| C058 | 视图与前端 | L021/L022 | Blade职责等价组件/布局/directives | 部分完成 | [view](../core/view.md) | MiniJinja不等于完整Blade职责；资产构建只在C059计数 |
| C059 | 视图与前端 | L008/L023 | 根前端资源构建与框架集成工作流 | 部分完成 | [view](../core/view.md) | 静态首页与模板已有，完整官方资源构建流程未闭合；Vite独立工具本体应在扩展列 |
| C060 | 视图与前端 | L022 | view composer/creator、共享数据与完整模板扩展 | 部分完成 | [view](../core/view.md) | 请求errors共享不等于完整composer/creator生命周期 |
| C061 | 控制器与中间件 | L018 | Controller调用、依赖注入、单动作与资源Controller完整工作流 | 部分完成 | [http](../core/http.md) | 已有资源宏与宿主Controller，缺完整参数/中间件/调用依赖与生成对照 |
| C062 | 控制器与中间件 | L016 | 中间件优先级、排除、terminable与完整默认web/api栈 | 部分完成 | [todo](../todo.md) | alias/group顺序已验，不能抵扣完整priority/terminable和默认栈 |
| C063 | 数据库连接 | L057 | 标准MySQL/PG Provider建池、校验与受管关闭 | 已验收 | [DB 通用能力与驱动分层限定验收](testing.md#db-通用能力与驱动分层限定验收) | 限已验MySQL/PG标准Provider路径，不代表完整Manager |
| C064 | 数据库连接 | L057 | 显式具名连接选择与实例隔离 | 已验收 | [验证](../core/db.md#验证) | 限定显式具名实例；动态默认切换另计 |
| C065 | 数据库连接 | L057 | 显式读副本、主库scope与task隔离 | 已验收 | [验证](../core/db.md#验证) | 真实复制拓扑专项，不等于SQL解析式自动路由 |
| C066 | 数据库连接 | L057 | Laravel默认惰性连接与首次失败时机 | 部分完成 | [后续边界](../core/db.md#后续边界) | 当前boot会解析校验，完整官方时机未闭合 |
| C067 | 数据库连接 | L057 | 动态默认连接切换及usingConnection恢复 | 待开发 | [DB 通用能力与驱动分层限定验收](testing.md#db-通用能力与驱动分层限定验收) | 默认连接切换与恢复仍开放 |
| C068 | 数据库连接 | L057 | Manager purge/reconnect及扩展resolver | 待开发 | [DB 通用能力与驱动分层限定验收](testing.md#db-通用能力与驱动分层限定验收) | 完整Manager公开契约未交付 |
| C069 | 数据库连接 | L057 | 五后端与Laravel最低版本完整兼容 | 部分完成 | [后续边界](../core/db.md#后续边界) | MariaDB/SQLServer及PG10/MySQL5.7缺口；发布矩阵未闭合 |
| C070 | 事务 | L057 | 闭包事务提交、业务错误回滚与错误保真 | 已验收 | [当前实现](../core/db.md#当前实现) | 限现有闭包事务；重试另计 |
| C071 | 事务 | L057/L058 | Query绑定原事务、主库及未提交可见性 | 已验收 | [插入标识返回与事务执行限定验收](testing.md#插入标识返回与事务执行限定验收) | 960aca43限定真实两库和生成路径 |
| C072 | 事务 | L057 | 完整嵌套事务和savepoint公开契约 | 部分完成 | [后续边界](../core/db.md#后续边界) | 底层SQLite savepoint不替代框架完整入口 |
| C073 | 事务 | L057 | 官方transaction attempts及死锁重试 | 待开发 | [后续边界](../core/db.md#后续边界) | 完整重试契约未闭合 |
| C074 | Schema与迁移 | L060 | 已登记迁移清单up/down/status及宿主运行 | 已验收 | [SQLite 配置与迁移限定验收](testing.md#sqlite-配置与迁移限定验收) | 限既有Migrator路径，不含完整Artisan参数行为 |
| C075 | Schema与迁移 | L057/L060 | 表名前缀、物理名映射与迁移scope | 已验收 | [当前实现](../core/db.md#当前实现) | 限定resolver-aware路径；不迁移既有数据 |
| C076 | Schema与迁移 | L060 | SQLite普通表删列重建保持约束及行标识 | 已验收 | [SQLite 配置与迁移限定验收](testing.md#sqlite-配置与迁移限定验收) | 限main普通表；attached/virtual完整Schema另计 |
| C077 | Schema与迁移 | L060 | Schema完整建表改表索引外键接口 | 部分完成 | [后续边界](../core/db.md#后续边界) | SeaORM底层资产不能替代完整Laravel Schema |
| C078 | Schema与迁移 | L060 | migrate:fresh/refresh/reset完整流程 | 部分完成 | [SQLite 配置与迁移限定验收](testing.md#sqlite-配置与迁移限定验收) | 生成首次迁移不抵fresh/refresh/reset完整验收 |
| C079 | Schema与迁移 | L060 | migration batch、rollback steps及失败恢复 | 部分完成 | [后续边界](../core/db.md#后续边界) | 已有up/down/status不足以关闭完整批次与恢复行为 |
| C080 | Schema与迁移 | L060 | schema:dump与prune/加载schema快照 | 待开发 | [Query 剩余对应](../todo.md#query-剩余对应) | 尚无完整公开流程验收 |
| C081 | Schema与迁移 | L057/L060 | schema inspect/table/index/foreign key及数据库CLI监控 | 待开发 | [Query 剩余对应](../todo.md#query-剩余对应) | 完整schema inspection/monitoring能力不由历史迁移API抵扣 |
| C082 | Query Builder | L058 | table默认*及select替换投影 | 已验收 | [当前实现](../core/db.md#当前实现) | 限定真实MySQL示例与当前main组合，不含所有select变体 |
| C083 | Query Builder | L058 | 基本比较运算及AND/OR条件组 | 已验收 | [总控核心与并行分工（最新真人排期）](../todo.md#总控核心与并行分工最新真人排期) | ed3f608b限定C07-W01/W02验收 |
| C084 | Query Builder | L058 | 四种between-columns入口及列操作数边界 | 已验收 | [Query 列边界与布尔条件链限定验收](testing.md#query-列边界与布尔条件链限定验收) | cd0ac0f0；完整Query另计 |
| C085 | Query Builder | L058 | 六个多列条件入口及Closure路径 | 已验收 | [总控核心与并行分工（最新真人排期）](../todo.md#总控核心与并行分工最新真人排期) | ed3f608b限定C07-W01/W02验收 |
| C086 | Query Builder | L058 | exists/doesntExist保留原查询语义 | 已验收 | [Query 存在性终结操作限定验收](testing.md#query-存在性终结操作限定验收) | 9f51471a；SQLServer未验不由本行关闭 |
| C087 | Query Builder | L058 | existsOr/doesntExistOr惰性分支及返回类型 | 已验收 | [Query 存在性条件回调限定验收](testing.md#query-存在性条件回调限定验收) | 73a845d6；限定借用构造器回调路径 |
| C088 | Query Builder | L058 | beforeQuery共享子查询及冻结绑定生命周期 | 已验收 | [Query 查询前回调与共享子查询限定验收](testing.md#query-查询前回调与共享子查询限定验收) | a9103f93；完整FROM与跨连接不包含 |
| C089 | Query Builder | L058 | insertGetId同次返回、空行及sequence转换 | 已验收 | [插入标识返回与事务执行限定验收](testing.md#插入标识返回与事务执行限定验收) | 960aca43；Model getKey与Processor不可混同 |
| C090 | Query Builder | L058 | 完整join/lateral及写目标组合 | 部分完成 | [Query 剩余对应](../todo.md#query-剩余对应) | SQLite UPDATE/DELETE join/order/limit仍缺等价行为 |
| C091 | Query Builder | L058 | 完整union/from/subquery及跨连接组合 | 部分完成 | [Query 查询前回调与共享子查询限定验收](testing.md#query-查询前回调与共享子查询限定验收) | exists的UNION专项不能抵完整组合族 |
| C092 | Query Builder | L058 | 完整JSON查询族、路径及各方言 | 部分完成 | [总控核心与并行分工（最新真人排期）](../todo.md#总控核心与并行分工最新真人排期) | J01候选未接线编译，完整JSON族开放 |
| C093 | Query Builder | L058 | 完整聚合group/having/distinct边界 | 部分完成 | [当前实现](../core/db.md#当前实现) | 有实现但未取得完整功能族对应验收 |
| C094 | Query Builder | L058 | chunk/lazy/chunkById及修改边界 | 部分完成 | [当前实现](../core/db.md#当前实现) | 旧BatchReader专项不是完整Laravel家族 |
| C095 | Query Builder | L058 | 通用insert/insertOrIgnore/insertUsing/upsert完整族 | 部分完成 | [Query 剩余对应](../todo.md#query-剩余对应) | insertGetId验收不能替代通用写入口完整差异 |
| C096 | Query Builder | L058 | 通用update/updateOrInsert/increment/decrement/delete/truncate完整族 | 部分完成 | [Query 剩余对应](../todo.md#query-剩余对应) | 当前业务CRUD不是全部公开写方法验收 |
| C097 | Query Builder | L057/L058 | pessimistic locking/sharedLock/lockForUpdate与死锁行为 | 部分完成 | [Query 剩余对应](../todo.md#query-剩余对应) | 行锁取消专项不代表官方公开locking及重试完整族 |
| C098 | Query Builder | L058 | 全部whereIn/Null/Date/Time/Like及动态where族 | 部分完成 | [Query 剩余对应](../todo.md#query-剩余对应) | 不可从基本比较及多列限定闭环外推 |
| C099 | Query Builder | L058 | selectRaw/whereRaw/orderByRaw及绑定/会话mode | 部分完成 | [Query 剩余对应](../todo.md#query-剩余对应) | 目前受控Raw拒绝歧义，完整raw后继开放 |
| C100 | Eloquent Model | L064/L067 | 模型metadata连接及原始身份 | 已验收 | [模型属性与认证前置限定验收](testing.md#模型属性与认证前置限定验收) | 06b4d50c限定MB-A01，不含完整生命周期 |
| C101 | Eloquent Model | L064/L067 | 原始行水合与原始/有效属性分离 | 已验收 | [模型属性与认证前置限定验收](testing.md#模型属性与认证前置限定验收) | 06b4d50c，真实双库及PHP对照 |
| C102 | Eloquent Model | L067 | getter与基础cast读取 | 已验收 | [模型属性与认证前置限定验收](testing.md#模型属性与认证前置限定验收) | 06b4d50c，限基础casts |
| C103 | Eloquent Model | L067 | 异步setter及拥有式多字段回执 | 已验收 | [模型属性与认证前置限定验收](testing.md#模型属性与认证前置限定验收) | 06b4d50c，限原验收输入/有效字段变更 |
| C104 | Eloquent Model | L064 | save/create/update/delete、dirty及完整事件 | 部分完成 | [模型属性与认证前置限定验收](testing.md#模型属性与认证前置限定验收) | 持久save/dirty全链明确开放 |
| C105 | Eloquent Model | L064/L065 | 关系、eager/lazy加载与scopes | 部分完成 | [后续边界](../core/db.md#后续边界) | SeaORM Entity与自定义scope不足以抵完整Eloquent |
| C106 | Eloquent Model | L067/L069 | 日期/对象载体、全部cast与序列化 | 部分完成 | [总控核心与并行分工（最新真人排期）](../todo.md#总控核心与并行分工最新真人排期) | 日期仅前置Spec；完整对象/值载体和序列化未闭合 |
| C107 | Eloquent Model | L066 | Eloquent集合、转换与模型身份比较 | 待开发 | [Query 剩余对应](../todo.md#query-剩余对应) | 独立官方章节不能省略 |
| C108 | Eloquent Model | L064 | Mass assignment/unguarded及属性安全边界 | 待开发 | [Query 剩余对应](../todo.md#query-剩余对应) | 已有模型基础不等于完整批量赋值安全 |
| C109 | Eloquent Model | L064 | SoftDeletes/restore/prune完整全局scope生命周期 | 待开发 | [Query 剩余对应](../todo.md#query-剩余对应) | 生成nullable deleted_at和手工not_deleted不等价 |
| C110 | 分页 | L059/L068 | 完整LengthAware/Simple分页与links/meta/resource | 部分完成 | [Laravel 能力族](../progress.md#laravel-能力族) | 现有QueryPage/手动分页不能当完整官方流程 |
| C111 | 分页 | L059 | CursorPaginator排序游标及链接边界 | 待开发 | [当前实现](../core/db.md#当前实现) | CursorPaginator明确开放 |
| C112 | Factory与Seeder | L070 | Factory states/relationships/callbacks | 待开发 | [Laravel 能力族](../progress.md#laravel-能力族) | seed/TestDatabase不是Laravel Factory；按R5之后R6依赖 |
| C113 | Factory与Seeder | L061 | 官方db:seed类调用、模型事件及完整选项 | 部分完成 | [当前实现](../core/db.md#当前实现) | 已有SeedServiceProvider回调及db seed不等于官方完整流程 |
| C114 | Factory与Seeder | L070 | Factory sequence/recycle关系完整行为 | 待开发 | [Query 剩余对应](../todo.md#query-剩余对应) | states与基础关系以外的完整工厂生命周期仍待对应 |
| C115 | SQLite | L057/L060/L078 | 文件/URI/内存标准连接及池生命周期 | 已验收 | [SQLite 连接基础限定验收](testing.md#sqlite-连接基础限定验收) | 标准Provider、fresh与唯一示例限定行为 |
| C116 | SQLite | L057/L060/L078 | 逐物理连接PRAGMA与最低3.26身份及配置默认合同 | 已验收 | [SQLite 配置与迁移限定验收](testing.md#sqlite-配置与迁移限定验收) | 最低版迁移只作环境证据；删列重建仅C076计数，非全业务矩阵 |
| C117 | SQLite | L057/L060/L078 | 完整业务SQL/Queue/锁/Schema及驱动故障 | 部分完成 | [后续边界](../core/db.md#后续边界) | 同机SharedLocks不等于CacheLock/Queue/多机完整兼容 |
| C118 | Hashing | L055 | bcrypt/Argon密码生成与验证 | 已验收 | [统一 Hashing 与认证接线限定验收](testing.md#统一-hashing-与认证接线限定验收) | 仅本行三驱动生成/校验，不含全部Manager表面 |
| C119 | Hashing | L055 | needsRehash与算法/成本迁移判断 | 已验收 | [统一 Hashing 与认证接线限定验收](testing.md#统一-hashing-与认证接线限定验收) | 不包含Model hashed cast |
| C120 | Hashing | L055 | HashManager默认驱动及公开自定义驱动接线 | 已验收 | [统一 Hashing 与认证接线限定验收](testing.md#统一-hashing-与认证接线限定验收) | getDrivers/forgetDrivers/getContainer/setContainer尚缺 |
| C121 | Hashing | L055 | HashManager缓存枚举清除和Container访问替换 | 待开发 | [db](../core/db.md) | 唯一todo明确保留通用Manager缺口 |
| C122 | 加密 | L054 | Laravel Crypt加解密/序列化/错误合同 | 部分完成 | [security](../core/security.md) | 现为AES-GCM-SIV与自定义envelope，官方默认算法/格式/异常未闭合 |
| C123 | 加密 | L054 | APP_KEY与历史key轮换公开流程 | 部分完成 | [security](../core/security.md) | 现keyring有专项，官方配置及previous keys机制未完整验收 |
| C124 | 缓存 | L031 | 默认和具名store获取与自定义driver | 部分完成 | [cache](../core/cache.md) | manager/provider有实现；完整Laravel默认、公开API对应证据未闭合 |
| C125 | 缓存 | L031 | get/put/forget/TTL及批量缓存操作 | 部分完成 | [cache](../core/cache.md) | 基础get/set/remove有实现，Laravel全操作/默认TTL/失败值未完整验收 |
| C126 | 缓存 | L031 | remember/rememberForever/pull/add/increment/decrement | 部分完成 | [cache](../core/cache.md) | singleflight loader不能替代完整Laravel方法合同 |
| C127 | 缓存 | L031 | 标签失效和受控旧值回填 | 部分完成 | [cache](../core/cache.md) | generation专项存在，Laravel tags/flush公开语义未完整对应 |
| C128 | 缓存 | L031 | 原子锁获取/阻塞/owner恢复/释放 | 部分完成 | [cache](../core/cache.md) | SharedLocks资产不等于Cache::lock完整默认与driver矩阵 |
| C129 | 缓存 | L031 | flexible及多层/故障转移store | 部分完成 | [cache](../core/cache.md) | 现SWR和本机协调不等于全部官方缓存合同 |
| C130 | 文件存储 | L037.05 | 本地disk公开visibility读取与变更 | 已验收 | [ST-V main 限定组合验收](testing.md#st-v-main-限定组合验收) | 真实S3 ACL及throw=false仍不在本行 |
| C131 | 文件存储 | L037.02–03/L037.09 | 具名/按需disk及自定义driver | 部分完成 | [storage](../core/storage.md) | Provider/manager资产已有，完整官方disk默认/按需配置仍缺对应验收 |
| C132 | 文件存储 | L037.04–05 | 文件读写、流式上传下载及metadata | 部分完成 | [storage](../core/storage.md) | 已有TCP/MinIO专项，throw=false/URL/元数据等完整合同未闭合 |
| C133 | 文件存储 | L037.05–07 | copy/move/append/prepend/delete与目录操作 | 部分完成 | [storage](../core/storage.md) | 完整官方操作族及各driver合同未闭合 |
| C134 | 文件存储 | L037.04–05 | S3可见性、URL/临时URL与失败语义 | 部分完成 | [ST-V main 限定组合验收](testing.md#st-v-main-限定组合验收) | 真实ACL启用/禁用bucket、迟到CopyObject、throw=false仍开放 |
| C135 | 文件存储 | L037.08 | Storage fake/assertions | 待开发 | [storage](../core/storage.md) | 未有完整官方fake断言闭合证据 |
| C136 | 事件与广播 | L036.03–05/L036.07 | 事件定义/手动监听/顺序分派 | 部分完成 | [events](../core/events.md) | typed listener实测已有，固定官方注册/分派/停止语义仍待全量对应 |
| C137 | 事件与广播 | L036.06–07 | 排队监听与事务后分派 | 部分完成 | [events](../core/events.md) | 现事务内持久排队不等于官方after_commit/shouldQueue完整行为 |
| C138 | 事件与广播 | L036.02–03/L036.08 | 事件发现/订阅者与生成器 | 待开发 | [events](../core/events.md) | 完整discovery/subscriber/CLI尚缺闭合证据 |
| C139 | 事件与广播 | L036.09 | Event fake及局部fake断言 | 待开发 | [events](../core/events.md) | 尚无固定官方fake对应的完整验收 |
| C140 | 事件与广播 | L030 | 广播事件/私有presence频道认证 | 部分完成 | [realtime](../core/realtime.md) | WS/SSE/Redis跨节点不等于Laravel Broadcast公开合同 |
| C141 | 队列与Bus | L046 | 具名连接按实际使用懒构造及公开driver贡献 | 已验收 | [标准依赖与队列开箱限定验收](testing.md#标准依赖与队列开箱限定验收) | 默认错误时点/DeferrableProvider另列未完成 |
| C142 | 队列与Bus | L046 | typed任务派发/延迟/worker消费 | 部分完成 | [queue](../core/queue.md) | 生产实库链已有，完整Laravel dispatch/job API和默认参数仍待对应 |
| C143 | 队列与Bus | L046 | after_commit及事务回滚不派发 | 部分完成 | [queue](../core/queue.md) | dispatch_in_transaction不等于Laravel完整after_commit/outbox |
| C144 | 队列与Bus | L046 | 重试/failed_jobs/timeout/释放与worker参数 | 部分完成 | [queue](../core/queue.md) | 租约/重试/SIGTERM专项已有，官方完整worker默认和参数仍开放 |
| C145 | 队列与Bus | L046 | 唯一任务/chain/batch及取消回调 | 部分完成 | [queue](../core/queue.md) | 现独立组parent模型重试从头运行，完整Bus/Chain/Batch不同 |
| C146 | 队列与Bus | L046/L079 | Queue/Bus fake与断言 | 待开发 | [queue](../core/queue.md) | driver fake专项不能代替Laravel完整fake表面 |
| C147 | 任务调度 | L050 | cron/时区/DST频率调度 | 部分完成 | [schedule](../core/schedule.md) | 现DB slot/UTC显式cron专项，完整Laravel表达式及默认公开合同仍待 |
| C148 | 任务调度 | L050 | onOneServer/withoutOverlapping/background及条件 | 部分完成 | [schedule](../core/schedule.md) | slot唯一性不等于逐任务官方mutex/重叠策略 |
| C149 | 任务调度 | L050 | schedule命令、任务输出和生命周期hooks | 部分完成 | [schedule](../core/schedule.md) | run/list已有；官方完整参数/输出/hooks/维护模式未闭合 |
| C150 | 邮件与通知 | L042 | 具名mailer、transport和完整MailManager | 部分完成 | [mail](../core/mail.md) | SMTP/log/fake有资产；具名mailer/channel/默认全合同未闭合 |
| C151 | 邮件与通知 | L042 | Mailable内容/附件/模板/Markdown与预览 | 部分完成 | [mail](../core/mail.md) | lettre/MiniJinja模板可用，不等于全部Mailable API与工作流 |
| C152 | 邮件与通知 | L043 | Notification路由、channel、队列与数据库通知 | 部分完成 | [mail](../core/mail.md) | 现email/SMS固定payload编排，完整Notification/channel/database/read状态未闭合 |
| C153 | 邮件与通知 | L042/L043/L079 | Mail/Notification fake与发送断言 | 部分完成 | [mail](../core/mail.md) | FakeMailer存在，不等于官方全部断言/按需通知等合同 |
| C154 | 翻译 | L041 | 应用错误显示统一locale及未指定时默认语言 | 已验收 | [应用错误国际化限定验收](testing.md#应用错误国际化限定验收) | 本行不包含完整Laravel Loader/namespace/plural API |
| C155 | 翻译 | L041 | 显式locale作用域与错误渲染脱敏/code稳定 | 已验收 | [应用错误国际化限定验收](testing.md#应用错误国际化限定验收) | 不把Accept-Language选择策略冒称Laravel默认 |
| C156 | 翻译 | L041 | 文件loader、JSON/namespace/缺失及fallback | 部分完成 | [i18n](../core/i18n.md) | 已有MessageCatalog/fallback；完整Laravel文件/包namespace/loader机制仍未闭合 |
| C157 | 翻译 | L041 | trans_choice复数与占位符/格式行为 | 部分完成 | [i18n](../core/i18n.md) | ICU/CLDR plural和select不等于Laravel全部区间选择/占位符 |
| C158 | 日志与Context | L028 | 具名日志channel/stack/级别/结构化上下文 | 部分完成 | [audit](../core/audit.md) | tracing/OTel专项存在，不等于Laravel LogManager全部公开配置/生命周期 |
| C159 | 日志与Context | L034 | Context公开管理、隐藏上下文与hydration | 部分完成 | [core](../core/core.md) | ExecutionContext传播已有，固定官方Context全表面及安全边界待验收 |
| C160 | HTTP Client与Process | L039 | 出站HTTP请求、认证、timeout/retry/错误 | 部分完成 | [http](../core/http.md) | 有界客户端实现不等于Laravel Http全部默认/扩展链 |
| C161 | HTTP Client与Process | L039.03–05 | HTTP pool/batch/macro/fake防意外请求 | 待开发 | [http](../core/http.md) | 完整公开并发/fake/macro合同未闭合 |
| C162 | HTTP Client与Process | L045 | Process命令/环境/流/超时/并发/fake | 待开发 | [core](../core/core.md) | Tokio进程能力不能算Laravel Process已实现 |
| C163 | CLI与生成器 | L029/L005.02 | env命令返回当前应用有效环境 | 已验收 | [应用环境命令限定验收](testing.md#应用环境命令限定验收) | 仅该命令，不代表完整Artisan使用方式 |
| C164 | CLI与生成器 | L029 | 共享命令注册覆盖及快照生命周期 | 已验收 | [共享命令所有权前置限定验收](testing.md#共享命令所有权前置限定验收) | 不包含完整延迟Provider/Artisan发现与全部命令 |
| C165 | CLI与生成器 | L004/L029 | 应用生成可直接构建启动与框架包解析 | 已验收 | [根应用与框架独立包限定验收](testing.md#根应用与框架独立包限定验收) | 只当前已交付生成选项，不含全部官方生成器 |
| C166 | CLI与生成器 | L029 | make命令族名称、参数、选项与生成行为 | 部分完成 | [cli](../core/cli.md) | 当前generate及部分生成器仍待Artisan完整对齐 |
| C167 | CLI与生成器 | L029 | 交互输入/提示、隔离命令、信号与程序调用 | 部分完成 | [cli](../core/cli.md) | 完整Laravel Console contract尚未闭合 |
| C168 | 测试支持 | L074/L075 | TestApp生产装配、Provider替换与基础HTTP响应断言 | 已验收 | [R2-D TestApp 验收证据](testing.md#r2-d-testapp-验收证据) | 限定当前状态/header/JSON与Provider覆盖，不含全部官方测试断言 |
| C169 | 测试支持 | L078 | RefreshDatabase及数据库断言工作流 | 部分完成 | [test](../core/test.md) | 隔离库和迁移不等于完整Laravel数据库测试trait/工厂 |
| C170 | 测试支持 | L079 | 时间旅行及容器mock/spy工作流 | 待开发 | [test](../core/test.md) | 完整Interacting With Time及通用mock契约未闭合；各能力fake在对应领域单列 |
| C171 | 测试支持 | L076 | Console命令交互/输入输出/退出码完整测试API | 部分完成 | [test](../core/test.md) | 当前有实际命令进程验收，未闭合官方全部console断言和交互测试公开接口 |
| C172 | 集合与辅助函数 | L032 | Collection/LazyCollection完整方法及高阶代理 | 部分完成 | [core](../core/core.md) | 部分集合资产不等于Laravel Collection完整语义 |
| C173 | 集合与辅助函数 | L038/L049 | Str/Stringable、Arr与数字/日期/path helpers | 部分完成 | [core](../core/core.md) | Rust标准库能力不自动等于Laravel公开helper流程 |
| C174 | 集合与辅助函数 | L033 | Concurrency驱动与并发/延后任务公开合同 | 部分完成 | [core](../core/core.md) | Tokio并发资产不等于固定官方driver/进程/同步及defer语义 |
| C175 | 限流 | L047 | RateLimiter计数/期限/attempt及命名路由限流完整合同 | 部分完成 | [http](../core/http.md) | 已有认证/共享计数限流专项，完整官方默认、命名策略和公开API未闭合 |
| C176 | 图像 | L040 | 图像读取/变换/响应/驱动扩展 | 待开发 | [laravel-rebuild-matrix-2](../plans/laravel-rebuild-matrix-2.md) | 固定Laravel images核心公开能力尚无完整实现和验收，现成Rust图像包不自动算交付 |
| C177 | 核心搜索 | L048 | 核心全文/向量检索、重排及组合查询 | 待开发 | [laravel-rebuild-matrix-2](../plans/laravel-rebuild-matrix-2.md) | 按固定search章节核核心公开行为；Scout独立包不计本行，尚无完整实现/验收 |
| C178 | Redis | L062 | Redis连接/命令/pipeline/事务/pubsub公开流程 | 部分完成 | [cache](../core/cache.md) | Cache/Queue内部Redis driver有实测，不等于框架Redis完整公开API/连接管理 |
| C179 | 认证与授权 | L051 | 具名Guard选择、默认切换与请求内实例缓存 | 已验收 | [R7 具名认证核心限定验收](testing.md#r7-具名认证核心限定验收) | 限R7已验Guard/Provider，不包含整个AuthManager全表面 |
| C180 | 认证与授权 | L051 | UserProvider凭据读取与校验 | 已验收 | [R7 具名认证核心限定验收](testing.md#r7-具名认证核心限定验收) | 限DatabaseUserProvider/GenericUser；账户身份归C182，完整Eloquent持久写链另列 |
| C181 | 认证与授权 | L051 | StatefulGuard登录、退出、once与remember | 已验收 | [R7 具名认证核心限定验收](testing.md#r7-具名认证核心限定验收) | 按R7实际SessionGuard范围；JWT/Fortify不计本行 |
| C182 | 认证与授权 | L051 | 数字/字符串账户身份及数据库/表来源隔离 | 已验收 | [R7 具名认证核心限定验收](testing.md#r7-具名认证核心限定验收) | 请求实例缓存只在C179计数；非整个R7全量关闭 |
| C183 | 认证与授权 | L051 | 完整HTTP Basic、viaRequest及自定义Guard工作流 | 部分完成 | [auth](../core/auth.md) | 现具名Guard已验不代表全部官方认证入口与扩展 |
| C184 | 认证与授权 | L052 | Gate定义、before/after、授权响应与Policy发现 | 部分完成 | [auth](../core/auth.md) | 现RBAC/Policy数据库实现不能替代官方Gate/Policy全契约 |
| C185 | 认证与授权 | L052 | 资源授权middleware、控制器授权与模型Policy全链 | 部分完成 | [auth](../core/auth.md) | 存在权限校验专项，完整Laravel公开工作流未闭合 |
| C186 | 认证与授权 | L053 | 邮箱验证通知、签名链接与Verified中间件 | 部分完成 | [auth](../core/auth.md) | 现验证码流程不等于Laravel官方邮箱验证流程 |
| C187 | 认证与授权 | L056 | PasswordBroker重置Token、通知与密码确认 | 部分完成 | [auth](../core/auth.md) | 现AuthFlows重置资产不等于官方broker/notification完整机制 |
| C188 | 请求 | L019 | 数值越界及非有限值整数转换的PHP异常边界 | 部分完成 | [Query 剩余对应](../todo.md#query-剩余对应) | PHP8.5 float越界/INF/NaN整数转换会触发框架异常，Request/scalars生产链仍待逐项复核 |

## 核心范围与扩展分类

核心相关章：L001、L002、L004、L005、L007、L008、L010、L011、L012、L013、L014、L015、L016、L017、L018、L019、L020、L021、L022、L023、L024、L025、L026、L027、L028、L029、L030、L031、L032、L033、L034、L035、L036、L037、L038、L039、L040、L041、L042、L043、L044、L045、L046、L047、L048、L049、L050、L051、L052、L053、L054、L055、L056、L057、L058、L059、L060、L061、L062、L064、L065、L066、L067、L068、L069、L070、L074、L075、L076、L078、L079。L001/L002只计版本/升级交付职责；文档介绍、贡献流程本身不作为额外已完成功能计分。每个核心章均有上表映射，脚本核对差集。

| 不计核心分母的范围 | 分类与原因 |
| --- | --- |
| L003 contributions | 贡献指南，不是运行/开发产品功能 |
| L006 ai、L071–L073 | 官方AI/MCP/Boost包及配套 |
| L009 starter-kits | 官方可选起步套件，不以其替代核心认证机制 |
| L063 mongodb | 单独可选数据库生态包；既定五后端兼容仍在核心 |
| L077 dusk、L080–L102 | 官方可选生态及工具；混合章中的核心公共机制仍由核心表计数 |
| laravel-modules | 独立第三方扩展；已授权必须实现，单列不表示取消 |
| JWT/OIDC、数据库翻译来源、gRPC/MQTT、DB动态设置 | 已授权增强或扩展；核心对应机制仍保留 |
| 守护、专项测试库持久恢复 | 工程增强，不以工程修复抵扣Laravel核心功能 |

## 扩展独立状态

扩展未建立与核心等粒度的完整闭环清单，因此本表不给扩展套用核心百分比；范围保留于[官方全量交付](../plans/laravel-official-coverage.md)，不得用“可选”删除已授权开发责任。

| 扩展 | 当前可证状态 | 尚未闭合 |
| --- | --- | --- |
| laravel-modules | 独立包及注册/资源/依赖/启停子范围已验，6c21bab0 | 完整发现、命令、生成/发布等35入口；不是全包100% |
| Fortify/TOTP | 官方Web默认/显式JWT组合有main限定验收 | Google真实客户端、完整Fortify行为与边界仍待闭合 |
| API+Bearer JWT | 显式无Cookie profile有组合验收 | 不能等同完整官方认证生态或所有标识/跨入口边界完成 |
| Passport、OIDC Provider、双应用SSO | 协议/Client/Repository候选为WIP | 授权码/PKCE持久签发、OP、双RP及退出完整链路 |
| 数据库翻译来源 | 来源叶/测试代码已编码 | Loader、持久化、Provider、配置、示例/生成/文档完整接线与验收 |
| gRPC、可选WS/SSE增强 | 历史真实TCP/TLS/跨节点专项已有 | 完整对应/生命周期/生产拓扑；不抵扣Laravel广播全契约 |
| MQTT Broker/Client、DB动态设置/动态计划 | 已授权后继 | 尚未形成可验完整交付；不建第二套Provider/调度体系 |
| Sanctum、Socialite | 既有PAT/外部OIDC资产 | 完整官方包对应不能由同类能力替代 |
| Horizon、Pulse、Telescope、Octane、Pail、Tinker | 范围已登记 | 各包运行机制及必要界面/工作流完整交付 |
| Scout、Pennant、Reverb、Echo | 范围已登记，存在部分相关底层资产 | 包公开契约和集成验收；不以日志/WS实现替代 |
| Cashier/Stripe、Cashier/Paddle | 范围已登记 | 固定版本、持久/回调/计费及真实服务条件 |
| AI SDK、MCP、Boost | 已授权范围，未形成已验完整包 | 不等同尚未开工的独立AI业务产品 |
| Folio、Head、Precognition、Passkeys、起步套件 | 部分请求Precognition相关子行为有验收，其余按计划 | 完整包及必要前端工作流 |
| Vite/Wayfinder/Stream、Sail/Pint/Valet/Homestead、Dusk/Envoy/Prompts/Mix及开发工具 | 范围登记，Rust对应需分包举证 | 不能以Cargo/Clippy存在直接认定所有工具完成 |
| MongoDB、具体SMS渠道等额外驱动 | 可选生态/驱动资产，Twilio真实投递缺外部授权 | 与框架核心数据库/Notification机制分开验收 |
| 持续开发守护、测试库安全回收 | 本机对应子范围已有提交与隔离/实进程证据 | 不将发送回执算业务推进，不将Unix专项算全平台 |

## 成熟 Rust 包复用原则

按真人最新要求，优先选择优秀成熟Rust包，经框架公开机制集成；必要时做有边界的二开/补丁，避免重复造轮子。无法满足明确contract时才自研，并在Spec说明缺口。复用要核维护、许可证、安全、版本与依赖约束；新增依赖写stack.md，来源与vendor补丁按现有规则维护。验收仍针对OpenRust对固定Laravel的默认、生命周期、错误安全和扩展契约；引入包不自动增加完成项。

## 维护与复算

运行 `python3 -B .agents/project/scripts/summarize_core_acceptance.py --json` 校验ID、核心章覆盖、证据文件/章节、总数及领域比例。每次有限簇验收提交后，只升级有新证据的对应行，重算本页与progress摘要；更新仍不得修改旧运行日志或把分支/WIP当main通过。
