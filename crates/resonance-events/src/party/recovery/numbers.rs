//! Saturate edited integer amounts before narrowing to their storage types.
//! References deliberately use ordinary serde decoding, never saturation.
use serde::{Deserialize, Deserializer};
use std::collections::BTreeMap;

pub(in super::super) trait Integer: Sized {
    fn clamp(value: i128) -> Self;
}
macro_rules! integers {
    ($($ty:ty),*) => {$(
        impl Integer for $ty {
            fn clamp(value: i128) -> Self {
                value.clamp(<$ty>::MIN as i128, <$ty>::MAX as i128) as Self
            }
        }
    )*};
}
integers!(u8, u16, u32, i8, i32);

pub(in super::super) struct Number<T>(T);
impl<'de, T: Integer> Deserialize<'de> for Number<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        i128::deserialize(deserializer).map(|value| Number(T::clamp(value)))
    }
}

pub(in super::super) fn number<'de, D: Deserializer<'de>, T: Integer>(
    deserializer: D,
) -> Result<T, D::Error> {
    Number::deserialize(deserializer).map(|value| value.0)
}

pub(in super::super) fn array<'de, D: Deserializer<'de>, T: Integer, const N: usize>(
    deserializer: D,
) -> Result<[T; N], D::Error>
where
    [Number<T>; N]: Deserialize<'de>,
{
    <[Number<T>; N]>::deserialize(deserializer).map(|values| values.map(|value| value.0))
}

pub(in super::super) fn counts<'de, D: Deserializer<'de>, T: Integer>(
    deserializer: D,
) -> Result<BTreeMap<u16, T>, D::Error> {
    BTreeMap::<u16, Number<T>>::deserialize(deserializer).map(|values| {
        values
            .into_iter()
            .map(|(id, value)| (id, value.0))
            .collect()
    })
}
