//! State and intents of the library grid (SPEC §7.5, §9.3).

use super::grid::{GridLayout, ZOOM_RANGE, columns_for};
use crate::error::Result;
use crate::events::AppEvent;
use crate::lightbox::Lightbox;
use crate::query::ViewSource;
use crate::search::{FilterChips, MonthBucket, PhotoQueryResult, ViewQuery};
use indexmap::IndexSet;
use lb_core::{EditOp, GroupBy, Photo, PhotoId, Selection, SortOrder};
use std::collections::HashMap;
use std::time::Duration;

/// Delay between the last keystroke in the search box and the query (SPEC §6.8.1).
pub const SEARCH_DEBOUNCE: Duration = Duration::from_millis(150);
/// Default thumbnail cell size.
pub const DEFAULT_ZOOM: u32 = 160;

/// Keyboard modifiers of a pointer or key event.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Modifiers {
    /// Shift extends a range.
    pub shift: bool,
    /// Ctrl (or Cmd on macOS) toggles.
    pub ctrl: bool,
}

impl Modifiers {
    /// No modifier.
    pub const NONE: Self = Self {
        shift: false,
        ctrl: false,
    };
    /// Shift only.
    pub const SHIFT: Self = Self {
        shift: true,
        ctrl: false,
    };
    /// Ctrl/Cmd only.
    pub const CTRL: Self = Self {
        shift: false,
        ctrl: true,
    };
}

/// Keys that move the focus in the grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavKey {
    /// Previous cell, wrapping to the previous row.
    Left,
    /// Next cell, wrapping to the next row.
    Right,
    /// Same column, previous row (crossing group boundaries).
    Up,
    /// Same column, next row (crossing group boundaries).
    Down,
    /// First photo.
    Home,
    /// Last photo.
    End,
}

/// The library grid's view-model.
pub struct LibraryVm {
    lb: Lightbox,
    view: ViewQuery,
    result: PhotoQueryResult,
    order: Vec<PhotoId>,
    pos: HashMap<PhotoId, (usize, usize)>,
    selection: Selection,
    focused: Option<PhotoId>,
    tray: IndexSet<PhotoId>,
    scroll_anchor: Option<PhotoId>,
    zoom: u32,
    viewport_width: f32,
    layout: GridLayout,
    pending_search: Option<(String, Duration)>,
    last_error: Option<String>,
}

impl LibraryVm {
    /// A grid over all photos, grouped by month, showing nothing until [`Self::refresh`].
    pub fn new(lb: &Lightbox) -> Self {
        let settings = lb.settings().load().unwrap_or_default();
        let mut view = ViewQuery::new(ViewSource::AllPhotos);
        view.group_by = settings.grid.group_by;
        view.sort = settings.grid.sort;
        view.show_hidden = settings.library.show_hidden;
        Self {
            lb: lb.clone(),
            view,
            result: PhotoQueryResult::default(),
            order: Vec::new(),
            pos: HashMap::new(),
            selection: Selection::default(),
            focused: None,
            tray: IndexSet::new(),
            scroll_anchor: None,
            zoom: settings.grid.thumb_size.zoom_px(),
            viewport_width: 1200.0,
            layout: GridLayout::default(),
            pending_search: None,
            last_error: None,
        }
    }

    /// The current query.
    pub fn view(&self) -> &ViewQuery {
        &self.view
    }

    /// Groups and totals of the last query.
    pub fn result(&self) -> &PhotoQueryResult {
        &self.result
    }

    /// Every photo id in display order.
    pub fn order(&self) -> &[PhotoId] {
        &self.order
    }

    /// Row layout for the current width and zoom.
    pub fn layout(&self) -> &GridLayout {
        &self.layout
    }

    /// Cells per row.
    pub fn columns(&self) -> usize {
        self.layout.columns
    }

    /// Cell edge in pixels.
    pub fn zoom(&self) -> u32 {
        self.zoom
    }

    /// Takes the message of the last failed intent, if any.
    pub fn take_error(&mut self) -> Option<String> {
        self.last_error.take()
    }

    /// Message of the last failed intent, if any.
    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    /// Location of `id` as `(group, index within group)`.
    pub fn position_of(&self, id: PhotoId) -> Option<(usize, usize)> {
        self.pos.get(&id).copied()
    }

    /// Rows for photos in `ids` (unknown ids skipped).
    pub fn photos(&self, ids: &[PhotoId]) -> Result<Vec<Photo>> {
        self.lb.photos().get_many(ids)
    }

    fn relayout(&mut self) {
        let cols = columns_for(self.viewport_width, self.zoom);
        self.layout = GridLayout::build(&self.result.groups, cols, self.zoom as f32);
    }

    fn index_result(&mut self) {
        self.order = self.result.ids();
        self.pos.clear();
        for (g, group) in self.result.groups.iter().enumerate() {
            for (i, id) in group.ids.iter().enumerate() {
                self.pos.insert(*id, (g, i));
            }
        }
    }

    /// Runs the query again, keeping selection, focus and the scroll anchor where the photos
    /// still exist. A vanished anchor moves to the nearest photo that remains.
    pub fn refresh(&mut self) -> Result<()> {
        let result = self.lb.search().query(&self.view)?;
        let old_order = std::mem::take(&mut self.order);
        let old_anchor_idx = self
            .scroll_anchor
            .and_then(|a| old_order.iter().position(|id| *id == a));
        self.result = result;
        self.index_result();
        self.selection.ids.retain(|id| self.pos.contains_key(id));
        if self
            .selection
            .anchor
            .is_some_and(|a| !self.pos.contains_key(&a))
        {
            self.selection.anchor = self.selection.ids.last().copied();
        }
        if self.focused.is_some_and(|f| !self.pos.contains_key(&f)) {
            self.focused = None;
        }
        if self
            .scroll_anchor
            .is_some_and(|a| !self.pos.contains_key(&a))
        {
            self.scroll_anchor = old_anchor_idx.and_then(|i| {
                old_order[i..]
                    .iter()
                    .chain(old_order[..i].iter().rev())
                    .find(|id| self.pos.contains_key(id))
                    .copied()
            });
        }
        self.relayout();
        Ok(())
    }

    fn refresh_logged(&mut self) {
        if let Err(e) = self.refresh() {
            self.last_error = Some(e.user_message());
        }
    }

    /// Reacts to a service event; returns whether the grid changed.
    pub fn handle_event(&mut self, event: &AppEvent) -> bool {
        match event {
            AppEvent::PhotosChanged { .. }
            | AppEvent::PhotosAdded { .. }
            | AppEvent::PhotosRemoved { .. }
            | AppEvent::AlbumsChanged
            | AppEvent::TagsChanged
            | AppEvent::PeopleChanged
            | AppEvent::FoldersChanged
            | AppEvent::ScanFinished { .. } => {
                self.refresh_logged();
                true
            }
            _ => false,
        }
    }

    // ---- view settings ----

    /// Changes where photos come from and clears the selection.
    pub fn set_source(&mut self, source: ViewSource) {
        self.view.source = source;
        self.selection = Selection::default();
        self.focused = None;
        self.scroll_anchor = None;
        self.refresh_logged();
    }

    /// Replaces the search text immediately.
    pub fn set_search(&mut self, text: &str) {
        self.pending_search = None;
        self.view.search = text.to_owned();
        self.refresh_logged();
    }

    /// Records a keystroke; the query runs after [`SEARCH_DEBOUNCE`] on [`Self::tick`].
    pub fn type_search(&mut self, text: &str) {
        self.pending_search = Some((text.to_owned(), self.lb.clock().elapsed()));
    }

    /// Applies a debounced search when it is due. Returns whether the query ran.
    pub fn tick(&mut self) -> bool {
        let Some((text, at)) = self.pending_search.clone() else {
            return false;
        };
        if self.lb.clock().elapsed().saturating_sub(at) < SEARCH_DEBOUNCE {
            return false;
        }
        self.set_search(&text);
        true
    }

    /// Replaces the filter chips.
    pub fn set_chips(&mut self, chips: FilterChips) {
        self.view.chips = chips;
        self.refresh_logged();
    }

    /// Applies the grid and library sections of changed settings.
    pub fn apply_settings(&mut self, settings: &crate::Settings) {
        self.view.group_by = settings.grid.group_by;
        self.view.sort = settings.grid.sort;
        self.view.show_hidden = settings.library.show_hidden;
        self.zoom = settings.grid.thumb_size.zoom_px();
        self.refresh_logged();
    }

    /// Changes header grouping.
    pub fn set_group_by(&mut self, group_by: GroupBy) {
        self.view.group_by = group_by;
        self.refresh_logged();
    }

    /// Changes the sort order.
    pub fn set_sort(&mut self, sort: SortOrder) {
        self.view.sort = sort;
        self.refresh_logged();
    }

    /// Shows or hides hidden photos.
    pub fn set_show_hidden(&mut self, show: bool) {
        self.view.show_hidden = show;
        self.refresh_logged();
    }

    /// Sets the cell size within the slider's range.
    pub fn set_zoom(&mut self, px: u32) {
        self.zoom = px.clamp(ZOOM_RANGE.0, ZOOM_RANGE.1);
        self.relayout();
    }

    /// Tells the VM how wide the grid area is.
    pub fn set_viewport_width(&mut self, width: f32) {
        self.viewport_width = width.max(1.0);
        self.relayout();
    }

    // ---- scroll anchor ----

    /// The photo the viewport is anchored to.
    pub fn scroll_anchor(&self) -> Option<PhotoId> {
        self.scroll_anchor
    }

    /// Anchors the viewport to `id`, typically the first visible photo after scrolling.
    pub fn set_scroll_anchor(&mut self, id: Option<PhotoId>) {
        self.scroll_anchor = id;
    }

    /// Scroll offset that puts the anchor's row at the top of the viewport.
    pub fn anchor_scroll_top(&self) -> f32 {
        self.scroll_anchor
            .and_then(|id| self.pos.get(&id))
            .and_then(|&(g, i)| self.layout.row_of(g, i))
            .map_or(0.0, |row| self.layout.row_top(row))
    }

    /// The first photo in the row at `scroll_y`, for updating the anchor while scrolling.
    pub fn photo_at_scroll(&self, scroll_y: f32) -> Option<PhotoId> {
        let range = self.layout.visible_rows(scroll_y, 1.0, 0.0);
        range.into_iter().find_map(|r| match self.layout.rows[r] {
            super::grid::GridRow::Cells { group, start, .. } => {
                self.result.groups.get(group)?.ids.get(start).copied()
            }
            super::grid::GridRow::Header { .. } => None,
        })
    }

    // ---- timeline ----

    /// Months of the current result in grid order, for the timeline scrubber.
    pub fn timeline(&self) -> &[MonthBucket] {
        &self.result.timeline
    }

    /// Anchors the grid to the first photo of month bucket `index` and returns its row, which the
    /// view scrolls to the top.
    pub fn timeline_jump(&mut self, index: usize) -> Option<usize> {
        let first = self.result.timeline.get(index)?.first;
        self.scroll_anchor = Some(first);
        let (g, i) = *self.pos.get(&first)?;
        self.layout.row_of(g, i)
    }

    /// Filters the grid to the months of buckets `a` to `b` (either order), as the date chip.
    pub fn timeline_filter(&mut self, a: usize, b: usize) {
        let (lo, hi) = (a.min(b), a.max(b));
        let (Some(first), Some(last)) =
            (self.result.timeline.get(lo), self.result.timeline.get(hi))
        else {
            return;
        };
        let mut months: Vec<(i32, u32)> = self.result.timeline[lo..=hi]
            .iter()
            .map(|m| (m.year, m.month))
            .collect();
        months.sort_unstable();
        let (from_y, from_m) = months.first().copied().unwrap_or((first.year, first.month));
        let (to_y, to_m) = months.last().copied().unwrap_or((last.year, last.month));
        let from = chrono::NaiveDate::from_ymd_opt(from_y, from_m, 1);
        let next = if to_m == 12 {
            chrono::NaiveDate::from_ymd_opt(to_y + 1, 1, 1)
        } else {
            chrono::NaiveDate::from_ymd_opt(to_y, to_m + 1, 1)
        };
        let to = next.and_then(|d| d.pred_opt());
        let mut chips = self.view.chips.clone();
        chips.date = Some((from, to));
        self.set_chips(chips);
    }

    // ---- selection ----

    /// The current selection.
    pub fn selection(&self) -> &Selection {
        &self.selection
    }

    /// Selected ids in display order.
    pub fn selected_ids(&self) -> Vec<PhotoId> {
        self.order
            .iter()
            .filter(|id| self.selection.ids.contains(*id))
            .copied()
            .collect()
    }

    /// The photo with keyboard focus.
    pub fn focused(&self) -> Option<PhotoId> {
        self.focused
    }

    fn select_range(&mut self, from: PhotoId, to: PhotoId, keep: bool) {
        let (Some(&a), Some(&b)) = (self.pos.get(&from), self.pos.get(&to)) else {
            return;
        };
        let flat = |p: (usize, usize)| {
            self.result.groups[..p.0]
                .iter()
                .map(|g| g.ids.len())
                .sum::<usize>()
                + p.1
        };
        let (lo, hi) = (flat(a).min(flat(b)), flat(a).max(flat(b)));
        if !keep {
            self.selection.ids.clear();
        }
        for id in &self.order[lo..=hi] {
            self.selection.ids.insert(*id);
        }
    }

    /// A click on a cell: plain selects it, Ctrl toggles it, Shift selects the range from the
    /// anchor (Ctrl+Shift adds the range to the selection).
    pub fn click(&mut self, id: PhotoId, mods: Modifiers) {
        if !self.pos.contains_key(&id) {
            return;
        }
        match (mods.shift, mods.ctrl) {
            (false, false) => {
                self.selection.ids.clear();
                self.selection.ids.insert(id);
                self.selection.anchor = Some(id);
            }
            (false, true) => {
                if !self.selection.ids.shift_remove(&id) {
                    self.selection.ids.insert(id);
                }
                self.selection.anchor = Some(id);
            }
            (true, ctrl) => {
                let anchor = self.selection.anchor.unwrap_or(id);
                self.select_range(anchor, id, ctrl);
                self.selection.anchor = Some(anchor);
            }
        }
        self.focused = Some(id);
        self.scroll_anchor = Some(id);
    }

    /// Selects every photo in the view.
    pub fn select_all(&mut self) {
        self.selection.ids = self.order.iter().copied().collect();
        if self.selection.anchor.is_none() {
            self.selection.anchor = self.order.first().copied();
        }
    }

    /// Clears the selection (Esc).
    pub fn clear_selection(&mut self) {
        self.selection = Selection::default();
    }

    /// Moves the focus like the arrow keys; `extend` (Shift) grows the selection from its anchor,
    /// otherwise the focused photo becomes the only selected one.
    pub fn move_focus(&mut self, key: NavKey, extend: bool) {
        let Some(target) = self.neighbor(key) else {
            return;
        };
        self.focused = Some(target);
        self.scroll_anchor = Some(target);
        if extend {
            let anchor = self.selection.anchor.unwrap_or(target);
            self.select_range(anchor, target, false);
            self.selection.anchor = Some(anchor);
        } else {
            self.selection.ids.clear();
            self.selection.ids.insert(target);
            self.selection.anchor = Some(target);
        }
    }

    fn neighbor(&self, key: NavKey) -> Option<PhotoId> {
        let groups = &self.result.groups;
        let first = self.order.first().copied()?;
        let Some((g, i)) = self.focused.and_then(|f| self.pos.get(&f).copied()) else {
            return Some(if key == NavKey::End {
                self.order.last().copied()?
            } else {
                first
            });
        };
        let cols = self.layout.columns;
        let n = groups[g].ids.len();
        let at = |g: usize, i: usize| groups.get(g).and_then(|gr| gr.ids.get(i)).copied();
        let nonempty_after =
            |from: usize| (from..groups.len()).find(|&k| !groups[k].ids.is_empty());
        let nonempty_before = |to: usize| (0..to).rev().find(|&k| !groups[k].ids.is_empty());
        match key {
            NavKey::Home => Some(first),
            NavKey::End => self.order.last().copied(),
            NavKey::Left => {
                if i > 0 {
                    at(g, i - 1)
                } else {
                    nonempty_before(g).and_then(|p| at(p, groups[p].ids.len() - 1))
                }
            }
            NavKey::Right => {
                if i + 1 < n {
                    at(g, i + 1)
                } else {
                    nonempty_after(g + 1).and_then(|p| at(p, 0))
                }
            }
            NavKey::Down => {
                let col = i % cols;
                if i + cols < n {
                    at(g, i + cols)
                } else if (i / cols + 1) * cols < n {
                    at(g, n - 1)
                } else {
                    nonempty_after(g + 1).and_then(|p| at(p, col.min(groups[p].ids.len() - 1)))
                }
            }
            NavKey::Up => {
                let col = i % cols;
                if i >= cols {
                    at(g, i - cols)
                } else {
                    nonempty_before(g).and_then(|p| {
                        let len = groups[p].ids.len();
                        let last_row = (len - 1) / cols;
                        at(p, (last_row * cols + col).min(len - 1))
                    })
                }
            }
        }
    }

    /// The photo Enter opens in the viewer.
    pub fn activate(&self) -> Option<PhotoId> {
        self.focused
    }

    // ---- tray ----

    /// Photos held in the tray, in the order they were added.
    pub fn tray(&self) -> Vec<PhotoId> {
        self.tray.iter().copied().collect()
    }

    /// Adds the selection (or the focused photo when nothing is selected) to the tray.
    pub fn hold_selected(&mut self) {
        let ids = self.selected_or_focused();
        self.tray.extend(ids);
    }

    /// Space: toggles the focused photo in the tray.
    pub fn toggle_hold_focused(&mut self) {
        if let Some(id) = self.focused
            && !self.tray.shift_remove(&id)
        {
            self.tray.insert(id);
        }
    }

    /// Empties the tray.
    pub fn clear_tray(&mut self) {
        self.tray.clear();
    }

    fn selected_or_focused(&self) -> Vec<PhotoId> {
        let sel = self.selected_ids();
        if sel.is_empty() {
            self.focused.into_iter().collect()
        } else {
            sel
        }
    }

    /// What toolbar actions apply to: the tray when it holds photos, else the selection.
    pub fn target_ids(&self) -> Vec<PhotoId> {
        if self.tray.is_empty() {
            self.selected_or_focused()
        } else {
            self.tray()
        }
    }

    // ---- organize intents ----

    fn run<T>(&mut self, f: impl FnOnce(&Lightbox) -> Result<T>) -> Option<T> {
        match f(&self.lb) {
            Ok(v) => {
                self.last_error = None;
                self.refresh_logged();
                Some(v)
            }
            Err(e) => {
                self.last_error = Some(e.user_message());
                None
            }
        }
    }

    /// `S`: stars the selection, or unstars it when everything is starred.
    pub fn toggle_star_selected(&mut self) {
        let ids = self.selected_or_focused();
        self.toggle_star_on(&ids);
    }

    /// Stars `ids`, or unstars them when all are starred.
    pub fn toggle_star_on(&mut self, ids: &[PhotoId]) {
        if ids.is_empty() {
            return;
        }
        self.run(|lb| lb.photos().toggle_star(ids));
    }

    /// `0` to `5`: sets the rating of the selection.
    pub fn rate_selected(&mut self, rating: u8) {
        let ids = self.selected_or_focused();
        self.rate_on(&ids, rating);
    }

    /// Sets the rating of `ids`.
    pub fn rate_on(&mut self, ids: &[PhotoId], rating: u8) {
        if ids.is_empty() {
            return;
        }
        self.run(|lb| lb.photos().set_rating(ids, rating));
    }

    /// Hides or shows the selection.
    pub fn set_hidden_selected(&mut self, hidden: bool) {
        let ids = self.selected_or_focused();
        if !ids.is_empty() {
            self.run(|lb| lb.photos().set_hidden(&ids, hidden));
        }
    }

    /// Rotates `ids` by quarter turns (positive is clockwise) as an edit.
    pub fn rotate_on(&mut self, ids: &[PhotoId], turns: i8) {
        if ids.is_empty() {
            return;
        }
        self.run(|lb| {
            for id in ids {
                lb.edits().push(*id, EditOp::RotateQuarter { turns })?;
            }
            Ok(())
        });
    }

    /// Cmd/Ctrl+R and Cmd/Ctrl+Shift+R.
    pub fn rotate_selected(&mut self, turns: i8) {
        let ids = self.selected_or_focused();
        self.rotate_on(&ids, turns);
    }

    /// Adds `ids` to an album.
    pub fn add_to_album(&mut self, album: lb_core::AlbumId, ids: &[PhotoId]) {
        if !ids.is_empty() {
            self.run(|lb| lb.albums().add_photos(album, ids));
        }
    }

    /// Adds tags to `ids`.
    pub fn add_tags(&mut self, ids: &[PhotoId], names: &[String]) {
        if !ids.is_empty() {
            self.run(|lb| lb.tags().add(ids, names));
        }
    }
}
