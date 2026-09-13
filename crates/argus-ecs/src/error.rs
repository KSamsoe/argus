//! Structured errors in the shape §5 of the design document prescribes:
//! a numeric code, a human sentence, a hint written to a capable colleague,
//! and a `file:line` reference when one is known.

use std::fmt;

/// Error codes used by the ECS core. The 4xxx block is reserved for
/// client-visible protocol errors; the daemon forwards these unchanged.
pub mod codes {
    /// Component name not registered.
    pub const UNKNOWN_COMPONENT: u32 = 4001;
    /// Field name not in the component's schema.
    pub const UNKNOWN_FIELD: u32 = 4002;
    /// Value type does not match the field type.
    pub const TYPE_MISMATCH: u32 = 4003;
    /// Field path is malformed (must be `Component.field`).
    pub const BAD_PATH: u32 = 4004;
    /// Schema is invalid (duplicate field, duplicate component name, bad layout).
    pub const BAD_SCHEMA: u32 = 4005;
    /// Value outside the field's declared range.
    pub const OUT_OF_RANGE: u32 = 4102;
    /// Entity is not alive (never spawned, or despawned).
    pub const ENTITY_NOT_ALIVE: u32 = 4201;
    /// Authored string id already used in this world.
    pub const DUPLICATE_ID: u32 = 4202;
    /// No entity with that string id.
    pub const UNKNOWN_ID: u32 = 4203;
    /// Entity does not have the component.
    pub const MISSING_COMPONENT: u32 = 4204;
    /// Invalid string id (empty, contains whitespace, or starts with `@`).
    pub const BAD_ID: u32 = 4205;
    /// A system touched a component it did not declare (§7).
    pub const ACCESS_VIOLATION: u32 = 4301;
    /// A column is already borrowed in a conflicting way.
    pub const BORROW_CONFLICT: u32 = 4302;
}

/// The one error type this crate returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArgusError {
    /// Numeric code from [`codes`].
    pub code: u32,
    /// One human sentence saying what went wrong, with the offending value.
    pub message: String,
    /// What to do instead. May be empty when nothing useful can be said.
    pub hint: String,
    /// `file:line` of the schema or declaration involved, when known.
    pub r#ref: Option<String>,
}

impl ArgusError {
    /// Build an error with a code and message.
    pub fn new(code: u32, message: impl Into<String>) -> Self {
        ArgusError {
            code,
            message: message.into(),
            hint: String::new(),
            r#ref: None,
        }
    }
    /// Attach a hint.
    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = hint.into();
        self
    }
    /// Attach a `file:line` reference.
    pub fn at(mut self, r#ref: Option<String>) -> Self {
        self.r#ref = r#ref;
        self
    }
}

impl fmt::Display for ArgusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)?;
        if !self.hint.is_empty() {
            write!(f, " Hint: {}", self.hint)?;
        }
        if let Some(r) = &self.r#ref {
            write!(f, " ({r})")?;
        }
        Ok(())
    }
}

impl std::error::Error for ArgusError {}

/// Result alias used throughout the crate.
pub type Result<T> = std::result::Result<T, ArgusError>;
