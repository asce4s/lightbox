//! Grid ordering, grouping and selection (SPEC §5.3).

use crate::{PhotoId, error::string_enum};
use indexmap::IndexSet;

/// A bound SQL parameter produced by the search planner and executed by the catalog.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum SqlValue {
    /// An integer.
    Int(i64),
    /// A floating-point number.
    Real(f64),
    /// Text.
    Text(String),
}

string_enum!(
    /// Order of photos in the grid.
    SortOrder, "sort order", {
        DateTakenAsc => "date_taken_asc",
        DateTakenDesc => "date_taken_desc",
        FileNameAsc => "file_name_asc",
        FileSizeDesc => "file_size_desc",
        ModifiedDesc => "modified_desc",
    }
);

string_enum!(
    /// How the grid groups photos under headers.
    GroupBy, "group by", {
        Folder => "folder",
        Month => "month",
        Year => "year",
        None => "none",
    }
);

/// An ordered multi-selection with the anchor used for shift-click ranges.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Selection {
    /// Selected photos in selection order.
    pub ids: IndexSet<PhotoId>,
    /// Photo a shift-click range extends from.
    pub anchor: Option<PhotoId>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sort_order_roundtrip() {
        assert_eq!(SortOrder::ALL.len(), 5);
        for &order in SortOrder::ALL {
            assert_eq!(order.as_str().parse::<SortOrder>().unwrap(), order);
            assert_eq!(order.to_string(), order.as_str());
            let json = serde_json::to_string(&order).unwrap();
            assert_eq!(
                json,
                format!("\"{}\"", order.as_str()),
                "serde uses the same string"
            );
            assert_eq!(serde_json::from_str::<SortOrder>(&json).unwrap(), order);
        }
        let err = "newest".parse::<SortOrder>().unwrap_err();
        assert_eq!(err.to_string(), "unknown sort order \"newest\"");
        assert!(serde_json::from_str::<SortOrder>("\"DateTakenAsc\"").is_err());
        assert!("".parse::<SortOrder>().is_err());
    }

    #[test]
    fn group_by_roundtrip() {
        assert_eq!(GroupBy::ALL.len(), 4);
        for &g in GroupBy::ALL {
            assert_eq!(g.as_str().parse::<GroupBy>().unwrap(), g);
            let json = serde_json::to_string(&g).unwrap();
            assert_eq!(serde_json::from_str::<GroupBy>(&json).unwrap(), g);
        }
        assert!("decade".parse::<GroupBy>().is_err());
    }

    #[test]
    fn selection_keeps_insertion_order_and_roundtrips() {
        let mut s = Selection::default();
        for id in [5, 3, 9, 3] {
            s.ids.insert(PhotoId::new(id));
        }
        s.anchor = Some(PhotoId::new(5));
        assert_eq!(
            s.ids.iter().map(|i| i.get()).collect::<Vec<_>>(),
            vec![5, 3, 9]
        );
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<Selection>(&json).unwrap(), s);
    }
}
