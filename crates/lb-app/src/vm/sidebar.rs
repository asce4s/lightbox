//! State of the library sidebar: sources, albums and the folder tree (SPEC §9.2).

use crate::error::Result;
use crate::lightbox::Lightbox;
use crate::query::ViewSource;
use chrono::Datelike;
use lb_core::{Album, AlbumId, FolderId};
use std::collections::BTreeMap;

/// Identity of a sidebar row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SidebarKey {
    /// All photos.
    All,
    /// Starred photos.
    Starred,
    /// Recently edited photos.
    Recent,
    /// An album.
    Album(AlbumId),
    /// A folder.
    Folder(FolderId),
    /// A non-selectable year heading in the folder tree; `None` groups undated folders.
    Year(Option<i32>),
}

/// One row of the sidebar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidebarItem {
    /// Row identity.
    pub key: SidebarKey,
    /// Text shown.
    pub label: String,
    /// Number of photos behind the row (badge).
    pub count: usize,
    /// Indentation level (0 or 1).
    pub depth: u8,
}

/// A titled group of rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidebarSection {
    /// Section title.
    pub title: &'static str,
    /// Rows in display order.
    pub items: Vec<SidebarItem>,
}

/// The sidebar's view-model.
pub struct SidebarVm {
    lb: Lightbox,
    sections: Vec<SidebarSection>,
    selected: SidebarKey,
}

impl SidebarVm {
    /// A sidebar showing all photos, populated from the library.
    pub fn new(lb: &Lightbox) -> Result<Self> {
        let mut vm = Self {
            lb: lb.clone(),
            sections: Vec::new(),
            selected: SidebarKey::All,
        };
        vm.refresh()?;
        Ok(vm)
    }

    /// Sections in display order.
    pub fn sections(&self) -> &[SidebarSection] {
        &self.sections
    }

    /// The highlighted row.
    pub fn selected(&self) -> SidebarKey {
        self.selected
    }

    /// Reloads albums, folders and counts.
    pub fn refresh(&mut self) -> Result<()> {
        let photos = self.lb.photos();
        let count = |source: &ViewSource| photos.list_ids(source, false).map(|ids| ids.len());
        let library = SidebarSection {
            title: "Library",
            items: vec![
                item(
                    SidebarKey::All,
                    "All photos",
                    count(&ViewSource::AllPhotos)?,
                    0,
                ),
                item(
                    SidebarKey::Starred,
                    "Starred",
                    count(&ViewSource::Starred)?,
                    0,
                ),
                item(
                    SidebarKey::Recent,
                    "Recently edited",
                    count(&ViewSource::RecentlyEdited)?,
                    0,
                ),
            ],
        };
        let mut albums = Vec::new();
        for album in self.lb.albums().list()? {
            let n = count(&ViewSource::Album(album.id))?;
            albums.push(item(SidebarKey::Album(album.id), &album.name, n, 0));
        }
        let mut by_year: BTreeMap<Option<i32>, Vec<(String, FolderId, usize)>> = BTreeMap::new();
        for root in self.lb.library().roots()? {
            for folder in self.lb.library().folders(root.id)? {
                let year = folder.sort_date.map(|d| d.year());
                let n = count(&ViewSource::Folder(folder.id))?;
                if n == 0 && year.is_none() {
                    continue;
                }
                by_year
                    .entry(year)
                    .or_default()
                    .push((folder.name, folder.id, n));
            }
        }
        let mut folders = Vec::new();
        let mut years: Vec<_> = by_year.into_iter().collect();
        years.sort_by(|a, b| match (a.0, b.0) {
            (Some(x), Some(y)) => y.cmp(&x),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        });
        for (year, mut list) in years {
            list.sort_by_key(|a| a.0.to_lowercase());
            let total = list.iter().map(|f| f.2).sum();
            let label = year.map_or_else(|| "Undated".to_owned(), |y| y.to_string());
            folders.push(item(SidebarKey::Year(year), &label, total, 0));
            for (name, id, n) in list {
                folders.push(item(SidebarKey::Folder(id), &name, n, 1));
            }
        }
        self.sections = vec![
            library,
            SidebarSection {
                title: "Albums",
                items: albums,
            },
            SidebarSection {
                title: "Folders",
                items: folders,
            },
        ];
        if !self.contains(self.selected) {
            self.selected = SidebarKey::All;
        }
        Ok(())
    }

    fn contains(&self, key: SidebarKey) -> bool {
        self.sections
            .iter()
            .flat_map(|s| &s.items)
            .any(|i| i.key == key)
    }

    /// Highlights `key` and returns the source to show; headings are not selectable.
    pub fn select(&mut self, key: SidebarKey) -> Option<ViewSource> {
        let source = source_of(key)?;
        if !self.contains(key) {
            return None;
        }
        self.selected = key;
        Some(source)
    }

    /// The row that shows `source`, if any.
    pub fn key_for(source: &ViewSource) -> Option<SidebarKey> {
        match source {
            ViewSource::AllPhotos => Some(SidebarKey::All),
            ViewSource::Starred => Some(SidebarKey::Starred),
            ViewSource::RecentlyEdited => Some(SidebarKey::Recent),
            ViewSource::Album(id) => Some(SidebarKey::Album(*id)),
            ViewSource::Folder(id) => Some(SidebarKey::Folder(*id)),
            ViewSource::Ids(_) => None,
        }
    }

    /// Creates an album with the first free name "New album", "New album 2", ... and selects it.
    pub fn create_album(&mut self) -> Result<(Album, ViewSource)> {
        let existing: Vec<String> = self
            .lb
            .albums()
            .list()?
            .into_iter()
            .map(|a| a.name)
            .collect();
        let mut name = "New album".to_owned();
        let mut n = 2;
        while existing.iter().any(|e| e.eq_ignore_ascii_case(&name)) {
            name = format!("New album {n}");
            n += 1;
        }
        let album = self.lb.albums().create(&name, None)?;
        self.refresh()?;
        self.selected = SidebarKey::Album(album.id);
        Ok((album.clone(), ViewSource::Album(album.id)))
    }
}

fn item(key: SidebarKey, label: &str, count: usize, depth: u8) -> SidebarItem {
    SidebarItem {
        key,
        label: label.to_owned(),
        count,
        depth,
    }
}

fn source_of(key: SidebarKey) -> Option<ViewSource> {
    match key {
        SidebarKey::All => Some(ViewSource::AllPhotos),
        SidebarKey::Starred => Some(ViewSource::Starred),
        SidebarKey::Recent => Some(ViewSource::RecentlyEdited),
        SidebarKey::Album(id) => Some(ViewSource::Album(id)),
        SidebarKey::Folder(id) => Some(ViewSource::Folder(id)),
        SidebarKey::Year(_) => None,
    }
}

impl SidebarVm {
    /// A sidebar with no rows, used when the library could not be read.
    pub fn empty(lb: &Lightbox) -> Self {
        Self {
            lb: lb.clone(),
            sections: Vec::new(),
            selected: SidebarKey::All,
        }
    }
}
