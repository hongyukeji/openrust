# sea-orm-migration 2.0.4 来源与局部依赖补丁

本目录完整保留官方发布 crate 的原始文件（含源码、测试、发布时 Cargo.lock 与 `.cargo_vcs_info.json`），只修改两个 manifest 的同一依赖默认 feature 设置。额外增加本说明及同一上游项目的双许可证。没有改写框架上游源码或测试，也没有修改 Cargo 全局缓存。

## 官方来源与归档校验

- 包：`sea-orm-migration`，版本 `2.0.4`。
- 官方仓库：[SeaQL/sea-orm](https://github.com/SeaQL/sea-orm)。
- 官方发布归档：[sea-orm-migration 2.0.4](https://static.crates.io/crates/sea-orm-migration/sea-orm-migration-2.0.4.crate)。
- 原归档 SHA256：`07970137dbbed6daf56a2429db349abfb68ed1c79206957c30d4a0e9ea808c08`。
- 官方归档内 VCS 信息：Git SHA `db40a19a7bde7be8e1c4b346b1b70d72e87f0245`，包路径 `sea-orm-migration`。
- 实际读取来源：本机 Cargo registry/cache 中上述原始 `.crate`；每个归档文件均逐字节比对 registry/src 解包缓存后复制。
- 当前解包缓存没有 `.cargo-checksum.json`，所以不存在可诚实引用的该文件 `package` 字段。没有伪造此文件；原归档 SHA256 已与复制时根 `Cargo.lock` 的 registry checksum，以及本地 crates.io sparse-index 对应 `vers=2.0.4` 的 `cksum` 分别核对一致。
- 不复制 registry 内部 `.cargo-ok`、缓存锁或其他运行器标记；`.cargo_vcs_info.json` 在官方 `.crate` 内原本存在，是发布来源元数据，予以保留。

## 唯一语义差异及原因

- `Cargo.toml` 的 `[dependencies.sea-orm]` 新增 `default-features = false`。
- `Cargo.toml.orig` 的 `sea-orm` 依赖同样新增 `default-features = false`。
- 版本约束 `~2.0.4`、显式 `schema-sync` 和其余依赖/feature/源码/测试均原样保留。
- 上游迁移 crate 原先没有关闭 SeaORM 默认 features，会重新启用 SeaORM 默认的 `sqlite-use-returning-for-3_35`。SQLite `RETURNING` 自 3.35 才支持，不能作为本仓库承诺最低 SQLite 3.26 的无条件执行语法。局部补丁使数据库驱动、RETURNING 等能力由实际消费 workspace 明确选择，避免通过迁移依赖暗中重新启用。
- 来源定位：官方 SeaORM 2.0.4 `Cargo.toml` default feature 列表包含 `sqlite-use-returning-for-3_35`；[SQLite RETURNING](https://www.sqlite.org/lang_returning.html) 记录其版本边界。本补丁仅修复依赖 feature 传播，不代表所有旧版 SQLite SQL 已适配。

## 许可补充与来源

迁移 crate 发布包未包含独立许可证文本；其 manifest 声明 `MIT OR Apache-2.0`。从同一官方 `sea-orm` 2.0.4 发布包补入 `LICENSE-MIT` 与 `LICENSE-APACHE`，字节内容另与其本机 registry/src 原件核对。

- 官方许可来源归档：[sea-orm 2.0.4](https://static.crates.io/crates/sea-orm/sea-orm-2.0.4.crate)。
- 该归档 SHA256：`40bc08e0aa1963295a30473f98d9b4c9c2b0b3a028381e0143cb2647d0a59a32`。
- 许可来源归档 VCS SHA：`db40a19a7bde7be8e1c4b346b1b70d72e87f0245`。
- `LICENSE-MIT` SHA256：`b937e4dc7e279159e5e61f9360ebb516fefb622791c445cdc88f504c4b58b019`。
- `LICENSE-APACHE` SHA256：`769f80b5bcb42ed0af4e4d2fd74e1ac9bf843cb80c5a29219d1ef3544428a6bb`。

## 逐文件 SHA256 与差异清单

下表左列摘要来自未经修改的官方迁移归档；右列来自本目录写入后的文件。除两份 manifest 外均应一致。官方归档未含的许可与说明不混入上游文件清单。

| 官方文件 | 官方原件 SHA256 | 本目录 SHA256 |
| --- | --- | --- |
| `.cargo_vcs_info.json` | `9c37571946905c95cd8c9e543cdbf1986953b21e2e4d4fbe71e483b650e91b0d` | `9c37571946905c95cd8c9e543cdbf1986953b21e2e4d4fbe71e483b650e91b0d` |
| `Cargo.lock` | `c55b1f1d81154b890383d8324887f3ea94ef4dc939c6ade44e87df8d90427aa9` | `c55b1f1d81154b890383d8324887f3ea94ef4dc939c6ade44e87df8d90427aa9` |
| `Cargo.toml` | `aa995b9339ed38607942440e25c0f76958e293b7c5fdf794bff2d3eb9c2e2e1a` | `40db4bbb1a0f2571a858775ce10b0f7c259603063046c6492dc81d77e41e22a0` |
| `Cargo.toml.orig` | `58a79263fcd959323ee172fc6a270b15bfd8dd48ef11a9a9ddd92969246b46eb` | `f7716cf017dbea4cf0ce9791eee1a16bbaf9df7cc77eb01eb34109203e1f44b3` |
| `README.md` | `26ef471ed7d7f78818c600aa016dcfafadec12ebf8d54d29dd9afd8708f46b77` | `26ef471ed7d7f78818c600aa016dcfafadec12ebf8d54d29dd9afd8708f46b77` |
| `src/cli.rs` | `0c9463f3e89f1dfc5a8480a09a665cf22051fd223e061abac7349c49b6bb39af` | `0c9463f3e89f1dfc5a8480a09a665cf22051fd223e061abac7349c49b6bb39af` |
| `src/connection.rs` | `cab0225955730694c7a43a45766665f9b7b24c183061b6fe7c999024f7d9323e` | `cab0225955730694c7a43a45766665f9b7b24c183061b6fe7c999024f7d9323e` |
| `src/lib.rs` | `3d30f3392a328bd59b6229542abf6d48ba302a85ffc9178ce6da127a01d2a3e9` | `3d30f3392a328bd59b6229542abf6d48ba302a85ffc9178ce6da127a01d2a3e9` |
| `src/manager.rs` | `95094ad773fa244ae89d8482083c287f7ac56a1d20c04d84752cb7d014d71ebb` | `95094ad773fa244ae89d8482083c287f7ac56a1d20c04d84752cb7d014d71ebb` |
| `src/migrator.rs` | `d980c1dc9f99409e2de37b9e3134eea044507d25136e01b746e0983530ead543` | `d980c1dc9f99409e2de37b9e3134eea044507d25136e01b746e0983530ead543` |
| `src/migrator/exec.rs` | `8142bd56a5cf362879f35ae877b869b7d4c238d82ce949ffc0e643bc45b68c95` | `8142bd56a5cf362879f35ae877b869b7d4c238d82ce949ffc0e643bc45b68c95` |
| `src/migrator/queries.rs` | `6968399a4447437c44f37e20e23e5e747acaabe8e0ba76bb97a107f91dc93bbd` | `6968399a4447437c44f37e20e23e5e747acaabe8e0ba76bb97a107f91dc93bbd` |
| `src/migrator/with_self.rs` | `1033e1de70accd04029628df72780b8f15c9caceb51fc97843129be03ca8f038` | `1033e1de70accd04029628df72780b8f15c9caceb51fc97843129be03ca8f038` |
| `src/prelude.rs` | `2389d5ee48774adc378ddfa9345a5e4e315b5245ff48d05da817cba696a1879d` | `2389d5ee48774adc378ddfa9345a5e4e315b5245ff48d05da817cba696a1879d` |
| `src/schema.rs` | `4166abade2b34fe9ad7b38f93def96125e3191aa60ec20fc552bb2f4b2397270` | `4166abade2b34fe9ad7b38f93def96125e3191aa60ec20fc552bb2f4b2397270` |
| `src/seaql_migrations.rs` | `ad8903343e836ab3e077236c6dc5133906cac845f35e3ac1d2cb56ed8af03b42` | `ad8903343e836ab3e077236c6dc5133906cac845f35e3ac1d2cb56ed8af03b42` |
| `src/util.rs` | `057acd88105e35afca2d57ef9f5f3d8af44c8fe501710832bf4b93ed558a1220` | `057acd88105e35afca2d57ef9f5f3d8af44c8fe501710832bf4b93ed558a1220` |
| `tests/common/migration/m20220118_000001_create_cake_table.rs` | `d8c3f926c230587650c781e8d09bfcd2ed64f2678a75fc1d64070eaf3c2ad64a` | `d8c3f926c230587650c781e8d09bfcd2ed64f2678a75fc1d64070eaf3c2ad64a` |
| `tests/common/migration/m20220118_000002_create_fruit_table.rs` | `ea0d9a2322dcdeceb28df7583313ce1b85386fb26591ee5bdf05c242df5290c0` | `ea0d9a2322dcdeceb28df7583313ce1b85386fb26591ee5bdf05c242df5290c0` |
| `tests/common/migration/m20220118_000003_seed_cake_table.rs` | `583389c17ad4b7928ddc512b5a2ea7bc00da1ea3b194d75e2ebe25fdf52ae7f4` | `583389c17ad4b7928ddc512b5a2ea7bc00da1ea3b194d75e2ebe25fdf52ae7f4` |
| `tests/common/migration/m20220118_000004_create_tea_enum.rs` | `dc5de576d4500841ff5f7e45b8d341a0df2584a21cb68390268e636a54c0a7e8` | `dc5de576d4500841ff5f7e45b8d341a0df2584a21cb68390268e636a54c0a7e8` |
| `tests/common/migration/m20220923_000001_seed_cake_table.rs` | `09e8ca5dff86154e4672b03b41bd894a38746dbdd8c1b063f25655287fb6af6d` | `09e8ca5dff86154e4672b03b41bd894a38746dbdd8c1b063f25655287fb6af6d` |
| `tests/common/migration/m20230109_000001_seed_cake_table.rs` | `9b19c1272404db9d94bebead5df568209e1b037e9a834b5bbfa296408a802e87` | `9b19c1272404db9d94bebead5df568209e1b037e9a834b5bbfa296408a802e87` |
| `tests/common/migration/m20250101_000001_create_test_table.rs` | `1c180b93aa18a3521d935315ed1eb414a3c6bafb11f8e248306cd657d713ffb3` | `1c180b93aa18a3521d935315ed1eb414a3c6bafb11f8e248306cd657d713ffb3` |
| `tests/common/migration/m20250101_000002_manual_transaction.rs` | `a8d4587c35e7c5c8cad16cd3a5b4763a971c22bfea4d5f973d11e69583211e2e` | `a8d4587c35e7c5c8cad16cd3a5b4763a971c22bfea4d5f973d11e69583211e2e` |
| `tests/common/migration/mod.rs` | `d57f17752e99e3b73015be3a3d9bfea6e82f6c07059e3766e47244d03bbdb423` | `d57f17752e99e3b73015be3a3d9bfea6e82f6c07059e3766e47244d03bbdb423` |
| `tests/common/migrator/default.rs` | `5344329bc66fb92400e8ed605d3d3668e84e7ff7957ee56abcfb35a2769f9196` | `5344329bc66fb92400e8ed605d3d3668e84e7ff7957ee56abcfb35a2769f9196` |
| `tests/common/migrator/mod.rs` | `af4151c9ddea31e1794c287ba5e60f21fac2730badb98834f6c65860ad8de4f5` | `af4151c9ddea31e1794c287ba5e60f21fac2730badb98834f6c65860ad8de4f5` |
| `tests/common/migrator/override_migration_table_name.rs` | `79d5ec638e8c8795a30bd834ac08b8a6426e981f268a24b96e54b69ce792bfe5` | `79d5ec638e8c8795a30bd834ac08b8a6426e981f268a24b96e54b69ce792bfe5` |
| `tests/common/migrator/transaction_test.rs` | `314d362eef99243532962e6d7c399d23712d837ab6bd9a00189fabf30c24f1b2` | `314d362eef99243532962e6d7c399d23712d837ab6bd9a00189fabf30c24f1b2` |
| `tests/common/migrator/with_self.rs` | `a91b40632750f983c40c1897b0b32ec4589744e2225368e630e9302fa9f9d8e4` | `a91b40632750f983c40c1897b0b32ec4589744e2225368e630e9302fa9f9d8e4` |
| `tests/common/mod.rs` | `9b50cbeff458290787a02da609952d0b92e40ff2d6fe3533957dbaf0dcdf27c4` | `9b50cbeff458290787a02da609952d0b92e40ff2d6fe3533957dbaf0dcdf27c4` |
| `tests/main.rs` | `59fc928fa3bc88930f92e7bdfdad99dc5c4fc9f3bc7e4bfe00b9fbe3547393bc` | `59fc928fa3bc88930f92e7bdfdad99dc5c4fc9f3bc7e4bfe00b9fbe3547393bc` |
| `tests/postgres.rs` | `d0f6082aaa67a14e6d976feb0234bbd74cf8232ad78a9cf115639a8fd87ad4e3` | `d0f6082aaa67a14e6d976feb0234bbd74cf8232ad78a9cf115639a8fd87ad4e3` |

新增文件：`SOURCES.md`（本说明）、`LICENSE-MIT`、`LICENSE-APACHE`（上述已记录来源与哈希）。本说明不自包含自身哈希，避免递归校验表。

## 维护、验证与后续发布边界

- 本目录是根 workspace 的有据局部依赖补丁；根 Cargo/lock 的接线、版本政策、feature 图及公开发布路径由集成负责人另行维护。
- 当前仅完成归档/逐文件来源比对与两处 manifest 修改，尚未在此子任务运行 Cargo、测试或真实服务，不声称上游或 SQLite 3.26 适配已经通过。
- 升级时重新取得固定官方归档、校验 registry checksum，逐文件检查此单一补丁是否仍需要；不得把上游新实现静默混进现有版本号目录。
- 若上游正式关闭该依赖默认 feature，应优先评估移除本补丁并回归 registry 依赖；仍须实际 feature 图与承诺后端验收。
- 本说明不授权发布修改后的同名 crate、替上游打 tag、修改上游仓库或发布 OpenRust。公开打包、path/patch 解析及依赖可重现性必须由后续发布门禁单独证明。
