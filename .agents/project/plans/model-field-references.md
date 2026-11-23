# 模型字段引用

日期：2026-11-23。用户指出手工声明不友好，当前采用数据库结构自动生成；自动生成方案定稿、实现完成、范围内验收通过。下述第一版的手写声明结论仅作历史依据；具体源码组合、命令、失败修复及未验边界见[验收记录](../audits/model-fields/README.md)，不将本范围通过推导为当前全部并行改动通过。本任务授权开发与验收，不暂存、提交或推送。

## 需求与现有入口

模型使用 ModelState 动态属性；ModelMetadata 没有完整字段集合。fillable 是批量赋值策略，casts 是属性转换策略，两者都不能作为数据库结构声明。现有筛选为 Eloquent Builder::r#where(&str, operator, value)，不是示意 whereEq；Query Column::Identifier 通过 Grammar 处理，更新使用 Attributes 字符串键，关系构造器使用静态键名。保持这些契约。

固定 Framework v13.40.0（1449bf241017087fbd38c118ecaabef3f6797fdc）Eloquent/Builder.php::where 与 DatabaseEloquentBuilderTest 的嵌套/普通筛选测试确认字符串列契约；下载并阅读固定源码和测试，不宣称执行上游 PHPUnit。v0.x 的 model.rs 是旧 ModelDateContext/属性布局，不包含可直接移植的当前字段引用设施。

## 调研与裁决

2026-11-23 实际下载的官方资料保存在 target/tmp/model-fields-20261123/research/；SeaORM 网站旧路径404，改查固定1.1.19官方源码。

| 方案 | 官方依据与机制 | 裁决 |
|---|---|---|
| 模型关联常量 | [Rust associated items](https://doc.rust-lang.org/reference/items/associated-items.html)：编译器解析关联常量，IDE 可补全 | 采用；常量类型为 &'static str，直接接入所有现有字符串入口 |
| 列枚举 | [SeaORM 1.1.19 entity derive](https://github.com/SeaQL/sea-orm/blob/1.1.19/sea-orm-macros/src/derives/entity_model.rs) 从真实Rust结构字段生成列枚举并支持column_name；[SeaQuery 1.0.2 Iden](https://docs.rs/sea-query/1.0.2/sea_query/derive.Iden.html) 提供标识符转换 | 借鉴显式映射与复用标识符语法器；不引入枚举转换层或平行 ORM，因为当前模型没有对应结构字段 |
| 带模型和值类型的字段 | [Diesel 2.3 table!](https://docs.diesel.rs/2.3.x/diesel/macro.table.html) 从表/SQL类型声明生成类型化DSL，约束列和表达式 | 本轮不采用；要正确约束模型、NULL、数值和转换，必须连同查询/关系/联表表达式重构，超出改善拼写体验的最小方案 |
| 声明宏 | [Rust macros by example](https://doc.rust-lang.org/reference/macros-by-example.html) | 采用 model_fields! 生成普通关联常量，支持可见性、文档属性及显式数据库列映射；无运行时注册 |
| getter 方法引用 | [MyBatis-Plus v3.5.14 AbstractLambdaWrapper](https://github.com/baomidou/mybatis-plus/blob/v3.5.14/mybatis-plus-core/src/main/java/com/baomidou/mybatisplus/core/conditions/AbstractLambdaWrapper.java) 使用LambdaUtils.extract/方法名解析及ColumnCache | 不模仿；Rust函数指针不能提供Java序列化Lambda的属性/列元数据 |
| 过程宏 | SeaORM同上，结构字段是生成来源 | 当前只需常量，声明宏足够；不新增proc-macro crate、syn/quote依赖或代码生成服务 |

没有引入第三方依赖，没有新增复制的第三方源码或许可义务；沿用项目MIT声明。宏仅使用稳定的macro_rules、关联常量和静态字符串，实际最低版本检查为Rust1.94.0。维护成本是一个声明宏和可选模型声明，保持现有ORM签名和执行路径。

## 单一来源与公开用法

```rust
openrust::database::eloquent::model_fields! {
    impl User {
        pub const ID = "id";
        pub const NAME = "display_name";
    }
}
let mut query = User::query()?;
query.r#where(User::NAME, "=", "张三");
let users = query.orderBy(User::ID, "asc").get().await?;
```

model_fields! 内的条目是该可选字段引用集合的唯一声明，不再维护字段字符串列表或列枚举，不改变ModelMetadata、fillable、casts。允许只声明常用字段，不声称集合完整，也不替代迁移。宏不会推断数据库或ModelState属性；声明随迁移人工更新。现有模型不必迁移；make:model 只为已知默认主键提供可删除的 ID 示例，其他列由开发者按实际迁移补充。自定义主键也须同步修改示例。

## 保障与边界

- 可保障：User::NAME 已声明且可见；删除或重命名常量后旧引用编译失败。列映射的字符串拼写本身不校验；未修改常量名而只改迁移，编译器不会感知。
- 不保障：字段属于查询的模型、值与列类型相容、真实数据库列存在。常量实际类型是 &str，可用于其他模型的查询；不称为 Field<Model, Value> 或静态类型ORM。
- 值继续使用现有Value/ValueOperand、NULL、日期、JSON、自定义cast和原始表达式规则；不新增类型强制转换，也不承诺数据库查询值经过模型cast。属性赋值/水合转换仍由既有模型机制处理。
- 字段为未限定列名，不自动加表名。普通模型查询沿用已有scope限定规则；联表/别名用format!("u.{}", User::NAME)，再交给现有标识符入口，禁止作为原始SQL拼接。更新键用User::NAME.into()；select([User::NAME])复用Column转换；关系键直接传关联常量。
- 宏不支持泛型impl参数声明；普通、带路径的具体模型可用。宏不验证列名字符串合法性，由现有Grammar在编译SQL时处理。原始SQL及动态字段保留原入口。

## 验收要求

三库独立实例执行同一实际查询：字符串/常量SQL、绑定与结果一致；映射列/保留字/NULL/恶意值、更新、表别名、自联表、模型水合与HasMany、动态属性和raw表达式。缺失常量真实编译失败，数据库缺列实际执行失败。生成器模板实际编译、实际Artisan生成消费者。严格fmt/Clippy、三语内容/站点/浏览器；所有证据和构建目录位于target/tmp/model-fields-20261123/。


## 自动生成修订（2026-11-23）

用户明确要求取消逐字段手工定义。采用开发阶段读取数据库结构并保存 Rust 源码的成熟路线，复用 DatabaseManager::connection / Schema Builder::getColumns、已有 ModelMakeCommand、源码修改锁及原子替换；不新增 ORM、管理器、依赖或编译期数据库访问。

新增 OpenRust 扩展入口 `make:model User --fields` 与 `model:fields User`。两者支持 `--table` 和 `--database`；默认表名按现有模型生成器的 snake_case + pluralizer 规则，默认连接来自应用配置。无法在命令中反射任意已编译模型的 metadata（亦不解析 Rust 表达式），自定义表和连接必须明确传入；刷新也需传入。make:model 的 --table 同时设置新模型 metadata，既有模型刷新不修改 metadata。

生成区域直接放在对应 app/models 源码中，BEGIN/END 标记只供生成器管理。后续刷新原子替换该区域，保留区域外业务代码；损坏/重复标记、缺表、缺连接、字段标识冲突均拒绝修改。已有手写宏迁移时移除旧声明，避免重复常量。数据库当前结构为生成集合的唯一来源，迁移为数据库演进来源；不要求维护另一份字段列表。普通 make:model 不生成假定存在的 ID。

列名转大写下划线常量，数字开头添加 COLUMN_，ASCII非字母数字转下划线；碰撞整体拒绝。非ASCII列及无法形成标识的列拒绝自动生成，可继续用字符串查询或高级显式宏映射。常量仍保存原始列名，经过已有Grammar，不拼接SQL，不新增值类型和模型归属承诺。

官方调研：SeaORM CLI 1.1.19 `generate entity` 从数据库生成实体；Diesel CLI v2.3.3 print_schema 输出数据库结构为 table! 声明；Laravel IDE Helper v3.6.0 ModelsCommand从Schema读取列生成IDE元数据；固定Laravel v13.40.0 ShowModelCommand提供模型展示而不生成静态Rust字段。采用显式开发工具读取数据库、生成代码供离线构建的模式。参考源码下载至本主题research目录，均只阅读机制，没有复制第三方代码或引入依赖；SeaORM/Diesel MIT OR Apache-2.0，IDE Helper/Laravel MIT，实际新增代码沿用项目许可。Rust1.94.0兼容性以实际构建/Clippy为准。

参考：
- https://github.com/SeaQL/sea-orm/blob/1.1.19/sea-orm-cli/README.md
- https://github.com/diesel-rs/diesel/blob/v2.3.3/diesel_cli/src/print_schema.rs
- https://github.com/barryvdh/laravel-ide-helper/blob/v3.6.0/src/Console/ModelsCommand.php
- https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Console/ShowModelCommand.php

验收增加：三库实际Artisan生成与执行查询、幂等刷新、列增删与重命名、保留业务代码、命名空间/显式表与连接、生成失败无半成品、损坏标记拒绝写入、引用旧字段真实编译失败。不把第一版历史验收计为此修订通过。


自动生成修订最终状态：方案定稿、实现完成、范围内验收通过。`auto-final.log` 中 fmt / clippy / build / sqlite / postgres / mysql 全部退出0，包含带引号表名、非ASCII字段拒绝、重复/反向标记及不覆盖保护。三语站点及18项定向回归见验收记录；未完成整体WIP、完整workspace测试、all-features与生产等范围，不据此宣称全仓完成。


## 配置复用与同步检查修订（2026-11-23）

方案定稿、实现完成、范围内验收通过。此节覆盖上版“自定义表和连接每次须显式传入”的限制；最新证据见验收记录末节。

- 自动生成区域携带模型类型入口，复用已有固定 inventory =0.3.25 的静态登记机制。命令按需创建空 ModelState 模型，读取 getTable / resolveConnectionName；没有共享可变模型、连接或新管理器。普通构建不连接数据库。
- 同步已编译并登记的 app::models 模型时复用模型配置；单模型显式 --table / --database 优先。make:model --fields --database 同时保存连接至模型 metadata，消除原版只选择读取连接却不保存的差异。
- 首次迁移未登记旧模型仍使用约定表名／默认连接或显式参数；刷新区域后须重新构建。源代码表／连接配置变化也须先重新构建；不解析任意 Rust 表达式，不宣称运行时反射。
- model:fields 支持多个模型名；--all 仅枚举已编译并携带自动字段区域的 app/models 约定模型，非所有数据库表或任意 trait 实现。未声明模块、未启用feature、包模型不保证自动发现；重复类型路径歧义拒绝。多模型禁止共用覆盖参数。
- --check 不取得写锁、不修改生成源码或创建源码临时文件。相同结果返回0，过期返回1；缺表、错误配置、损坏标记也非零但输出具体错误。普通应用启动自身的缓存／日志不属于该只读源码保证。
- 批量先全部读取、查询结构、生成和校验，再写入各文件；预检失败不写任何模型。每个文件复用源码锁、并发内容检查及原子替换；跨文件不承诺文件系统事务，写入阶段IO失败可能已有部分完成，应修复后幂等重跑。

本轮实际联网下载：Laravel固定 ShowModelCommand，IDE Helper v3.6.0 ModelsCommand（getTable / getConnection / getColumns、批量加载模型），SeaORM1.1.19 CLI、Diesel2.3.3 print_schema、inventory0.3.25 README和Cargo.toml，见 target/tmp/model-fields-sync-20261123/research。web检索接口返回空；通过官方GitHub raw HTTPS实际取得源码。借鉴结构生成与静态类型入口，不复制源码。inventory最低Rust1.68、MIT OR Apache-2.0，项目已有依赖，无新增依赖；实际Rust1.94.0构建、严格Clippy及消费者已通过。旧版v0.x无当前模型字段生成机制可移植。


补充裁决：类型入口保存宏调用处的 module_path! 与模型标识，避免依赖 std::any::type_name 的诊断字符串格式；只在命令运行时创建空模型读取配置。Diesel v2.3.3 diesel_cli/src/main.rs::regenerate_schema_if_file_specified 在 LOCKED_SCHEMA 下比较生成结果、差异返回 SchemaWouldChange；借鉴此CI检查语义，不复制其目录创建副作用。inventory0.3.25的MIT许可已实际下载核对。固定Laravel ShowModelCommand直接构造模型读取属性；本轮查找固定源码树没有找到单独的ShowModel命令测试，不宣称执行PHPUnit。


最终验收：最新HEAD归档加当时工作区在途修改的隔离组合，完成30项定向Rust测试、三库真实生成/同步/查询与编译正反例、fmt、workspace/all-targets严格Clippy及三语构建/浏览器/导航。最终阶段全0，证据在 target/tmp/model-fields-sync-20261123/final.log；实际交付源码、消费者和正文与验收组合一致。无本修订剩余实现阻塞；完整workspace测试、all-features、Windows、release、生产与性能未验，不暂存/提交/推送。


## 数据库列说明修订（2026-11-23）

- [x] 方案定稿：复用Column.comment生成Rust字段文档属性，不维护平行说明列表、不猜测或翻译；dead_code理由在生成impl集中一次。
- [x] 实现完成：生成器保留数据库说明并安全转义；宏透传impl属性；说明变更复用--check比较；更新User真实生成区域和三语说明。
- [x] 范围内验收通过：SQLite/PG18/MySQL8.4实际命令消费者、rustdoc说明、特殊字符及编译正反、说明变更/删除/幂等/只读保护，真实迁移生成User；fmt、严格Clippy、构建、三语构建与浏览器通过。

实际命令、调研及未验边界见[数据库列描述验收](../audits/model-fields/README.md#数据库列描述生成文档注释2026-11-23)。SQLite没有原生列说明时无描述；原有类型安全边界不变。本次未暂存、提交或推送。
