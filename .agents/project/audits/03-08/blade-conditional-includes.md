# 03-08 Blade 条件子视图

核对固定 Laravel 的 `@includeIf`、`@includeWhen` 和 `@includeUnless`。旧版 `6d1a8192` 未提供对应 Blade 指令，沿用当前子视图查找、局部数据继承、真值判断及渲染隔离。Rust 入口使用现有 Blade 表达式和父视图数据，显式附加数据参数留待表达式和模板参数完善。 [Laravel CompilesIncludes](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/View/Compilers/Concerns/CompilesIncludes.php)

- Rust 1.94.0 锁定离线 `view_finder` 共 5 项通过，专项严格 Clippy 通过。真实回环 HTTP 覆盖两应用并发渲染、条件选中和跳过、缺失的选中视图返回 500、原有子视图和错误后独立渲染。
- 编译缓存包含编译器版本。旧工件在相同源内容时重新编译，仍可由 `view:clear` 清理。测试覆盖过期判断和重新编译。
- 三语 Blade 文档已更新。VitePress 构建、12 项通用和 6 项视图文档浏览器检查通过。
