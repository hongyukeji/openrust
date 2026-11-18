use openrust::console::ConsoleError;

use crate::value_objects::MessageLogged;

/// Renders a captured message to the selected output stream.
pub trait Printer {
    fn print(&mut self, message: &MessageLogged) -> Result<(), ConsoleError>;
}
