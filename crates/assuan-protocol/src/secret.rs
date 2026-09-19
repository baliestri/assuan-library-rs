use alloc::{boxed::Box, vec::Vec};
use core::fmt;
use zeroize::{Zeroize, Zeroizing};

use crate::{LimitError, limits::extended_length};

/// Owned bytes in a fixed-capacity heap allocation that is wiped on drop.
///
/// Capacity is allocated before inserting a secret and never grows. Neither
/// this type nor its borrowed view implements `Clone`, `Copy`, `Display`,
/// `Deref`, or `AsRef`; reading requires an explicit [`Self::expose`] call.
/// `Debug` reports only length and capacity.
///
/// Dropping or [`clearing`](Self::clear) wipes the entire allocation using
/// `zeroize`. This does not wipe input slices, caller-made copies, primitive
/// values moved on the stack, OS buffers, swap, or crash dumps. Leaking the
/// value with `core::mem::forget` or abruptly terminating the process prevents
/// its destructor from running. Moving this wrapper does not move its heap
/// contents. Choose capacity from a trusted limit before handling input.
///
/// ```
/// use assuan_protocol::SecretBytes;
/// let mut secret = SecretBytes::with_capacity(64)?;
/// secret.extend_from_slice(b"example")?;
/// assert_eq!(secret.expose(), b"example");
/// secret.clear();
/// assert!(secret.is_empty());
/// # Ok::<(), assuan_protocol::LimitError>(())
/// ```
pub struct SecretBytes {
    storage: Zeroizing<Box<[u8]>>,
    len: usize,
}

impl SecretBytes {
    /// Allocates exactly `capacity` usable bytes, initially zeroed.
    ///
    /// A zero capacity is valid. No secret is present during allocation.
    ///
    /// # Errors
    /// Returns [`LimitError::AllocationFailed`] if reservation fails, including
    /// requests too large for the platform's allocator.
    pub fn with_capacity(capacity: usize) -> Result<Self, LimitError> {
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(capacity)
            .map_err(|_| LimitError::AllocationFailed)?;
        bytes.resize(capacity, 0);
        Ok(Self {
            storage: Zeroizing::new(bytes.into_boxed_slice()),
            len: 0,
        })
    }

    /// Copies bytes into the unused capacity without reallocating.
    ///
    /// The input remains the caller's responsibility to protect and wipe.
    /// An error leaves both the contents and length unchanged.
    ///
    /// # Errors
    /// Returns [`LimitError::LengthOverflow`] if length addition overflows or
    /// [`LimitError::CapacityExceeded`] if the bytes do not fit.
    pub fn extend_from_slice(&mut self, bytes: &[u8]) -> Result<(), LimitError> {
        let end = extended_length(self.len, bytes.len(), self.capacity())?;
        self.storage[self.len..end].copy_from_slice(bytes);
        self.len = end;
        Ok(())
    }

    /// Explicitly exposes the initialized bytes for the duration of the borrow.
    #[must_use]
    pub fn expose(&self) -> &[u8] {
        &self.storage[..self.len]
    }

    /// Wipes the whole allocation and resets the length, retaining capacity.
    pub fn clear(&mut self) {
        self.storage[..].zeroize();
        self.len = 0;
    }

    /// Returns the number of initialized bytes.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Returns whether no initialized bytes are stored.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Returns the fixed maximum number of bytes this allocation can store.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.storage.len()
    }
}

impl fmt::Debug for SecretBytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SecretBytes")
            .field("len", &self.len)
            .field("capacity", &self.capacity())
            .finish_non_exhaustive()
    }
}

/// A borrowed secret whose diagnostics never include its contents.
///
/// This view neither owns nor wipes the bytes. The owner must manage their
/// lifetime and cleanup. Access is explicit through [`Self::expose`].
pub struct SecretRef<'a> {
    bytes: &'a [u8],
}

impl<'a> SecretRef<'a> {
    /// Borrows bytes without allocating, copying, or taking ownership.
    #[must_use]
    pub const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes }
    }

    /// Explicitly exposes the bytes with the original owner's lifetime.
    #[must_use]
    pub const fn expose(&self) -> &'a [u8] {
        self.bytes
    }
}

impl fmt::Debug for SecretRef<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SecretRef")
            .field("len", &self.bytes.len())
            .finish_non_exhaustive()
    }
}

/// How a payload should be handled by a protocol consumer.
///
/// Classification carries no bytes and does not itself wipe any storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sensitivity {
    /// The caller permits ordinary handling of the payload.
    Public,
    /// The payload requires protected storage and redacted diagnostics.
    Secret,
}

/// A borrowed payload with an explicit public or secret classification.
///
/// Both variants omit content from `Debug`. Neither variant takes ownership
/// or wipes the borrowed memory; the owner remains responsible for cleanup.
pub enum PayloadRef<'a> {
    /// Public bytes, borrowed without copying.
    Public(&'a [u8]),
    /// Sensitive bytes that require explicit exposure.
    Secret(SecretRef<'a>),
}

impl fmt::Debug for PayloadRef<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (variant, len) = match self {
            Self::Public(bytes) => ("Public", bytes.len()),
            Self::Secret(secret) => ("Secret", secret.bytes.len()),
        };
        f.debug_struct(variant)
            .field("len", &len)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clear_wipes_the_entire_allocation_and_preserves_it() {
        let mut secret = SecretBytes::with_capacity(16).unwrap();
        secret.extend_from_slice(b"pass").unwrap();
        // Simulate stale contents outside the logical length, without freeing.
        secret.storage[4..].fill(0xAA);
        let pointer = secret.storage.as_ptr();
        secret.clear();
        assert!(secret.storage.iter().all(|byte| *byte == 0));
        assert_eq!(secret.storage.as_ptr(), pointer);
        assert_eq!(secret.capacity(), 16);
        assert!(secret.is_empty());
    }
}
