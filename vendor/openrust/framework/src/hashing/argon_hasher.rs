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
