# 02-09 惰性集合

核对固定 Laravel 13.x 的[惰性集合章节](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/collections.md#lazy-collections)、[LazyCollection 源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Collections/LazyCollection.php)和[回归测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Support/SupportLazyCollectionTest.php)。Goravel 固定版本的[类型化集合](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/support/collect/collection.go)使用实体切片，仅参考其编译期类型化 API。旧版 `6d1a8192` 的框架源码及测试无惰性集合，沿用 02-08 从旧版提取的 `ArrayKey` 归一化规则。

- 接通迭代器工厂、带键和可失败的输入、`Enumerable`、`Collection::lazy()`、过滤与映射、分块、正负 `take`、跳过、首次匹配和副作用。组合后的来源在取值时打开，`take(n)` 不提前读取第 n+1 个产出。负数 `take` 需要遍历到末尾，仅保留指定长度的尾部。
- `remember()` 共用一个源迭代器，按位置缓存已读条目。多次遍历可交错并保留重复键。实体化时同键的最后值覆盖前值，可先调用 `values()` 保留各次出现。失败立即终止，记忆模式保留源错误并向后续遍历重放。
- Rust 使用线程内的迭代器和 `Result`，允许回调借用局部状态。按位置重放要求值可克隆。分块输出已实体化的 `Collection`。期限与心跳使用单调时钟，`throttle` 在同步调用线程等待，期限检查发生在两次读取之间。PHP 的动态回调模式、完整扩展方法表及异步数据库游标衔接仍属后续范围。

## 验收

macOS arm64、Rust 1.94.0。在 `/private/tmp/openrust-02-09.vzJ6p7/app-under-test` 以 02-08 候选副本为底运行。

- `cargo +1.94.0 fmt --all --check`、`cargo +1.94.0 test --workspace --locked --offline`、`cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings` 通过。测试报告 369 项通过，含本批 6 项回归。真实 MySQL 未在本批复跑。
- 用 PHP 8.5 CLI 运行固定源码，13 项对照通过。验证惰性打开、过滤短路、普通遍历重新打开、带重复键的缓存、双迭代器和负数截取。独立 Rust 程序用真实文件验证按需读取、缓存重放及来源删除后的重新读取错误。`RUSTUP_TOOLCHAIN=1.94.0 ./artisan inspire` 正常退出。
- 三语 Markdown 的惰性集合示例均使用 Rust 1.94.0 编译并执行。`npm run build`、12 项常规浏览器组合及 6 次集合页正文、侧栏和 `remember()` 锚点检查通过。
- 与隔离副本逐文件核对本批实现、测试和文档。工作区锁定依赖解析及 `git diff --check` 通过。

下一单元为 03-01 本地文件系统与公共磁盘，依赖 01-01 的配置和应用基础。
