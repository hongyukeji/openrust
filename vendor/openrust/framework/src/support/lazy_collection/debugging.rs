use std::{
    fmt::Debug,
    io::{self, Write},
};

use super::{CollectionError, CollectionResult, LazyCollection};

#[allow(non_snake_case, reason = "Matches Laravel's LazyCollection API")]
impl<'source, T: Debug + 'source> LazyCollection<'source, T> {
    /// Writes a snapshot of the collection to standard output and returns the source.
    pub fn dump(self) -> CollectionResult<Self> {
        self.dumpWith(&[])
    }

    /// Also writes the supplied context values after the collection snapshot.
    pub fn dumpWith(self, context: &[&dyn Debug]) -> CollectionResult<Self> {
        self.dumpTo(io::stdout(), context)
    }

    /// Writes the collection and context to a caller-selected output.
    pub fn dumpTo(self, mut output: impl Write, context: &[&dyn Debug]) -> CollectionResult<Self> {
        let entries = self.all()?;
        let mut write = || -> io::Result<()> {
            writeln!(output, "{entries:#?}")?;
            for value in context {
                writeln!(output, "{value:#?}")?;
            }
            Ok(())
        };
        write().map_err(|error| CollectionError::Source(Box::new(error)))?;
        Ok(self)
    }
}
