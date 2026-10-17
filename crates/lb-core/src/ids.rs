//! Strongly typed database identifiers (SPEC §5.1).

macro_rules! id_type {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord,
            serde::Serialize, serde::Deserialize,
        )]
        #[repr(transparent)]
        #[serde(transparent)]
        pub struct $name(i64);

        impl $name {
            /// Wraps a raw database row id.
            pub const fn new(raw: i64) -> Self {
                Self(raw)
            }

            /// The raw database row id.
            pub const fn get(self) -> i64 {
                self.0
            }
        }

        impl ::std::fmt::Display for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                ::std::fmt::Display::fmt(&self.0, f)
            }
        }
    };
}

id_type!(
    /// Identifies a photo or video row.
    PhotoId
);
id_type!(
    /// Identifies an indexed folder row.
    FolderId
);
id_type!(
    /// Identifies an album.
    AlbumId
);
id_type!(
    /// Identifies a tag.
    TagId
);
id_type!(
    /// Identifies a person (named face cluster).
    PersonId
);
id_type!(
    /// Identifies a detected face.
    FaceId
);
id_type!(
    /// Identifies a background job.
    JobId
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_serde_transparent() {
        macro_rules! check {
            ($($t:ident),+) => {$(
                let id = $t::new(42);
                assert_eq!(serde_json::to_string(&id).unwrap(), "42", stringify!($t));
                assert_eq!(serde_json::from_str::<$t>("42").unwrap(), id);
                assert_eq!(serde_json::from_str::<$t>("-7").unwrap().get(), -7);
                assert!(serde_json::from_str::<$t>("\"42\"").is_err(), "ids are numbers, not strings");
                assert!(serde_json::from_str::<$t>("{\"0\":42}").is_err());
                assert_eq!(serde_json::to_string(&vec![id, $t::new(1)]).unwrap(), "[42,1]");
                assert_eq!(std::mem::size_of::<$t>(), std::mem::size_of::<i64>());
                assert_eq!(id.to_string(), "42");
            )+};
        }
        check!(PhotoId, FolderId, AlbumId, TagId, PersonId, FaceId, JobId);
    }

    #[test]
    fn ids_extremes_and_ordering() {
        assert_eq!(PhotoId::new(i64::MAX).get(), i64::MAX);
        assert_eq!(PhotoId::new(i64::MIN).get(), i64::MIN);
        assert!(PhotoId::new(1) < PhotoId::new(2));
        let json = serde_json::to_string(&PhotoId::new(i64::MAX)).unwrap();
        assert_eq!(
            serde_json::from_str::<PhotoId>(&json).unwrap(),
            PhotoId::new(i64::MAX)
        );
    }
}
