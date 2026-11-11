# 05-05 路由模型绑定

- 对照固定 Laravel 的显式注册、路由键、嵌套作用域和软删除。核对 `v0.x` 路由与 ORM，复用现有 `SubstituteBindings`、显式绑定优先顺序和 Eloquent 连接、主键及作用域查询。 [Laravel 绑定文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/routing.md#route-model-binding) · [Laravel 隐式绑定源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Routing/ImplicitRouteBinding.php)
- 路由局部模型用泛型声明，处理器通过请求读取已绑定模型。Rust 由子模型显式指定父模型类型与关系，代替 PHP 处理器参数反射和关系名推断。默认自增主键进行整数解析，SQLite 的超范围无符号值作为缺失处理。
- 以 `2eaf0de6` 为基线验收。Rust 1.94.0 格式、451 项锁定离线测试、全目标严格 Clippy 和 `artisan inspire` 通过。SQLite 与真实 MySQL 各执行回环 HTTP，验证显式和自定义绑定、主键和 slug、父子作用域及关闭、软删除、默认 404、缺失回调与绑定先于路由中间件。
- 三语 VitePress 构建和 12 项浏览器检查通过。
