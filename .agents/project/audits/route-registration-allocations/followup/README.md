# 路由注册与匹配后续审计

2026-11-24。复核 `7c3abbd78`，不重复实现[字面路径优化](../README.md)。本轮基线见 `results.json`，测量和验收使用同一隔离源码，不带入其他协作者的在途改动。

## 调研与取舍

- 固定 Laravel Framework v13.40.0，提交 `1449bf241017087fbd38c118ecaabef3f6797fdc`：[RouteCollection](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Routing/RouteCollection.php) 按方法、域名和 URI 登记／覆盖，匹配仍须保留选择顺序；[Route](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Routing/Route.php) 延迟编译并复用单条 compiled route；[RouteCacheCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Console/RouteCacheCommand.php) 将编译结果序列化，[CompiledRouteCollection](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Routing/CompiledRouteCollection.php) 消费 Symfony matcher 并保留后加路由与 fallback 处理。沿用登记、匹配、缓存职责划分，不机械照搬 PHP 容器。
- 既有本地固定对照环境实际安装 Symfony Routing v7.4.0，提交 `4720254cb2644a0b876233d258a32bf017330db7`（composer installed.json 核对）。[RouteCompiler](https://github.com/symfony/routing/blob/4720254cb2644a0b876233d258a32bf017330db7/RouteCompiler.php) 处理参数、静态前缀、默认值、约束与 UTF-8；[Dumper](https://github.com/symfony/routing/blob/4720254cb2644a0b876233d258a32bf017330db7/Matcher/Dumper/CompiledUrlMatcherDumper.php) 将会命中先前动态路由的静态路由保留在动态集合，不能把“静态路径优先”当作无条件规则；[Matcher](https://github.com/symfony/routing/blob/4720254cb2644a0b876233d258a32bf017330db7/Matcher/Dumper/CompiledUrlMatcherTrait.php) 独立处理域名、解码、HEAD 和方法错误。其[测试](https://github.com/symfony/routing/blob/4720254cb2644a0b876233d258a32bf017330db7/Tests/Matcher/Dumper/CompiledUrlMatcherDumperTest.php)覆盖覆盖路由、参数／默认值／约束、编码与缓存产物。本次保留 OpenRust 已有缓存格式和有序候选合并，不移植新的编译缓存。
- Gin v1.11.0，提交 `6ad6205e9c94a4b8a320219e28c37c29d22a7a2c`：[tree.go](https://github.com/gin-gonic/gin/blob/6ad6205e9c94a4b8a320219e28c37c29d22a7a2c/tree.go) 使用短方法切片、压缩路径节点、静态／参数／catch-all 节点及参数切片，注册时按 priority 调整节点；[tree_test.go](https://github.com/gin-gonic/gin/blob/6ad6205e9c94a4b8a320219e28c37c29d22a7a2c/tree_test.go) 有静态、Unicode、wildcard、优先级和冲突测试。借鉴小型连续集合避免临时哈希结构的设计，不引入整棵路由树、节点重排或另一套冲突规则。OpenRust 需要保留动态先注册时的选择结果及同 URI 覆盖，接入树的测试和迁移成本高于本轮局部收益。
- 三者都有明确版本、源码及测试依据；Laravel／Symfony 主仓为 MIT，Gin 主仓 MIT，tree.go 另保留 httprouter 的 BSD 风格来源标注。本轮未复制第三方代码、未引入依赖，不以 Star 数量证明适用性。已有 v0.x 的 Axum 登记不能直接复用为当前 Router。

## 定位与实现

哈希校验旧二进制后复跑：原始 GET 仍为13700 alloc／3471 realloc／2826604请求字节／988069峰值，旧优化后仍为1028／143／181420／92328。它确认历史证据可复现，不代替当前源码构建。

当前源码 Release 测量同样得到1028／143／181420／92328。旧93.43%的路径编译占比只适用于13700次的旧引擎，当前字面路径编译128次只占1028次的12.45%，不能继续当作现状归因。对128条16字节字面路径的分配控制如下，计时不使用该计数分配器：

| 测点 | alloc | realloc | 请求字节 |
| --- | ---: | ---: | ---: |
| prepare_route（持有预建 handler，含 URI 与路径） | 256 | 128 | 5120 |
| 单独字面路径编译 | 128 | 0 | 2048 |
| URI 准备对照 | 128 | 128 | 3072 |
| handler Arc 对照 | 128 | 0 | 2048 |
| 实际 GET／HEAD 方法复制独立控制 | 128 | 0 | 6144 |
| 方法字符串元数据 | 384 | 0 | 7040 |
| 短方法 HashSet | 128 | 0 | 5632 |
| 完整登记减去 prepare、handler、方法复制、元数据及集合的剩余索引／routes Vec 增长 | 4 | 15 | 155436 |

这些是差分子流程，不是调用栈剖析。prepare 已含 URI 和路径，不能再次相加；峰值因生命周期不同不能求和。`support.rs` 的 method_vec 额外测点只传 GET 再补 HEAD，为128 alloc／128 realloc／15360字节；它是 GET-only match 控制，**不是 get() 的复制开销**。实际 get() 已传 GET／HEAD，另用相同 Counter 和 OpenRust Method 的独立程序测得上表6144字节，源码为本目录 method_copy.rs，原始输出保留在临时证据目录；以相同 Release rlib、`rustc --edition=2024 -O --extern openrust=<rlib> -L dependency=<deps>` 单独编译执行。元数据和 routes／方法索引的持久存储保留。

生产优化只改 `Router::insert_route`：借用原方法切片，追加的 HEAD 置于栈上；按精确容量构造既有字符串元数据，保留原顺序及重复项。至多8项的输入通过有界前缀扫描去重；更长列表仍用 HashSet，避免把任意长度输入变成平方级扫描。每个方法中的路由槽位查找、原位覆盖和追加规则不动。没有修改 matched、URI、约束、域名、模型绑定、应用状态或 RouteCache。

## Release 构建前置问题

未修改的当前基线首次 Release 编译失败：Rust 1.94.0 在 `FormRequest::validate_resolved` 计算验证 Future 布局时查询深度达到130，超过默认限制。失败日志保留为 `build-before-blocked.log`。没有提高 recursion_limit 或降低优化级别。

必要修复在已有 ValidationFuture 类型擦除边界装箱 `validate_request` Future，保留轮询、错误包与 redirect、授权、after 和 precognition 顺序。**这会为每次该 FormRequest 验证增加一个 Box 分配**，不是路由注册优化收益；未测其单独吞吐或宣称零成本。此修复在 before／after 中完全相同，两组唯一待测实现差异仍是 insert_route。另有一次临时测点准备不同步导致 example 缺少辅助入口，失败日志单独保留，冻结源码后重新构建成功，不计入测量或通过结果。正式验收前删除全部测点入口和 examples。

## 测量结果

Rust 1.94.0、同一 Apple M4 Max 主机、同一隔离源码路径；两组 `cargo build --release --locked`，均使用 `CARGO_PROFILE_RELEASE_INCREMENTAL=true`，其他 Release 默认。输入在计数／计时前建立；计时二进制使用默认分配器，计数二进制包装 System 并有自检；argv0、case、iterations 一致。31对随机交错采样、10000次配对 bootstrap；各分配测点重复5次完全一致。预热 GET 与新进程首次 GET 分开。主机未绑核锁频，也未停止其他开发任务。

实际完整统计、范围、IQR、95%区间见 `results.json`。注册含创建／销毁；匹配是预建表上的内部候选选择，不包括解码、中间件、handler、网络和响应。请求字节含 realloc 新尺寸，不是 RSS，不覆盖绕过 Rust 分配器的 C／OS 分配。

| 128条完整登记 | alloc 前→后 | realloc 前→后 | 请求字节 前→后 | 峰值存活字节 前→后 |
| --- | ---: | ---: | ---: | ---: |
| GET（预热与首次计数一致） | 1028→772 | 143→143 | 181420→169644 | 92328→92236 |
| any | 1804→1548 | 173→173 | 249236→205204 | 121584→121240 |
| 动态 GET | 24452→24196 | 6415→6415 | 51948588→51936812 | 2677897→2677897 |
| 同 URI 重复 GET | 1028→772 | 5→5 | 173996→162220 | 87912→87820 |

GET alloc 减少24.90%，请求字节减少6.49%；峰值只少92字节，不能宣传为显著降低常驻内存。所有完整登记测点结束保留字节为0。

| 测点 | before 中位数 | after 中位数 | 配对变化及95%区间 |
| --- | ---: | ---: | --- |
| 128条 GET 创建／登记／销毁，预热 | 85.218 µs | 73.745 µs | -13.59% [-14.83%, -12.13%] |
| 同上，进程内首次登记 | 129.666 µs | 116.667 µs | -10.02% [-12.60%, -7.41%] |
| 128条 any | 226.746 µs | 207.128 µs | -7.37% [-9.69%, -6.46%] |
| 128条动态 GET | 2971.329 µs | 2949.296 µs | -0.03% [-1.28%, +0.28%] |
| 128次同 URI 覆盖 | 45.922 µs | 36.337 µs | -20.93% [-22.29%, -19.47%] |
| 字面路径编译控制 | 4.952 µs | 4.994 µs | +1.28% [-0.79%, +5.20%] |
| 原正则编译控制 | 741.825 µs | 740.656 µs | -0.85% [-1.33%, -0.05%] |
| 预建表首项匹配 | 12.875 ns | 12.843 ns | -0.32% [-1.22%, +0.06%] |
| 预建表末项匹配 | 389.313 ns | 389.050 ns | -0.83% [-1.40%, +0.21%] |
| 预建表未命中 | 240.354 ns | 243.541 ns | -0.31% [-2.31%, +1.74%] |

配对变化不是两列总体中位数相除。预热 GET 的 before 范围78.811–99.839µs、IQR83.851–89.622µs，after 范围69.393–90.696µs、IQR72.717–75.822µs；any 的 after 最大值349.980µs高于 before 最大值289.245µs，原始不利样本未剔除。其他完整范围／IQR保存在 results.json。旧记录78.802µs和本轮85.218µs不作为同轮对照；旧13700→1028的结构性计数可重复，但计时绝对值有环境波动。

匹配三个测点前后均为0 alloc／0 realloc／0请求字节／0峰值。动态登记未显示明确计时改善，匹配也没有明确收益；未改的正则控制仍出现约0.85%差异，不能归因于本轮优化。迟命中仍需有序扫描，动态路由的正则编译仍占大量分配；本轮不以新树、全局缓存或推迟元数据构造到每个请求来换取启动数字。剩余 URI realloc 及持久索引／元数据保留，未对未测候选宣称收益。上述结果不等于 HTTP 吞吐。

## 消费者与复验

新增用例从实际 HTTP 的 RouteMetadata 检查重复方法及原顺序，验证短列表、显式 HEAD、自动 HEAD、12项扩展方法长列表回退、小写方法注册归一化、405／Allow，以及 route:cache 前后。原有测试继续覆盖动态／静态先后、同 URI 覆盖、域名大小写、Unicode、百分号解码、默认参数、约束、组／前缀、中间件、Facade、多应用隔离、非法及超长模板、损坏缓存拒绝。新用例最初误把小写方法的传输行为当作注册归一化后的行为，依据原 Registrar 契约修正，并先在未优化 Router 上运行同组用例通过；失败日志保留，不算生产回归或降低检查。

正式检查移除全部临时诊断入口及 examples。使用 `tests/build_guard.py --workflow route-registration-followup --target-dir target/tmp/array-release/cargo-target -- <命令>`，source 内执行的完整命令见本工作流 gates.json。最终实际通过91项 Rust 测试：框架路由／输入／元数据／FormRequest／状态感知验证53项，精确 SQLite 模型绑定1项，根应用 startup／console／resource_generator 37项，均无失败或忽略；另有未优化 Router 的4项兼容性控制通过，不与最终91项重复累计。没有把缺 MySQL 环境时返回的用例算通过，本轮未新增 MySQL／PostgreSQL 绑定验收结论。

workspace 格式、根应用及 framework all-targets 严格 Clippy、默认 Release 主程序构建通过。实际 Release `main artisan` 核对134个无重名命令、23项CLI检查（含预期错误）、route:list JSON、route:cache两次生成字节一致、route:clear；缓存前后重启各自真实监听器，合计12项HTTP检查通过，含首页GET／HEAD、健康、HTML／JSON404，两个进程SIGINT退出均为0。本轮没有额外运行源码 ./artisan 包装器，不宣称队列／调度发布验收。相关完整命令及检查结果归入 results.json。

Clippy 首次因32GB停线被守卫中断，退出75，保留 clippy-interrupted.log，不计通过。空间恢复后在相同源码从未完成项接续，严格 Clippy 和正式 Release 验收全部重新完成。守卫事件记录清理前31,978,926,080字节、清理后96,993,189,888字节，释放65,014,263,808字节；源码、未提交工作、日志、下载、数据库及数据卷保留。中断输出另有 Operation not permitted，未据此当作检查成功；空间恢复后的命令均返回0。

收尾核对本批生产代码与最终受检快照一致，删除本批隔离源码、运行缓存、临时适配器、独立控制程序及一次性编排脚本，保留独立测量二进制、统计／日志和可复用 Cargo target。未改默认路由、缓存格式、公开API或三语用法文档；其他协作者的在途验证规则／并行测试等改动不纳入本批提交，不推送。

复测已保存二进制：同一 guard 下执行 `python3 -B .agents/project/audits/route-registration-allocations/followup/measure.py`，默认读取本工作流 bin/before_alloc、before_time、after_alloc、after_time；另设 ROUTE_PERF_DIR 只能指向项目 target/tmp 下其他同布局目录。原始分配、配对样本和波动统计保留，不用旧测试替代本轮执行。

重新构建诊断二进制时，用 results.json 的 base 在同一项目内隔离 source 创建前后两版；两版都叠加上述 FormRequest 构建修复，只切换 insert_route。将本目录 alloc.rs／time.rs 作为 framework 的两个 example，support.rs 放在 examples/audit_support 中并修改二者的 path 属性；以相同工作流 target、Rust 1.94.0 和 Release incremental=true 分别构建并保存独立二进制。在两版隔离 router.rs 末尾临时追加下列测点，测量后移除，不能提交为公开 API：

```rust
impl Router {
    pub fn __audit_pattern(path: &str) {
        std::hint::black_box(super::route_pattern::RoutePattern::compile(path, &HashMap::new()).unwrap());
    }
    pub fn __audit_prepare(&self, path: &str, handler: Arc<dyn Handler>) {
        std::hint::black_box(self.prepare_route(path, handler).unwrap());
    }
    pub fn __audit_match(&self, path: &str) -> bool {
        self.matched(&Method::GET, "", path).is_some()
    }
}
```
