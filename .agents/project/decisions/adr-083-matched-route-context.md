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
