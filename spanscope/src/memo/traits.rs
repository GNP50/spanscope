//! Key conversion, exact equality, and size estimation for memoized functions.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::hash::Hash;
use std::rc::Rc;
use std::sync::Arc;

/// Converts a borrowed argument into the owned key stored by a memo cache.
///
/// The stored key is the full value, never a hash: two calls share a result
/// only when their keys are equal. Floats convert to their bit patterns, so
/// `0.0` and `-0.0` are different keys and a NaN only matches the same NaN.
///
/// Implement it for your own types by returning an owned, comparable value:
///
/// ```
/// use spanscope::memo::MemoKey;
///
/// struct Fiber { length: f64, loss_db: f64, label: String }
///
/// impl MemoKey for Fiber {
///     type Key = (u64, u64, String);
///     fn memo_key(&self) -> Self::Key {
///         (self.length.memo_key(), self.loss_db.memo_key(), self.label.clone())
///     }
/// }
/// # let fiber = Fiber { length: 1.0, loss_db: 0.2, label: "a".into() };
/// # assert_eq!(fiber.memo_key(), (1.0f64.to_bits(), 0.2f64.to_bits(), "a".to_owned()));
/// ```
pub trait MemoKey {
    /// Owned key type compared and hashed by the cache.
    type Key: Eq + Hash + 'static;

    /// Returns the owned key for this value.
    fn memo_key(&self) -> Self::Key;
}

/// Exact equality used by `verify` mode to compare a cached and a fresh result.
///
/// Floats compare bit for bit, so a hit is only accepted when it is
/// indistinguishable from recomputation.
pub trait MemoEq {
    /// Returns whether both values are exactly the same result.
    fn memo_eq(&self, other: &Self) -> bool;
}

/// Estimated memory held by a cached key or value, used by `max_bytes`.
///
/// The default is the shallow size. Implement it for types that own heap
/// memory; `impl MemoSize for MyType {}` keeps the default.
pub trait MemoSize {
    /// Returns the estimated bytes held by this value, including its heap data.
    fn memo_size(&self) -> usize {
        std::mem::size_of_val(self)
    }
}

macro_rules! exact {
    ($($ty:ty),* $(,)?) => {$(
        impl MemoKey for $ty {
            type Key = $ty;
            #[inline]
            fn memo_key(&self) -> Self::Key {
                *self
            }
        }
        impl MemoEq for $ty {
            #[inline]
            fn memo_eq(&self, other: &Self) -> bool {
                self == other
            }
        }
        impl MemoSize for $ty {}
    )*};
}

exact!(
    u8,
    u16,
    u32,
    u64,
    u128,
    usize,
    i8,
    i16,
    i32,
    i64,
    i128,
    isize,
    bool,
    char,
    ()
);

macro_rules! float {
    ($($ty:ty => $bits:ty),* $(,)?) => {$(
        impl MemoKey for $ty {
            type Key = $bits;
            #[inline]
            fn memo_key(&self) -> Self::Key {
                self.to_bits()
            }
        }
        impl MemoEq for $ty {
            #[inline]
            fn memo_eq(&self, other: &Self) -> bool {
                self.to_bits() == other.to_bits()
            }
        }
        impl MemoSize for $ty {}
    )*};
}

float!(f32 => u32, f64 => u64);

impl MemoKey for str {
    type Key = String;
    fn memo_key(&self) -> String {
        self.to_owned()
    }
}

impl MemoKey for String {
    type Key = String;
    fn memo_key(&self) -> String {
        self.clone()
    }
}

impl MemoEq for str {
    fn memo_eq(&self, other: &Self) -> bool {
        self == other
    }
}

impl MemoEq for String {
    fn memo_eq(&self, other: &Self) -> bool {
        self == other
    }
}

impl MemoSize for str {}

impl MemoSize for String {
    fn memo_size(&self) -> usize {
        std::mem::size_of::<String>() + self.capacity()
    }
}

macro_rules! pointer {
    ($($ptr:ident),*) => {$(
        impl<T: MemoKey + ?Sized> MemoKey for $ptr<T> {
            type Key = T::Key;
            #[inline]
            fn memo_key(&self) -> Self::Key {
                (**self).memo_key()
            }
        }
        impl<T: MemoEq + ?Sized> MemoEq for $ptr<T> {
            #[inline]
            fn memo_eq(&self, other: &Self) -> bool {
                (**self).memo_eq(&**other)
            }
        }
        impl<T: MemoSize + ?Sized> MemoSize for $ptr<T> {
            fn memo_size(&self) -> usize {
                std::mem::size_of::<Self>() + (**self).memo_size()
            }
        }
    )*};
}

pointer!(Box, Rc, Arc);

impl<T: MemoKey + ?Sized> MemoKey for &T {
    type Key = T::Key;
    #[inline]
    fn memo_key(&self) -> Self::Key {
        (**self).memo_key()
    }
}

impl<T: MemoKey + ?Sized> MemoKey for &mut T {
    type Key = T::Key;
    #[inline]
    fn memo_key(&self) -> Self::Key {
        (**self).memo_key()
    }
}

impl<T: MemoEq + ?Sized> MemoEq for &T {
    fn memo_eq(&self, other: &Self) -> bool {
        (**self).memo_eq(&**other)
    }
}

impl<T: MemoKey> MemoKey for [T] {
    type Key = Vec<T::Key>;
    fn memo_key(&self) -> Self::Key {
        self.iter().map(MemoKey::memo_key).collect()
    }
}

impl<T: MemoKey> MemoKey for Vec<T> {
    type Key = Vec<T::Key>;
    fn memo_key(&self) -> Self::Key {
        self.as_slice().memo_key()
    }
}

impl<T: MemoKey> MemoKey for VecDeque<T> {
    type Key = Vec<T::Key>;
    fn memo_key(&self) -> Self::Key {
        self.iter().map(MemoKey::memo_key).collect()
    }
}

impl<T: MemoKey, const N: usize> MemoKey for [T; N] {
    type Key = Vec<T::Key>;
    fn memo_key(&self) -> Self::Key {
        self.as_slice().memo_key()
    }
}

impl<T: MemoKey> MemoKey for BTreeSet<T> {
    type Key = Vec<T::Key>;
    fn memo_key(&self) -> Self::Key {
        self.iter().map(MemoKey::memo_key).collect()
    }
}

impl<K: MemoKey, V: MemoKey> MemoKey for BTreeMap<K, V> {
    type Key = Vec<(K::Key, V::Key)>;
    fn memo_key(&self) -> Self::Key {
        self.iter()
            .map(|(key, value)| (key.memo_key(), value.memo_key()))
            .collect()
    }
}

impl<T: MemoEq> MemoEq for [T] {
    fn memo_eq(&self, other: &Self) -> bool {
        self.len() == other.len() && self.iter().zip(other).all(|(a, b)| a.memo_eq(b))
    }
}

impl<T: MemoEq> MemoEq for Vec<T> {
    fn memo_eq(&self, other: &Self) -> bool {
        self.as_slice().memo_eq(other.as_slice())
    }
}

impl<T: MemoEq, const N: usize> MemoEq for [T; N] {
    fn memo_eq(&self, other: &Self) -> bool {
        self.as_slice().memo_eq(other.as_slice())
    }
}

impl<T: MemoSize> MemoSize for [T] {
    fn memo_size(&self) -> usize {
        self.iter().map(MemoSize::memo_size).sum()
    }
}

impl<T: MemoSize> MemoSize for Vec<T> {
    fn memo_size(&self) -> usize {
        let spare = (self.capacity() - self.len()) * std::mem::size_of::<T>();
        std::mem::size_of::<Self>() + spare + self.as_slice().memo_size()
    }
}

impl<T: MemoSize, const N: usize> MemoSize for [T; N] {
    fn memo_size(&self) -> usize {
        self.as_slice().memo_size()
    }
}

impl<T: MemoKey> MemoKey for Option<T> {
    type Key = Option<T::Key>;
    fn memo_key(&self) -> Self::Key {
        self.as_ref().map(MemoKey::memo_key)
    }
}

impl<T: MemoEq> MemoEq for Option<T> {
    fn memo_eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Some(a), Some(b)) => a.memo_eq(b),
            (None, None) => true,
            _ => false,
        }
    }
}

impl<T: MemoSize> MemoSize for Option<T> {
    fn memo_size(&self) -> usize {
        let inline = std::mem::size_of::<Self>();
        self.as_ref().map_or(inline, |value| {
            inline + value.memo_size().saturating_sub(std::mem::size_of::<T>())
        })
    }
}

impl<T: MemoKey, E: MemoKey> MemoKey for Result<T, E> {
    type Key = Result<T::Key, E::Key>;
    fn memo_key(&self) -> Self::Key {
        self.as_ref()
            .map(MemoKey::memo_key)
            .map_err(MemoKey::memo_key)
    }
}

impl<T: MemoEq, E: MemoEq> MemoEq for Result<T, E> {
    fn memo_eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Ok(a), Ok(b)) => a.memo_eq(b),
            (Err(a), Err(b)) => a.memo_eq(b),
            _ => false,
        }
    }
}

impl<T: MemoSize, E: MemoSize> MemoSize for Result<T, E> {
    fn memo_size(&self) -> usize {
        let inline = std::mem::size_of::<Self>();
        match self {
            Ok(value) => inline + value.memo_size().saturating_sub(std::mem::size_of::<T>()),
            Err(error) => inline + error.memo_size().saturating_sub(std::mem::size_of::<E>()),
        }
    }
}

macro_rules! tuple {
    ($(($($name:ident $index:tt),+)),* $(,)?) => {$(
        impl<$($name: MemoKey),+> MemoKey for ($($name,)+) {
            type Key = ($($name::Key,)+);
            #[inline]
            fn memo_key(&self) -> Self::Key {
                ($(self.$index.memo_key(),)+)
            }
        }
        impl<$($name: MemoEq),+> MemoEq for ($($name,)+) {
            fn memo_eq(&self, other: &Self) -> bool {
                $(self.$index.memo_eq(&other.$index))&&+
            }
        }
        impl<$($name: MemoSize),+> MemoSize for ($($name,)+) {
            fn memo_size(&self) -> usize {
                let fields = 0 $(+ std::mem::size_of::<$name>())+;
                std::mem::size_of::<Self>() - fields $(+ self.$index.memo_size())+
            }
        }
    )*};
}

tuple!(
    (A 0),
    (A 0, B 1),
    (A 0, B 1, C 2),
    (A 0, B 1, C 2, D 3),
    (A 0, B 1, C 2, D 3, E 4),
    (A 0, B 1, C 2, D 3, E 4, F 5),
    (A 0, B 1, C 2, D 3, E 4, F 5, G 6),
    (A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7),
    (A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8),
    (A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8, J 9),
    (A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8, J 9, K 10),
    (A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8, J 9, K 10, L 11),
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floats_compare_by_bits() {
        assert_ne!(0.0f64.memo_key(), (-0.0f64).memo_key());
        assert_eq!(f64::NAN.memo_key(), f64::NAN.memo_key());
        assert_ne!(f64::NAN.memo_key(), (-f64::NAN).memo_key());
        assert!(!0.0f32.memo_eq(&-0.0));
        assert!(f32::NAN.memo_eq(&f32::NAN));
    }

    #[test]
    fn composite_keys_are_owned() {
        let text = String::from("x");
        let key = (&text.as_str(), &[1.5f64, 2.0][..], &Some(3u8)).memo_key();
        assert_eq!(
            key,
            (
                "x".to_owned(),
                vec![1.5f64.to_bits(), 2.0f64.to_bits()],
                Some(3)
            )
        );
    }

    #[test]
    fn sizes_include_heap_data() {
        let text = String::with_capacity(100);
        assert!(text.memo_size() >= 100);
        assert_eq!(7u64.memo_size(), 8);
        assert!(vec![String::with_capacity(10)].memo_size() >= 10);
        assert!(Arc::new(String::with_capacity(50)).memo_size() >= 50);
    }
}
