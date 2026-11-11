use super::{
    ArgonConfig,
    driver::{Settings, define_hasher},
};

define_hasher!(
    Argon2IdHasher,
    ArgonConfig,
    |config| Settings::Argon(config, argon2::Algorithm::Argon2id),
    "Hashes passwords using Argon2id."
);
