//! 应用命令通过编译清单与显式注册装配。

use openrust::console::{Application as Artisan, ConsoleError};

type Registrar = fn(&mut Artisan) -> Result<(), ConsoleError>;

mod compiled {
    use super::Registrar;
    include!(concat!(env!("OUT_DIR"), "/commands.rs"));

    pub(super) const fn entries() -> &'static [Registrar] {
        include!(concat!(env!("OUT_DIR"), "/command_entries.rs"))
    }
}

pub(crate) fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    for command in compiled::entries() {
        command(artisan)?;
    }
    Ok(())
}
