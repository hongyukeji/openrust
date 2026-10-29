---
name: uniapp-development
description: 规划、实现或验证 OpenRust 配套 uni-app App 的页面、导航、生命周期、请求和平台差异。适用于移动端任务，不将 Vue Web、H5 浏览器成功或 PC 规划等同于原生 App 实现。
---

# uni-app 移动端开发

项目自编技能，参考 DCloud 官方文档；不是 DCloud 官方发布的技能。范围与启动顺序见 [可选快速开发模块](../../project/plans/optional-rapid-development-modules.md)，本技能不授权提前创建 App 或开发 PC 客户端。

## 先确认运行目标

读取当前项目配置、编译器、语言与目标平台。用户已选 uni-app 技术路线；经典 uni-app、uni-app x、渲染模式与插件兼容性必须在落地前明确，不混用 `.vue`、`.nvue`、`.uvue`、TS 与 UTS 假定。核对 [当前官方选型与维护说明](https://doc.dcloud.net.cn/uni-app-x/)，不要将历史编译支持表当作当前维护承诺。

只针对已批准目标构建和验证。Android/iOS 的版本、设备、权限与打包条件在实现 contract 中明确；小程序、鸿蒙等不是采用 uni-app 就自动承诺支持。

## 平台边界

- 页面注册和导航遵循 [pages.json](https://uniapp.dcloud.net.cn/collocation/pages.html)，不把 Vue Router 的守卫、history 和动态菜单机制直接移植到 App。
- 按 [页面生命周期](https://uniapp.dcloud.net.cn/tutorial/page.html) 区分组件挂载、页面显示/隐藏和卸载。返回栈、前后台切换、重新进入时的数据刷新和订阅清理要有明确语义。
- 共享逻辑保持平台无关；DOM、window、浏览器存储和 Web UI 组件不能默认用于非 Web 目标。[条件编译](https://uniapp.dcloud.net.cn/tutorial/platform.html) 只封装真实平台差异，不复制整套业务流程。
- 通过合适的跨端请求适配调用 Rust API。以 [uni.request](https://uniapp.dcloud.net.cn/api/request/request.html) 为例，传输完成仍需检查 HTTP 状态与业务错误；定义超时、取消、刷新轮换与失败恢复。不能直接照搬 Web cookie/CORS 假设，也不为模板引入另一套云端业务后端。
- 令牌保存与清理遵循已定安全契约；客户端缓存不承担授权。日志、持久缓存、截图和打包配置不得暴露凭据。
- App 布局检查安全区域、软键盘、返回键、字体缩放、弱网、离线和权限拒绝。后台大表格应按移动任务重组，不只缩小字号。

## 验证与证据

按功能簇执行实际目标构建及设备/模拟器运行，覆盖登录刷新与退出、导航返回、前后台恢复、网络失败，以及该功能涉及的文件/系统权限。连接真实 Rust API；数据相关流程按项目要求分别验证 PostgreSQL/MySQL。

记录目标平台、系统/运行时版本、设备或模拟器、打包类型、真实与 mock 范围、结果和证据。H5/浏览器移动视口测试不能替代 App 验收；设备、签名或供应商条件缺失时继续独立工作并保留缺口，不虚报通过。发布与商店提交仍按独立授权执行。
