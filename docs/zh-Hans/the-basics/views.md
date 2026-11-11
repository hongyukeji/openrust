# 视图

## 创建和渲染视图

将视图放在 `resources/views`。路由通过 `view` 返回命名视图。比如 `view("admin.profile")` 按 `view.paths` 的目录顺序，在每个目录查找 `admin/profile.blade.html` 和 `admin/profile.html`。

## 向视图传递数据

用 `with` 传递数据。[Blade 模板](./blade.md)可以通过 `$name` 读取该值。

```rust
use openrust::{routing::RoutingError, support::facades::Route, view::view};

fn register() -> Result<(), RoutingError> {
    Route::get("/hello", || view("greeting").with("name", "James"))
}
```

## 与所有视图共享数据

在应用服务提供者中调用 `View::share`，为该应用的视图设置默认数据。传入视图的数据优先于共享默认值。

## 视图组合器

在服务提供者的 `boot` 方法中注册组合器。它会在匹配的视图渲染前执行，视图名称可使用 `*` 通配符。需要在创建视图时执行回调，可以使用 `View::creator`。

```rust
use openrust::{support::facades::View, view::ViewResult};

fn configure_views() -> ViewResult<()> {
    View::share("site", "OpenRust")?;
    View::composer("dashboard", |view| {
        view.data_mut().insert("title".into(), "Dashboard".into());
        Ok(())
    })
}
```

## 优化视图

Blade 模板在首次使用时编译。源文件变化后，OpenRust 会在 `view.compiled` 中重新生成编译产物，默认目录为 `storage/framework/views`。可以通过 Artisan 预编译视图或清理编译产物。

```sh
./artisan view:cache
./artisan view:clear
```
