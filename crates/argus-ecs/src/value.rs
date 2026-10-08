//! Field types and dynamic values (§3 "every field has a type, a default, an
//! optional range, and a doc string"). Every field type is plain old data of
//! fixed size, which is what makes a column a memcpy-able byte array and a
//! snapshot a memcpy (§2, §6). Strings are interned handles; entity references
//! are the 8-byte [`Entity`] itself.

use crate::entity::Entity;
use crate::error::{codes, ArgusError, Result};
use crate::intern::{Interner, StrId};
use std::fmt;

/// A 2D vector, the engine's only compound field type in phase 0.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
pub struct Vec2 {
    /// X.
    pub x: f32,
    /// Y.
    pub y: f32,
}

impl Vec2 {
    /// Zero vector.
    pub const ZERO: Vec2 = Vec2 { x: 0.0, y: 0.0 };
    /// (1, 1).
    pub const ONE: Vec2 = Vec2 { x: 1.0, y: 1.0 };
    /// Construct.
    pub const fn new(x: f32, y: f32) -> Vec2 {
        Vec2 { x, y }
    }
}

/// The type of a component field. The set is closed on purpose: everything
/// here has a fixed size and alignment, so a column layout is a C struct
/// layout and a Rust `#[repr(C)]` component and a Luau-declared schema with
/// the same fields have byte-identical rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FieldType {
    /// 1 byte, 0 or 1.
    Bool,
    /// Unsigned 8-bit.
    U8,
    /// Unsigned 16-bit.
    U16,
    /// Unsigned 32-bit.
    U32,
    /// Unsigned 64-bit.
    U64,
    /// Signed 8-bit.
    I8,
    /// Signed 16-bit.
    I16,
    /// Signed 32-bit.
    I32,
    /// Signed 64-bit.
    I64,
    /// 32-bit float. The sim's default real type (§6).
    F32,
    /// 64-bit float.
    F64,
    /// Two f32s.
    Vec2,
    /// Interned string handle (4 bytes).
    Str,
    /// Entity reference (8 bytes). Nulled at frame end if the target dies.
    Entity,
    /// Reference to a def, asset, or prefab by path, e.g. `item:key.brass` (§4).
    /// Stored like `Str`; typed separately so `describe` can say it is a reference.
    Ref,
}

impl FieldType {
    /// Size in bytes.
    pub const fn size(self) -> usize {
        match self {
            FieldType::Bool | FieldType::U8 | FieldType::I8 => 1,
            FieldType::U16 | FieldType::I16 => 2,
            FieldType::U32 | FieldType::I32 | FieldType::F32 | FieldType::Str | FieldType::Ref => 4,
            FieldType::U64
            | FieldType::I64
            | FieldType::F64
            | FieldType::Entity
            | FieldType::Vec2 => 8,
        }
    }
    /// Alignment in bytes (natural alignment; `Vec2` aligns to 4).
    pub const fn align(self) -> usize {
        match self {
            FieldType::Vec2 => 4,
            _ => self.size(),
        }
    }
    /// The name used in schemas and `describe` output.
    pub const fn name(self) -> &'static str {
        match self {
            FieldType::Bool => "bool",
            FieldType::U8 => "u8",
            FieldType::U16 => "u16",
            FieldType::U32 => "u32",
            FieldType::U64 => "u64",
            FieldType::I8 => "i8",
            FieldType::I16 => "i16",
            FieldType::I32 => "i32",
            FieldType::I64 => "i64",
            FieldType::F32 => "f32",
            FieldType::F64 => "f64",
            FieldType::Vec2 => "vec2",
            FieldType::Str => "str",
            FieldType::Entity => "entity",
            FieldType::Ref => "ref",
        }
    }
    /// Parse a schema type name.
    pub fn parse(s: &str) -> Option<FieldType> {
        Some(match s {
            "bool" => FieldType::Bool,
            "u8" => FieldType::U8,
            "u16" => FieldType::U16,
            "u32" => FieldType::U32,
            "u64" => FieldType::U64,
            "i8" => FieldType::I8,
            "i16" => FieldType::I16,
            "i32" => FieldType::I32,
            "i64" => FieldType::I64,
            "f32" => FieldType::F32,
            "f64" => FieldType::F64,
            "vec2" => FieldType::Vec2,
            "str" | "string" => FieldType::Str,
            "entity" => FieldType::Entity,
            "ref" => FieldType::Ref,
            _ => return None,
        })
    }
    /// True for numeric types that a range can apply to.
    pub const fn is_numeric(self) -> bool {
        !matches!(
            self,
            FieldType::Bool | FieldType::Str | FieldType::Entity | FieldType::Ref | FieldType::Vec2
        )
    }
    /// The zero value of this type.
    pub fn zero(self) -> Value {
        match self {
            FieldType::Bool => Value::Bool(false),
            FieldType::U8 | FieldType::U16 | FieldType::U32 | FieldType::U64 => Value::UInt(0),
            FieldType::I8 | FieldType::I16 | FieldType::I32 | FieldType::I64 => Value::Int(0),
            FieldType::F32 | FieldType::F64 => Value::Float(0.0),
            FieldType::Vec2 => Value::Vec2(Vec2::ZERO),
            FieldType::Str => Value::Str(String::new()),
            FieldType::Entity => Value::Entity(Entity::NULL),
            FieldType::Ref => Value::Ref(String::new()),
        }
    }

    /// Read a value of this type from `ptr`, which must point at `size()`
    /// initialized bytes with the type's alignment.
    ///
    /// # Safety
    /// `ptr` must be valid for reads of `self.size()` bytes.
    pub unsafe fn read(self, ptr: *const u8, strings: &Interner) -> Value {
        macro_rules! rd {
            ($t:ty) => {
                core::ptr::read_unaligned(ptr as *const $t)
            };
        }
        match self {
            FieldType::Bool => Value::Bool(rd!(u8) != 0),
            FieldType::U8 => Value::UInt(rd!(u8) as u64),
            FieldType::U16 => Value::UInt(rd!(u16) as u64),
            FieldType::U32 => Value::UInt(rd!(u32) as u64),
            FieldType::U64 => Value::UInt(rd!(u64)),
            FieldType::I8 => Value::Int(rd!(i8) as i64),
            FieldType::I16 => Value::Int(rd!(i16) as i64),
            FieldType::I32 => Value::Int(rd!(i32) as i64),
            FieldType::I64 => Value::Int(rd!(i64)),
            FieldType::F32 => Value::Float(rd!(f32) as f64),
            FieldType::F64 => Value::Float(rd!(f64)),
            FieldType::Vec2 => Value::Vec2(rd!(Vec2)),
            FieldType::Str => Value::Str(strings.get(rd!(StrId)).to_string()),
            FieldType::Ref => Value::Ref(strings.get(rd!(StrId)).to_string()),
            FieldType::Entity => Value::Entity(rd!(Entity)),
        }
    }

    /// Write `value` (already coerced to this type) at `ptr`.
    ///
    /// # Safety
    /// `ptr` must be valid for writes of `self.size()` bytes.
    pub unsafe fn write(self, ptr: *mut u8, value: &Value, strings: &Interner) -> Result<()> {
        macro_rules! wr {
            ($t:ty, $v:expr) => {
                core::ptr::write_unaligned(ptr as *mut $t, $v)
            };
        }
        let mismatch = || {
            ArgusError::new(
                codes::TYPE_MISMATCH,
                format!(
                    "expected a {} value; got {}",
                    self.name(),
                    value.type_name()
                ),
            )
        };
        match (self, value) {
            (FieldType::Bool, Value::Bool(b)) => wr!(u8, *b as u8),
            (FieldType::U8, Value::UInt(v)) => wr!(u8, *v as u8),
            (FieldType::U16, Value::UInt(v)) => wr!(u16, *v as u16),
            (FieldType::U32, Value::UInt(v)) => wr!(u32, *v as u32),
            (FieldType::U64, Value::UInt(v)) => wr!(u64, *v),
            (FieldType::I8, Value::Int(v)) => wr!(i8, *v as i8),
            (FieldType::I16, Value::Int(v)) => wr!(i16, *v as i16),
            (FieldType::I32, Value::Int(v)) => wr!(i32, *v as i32),
            (FieldType::I64, Value::Int(v)) => wr!(i64, *v),
            (FieldType::F32, Value::Float(v)) => wr!(f32, *v as f32),
            (FieldType::F64, Value::Float(v)) => wr!(f64, *v),
            (FieldType::Vec2, Value::Vec2(v)) => wr!(Vec2, *v),
            (FieldType::Str, Value::Str(s)) | (FieldType::Ref, Value::Ref(s)) => {
                wr!(StrId, strings.try_intern(s)?)
            }
            (FieldType::Entity, Value::Entity(e)) => wr!(Entity, *e),
            _ => return Err(mismatch()),
        }
        Ok(())
    }
}

impl fmt::Display for FieldType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// A dynamically typed field value: what the protocol, the Luau binding and
/// the scene loader hand to `set_field`, and what `get_field` returns.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// Boolean.
    Bool(bool),
    /// Signed integer (all `i*` types).
    Int(i64),
    /// Unsigned integer (all `u*` types).
    UInt(u64),
    /// Float (`f32` and `f64`).
    Float(f64),
    /// Two floats.
    Vec2(Vec2),
    /// String.
    Str(String),
    /// Entity reference.
    Entity(Entity),
    /// Def/asset/prefab reference by path.
    Ref(String),
}

impl Value {
    /// Human name of the value's kind, for error messages.
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Bool(_) => "bool",
            Value::Int(_) => "int",
            Value::UInt(_) => "uint",
            Value::Float(_) => "float",
            Value::Vec2(_) => "vec2",
            Value::Str(_) => "str",
            Value::Entity(_) => "entity",
            Value::Ref(_) => "ref",
        }
    }

    /// Numeric view, used for range checks.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Int(v) => Some(*v as f64),
            Value::UInt(v) => Some(*v as f64),
            Value::Float(v) => Some(*v),
            _ => None,
        }
    }

    /// Coerce to the representation `ty` stores. Integers widen to floats,
    /// non-negative ints become uints and vice versa, a `Str` becomes a `Ref`
    /// when the field is a reference. Anything else is a type mismatch.
    pub fn coerce(self, ty: FieldType) -> Result<Value> {
        use FieldType as T;
        let v = match (ty, self) {
            (T::Bool, v @ Value::Bool(_)) => v,
            (T::U8 | T::U16 | T::U32 | T::U64, v @ Value::UInt(_)) => v,
            (T::U8 | T::U16 | T::U32 | T::U64, Value::Int(i)) if i >= 0 => Value::UInt(i as u64),
            (T::I8 | T::I16 | T::I32 | T::I64, v @ Value::Int(_)) => v,
            (T::I8 | T::I16 | T::I32 | T::I64, Value::UInt(u)) if u <= i64::MAX as u64 => {
                Value::Int(u as i64)
            }
            (T::F32 | T::F64, v @ Value::Float(_)) => v,
            (T::F32 | T::F64, Value::Int(i)) => Value::Float(i as f64),
            (T::F32 | T::F64, Value::UInt(u)) => Value::Float(u as f64),
            (T::Vec2, v @ Value::Vec2(_)) => v,
            (T::Str, v @ Value::Str(_)) => v,
            (T::Ref, v @ Value::Ref(_)) => v,
            (T::Ref, Value::Str(s)) => Value::Ref(s),
            (T::Entity, v @ Value::Entity(_)) => v,
            (ty, other) => {
                return Err(ArgusError::new(
                    codes::TYPE_MISMATCH,
                    format!(
                        "expected a {} value; got {} {}",
                        ty.name(),
                        other.type_name(),
                        other
                    ),
                ))
            }
        };
        // Integer width checks: a u8 field must not silently truncate 300.
        let width_err = |v: &Value| {
            ArgusError::new(
                codes::OUT_OF_RANGE,
                format!("{v} does not fit in {}", ty.name()),
            )
        };
        match (&v, ty) {
            (Value::UInt(u), T::U8) if *u > u8::MAX as u64 => return Err(width_err(&v)),
            (Value::UInt(u), T::U16) if *u > u16::MAX as u64 => return Err(width_err(&v)),
            (Value::UInt(u), T::U32) if *u > u32::MAX as u64 => return Err(width_err(&v)),
            (Value::Int(i), T::I8) if *i < i8::MIN as i64 || *i > i8::MAX as i64 => {
                return Err(width_err(&v))
            }
            (Value::Int(i), T::I16) if *i < i16::MIN as i64 || *i > i16::MAX as i64 => {
                return Err(width_err(&v))
            }
            (Value::Int(i), T::I32) if *i < i32::MIN as i64 || *i > i32::MAX as i64 => {
                return Err(width_err(&v))
            }
            _ => {}
        }
        Ok(v)
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Bool(b) => write!(f, "{b}"),
            Value::Int(v) => write!(f, "{v}"),
            Value::UInt(v) => write!(f, "{v}"),
            Value::Float(v) => write!(f, "{v}"),
            Value::Vec2(v) => write!(f, "[{}, {}]", v.x, v.y),
            Value::Str(s) => write!(f, "{s:?}"),
            Value::Entity(e) => write!(f, "{e}"),
            Value::Ref(s) => write!(f, "{s}"),
        }
    }
}

impl From<bool> for Value {
    fn from(v: bool) -> Self {
        Value::Bool(v)
    }
}
impl From<i64> for Value {
    fn from(v: i64) -> Self {
        Value::Int(v)
    }
}
impl From<i32> for Value {
    fn from(v: i32) -> Self {
        Value::Int(v as i64)
    }
}
impl From<u64> for Value {
    fn from(v: u64) -> Self {
        Value::UInt(v)
    }
}
impl From<u32> for Value {
    fn from(v: u32) -> Self {
        Value::UInt(v as u64)
    }
}
impl From<f64> for Value {
    fn from(v: f64) -> Self {
        Value::Float(v)
    }
}
impl From<f32> for Value {
    fn from(v: f32) -> Self {
        Value::Float(v as f64)
    }
}
impl From<Vec2> for Value {
    fn from(v: Vec2) -> Self {
        Value::Vec2(v)
    }
}
impl From<&str> for Value {
    fn from(v: &str) -> Self {
        Value::Str(v.to_string())
    }
}
impl From<String> for Value {
    fn from(v: String) -> Self {
        Value::Str(v)
    }
}
impl From<Entity> for Value {
    fn from(v: Entity) -> Self {
        Value::Entity(v)
    }
}
