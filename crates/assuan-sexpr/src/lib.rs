#![doc = include_str!("../README.md")]
#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

mod error;
mod limits;
mod parser;
mod value;
mod writer;

pub use error::{EncodeError, ParseError, ParseErrorKind};
pub use limits::{MAX_NESTING_DEPTH, ParseLimits};
pub use parser::{parse_complete, parse_prefix};
pub use value::{OwnedSexpr, Sexpr};
