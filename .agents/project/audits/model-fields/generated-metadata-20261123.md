# 模型字段宏内部生成元信息（2026-11-23）

## 用户确认与实现

本侧对话获用户授权，将字段生成布局改为宏内部元信息；命令提示去掉平台前缀：

```rust
openrust::model_fields! {
    #[generated(command = "artisan model:fields User")]
    User {
        /// 用户表
        TABLE = "users";
        /// 主键
        ID = "id";
    }
}
```

- `generated` 由现有声明宏消费，不是编译器属性；命令字符串仅作提示，不执行。保留原有紧凑与显式 impl 声明方式，不增加过程宏 crate 或依赖特性。
- 生成器复用现有 syn 2.0.119，解析并验证宏内部元信息、模型类型、声明唯一性及参数。刷新已带元信息的宏时原地替换整个宏，保留区域外的业务注释与位置；不使用 begin/end 注释或大段说明块。
- 已提交旧块注释与经典 `openrust:model-fields` 标记格式继续安全升级；无效元信息、重复声明、模型不匹配、旧区混入业务项等仍拒绝写入。旧声明模式、表名绑定、字段类型和认证行为不变。
- `app/models/user.rs` 仅更换生成说明格式，业务代码与字段声明不改。三语 Eloquent 文档同步新格式。真实命令入口仍为 Unix 的 `./artisan`、Windows 的 `artisan.bat`，没有改动启动脚本。

参考 Rust 官方[声明宏](https://doc.rust-lang.org/reference/macros-by-example.html)、[属性](https://doc.rust-lang.org/reference/attributes.html)及 [syn 文件解析](https://docs.rs/syn/2.0.119/syn/fn.parse_file.html)。本功能为 OpenRust 已确认扩展，不宣称 Rust 自带 `generated` 属性，也不将此格式作为上游命令契约。

## 实际验收

全部源码、日志与临时文件位于项目 `target/tmp/model-fields-markers-20261123/`，经 `tests/build_guard.py` 运行，Rust 1.94.0。使用当时 HEAD 快照叠加本任务文件，不包含其他语言资源、路径接口或欢迎页在途修改。

- `verify.py`：构建 main、SQLite 真实 Artisan 与编译消费者、框架 lib 严格 Clippy 通过，见 `build.log`、`sqlite.log`、`clippy.log`。
- `databases.py`：独立 MySQL 8.4.13 与 PostgreSQL 18.7 夹具实际通过生成、刷新、查询、字段编译正反例及失败保护，见 `mysql.log`、`postgres.log`。
- 覆盖简体、繁体、英文及新增语言资源的表说明；生成元信息不随语言变化。覆盖嵌套模型的无平台前缀命令提示、元信息删除后的重建、未知参数、重复元信息、错误模型、重复宏、邻接业务注释保留、尾逗号、旧格式升级、BOM/shebang、CRLF 和字符串中的伪标记。
- 三语内容检查与 VitePress 构建、12 项 `/` 和 `/docs/` 桌面／手机浏览器检查通过，见 `docs-build.log`、`docs-browser.log`。全快照 fmt 与工作区差异空白检查通过。
- 四份最终实现／测试／User 文件与验收快照逐文件 SHA-256 相同，见 `final-source.json`。

保留早期失败记录：首次选用 syn 的 Spanned 时发现现有依赖未启用 printing，最终方案已移除该依赖特性需求；一次测试从快照内启动导致项目根校验失败，已改用仓库正式验收入口；显示去掉 `./` 后，嵌套模型断言同步修正。另有一次 MySQL 验收启动时遇到32 GB停线清理后的缓存缺失，未计通过，重建同一独占缓存后完整重跑通过。历史日志不替代最终结果。

## 资源与交付边界

### 首次生成与更新补验

用户于同日追加要求，重点验证无生成标识的首次插入。新增回归场景先执行普通 `make:model`（不带 `--fields`），确认源文件没有字段宏或任何生成标识，再追加业务方法与注释。通过快照内真实 `./artisan model:fields FieldReferenceProbe` 首次生成，验证宏位于导入后、模型声明前，业务内容原样保留；生成前后均以实际 Rust 编译器严格编译。首次 `--check` 拒绝过期状态且不写文件；生成后重复同步与 `--check` 成功，内容及修改时间不变。

经 `tests/build_guard.py --workflow model-fields-markers-20261123` 运行 `target/tmp/model-fields-markers-20261123/first-generation.py`，SQLite、独立 MySQL 8.4.13、PostgreSQL 18.7 三套完整场景全部通过；同时重验字段新增、删除、重命名与已有宏更新。日志为 `first-generation-{sqlite,mysql,postgres}.log`，逐项命令及编译证据在同名子目录。无需修改生产实现；本轮仅新增回归测试和本记录，未暂存、未提交、未推送。标准夹具已移除本轮数据库容器，独立编译消费者已直接删除，保留源码、日志及复用缓存；补验完成时卷可用空间约52 GiB。

数据库容器由标准夹具收尾，不使用开发数据库或持久卷。仅直接删除已结束场景的独立 rustc 消费者二进制／rmeta，保留其源码、日志、SQLite 夹具及可复用 Cargo 缓存，逻辑回收量见 `consumer-artifacts-cleanup.json`；期间其他工作流触发的全仓停线清理以 `target/tmp/build-guard-events.jsonl` 为准，不计作本任务独立释放量。

未重跑 Windows、完整 workspace 测试或发布矩阵；已通过范围仅限上述源码组合与消费者。实施验收期间未暂存、未提交。用户随后于2026-11-23授权提交代码，本批仅本地提交上述实现、User示例、测试、三语文档和本验收记录，不推送；其他协作者的在途改动保留。
