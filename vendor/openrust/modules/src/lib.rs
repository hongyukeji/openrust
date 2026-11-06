//! 固定 nWidart laravel-modules v13.2.0 的独立扩展。
//!
//! 发现、启用状态、模块资源与命令复用唯一 OpenRust Provider/Container 生命周期。
//! 运行发现仅选择已编译的 Rust 绑定，不执行未编译源码。
//!
#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]
#![deny(missing_docs)]

/// 模块配置类型；默认值与显式配置由同一扩展 Provider 登记。
pub mod config;
mod module;
mod registration;
mod testing;

pub use config::{
    FileModuleActivatorConfig, ModuleActivatorsConfig, ModulePathsConfig, ModuleScanConfig,
    ModulesConfig,
};
pub use module::{
    FileModuleActivator, Module, ModuleActivationSnapshot, ModuleActivator, ModuleConfigFiles,
    ModuleDescriptor, ModuleExecution, ModuleFile, ModuleLifecycleEvent, ModuleLifecyclePhase,
    ModuleLoadContext, ModuleManifest, ModuleRepository,
};
pub use registration::ModuleProviderBuilderExt;
pub use testing::ModuleTestAppBuilderExt;
