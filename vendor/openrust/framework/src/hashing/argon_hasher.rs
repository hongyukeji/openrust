use super::{
    ArgonConfig,
    driver::{Settings, define_hasher},
};

define_hasher!(
    ArgonHasher,
    ArgonConfig,
    |config| Settings::Argon(config, argon2::Algorithm::Argon2i),
    "Hashes passwords using Argon2i."
);

#[allow(non_snake_case)]
impl ArgonHasher {
    /// 修改默认参数，执行时继续检查有效性及资源上限。
    pub fn setMemory(&mut self, value: u32) -> &mut Self {
        let Settings::Argon(config, _) = &mut self.0.settings else {
            unreachable!("Argon 驱动配置类型不匹配")
        };
        config.memory = value;
        self
    }
    /// 修改默认参数，执行时继续检查有效性及资源上限。
    pub fn setTime(&mut self, value: u32) -> &mut Self {
        let Settings::Argon(config, _) = &mut self.0.settings else {
            unreachable!("Argon 驱动配置类型不匹配")
        };
        config.time = value;
        self
    }
    /// 修改默认参数，执行时继续检查有效性及资源上限。
    pub fn setThreads(&mut self, value: u32) -> &mut Self {
        let Settings::Argon(config, _) = &mut self.0.settings else {
            unreachable!("Argon 驱动配置类型不匹配")
        };
        config.threads = value;
        self
    }
}
