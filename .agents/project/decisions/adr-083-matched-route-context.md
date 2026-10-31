# ADR-083：真实路由上下文与受管理方法声明

- 状态：Accepted；D7限定子范围已验收
- 日期：2026-10-30
- 对应：R3/C04-D7、L015/L019；不关闭完整Routing/绑定/Request

## 决策

1. 原生Axum MethodRouter隐藏方法及fallback声明，不能可靠反射；route_layer漏any、layer又包含默认405。采用仅包装现Axum的公开`RouteMethods`受管理声明，保存已知方法/handler和路由middleware链，最终装配后在业务middleware之前发布真实命中，不另建路径匹配器。
2. `Routes::route(path, RouteMethods)`替代旧opaque MethodRouter参数；公开`http::routing::{get,post,put,patch,delete,head,options,query,on,any}`及方法链。既有`Routes::get/post/...`不增加业务样板；原生Axum Router/MethodRouter仍是明确逃生口，不偷偷转换为有完整上下文的受管理声明。所有第一方调用方、生成器和文档同步迁移。
3. `on`/`match_methods`以方法字符串列表声明并校验，不局限于Axum MethodFilter；必须支持固定上游QUERY与显式自定义合法方法。`any`严格采用固定Laravel的GET/HEAD/POST/PUT/PATCH/DELETE/OPTIONS/QUERY，不把TRACE/CONNECT自动扩进去。
4. 新公开`Route`是当前请求的owned只读快照，提供name（可空）、uri（Laravel去首斜线、根保/）、domain、methods及保序parameters；参数保存当前路径/域解码值，路径同名优先。提供parameter/parameter_or及named多模式匹配，不能从query/body取值。
5. RequestInfo/Request公开`route() -> Result<Option<Route>>`、`route_name()`、`route_is(&[&str])`与`route_parameter`/`route_parameter_or`。无Route时连带default读取仍返回None；有Route且缺参数才取default。参数当前以serde_json Value表示已支持绑定值，不冒称模型/枚举绑定已实现。
6. 根HTTP入口逐请求初始化私有共享slot，Info保存同一句柄；匹配成功并bind参数后、路由middleware之前发布。全局前为空、下游后可见；显式业务404/405/500或路由middleware短路仍保命中。不能凭状态码、header、MatchedPath或named URL registry推断。
7. 公开RouteResolver（Send+Sync同步closure，返回Result<Option<Route>>）及get/set_route_resolver允许可信应用扩展，镜像上游公开setter。默认resolver用弱引用避免get后再set形成内部Arc环；客户端头不能修改私有状态。应用主动覆盖与框架实际匹配事实区分，不能把上游应用扩展误禁。
8. 延迟materialize每条路由，保group/nest/merge的最终URI/domain/name、方法及middleware顺序；候选InternalMiss显式回滚共享slot内部状态，不能只还原extensions。forward后最终命中覆盖原候选，不泄至其他请求。
9. 固定Laravel显式fallback仅GET/HEAD，低于同方法普通路由；GET到仅POST路径可命中GET fallback。原先方法miss先于fallback/任意方法业务fallback的规则按本ADR纠正，相关旧ADR/测试/文档同步更新，不能沿旧断言保留已知偏差。
10. 有alternate可用方法且当前为OPTIONS时合成并bind真实OPTIONS Route，返回200空正文及官方Allow；alternate仅按固定官方verbs顺序检查，Allow以逗号无空格连接。已注册GET/HEAD根fallback也参与alternate候选，因此未知路径OPTIONS可返回200及GET,HEAD；只有不存在任何候选时才为内置404且无Route，默认405也无Route。静态文件直出不记录业务Route；声明fallback仅在真正选中执行时发布，不把OPTIONS合成路由误记成fallback本身。不得把所有自动OPTIONS视作无命中。
11. public参数、resolver与原生逃生口的Rust所有权/Result/限额适配须真实测试并文档化；声明defaults、可选参数、约束及显式binder/枚举/模型绑定等未支持项仍按后继Routing/R5依赖交付，不用本簇字符串捕获关闭完整Route。
12. 无新依赖或第二Registry。middleware/alias/group/Provider/执行scope沿现机制；路由名称仍供原出站URL生成器使用，独立于本请求状态但共享同一声明身份，重命名/嵌套/合并不漂移。
13. 验收覆盖全局前/route middleware/handler/全局后、GET/HEAD/QUERY/any/自定义方法、匿名与具名、参数与default、domain候选miss、nest/merge/forward、业务错误/短路、显式fallback/静态404405/自动OPTIONS、resolver get/set/clone与请求隔离；唯一模块/生成应用/真实TCP/适用双库/完整门禁及不同模型审查后限定提交。

## 依据与当前边界

固定[Laravel Router](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Routing/Router.php)、Route、Request、RouteParameterBinder与AbstractRouteCollection及其真实测试已只读核对，来源清单在`/tmp/openrust-c04d7-route-audit/sources.json`。任何源码推导不冒称执行过PHP。原型仅用缓存Axum0.8.9/Tower0.5.3/Tokio1.54.1在独立/tmp target离线通过8项，证明受管理方法声明/发布/回滚可行，未证明OpenRust完整生产集成。

同名参数额外证据：`/tmp/openrust-c04d7-route-audit/duplicate-probe.php`使用真实PHP8.5、Symfony Routing7.4官方RouteCompiler/Route/CompiledRoute及固定Laravel RouteParameterBinder；2026-10-30审查者执行与主控独立复跑均退出0。跨host `{id}.example.test`与path `/{id}`编译成功且绑定为路径值；单个host或path内部重复名分别抛LogicException。Laravel Route/Request仅为最小fixture，此证据不代表完整Laravel HTTP或OpenRust应用验收。

本ADR限定实现及验收已完成；D7真实Route/Resolver与受管理方法声明已通过限定验收：final3完整12门禁、默认695/29 ignored、全特性1403/103 ignored、根PG18/MySQL8.4各64、fresh5/2/2、真实TCP7及不同模型复审；忽略项不算真实服务。根target清零，五旧consumer不变，限定提交以Git为准；完整Routing/Request与previous/Precognition仍开放。后继Session previous依赖本簇，Precognition必须真实验证/分发而非仅信header。R3-E/F旧fallback已验收事实保留，冲突默认语义以本ADR实现后新证据替代。


## D10-G：动作身份与URL生成契约（限定子范围已验收）

1. 沿同一真实路由登记链新增 `get_action(path, action, handler)` 与通用 `on_action(path, methods, action, handler)`；GET含HEAD，其他方法/组合/自定义方法复用现on，不另造执行器或URL-only注册表。
2. 公开owned `RouteAction` 保存字符串与二元组输入形态，支持&str/String及借用/owned二元组转换；方法字符串、裸invokable身份与二元组均与显式Rust handler同次登记，不用函数地址/类型名假反射或声称类字符串自动实例化控制器。
3. `RouteGroup::namespace`按固定登记规范组合；BootContext公开 `set_root_controller_namespace(namespace)->Result<&mut Self>`只设置最终生成器查询默认根空间，不改路由登记namespace，两者与name_prefix独立。
4. 同时提供 `RouteUrlGenerator::set_root_controller_namespace(&mut self, namespace)->Result<&mut Self>`，空串清空，仅本实例查询状态变化；clone及已捕获请求快照隔离，索引仍共享不可变Arc，不用全局锁或重建整表。明确这是Rust所有权适配，不声称跨clone共享PHP对象引用。
5. 实际命中Route提供 `action()->Option<&str>`，可信resolver可用 `with_action`构造元数据但不注册URL；默认动作只来自真实匹配条目，保留上游声明形式，索引另规范化。
6. 最终动作索引从与匹配同源的完整声明集合派生；实际固定PHP Route/RouteCollection的add/refresh/get/getByAction探针7场景已单次通过：固定源码以methods顺序的管道拼接再直接连接domain与uri所得完整字符串键相同时替换淘汰旧项（不等同三元组相等；无分隔连接碰撞也属同键），分拆逐方法全覆盖或不同methods顺序仍保旧动作索引；域集合先、普通同动作首项，后写前导反斜线的exists/trim不对称可覆盖规范key。保留声明顺序与原形，不用逐方法有效handler过滤代替collection索引，也不改既有请求分派。证据 `/tmp/openrust-c04d10g-collection-probe.output.jsonl`；这是实际集合与Route构造，不是完整Laravel HTTP，前三次helper装载失败保留。
   补充实际碰撞探针 `/tmp/openrust-c04d10g-collection-collision-probe.output.jsonl` 两案通过：domain/uri无分隔碰撞和domain/ordinary左侧union均可淘汰旧动作。当前Rust请求分派仍按(domain,path)结构化身份保留处理器，与PHP集合碰撞后的隐藏路由不同；本簇仅对应动作索引，此请求分派差异作为Routing后继缺口保留，不宣称全Routing等价。
7. 同一 `RouteUrlGenerator::action(action,&BTreeMap<String,String>)->Result<String>` 与route共享渲染；只有动作没有name时仍提供完整生成器。root字符串、绝对字符串、二元组及invokable查询按固定formatAction处理。
8. `Redirector::action/action_with`默认302/空头，先动作生成再to_with，保留原请求借用/Session闪存链，无Session普通跳转可用。
9. 非法动作通过 `http_route_action_invalid`启动/查询Result失败，未知动作 `http_route_action_not_found`，缺生成器/参数/URL错误沿既有code；不panic，不擅加PHP类名正则，空成员/控制字符等必要Rust输入边界须分类说明。
10. 当前字符串映射参数/必需段/绝对URL语义共享，不以本簇关闭mixed/positional/UrlRoutable/可选/default/relative生成或完整Controller/URLGenerator。唯一模块与生成helper演示匿名动作路由、真实命中身份与动作跳转，不建假控制器框架。
11. 核心负责值类型、route/routing/routes/url与Boot装配；桥接及调用方owner负责redirect、HTTP集成测试、唯一模块/模板。共享mod/export、清单/锁、文档与提交单一owner，公有API若发现确切编译障碍先给最小等价调整，不并行互改文件。
12. 上游固定d52c3b3，窄证据 `/tmp/openrust-c04d10f-next-action-evidence.md`；先一次完成源码/测试/调用方/中文文档，再独审、集中门禁、fresh生成/MySQL与真实TCP，不把契约或静态片段当验收。

D10-G上述限定契约已通过核心/调用方/生成模板、main适用门禁与MySQL及真实TCP验收，资源收尾已完成，限定提交见Git。具体默认824/全特性1611、fresh7/MySQL2+1/TCP10+15与失败保留、不同模型审查、清理证明见[测试记录](../memory/testing.md#c04-d10-g-动作身份与-action-url限定子范围已验收提交见git)。原始集合碰撞的实际dispatch及完整参数族仍按上述边界开放。


## D10-H：最终声明集合与真实分派（限定范围已验收）

1. 不新增公共路由API。最终group/nest/merge/middleware处理后的真实声明是handler、原方法顺序、metadata与顺序的唯一来源；匿名路由同样参与。私有集合分别派生逐method分派view及G完整methods动作view，不将两者强行相等、不新增URL-only内核。
2. 逐method按原始domain+uri无分隔键、动作按methods管道拼接+domain+uri键，在域/普通集合内后写覆盖并保留首位置，再域左union。隐藏发生在host验证前，不因host不匹配恢复已淘汰普通handler。partial/split覆盖的实际Route仍保留原methods/action等元数据。
3. 真实匹配按最终有序候选进行：非fallback声明首匹配，包括更早动态路由先于后静态路由；先试完全部非fallback，再执行首个适用fallback。继续用Axum路径解析与现InternalMiss/RouteSlot恢复，不自写匹配器，不让域fallback抢普通显式route或将业务404/405当miss。
4. 当前方法无普通或适用fallback后，从幸存方法view忽略MethodValidator求alternate；GET/HEAD亦可405，OPTIONS可合成200，无有效path/host才404。固定Router::$verbs顺序为GET、HEAD、POST、PUT、PATCH、DELETE、OPTIONS、QUERY（排除当前方法），不能沿旧字典序或以TRACE/CONNECT替代QUERY；已显式登记其他方法的直接命中保持，alternate仅依固定列表。405的Allow按固定HttpKernel逗号空格格式，合成OPTIONS按固定Laravel逗号格式；不以token排序掩盖默认输出差异。
5. 既有scoped fallback、PendingRootTarget静态站点、内部forward、HEAD空体/头保留及资源生命周期边界继续受保护；固定PHP探针未覆盖其Rust适配，不宣称由PHP证据证明。发现真实冲突先核固定对应并记录，不为保旧测试放宽新contract。
6. 固定PHP8.5真实Illuminate Request、RouteCollection::match/validators/bind及OPTIONS Response已有35案actual0。仅status/name/Allow是逐字段断言，其他输出为观察，闭包不代表业务handler执行，未启动完整Laravel kernel。Symfony8.0三组件为明确固定的参考输入，符合framework版本约束但非已解析composer.lock；来源/许可/文件SHA见 `/tmp/openrust-c04d10h-matcher/provenance.json`。
7. 根核心owner独占routes.rs/routing.rs及必要url.rs/route.rs/src/routes.rs接线与纯集合unit；调用方owner独占HTTP router测试/README、唯一模块路由与自然业务示例、模板/快照及CHANGELOG。源契约不变时同步实施；清单/锁、共享Agent文档、构建/服务/提交归root串行协调，无新依赖预期。
8. 纯集合测试覆盖两view与覆盖位置；HTTP真实handler/中间件计数器证明淘汰handler零次、首匹配、元数据/slot、Allow/OPTIONS/fallback/业务终态。模块/生成/TCP只补公开接线与高价值回归，不复制35案全矩阵，不机械新增test target。
9. 完成源码/测试/调用方/模板/中文文档后集中验证；不同模型复审生产高风险改动，按适用workspace门禁、生成/MySQL及真实Host/path TCP验收、资源收尾后限定本地提交。
10. 本簇不实现signed或关闭完整Routing/参数族；修正G已记录的真实分派缺口，保留所有G旧证据。内部结构由owner按Rust类型/所有权/异步约束选择，若契约确有编译障碍须给最小等价方案，不凭语言惯用缩减语义。

H补证 `/tmp/openrust-c04d10h-matcher/probe-v2.output.jsonl` 9案actual0，输出SHA `630df0c2b96c93c62c0bf9a5264b042bf1b52d7e9cf4cb1224900fd941f5cde1`：确认QUERY/TRACE/CONNECT直接命中与alternate资格差异、405/OPTIONS原始Allow格式及动态先声明首匹配；旧35案脚本/输出保留。该参考证据不代替Rust实现和业务handler执行验证。
