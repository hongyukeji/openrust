---
name: webapp-testing
description: 验证 OpenRust 的 Vue Web 页面与真实 Rust API 完整流程，适用于浏览器功能测试、截图排错和端到端回归。不将 H5 测试当作 uni-app Android/iOS 验收。
license: Apache-2.0; see LICENSE.txt
metadata:
  upstream: https://github.com/anthropics/skills
  revision: dbd4588f9e1033efb41dad4bef2f7947c8993d44
  path: skills/webapp-testing
  adaptation: Project runner and real API validation; no bundled server launcher
---

# Web 应用验收

改编了上游 Playwright 工作流，移除 Python 强制路径、服务启动脚本和固定网络空闲等待。许可见 [LICENSE.txt](LICENSE.txt)。

## 准备

- 读取根 `AGENTS.md`、当前应用测试脚本和验收 contract；确认页面、API 地址、角色、数据隔离与服务所有者。复用合适的已运行服务，不停止他人进程。
- 优先使用项目已有测试 runner；持久回归测试跟随前端语言和工具链。临时操作使用当前获准浏览器工具，先读取其文档，不假设所有环境都有同一工具或 Playwright 包。
- 缺少页面或运行环境时报告缺口，不为验证技能擅自创建 V2 应用。截图、trace 和测试数据避免保存真实凭据。

## 执行

1. 明确初始状态、用户动作、页面断言与服务端结果；按需选择 [后台流程矩阵](references/admin-flows.md)。
2. 用实际 DOM、可访问名称或项目测试标识定位；浏览器操作基于当前观察，不猜测控件和未确认的路径。
3. 等待明确的页面状态、响应或可重试断言，不把 `networkidle` 当通用就绪条件；SSE、轮询等可能长期保持网络活动。`nextTick` 也不等于真实网络请求完成。
4. 行为验收连接真实 Rust API。组件 fake 与受控网络故障注入单独报告，不能代替真实认证、权限和持久化流程。涉及数据的功能簇在 PostgreSQL/MySQL 分别验证适用路径。
5. 检查页面结果、控制台、网络失败与必要的后端状态；通过重新读取或重载确认持久化，而不是只看成功 toast。

## 收口

输出实际命令/动作、浏览器与版本、视口、角色、数据后端、通过/失败/跳过及证据位置。视觉截图与功能断言分别报告；只在移动视口测试不能宣称真机测试。

只关闭本任务创建的浏览器会话、进程和测试数据；按现有 supervisor/fixture 处理退出与回收。服务启动成功、编译通过和截图存在都不等于流程通过。
