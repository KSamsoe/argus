//! Field types and dynamic values (§3 "every field has a type, a default, an
//! optional range, and a doc string"). Every field type is plain old data of
//! fixed size, which is what makes a column a memcpy-able byte array and a
//! snapshot a memcpy (§2, §6). Strings are interned handles; entity references
//! are the 8-byte [`Entity`] itself. A list (issue 22) is one value, as a
//! string is: its elements are written as one string in the interner, and
//! the row holds that string's handle.

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
    /// A list of scalars (issue 22), held as an interned handle (4 bytes),
    /// so a list is one value: changing one is writing a new one.
    List(ListOf),
}

/// What a list field holds: any scalar but an entity (a dead entity in a
/// list would have to be found and taken out at every frame's end).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ListOf {
    /// `bool`.
    Bool,
    /// `u8`.
    U8,
    /// `u16`.
    U16,
    /// `u32`.
    U32,
    /// `u64`.
    U64,
    /// `i8`.
    I8,
    /// `i16`.
    I16,
    /// `i32`.
    I32,
    /// `i64`.
    I64,
    /// `f32`.
    F32,
    /// `f64`.
    F64,
    /// `vec2`.
    Vec2,
    /// `str`.
    Str,
    /// `ref`: every element must name a row that exists.
    Ref,
}

/// The most elements a list field holds.
pub const MAX_LIST: usize = 1024;

impl ListOf {
    /// The element's type.
    pub const fn ty(self) -> FieldType {
        match self {
            ListOf::Bool => FieldType::Bool,
            ListOf::U8 => FieldType::U8,
            ListOf::U16 => FieldType::U16,
            ListOf::U32 => FieldType::U32,
            ListOf::U64 => FieldType::U64,
            ListOf::I8 => FieldType::I8,
            ListOf::I16 => FieldType::I16,
            ListOf::I32 => FieldType::I32,
            ListOf::I64 => FieldType::I64,
            ListOf::F32 => FieldType::F32,
            ListOf::F64 => FieldType::F64,
            ListOf::Vec2 => FieldType::Vec2,
            ListOf::Str => FieldType::Str,
            ListOf::Ref => FieldType::Ref,
        }
    }

    /// A list of `t`, if a list can hold it.
    pub const fn of(t: FieldType) -> Option<ListOf> {
        Some(match t {
            FieldType::Bool => ListOf::Bool,
            FieldType::U8 => ListOf::U8,
            FieldType::U16 => ListOf::U16,
            FieldType::U32 => ListOf::U32,
            FieldType::U64 => ListOf::U64,
            FieldType::I8 => ListOf::I8,
            FieldType::I16 => ListOf::I16,
            FieldType::I32 => ListOf::I32,
            FieldType::I64 => ListOf::I64,
            FieldType::F32 => ListOf::F32,
            FieldType::F64 => ListOf::F64,
            FieldType::Vec2 => ListOf::Vec2,
            FieldType::Str => ListOf::Str,
            FieldType::Ref => ListOf::Ref,
            FieldType::Entity | FieldType::List(_) => return None,
        })
    }
}

/// A list's elements (coerced to `of`) as the one string its handle names:
/// each element in turn, a string or a ref as its length in bytes, `:`,
/// and its bytes; anything else as text and `;`, a float as its bits in
/// hex so it reads back exactly.
fn encode_list(of: ListOf, items: &[Value]) -> Result<String> {
    use std::fmt::Write as _;
    if items.len() > MAX_LIST {
        return Err(too_long(items.len()));
    }
    let mut out = String::new();
    for v in items {
        let _ = match (of, v) {
            (ListOf::Bool, Value::Bool(b)) => write!(out, "{};", *b as u8),
            (ListOf::U8 | ListOf::U16 | ListOf::U32 | ListOf::U64, Value::UInt(u)) => {
                write!(out, "{u};")
            }
            (ListOf::I8 | ListOf::I16 | ListOf::I32 | ListOf::I64, Value::Int(i)) => {
                write!(out, "{i};")
            }
            (ListOf::F32, Value::Float(f)) => write!(out, "{:x};", (*f as f32).to_bits()),
            (ListOf::F64, Value::Float(f)) => write!(out, "{:x};", f.to_bits()),
            (ListOf::Vec2, Value::Vec2(p)) => {
                write!(out, "{:x},{:x};", p.x.to_bits(), p.y.to_bits())
            }
            (ListOf::Str, Value::Str(t)) | (ListOf::Ref, Value::Ref(t)) => {
                write!(out, "{}:{t}", t.len())
            }
            _ => {
                return Err(ArgusError::new(
                    codes::TYPE_MISMATCH,
                    format!(
                        "a list of {} holds no {} ({v})",
                        of.ty().name(),
                        v.type_name()
                    ),
                ))
            }
        };
    }
    Ok(out)
}

/// The elements [`encode_list`] wrote.
pub(crate) fn decode_list(of: ListOf, s: &str) -> Vec<Value> {
    let mut out = Vec::new();
    let mut rest = s;
    while !rest.is_empty() {
        if matches!(of, ListOf::Str | ListOf::Ref) {
            let Some((n, after)) = rest.split_once(':') else {
                break;
            };
            let n: usize = n.parse().unwrap_or(0);
            let Some(t) = after.get(..n) else {
                break;
            };
            out.push(if of == ListOf::Ref {
                Value::Ref(t.to_string())
            } else {
                Value::Str(t.to_string())
            });
            rest = &after[n..];
            continue;
        }
        let Some((t, after)) = rest.split_once(';') else {
            break;
        };
        rest = after;
        let bits32 = |h: &str| f32::from_bits(u32::from_str_radix(h, 16).unwrap_or(0));
        out.push(match of {
            ListOf::Bool => Value::Bool(t == "1"),
            ListOf::U8 | ListOf::U16 | ListOf::U32 | ListOf::U64 => {
                Value::UInt(t.parse().unwrap_or(0))
            }
            ListOf::I8 | ListOf::I16 | ListOf::I32 | ListOf::I64 => {
                Value::Int(t.parse().unwrap_or(0))
            }
            ListOf::F32 => Value::Float(bits32(t) as f64),
            ListOf::F64 => Value::Float(f64::from_bits(u64::from_str_radix(t, 16).unwrap_or(0))),
            ListOf::Vec2 => {
                let (x, y) = t.split_once(',').unwrap_or(("0", "0"));
                Value::Vec2(Vec2::new(bits32(x), bits32(y)))
            }
            ListOf::Str | ListOf::Ref => unreachable!(),
        });
    }
    out
}

fn too_long(n: usize) -> ArgusError {
    ArgusError::new(
        codes::OUT_OF_RANGE,
        format!("a list holds at most {MAX_LIST} elements; this one has {n}"),
    )
}

impl FieldType {
    /// Size in bytes.
    pub const fn size(self) -> usize {
        match self {
            FieldType::Bool | FieldType::U8 | FieldType::I8 => 1,
            FieldType::U16 | FieldType::I16 => 2,
            FieldType::U32
            | FieldType::I32
            | FieldType::F32
            | FieldType::Str
            | FieldType::Ref
            | FieldType::List(_) => 4,
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
            FieldType::List(of) => match of {
                ListOf::Bool => "list of bool",
                ListOf::U8 => "list of u8",
                ListOf::U16 => "list of u16",
                ListOf::U32 => "list of u32",
                ListOf::U64 => "list of u64",
                ListOf::I8 => "list of i8",
                ListOf::I16 => "list of i16",
                ListOf::I32 => "list of i32",
                ListOf::I64 => "list of i64",
                ListOf::F32 => "list of f32",
                ListOf::F64 => "list of f64",
                ListOf::Vec2 => "list of vec2",
                ListOf::Str => "list of str",
                ListOf::Ref => "list of ref",
            },
        }
    }
    /// Parse a schema type name (`list of ref` for a list).
    pub fn parse(s: &str) -> Option<FieldType> {
        if let Some(e) = s.strip_prefix("list of ") {
            return ListOf::of(FieldType::parse(e)?).map(FieldType::List);
        }
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
            FieldType::Bool
                | FieldType::Str
                | FieldType::Entity
                | FieldType::Ref
                | FieldType::Vec2
                | FieldType::List(_)
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
            FieldType::List(_) => Value::List(Vec::new()),
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
            FieldType::List(of) => Value::List(decode_list(of, &strings.get(rd!(StrId)))),
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
            (FieldType::List(of), Value::List(items)) => {
                wr!(StrId, strings.try_intern(&encode_list(of, items)?)?)
            }
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
    /// A list field's elements (issue 22).
    List(Vec<Value>),
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
            Value::List(_) => "list",
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
            (T::List(of), Value::List(items)) => {
                if items.len() > MAX_LIST {
                    return Err(too_long(items.len()));
                }
                let items = items
                    .into_iter()
                    .enumerate()
                    .map(|(i, v)| {
                        v.coerce(of.ty()).map_err(|e| {
                            ArgusError::new(e.code, format!("element {}: {}", i + 1, e.message))
                        })
                    })
                    .collect::<Result<Vec<Value>>>()?;
                Value::List(items)
            }
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
            Value::List(items) => {
                f.write_str("[")?;
                for (i, v) in items.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{v}")?;
                }
                f.write_str("]")
            }
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
