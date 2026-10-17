//! Shared error types of the domain layer.

/// A string did not name any variant of a closed enum (SPEC §5).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown {kind} {value:?}")]
pub struct ParseEnumError {
    /// Name of the enum being parsed, e.g. `"sort order"`.
    pub kind: &'static str,
    /// The rejected input.
    pub value: String,
}

impl ParseEnumError {
    pub(crate) fn new(kind: &'static str, value: &str) -> Self {
        Self {
            kind,
            value: value.to_owned(),
        }
    }
}

/// Implements `as_str`, `Display`, `FromStr` and `ALL` for a fieldless enum whose variants map
/// to stable lowercase strings (database values, settings, CLI).
macro_rules! string_enum {
    ($(#[$meta:meta])* $name:ident, $kind:literal, { $($variant:ident => $text:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
        pub enum $name {
            $(
                #[doc = concat!("`", $text, "`")]
                #[serde(rename = $text)]
                $variant,
            )+
        }

        impl $name {
            /// Every variant, in declaration order.
            pub const ALL: &'static [$name] = &[$($name::$variant),+];

            /// The stable string form used in the database, settings and the CLI.
            pub fn as_str(self) -> &'static str {
                match self { $($name::$variant => $text),+ }
            }
        }

        impl ::std::fmt::Display for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl ::std::str::FromStr for $name {
            type Err = $crate::ParseEnumError;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                match s {
                    $($text => Ok($name::$variant),)+
                    other => Err($crate::ParseEnumError::new($kind, other)),
                }
            }
        }
    };
}

pub(crate) use string_enum;
