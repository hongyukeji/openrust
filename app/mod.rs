//! 应用模块注册清单；新增业务只需声明模块并注册其公开插件。

pub(crate) mod example;

pub(crate) fn register(app: &mut openrust::AppBuilder) {
    app.plugin(example::ExamplePlugin);
}
