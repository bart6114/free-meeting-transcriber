use serde::{Serialize, ser::Serializer};

pub type Result<T> = std::result::Result<T, Error>;

// Analytics is a no-op (see `lib.rs`), so nothing here ever actually fails.
// The type is kept so the `Analytics` methods keep a stable `Result<_, Error>`
// signature for their in-process Rust callers.
#[derive(Debug, thiserror::Error)]
pub enum Error {}

impl Serialize for Error {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.to_string().as_ref())
    }
}
