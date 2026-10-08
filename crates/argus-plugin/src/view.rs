//! Components as a plugin reads them: a `#[repr(C)]` struct per component,
//! field for field as the game declares it ([`component!`]), checked
//! against the game's layout before any system runs, and fetched a column
//! at a time.

use crate::abi;
use crate::ctx::{Ctx, Entity, Ref, Result, StrId, Vec2};

/// A Rust type a field is read as.
pub trait FieldTy: Copy + 'static {
    /// Its [`abi::ty`] number.
    const TY: u32;
}

macro_rules! field_ty {
    ($($t:ty => $n:ident),*) => {$(
        impl FieldTy for $t {
            const TY: u32 = abi::ty::$n;
        }
    )*};
}
field_ty!(bool => BOOL, u8 => U8, u16 => U16, u32 => U32, u64 => U64, i8 => I8, i16 => I16,
    i32 => I32, i64 => I64, f32 => F32, f64 => F64, Vec2 => VEC2, StrId => STR, Ref => REF,
    Entity => ENTITY);

/// A component's row as the plugin lays it out. Implemented by
/// [`component!`]; the engine refuses a view whose layout differs from the
/// game's component, at load and at every fetch.
///
/// # Safety
/// `FIELDS` must be the struct's fields in memory order with their true
/// offsets, and the struct `#[repr(C)]`.
pub unsafe trait View: Copy + 'static {
    /// The component's name in the game.
    const NAME: &'static str;
    /// `(name, type, offset)` per field.
    const FIELDS: &'static [(&'static str, u32, usize)];
    /// Where the plugin declares it.
    const SOURCE: &'static str;
    /// The layout's fingerprint ([`abi::fingerprint`]).
    const FINGERPRINT: u64 = fingerprint(
        core::mem::size_of::<Self>(),
        core::mem::align_of::<Self>(),
        Self::FIELDS,
    );
}

/// [`abi::fingerprint`], at compile time.
pub const fn fingerprint(size: usize, align: usize, fields: &[(&str, u32, usize)]) -> u64 {
    const fn put(mut h: u64, bytes: &[u8]) -> u64 {
        let mut i = 0;
        while i < bytes.len() {
            h ^= bytes[i] as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            i += 1;
        }
        h
    }
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    h = put(h, &(size as u64).to_le_bytes());
    h = put(h, &(align as u64).to_le_bytes());
    let mut i = 0;
    while i < fields.len() {
        let (name, ty, offset) = fields[i];
        h = put(h, name.as_bytes());
        h = put(h, &[0]);
        h = put(h, &ty.to_le_bytes());
        h = put(h, &(offset as u64).to_le_bytes());
        i += 1;
    }
    h
}

/// Declare a component as the game declares it, field for field in the
/// same order and types (`f32`, `i32`, `u8`, `bool`, [`Vec2`], [`StrId`]
/// for a string, [`Ref`], [`Entity`], ...). The struct is `#[repr(C)]`,
/// which is the engine's layout too; a mistake is refused at load with
/// both layouts in the message.
///
/// ```
/// use argus_plugin::component;
///
/// component! {
///     /// components/swarm.luau's Enemy.
///     pub struct Enemy { pub speed: f32, pub damage: i32, pub wave: u32 }
/// }
/// ```
#[macro_export]
macro_rules! component {
    ($(#[$m:meta])* $vis:vis struct $name:ident { $($(#[$fm:meta])* $fvis:vis $field:ident : $ty:ty),* $(,)? }) => {
        $(#[$m])*
        #[repr(C)]
        #[derive(Clone, Copy, Debug, Default, PartialEq)]
        $vis struct $name { $($(#[$fm])* $fvis $field: $ty),* }

        // SAFETY: repr(C), fields listed in order with offset_of.
        unsafe impl $crate::View for $name {
            const NAME: &'static str = stringify!($name);
            const FIELDS: &'static [(&'static str, u32, usize)] = &[
                $((stringify!($field), <$ty as $crate::FieldTy>::TY, ::core::mem::offset_of!($name, $field))),*
            ];
            const SOURCE: &'static str = concat!(file!(), ":", line!());
        }
    };
}

/// One fetched column: `&T` reads it, `&mut T` writes it.
///
/// # Safety
/// `get` must produce the item from a column of `T`'s rows.
pub unsafe trait One {
    /// The component.
    type T: View;
    /// Whether it is written.
    const WRITE: bool;
    /// The item.
    type Item<'c>;
    /// Row `row` of the column at `ptr`.
    ///
    /// # Safety
    /// `ptr` is a column of `T` with more than `row` rows, borrowed as
    /// `WRITE` says.
    unsafe fn get<'c>(ptr: *mut u8, row: usize) -> Self::Item<'c>;
}

// SAFETY: a shared view of a T row.
unsafe impl<T: View> One for &T {
    type T = T;
    const WRITE: bool = false;
    type Item<'c> = &'c T;
    unsafe fn get<'c>(ptr: *mut u8, row: usize) -> &'c T {
        &*(ptr as *const T).add(row)
    }
}

// SAFETY: an exclusive view of a T row; the column is borrowed exclusively.
unsafe impl<T: View> One for &mut T {
    type T = T;
    const WRITE: bool = true;
    type Item<'c> = &'c mut T;
    unsafe fn get<'c>(ptr: *mut u8, row: usize) -> &'c mut T {
        &mut *(ptr as *mut T).add(row)
    }
}

/// What [`Ctx::each`] fetches: a tuple of `&T` and `&mut T`, one to six.
///
/// # Safety
/// `columns` and `item` must agree on the order of columns.
pub unsafe trait Fetch {
    /// One row's items.
    type Item<'c>;
    /// Columns fetched.
    const LEN: usize;
    /// The columns, in order.
    fn columns(ctx: &Ctx<'_>, out: &mut Vec<abi::Column>) -> Result;
    /// Row `row`'s items.
    ///
    /// # Safety
    /// `cols` holds `LEN` columns as `columns` asked for, each with more
    /// than `row` rows.
    unsafe fn item<'c>(cols: *const *mut u8, row: usize) -> Self::Item<'c>;
}

macro_rules! fetch_tuple {
    ($n:expr; $($t:ident $i:tt),*) => {
        // SAFETY: columns pushed and read in the same order.
        unsafe impl<$($t: One),*> Fetch for ($($t,)*) {
            type Item<'c> = ($($t::Item<'c>,)*);
            const LEN: usize = $n;
            fn columns(ctx: &Ctx<'_>, out: &mut Vec<abi::Column>) -> Result {
                $(out.push(ctx.column::<$t::T>($t::WRITE)?);)*
                Ok(())
            }
            unsafe fn item<'c>(cols: *const *mut u8, row: usize) -> Self::Item<'c> {
                ($($t::get(*cols.add($i), row),)*)
            }
        }
    };
}
fetch_tuple!(1; A 0);
fetch_tuple!(2; A 0, B 1);
fetch_tuple!(3; A 0, B 1, C 2);
fetch_tuple!(4; A 0, B 1, C 2, D 3);
fetch_tuple!(5; A 0, B 1, C 2, D 3, E 4);
fetch_tuple!(6; A 0, B 1, C 2, D 3, E 4, F 5);

/// The engine's own components, as plugins read them.
pub mod engine {
    use crate::ctx::Vec2;

    crate::component! {
        /// Position, rotation (radians), and scale; local to the Parent
        /// when there is one.
        pub struct Transform {
            /// Position in world units.
            pub pos: Vec2,
            /// Rotation in radians, counter-clockwise.
            pub rot: f32,
            /// Scale per axis; 1 is unscaled.
            pub scale: Vec2,
        }
    }

    crate::component! {
        /// World-space transform, written by the hierarchy stage.
        pub struct GlobalTransform {
            /// World position.
            pub pos: Vec2,
            /// World rotation in radians.
            pub rot: f32,
            /// World scale per axis.
            pub scale: Vec2,
        }
    }
}
