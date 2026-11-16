use fake::rand::{
    self,
    distr::{Alphanumeric, SampleString},
    seq::{IndexedRandom, SliceRandom},
};

/// Character pools for a generated password.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PasswordOptions {
    pub letters: bool,
    pub numbers: bool,
    pub symbols: bool,
    pub spaces: bool,
}

impl Default for PasswordOptions {
    fn default() -> Self {
        Self {
            letters: true,
            numbers: true,
            symbols: true,
            spaces: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EmptyPasswordAlphabet;

impl std::fmt::Display for EmptyPasswordAlphabet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("at least one password character pool must be enabled")
    }
}

impl std::error::Error for EmptyPasswordAlphabet {}

pub(super) fn alphanumeric(length: usize) -> String {
    Alphanumeric.sample_string(&mut rand::rng(), length)
}

pub(super) fn password(
    length: usize,
    options: PasswordOptions,
) -> Result<String, EmptyPasswordAlphabet> {
    let mut pools = Vec::new();
    for (enabled, alphabet) in [
        (
            options.letters,
            b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ".as_slice(),
        ),
        (options.numbers, b"0123456789".as_slice()),
        (options.symbols, b"~!#$%^&*()-_.,<>?/\\{}[]|:;".as_slice()),
        (options.spaces, b" ".as_slice()),
    ] {
        if enabled {
            pools.push(alphabet);
        }
    }
    if pools.is_empty() {
        return Err(EmptyPasswordAlphabet);
    }

    let mut random = rand::rng();
    pools.shuffle(&mut random);
    let mut output = Vec::with_capacity(length);
    for pool in pools.iter().take(length) {
        output.push(*pool.choose(&mut random).expect("non-empty character pool"));
    }
    let alphabet: Vec<_> = pools.into_iter().flatten().copied().collect();
    while output.len() < length {
        output.push(*alphabet.choose(&mut random).expect("non-empty alphabet"));
    }
    output.shuffle(&mut random);
    Ok(output.into_iter().map(char::from).collect())
}
