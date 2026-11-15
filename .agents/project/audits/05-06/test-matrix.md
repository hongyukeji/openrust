# Laravel 13.x 固定分页用例核对

基线为 `1449bf241017087fbd38c118ecaabef3f6797fdc`。[固定用例目录](https://github.com/laravel/framework/tree/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Pagination) 共有 11 个测试文件和 51 个 `test*` 方法。下表记录对应行为及负责验收的交付单元。

| 固定用例组 | 数量 | OpenRust 证据与差异 |
| --- | ---: | --- |
| `CursorTest.php` | 7 | 复合键和方向编解码、具名键查询、非法 Base64、JSON、数组和缺失方向由 `rejects_invalid_manual_page_sizes_and_cursor_parameters` 覆盖。输入在 Rust 编译期限定为字符串，解码错误返回 `PaginationError`，请求解析层回退第一页。 |
| `PaginatorTest.php` | 9 | `pagination_on_*_and_http` 覆盖简易预读、请求命名、回翻和类型转换，`manual_paginators_serialize_and_keep_link_boundaries` 验证手工路径及 JSON 三种形式。Rust 选项借用当前生效的路径、参数和片段。 |
| `LengthAwarePaginatorTest.php` | 10 | 双库 HTTP 验证页名、总数、范围与偏移。手工回归覆盖空集合、零页容量、首尾状态、尾部斜杠和已有编码查询的路径。手工零容量保留零值，末页除数保护与固定 Laravel 一致。 |
| `CursorPaginatorTest.php` | 11 | 双库 HTTP 验证正向、逆向、回到首页和转换后继续导航。手工回归验证空页、尾斜杠、前后状态、紧凑及格式化 JSON。Rust 由键回调提供游标值，在类型转换前保留链接。 |
| `UrlWindowTest.php` | 4 | `length_aware_url_window_preserves_each_segment` 覆盖短窗口、开头、中部、末尾和自定义相邻范围。实际 HTTP 验证两个省略号及当前页标记。 |
| `{Paginator,CursorPaginator}LoadMorph{,Count}Test.php` | 4 | 三类分页器的同名入口直接复用集合批量加载与回填。双库 HTTP 验证同一分页器的链式返回、混合类型、计数、空页及非法类型到页才报错。 |
| `{Paginator,Cursor}ResourceTest.php` | 6 | 显式 Resource、自动推断及找不到 Resource 的契约在 05-07 验收。 |

上层关系分页、Blade 渲染、翻译和 Artisan 模板发布的实际入口验收见[本单元记录](README.md)。命名路由链接按计划在 06-01 验证。
