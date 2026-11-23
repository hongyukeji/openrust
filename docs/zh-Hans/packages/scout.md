# Scout

Scout 为 Eloquent 模型提供搜索引擎、模型生命周期同步和队列接入。内置 `collection` 引擎在内存中搜索模型数据，`null` 禁用搜索；应用可以注册其他引擎。

## 安装 {#installation}

使用 `./artisan` 时，在应用 `Cargo.toml` 的 `[features].default` 中加入 `"scout"`，保留已有功能。包发现加载 `ScoutServiceProvider`，配置归包所有，不创建搜索数据表。

```sh
./artisan vendor:publish --tag=scout-config
```

发布后的 Rust 配置需要接入编译型应用加载器并重新构建。重复发布保留已有文件；配置缓存命中时不重新读取包环境变量。默认骨架无需额外登记扩展 Provider 或预装配置。

## 配置 {#configuration}

`SCOUT_DRIVER` 默认 `collection`，`SCOUT_PREFIX` 默认空字符串。`SCOUT_QUEUE`、`SCOUT_AFTER_COMMIT`、`SCOUT_SOFT_DELETE` 和 `SCOUT_IDENTIFY` 默认 `false`。导入分块默认 500。编译型配置可设置 `scout.queue = {"connection":"database","queue":"search"}` 指定连接和队列；设为 `true` 则使用原生默认队列。

集合搜索使用模型的 PostgreSQL、MySQL 或 SQLite 连接，读取符合数据库条件的所有候选模型，再对 `toSearchableArray` 执行不区分大小写的子串匹配。空查询保留符合条件的模型，空格按字面匹配，`"0"` 是有效搜索词。该引擎适合较小数据集；数据库全文搜索可使用独立的[原生搜索接口](../digging-deeper/search.md)。

集合引擎不维护独立索引，更新、删除、清空和索引创建没有额外索引状态需要变更；清空不会删除模型数据。未内置远程引擎、Scout database 引擎、语义搜索、混合搜索和嵌入提供方。未知驱动及集合引擎上的语义／混合搜索明确返回错误。

## 可搜索模型 {#searchable-models}

为已有 `Model + Clone` 实现 `openrust_scout::Searchable`。默认 `toSearchableArray` 使用 `to_array`，可以覆盖以选择字段。`shouldBeSearchable` 控制搜索资格，`searchIndexShouldBeUpdated` 控制自动更新。

```rust
use openrust_scout::{EngineManager, Searchable};

impl Searchable for Article {}

// 在应用 Provider 的 boot 方法中：
app.make::<EngineManager>().await?
    .register::<Article>(app, "Article").await?;
```

显式注册代替 PHP 动态 trait 启动，并提供命令使用的模型别名。业务模型保留在应用目录。`bootSearchable(app)` 可使用完整 Rust 类型名注册。`searchableAs` 将前缀加到表名；可覆盖 `indexableAs`、`getScoutKey` 和 `getScoutKeyName`。自定义字符串键还应从 `getScoutKeyType` 返回 `ModelKeyType::String`。

原生观察器处理保存、删除、强制删除和恢复。`after_commit=true` 时等待最外层事务提交，回滚丢弃相应回调；关闭时，外部引擎可能在事务回滚前已经观察到改动。`soft_delete=true` 保留软删除模型用于索引，普通搜索仍排除它们。

## 搜索 {#searching}

在已经启动的 HTTP、Artisan 或 Worker 生命周期中调用模型接口。

```rust
let page = Article::search("rust")?
    .where_in("category", vec![1_i64.into(), 2_i64.into()])
    .order_by("id", "asc")
    .paginate(15, 1).await?;
```

支持 `where(field, operator, value)`、`where_in`、`where_not_in`、`order_by`、`order_by_desc`、`latest`、`oldest` 和 `take`。Rust 使用显式运算符参数表达 PHP 两参数 `where` 的重载。集合引擎未指定排序时按 Scout 键倒序。`with_trashed` 和 `only_trashed` 控制软删除结果。分页使用原生分页器，页大小须为正数；集合分页统计所有匹配模型，设置 `take` 不改变该总数。

`raw` 返回有序 ID、总数和提供方 JSON。`keys`、`first`、`get`、`paginate` 和 `simple_paginate` 返回 ID、模型或分页。`cursor` 返回流：集合匹配先确定 ID，再通过原生 SQL 按结果顺序流式读取模型。`query(callback)` 修改模型加载查询；`callback` 替换集合引擎初始数据库筛选；`withRawResults` 在映射模型前调整原始结果。回调返回 `Result` 并保留错误。`within` 和 `options` 向自定义引擎传递设置；集合引擎没有命名索引。

## 同步与队列 {#synchronization}

模型支持 `searchable`、`unsearchable`、`searchableSync` 和 `unsearchableSync`。导入 `SearchableCollection` 后，原生 Eloquent 集合也可使用这些方法。Sync 方法绕过队列。队列更新重新加载当前模型数据；删除任务携带键值，数据行已经删除时仍能移除索引。

为配置的连接和队列运行普通[队列 Worker](../digging-deeper/queues.md)。每次应用启动都要注册搜索模型，确保新 Worker 进程知道任务处理器。队列错误交由原生重试和失败处理。集合引擎执行搜索本身不需要 Worker。

`disableSearchSyncing` 与 `enableSearchSyncing` 按模型类型控制应用内自动同步。`withoutSyncingToSearch(future)` 临时抑制当前异步调用链的自动同步，并在嵌套调用和取消后恢复；手动调用 `searchable` 仍表示明确执行请求。

## 命令 {#commands}

```sh
./artisan scout:import Article
./artisan scout:import Article --fresh --chunk=500
./artisan scout:flush Article
```

`scout:import` 遵循模型搜索资格及队列配置；`--fresh` 先清空引擎再导入，`-c` 为 `--chunk` 的短选项。`scout:flush` 只清空引擎状态。未知模型别名和零分块大小明确失败。部署后使用 `./main artisan`。

## 自定义引擎 {#custom-engines}

实现 `engines::Engine<Article>`，通过 `manager.extend::<Article>("custom", engine)` 注册实例，再使用 `scout.driver` 或模型 `scoutDriver` 选择。引擎接收强类型模型与 `Builder` 设置，返回 `SearchResults`，通过 `ScoutFuture` 保留错误。结果 ID 按引擎顺序映射回原生模型。引擎注册按应用隔离；自定义引擎自行提供网络客户端、凭据、索引设置和一致性保证。
