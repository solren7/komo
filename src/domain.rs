//! The domain layer now lives in the dependency-light `komo-core` crate so the
//! GUI client can reuse the value types without komo's heavy runtime. This
//! facade re-exports it verbatim, so every existing `crate::domain::…` path
//! (and submodule path like `crate::domain::memory`) keeps resolving unchanged.
pub use komo_core::domain::*;
