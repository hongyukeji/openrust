use super::{
    BcryptConfig,
    driver::{Settings, define_hasher},
};

define_hasher!(
    BcryptHasher,
    BcryptConfig,
    Settings::Bcrypt,
    "Hashes passwords using bcrypt."
);

impl BcryptHasher {
    /// 修改后续生成和重哈希检查使用的默认成本；单次选项仍可覆盖。
    pub fn set_rounds(&mut self, rounds: u32) -> &mut Self {
        let Settings::Bcrypt(config) = &mut self.0.settings else {
            unreachable!("bcrypt 驱动配置类型不匹配")
        };
        config.rounds = rounds;
        self
    }
}
