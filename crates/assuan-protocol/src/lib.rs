#![doc = include_str!("../README.md")]
#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

mod error;
mod limits;
mod secret;

pub use error::LimitError;
pub use secret::{PayloadRef, SecretBytes, SecretRef, Sensitivity};
