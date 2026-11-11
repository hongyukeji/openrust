//! Plain-file and compiled template engines.

mod compiler_engine;
mod engine_resolver;
mod file_engine;

pub use compiler_engine::CompilerEngine;
pub use engine_resolver::{Engine, EngineResolver};
pub use file_engine::FileEngine;
