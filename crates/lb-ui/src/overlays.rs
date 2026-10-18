//! Context menu and modal dialogs drawn above the library (SPEC §9.3, §9.6).

use crate::ids;
use crate::window::{LightboxWindow, Overlay};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::Disableable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, Context, Focusable as _, FontWeight, IntoElement, MouseButton, MouseDownEvent,
    Pixels, Point, TestSupportExt as _, Window, div, px,
};
use lb_app::vm::{ConfirmAction, ConfirmVm, ExportDialogVm, ExportPhase, RenameVm, SizeChoice};
use lb_app::{ExportFormat, MetadataPolicy};
use lb_core::PhotoId;

impl LightboxWindow {
    // ---- opening ----

    /// Opens the context menu for `photo`, selecting it first unless it is already selected.
    pub fn open_menu(&mut self, photo: PhotoId, at: Point<Pixels>, cx: &mut Context<Self>) {
        if !self.vm.selection().ids.contains(&photo) {
            self.vm.click(photo, lb_app::vm::Modifiers::NONE);
        }
        self.overlay = Overlay::Menu { photo, at };
        cx.notify();
    }

    /// Asks for confirmation before deleting the selection (or focused photo).
    pub fn ask_delete(&mut self, cx: &mut Context<Self>) {
        let mut ids = self.vm.selected_ids();
        if ids.is_empty() {
            ids.extend(self.vm.focused());
        }
        if ids.is_empty() {
            return;
        }
        self.overlay = Overlay::Confirm(ConfirmVm::delete(ids));
        cx.notify();
    }

    /// Opens the rename dialog for the focused or only selected photo.
    pub fn open_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ids = self.vm.selected_ids();
        let Some(id) = ids.first().copied().or(self.vm.focused()) else {
            return;
        };
        let Some(vm) = self.report(RenameVm::new(&self.app, id), cx) else {
            return;
        };
        let text = vm.text().to_owned();
        let input = cx.new(|cx| InputState::new(window, cx).default_value(text));
        let sub = cx.subscribe(&input, |this: &mut Self, input, event, cx| {
            if matches!(event, InputEvent::Change) {
                let text = input.read(cx).value().to_string();
                if let Overlay::Rename { vm, .. } = &mut this.overlay {
                    vm.set_text(&this.app, &text);
                    cx.notify();
                }
            }
        });
        window.focus(&input.focus_handle(cx), cx);
        self.overlay = Overlay::Rename {
            vm,
            input,
            _sub: sub,
        };
        cx.notify();
    }

    /// Opens the tag dialog for the target photos.
    pub fn open_tag_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("tag, another tag"));
        window.focus(&input.focus_handle(cx), cx);
        self.overlay = Overlay::Tag { input };
        cx.notify();
    }

    /// Opens the album picker for the target photos.
    pub fn open_album_picker(&mut self, cx: &mut Context<Self>) {
        self.overlay = Overlay::AlbumPicker;
        cx.notify();
    }

    /// Opens the export dialog for the target photos.
    pub fn open_export(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ids = self.vm.target_ids();
        self.open_export_for(ids, window, cx);
    }

    /// Opens the export dialog for `ids`.
    pub fn open_export_for(
        &mut self,
        ids: Vec<PhotoId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if ids.is_empty() {
            return;
        }
        let vm = Box::new(ExportDialogVm::new(&self.app, ids));
        let dest = cx.new(|cx| InputState::new(window, cx).placeholder("Destination folder"));
        let sub = cx.subscribe(&dest, |this: &mut Self, input, event, cx| {
            if matches!(event, InputEvent::Change) {
                let text = input.read(cx).value().to_string();
                if let Overlay::Export { vm, .. } = &mut this.overlay {
                    vm.dest = text;
                    cx.notify();
                }
            }
        });
        window.focus(&dest.focus_handle(cx), cx);
        self.overlay = Overlay::Export {
            vm,
            dest,
            _sub: sub,
        };
        cx.notify();
    }

    // ---- accepting ----

    fn accept_confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Overlay::Confirm(vm) = &self.overlay else {
            return;
        };
        let vm = vm.clone();
        let result = vm.accept(&self.app);
        if let Some(n) = self.report(result, cx) {
            match vm.action() {
                ConfirmAction::Delete(_) => {
                    self.toast = Some(format!(
                        "Moved {n} photo{} to the trash",
                        if n == 1 { "" } else { "s" }
                    ));
                }
                ConfirmAction::Revert(_) => self.editor_changed(cx),
            }
        }
        self.close_overlay(window, cx);
        self.refresh_all(cx);
    }

    fn accept_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Overlay::Rename { vm, .. } = &mut self.overlay else {
            return;
        };
        if !vm.can_accept() {
            return;
        }
        if vm.accept(&self.app).is_ok() {
            self.close_overlay(window, cx);
            self.refresh_all(cx);
        } else {
            cx.notify();
        }
    }

    fn accept_tags(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Overlay::Tag { input } = &self.overlay else {
            return;
        };
        let names: Vec<String> = input
            .read(cx)
            .value()
            .split(',')
            .map(|t| t.trim().to_owned())
            .filter(|t| !t.is_empty())
            .collect();
        let ids = self.vm.target_ids();
        if !names.is_empty() {
            self.vm.add_tags(&ids, &names);
        }
        self.after_intent(cx);
        self.close_overlay(window, cx);
    }

    fn pick_album(&mut self, album: lb_core::AlbumId, window: &mut Window, cx: &mut Context<Self>) {
        let ids = self.vm.target_ids();
        self.vm.add_to_album(album, &ids);
        self.after_intent(cx);
        self.close_overlay(window, cx);
    }

    fn start_export(&mut self, cx: &mut Context<Self>) {
        let Overlay::Export { vm, .. } = &mut self.overlay else {
            return;
        };
        let result = vm.start();
        self.report(result, cx);
        self.pump(cx);
        cx.notify();
    }

    /// Surfaces a failed VM intent as a toast and repaints.
    pub fn after_intent(&mut self, cx: &mut Context<Self>) {
        if let Some(message) = self.vm.take_error() {
            self.toast = Some(message);
        }
        cx.notify();
    }

    // ---- rendering ----

    pub(crate) fn render_overlay(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let theme = cx.theme().clone();
        let scrim = || {
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .bg(gpui_kit::black().opacity(0.45))
                .occlude()
        };
        let panel = |id: &'static str| {
            div()
                .id(id)
                .test_support()
                .w(px(420.))
                .p_4()
                .flex()
                .flex_col()
                .gap_3()
                .rounded_lg()
                .bg(theme.popover)
                .text_color(theme.popover_foreground)
                .border_1()
                .border_color(theme.border)
        };
        match &self.overlay {
            Overlay::None => None,
            Overlay::Menu { photo, at } => Some(self.render_menu(*photo, *at, cx)),
            Overlay::Confirm(vm) => {
                let (title, message, ok) = (
                    vm.title().to_owned(),
                    vm.message().to_owned(),
                    vm.ok_label().to_owned(),
                );
                Some(
                    scrim()
                        .child(
                            panel(ids::CONFIRM_DIALOG)
                                .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
                                .child(div().child(message))
                                .child(
                                    div()
                                        .flex()
                                        .justify_end()
                                        .gap_2()
                                        .child(
                                            Button::new(ids::CONFIRM_CANCEL)
                                                .label("Cancel")
                                                .on_click(cx.listener(|this, _, window, cx| {
                                                    this.close_overlay(window, cx);
                                                })),
                                        )
                                        .child(
                                            Button::new(ids::CONFIRM_OK)
                                                .label(ok)
                                                .danger()
                                                .on_click(cx.listener(|this, _, window, cx| {
                                                    this.accept_confirm(window, cx);
                                                })),
                                        ),
                                ),
                        )
                        .into_any_element(),
                )
            }
            Overlay::Rename { vm, input, .. } => {
                let error = vm.error().map(str::to_owned);
                let can_accept = vm.can_accept();
                let input = input.clone();
                Some(
                    scrim()
                        .child(
                            panel(ids::RENAME_DIALOG)
                                .child(div().font_weight(FontWeight::SEMIBOLD).child("Rename"))
                                .child(Input::new(&input).id(ids::RENAME_INPUT))
                                .children(error.map(|e| {
                                    div()
                                        .id(ids::RENAME_ERROR)
                                        .test_support()
                                        .aria_label(e.clone())
                                        .text_color(theme.danger)
                                        .child(e)
                                }))
                                .child(
                                    div()
                                        .flex()
                                        .justify_end()
                                        .gap_2()
                                        .child(
                                            Button::new(ids::RENAME_CANCEL)
                                                .label("Cancel")
                                                .on_click(cx.listener(|this, _, window, cx| {
                                                    this.close_overlay(window, cx);
                                                })),
                                        )
                                        .child(
                                            Button::new(ids::RENAME_OK)
                                                .label("Rename")
                                                .disabled(!can_accept)
                                                .on_click(cx.listener(|this, _, window, cx| {
                                                    this.accept_rename(window, cx);
                                                })),
                                        ),
                                ),
                        )
                        .into_any_element(),
                )
            }
            Overlay::Tag { input } => {
                let input = input.clone();
                Some(
                    scrim()
                        .child(
                            panel(ids::TAG_DIALOG)
                                .child(div().font_weight(FontWeight::SEMIBOLD).child("Add tags"))
                                .child(Input::new(&input).id(ids::TAG_INPUT))
                                .child(
                                    div()
                                        .flex()
                                        .justify_end()
                                        .gap_2()
                                        .child(
                                            Button::new(ids::TAG_CANCEL).label("Cancel").on_click(
                                                cx.listener(|this, _, window, cx| {
                                                    this.close_overlay(window, cx);
                                                }),
                                            ),
                                        )
                                        .child(Button::new(ids::TAG_OK).label("Add").on_click(
                                            cx.listener(|this, _, window, cx| {
                                                this.accept_tags(window, cx);
                                            }),
                                        )),
                                ),
                        )
                        .into_any_element(),
                )
            }
            Overlay::AlbumPicker => {
                let albums = self.app.albums().list().unwrap_or_default();
                let mut list = div().flex().flex_col().gap_1();
                for album in albums {
                    let id = album.id;
                    list = list.child(
                        div()
                            .id(ids::album_pick(id))
                            .test_support()
                            .aria_label(album.name.clone())
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .cursor_pointer()
                            .hover(|s| s.bg(theme.list_hover))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _: &MouseDownEvent, window, cx| {
                                    this.pick_album(id, window, cx);
                                }),
                            )
                            .child(album.name),
                    );
                }
                Some(
                    scrim()
                        .child(
                            panel(ids::ALBUM_PICKER)
                                .child(
                                    div()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child("Add to album"),
                                )
                                .child(list)
                                .child(
                                    div()
                                        .flex()
                                        .justify_between()
                                        .child(
                                            Button::new(ids::ALBUM_PICKER_NEW)
                                                .label("+ New album")
                                                .on_click(cx.listener(|this, _, window, cx| {
                                                    this.new_album_for_targets(window, cx);
                                                })),
                                        )
                                        .child(
                                            Button::new("album-picker-cancel")
                                                .label("Cancel")
                                                .on_click(cx.listener(|this, _, window, cx| {
                                                    this.close_overlay(window, cx);
                                                })),
                                        ),
                                ),
                        )
                        .into_any_element(),
                )
            }
            Overlay::Export { vm, dest, .. } => {
                let dest = dest.clone();
                let phase = vm.phase().clone();
                let errors = vm.errors();
                let size = vm.size;
                let strip_gps = vm.metadata == MetadataPolicy::KeepExifNoGps;
                let png = vm.format == ExportFormat::Png;
                let editing = matches!(phase, ExportPhase::Editing);
                let status = match &phase {
                    ExportPhase::Editing => errors
                        .first()
                        .map(|e| e.message.clone())
                        .unwrap_or_else(|| format!("{} photos", vm.ids().len())),
                    ExportPhase::Running { progress } => {
                        format!("Exporting… {:.0}%", progress * 100.0)
                    }
                    ExportPhase::Done(r) => format!(
                        "Exported {} photo{}{}{}",
                        r.written.len(),
                        if r.written.len() == 1 { "" } else { "s" },
                        if r.skipped.is_empty() {
                            String::new()
                        } else {
                            format!(", skipped {}", r.skipped.len())
                        },
                        if r.failed.is_empty() {
                            String::new()
                        } else {
                            format!(", {} failed", r.failed.len())
                        },
                    ),
                };
                let can_start = editing && errors.is_empty();
                let choice = |label: &'static str,
                              id: &'static str,
                              value: SizeChoice,
                              cx: &mut Context<Self>| {
                    Button::new(id)
                        .label(label)
                        .compact()
                        .toggled(size == value)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Overlay::Export { vm, .. } = &mut this.overlay {
                                vm.size = value;
                                cx.notify();
                            }
                        }))
                };
                Some(
                    scrim()
                        .child(
                            panel(ids::EXPORT_DIALOG)
                                .child(div().font_weight(FontWeight::SEMIBOLD).child("Export"))
                                .child(Input::new(&dest).id(ids::EXPORT_DEST))
                                .child(
                                    div()
                                        .flex()
                                        .gap_1()
                                        .child(choice(
                                            "Original",
                                            "export-size-original",
                                            SizeChoice::Original,
                                            cx,
                                        ))
                                        .child(choice(
                                            "2048",
                                            "export-size-2048",
                                            SizeChoice::Px2048,
                                            cx,
                                        ))
                                        .child(choice(
                                            "1024",
                                            "export-size-1024",
                                            SizeChoice::Px1024,
                                            cx,
                                        )),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .gap_1()
                                        .child(
                                            Button::new("export-strip-gps")
                                                .label("Remove GPS")
                                                .compact()
                                                .toggled(strip_gps)
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    if let Overlay::Export { vm, .. } =
                                                        &mut this.overlay
                                                    {
                                                        vm.metadata = if vm.metadata
                                                            == MetadataPolicy::KeepExifNoGps
                                                        {
                                                            MetadataPolicy::KeepAll
                                                        } else {
                                                            MetadataPolicy::KeepExifNoGps
                                                        };
                                                        cx.notify();
                                                    }
                                                })),
                                        )
                                        .child(
                                            Button::new("export-png")
                                                .label("PNG")
                                                .compact()
                                                .toggled(png)
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    if let Overlay::Export { vm, .. } =
                                                        &mut this.overlay
                                                    {
                                                        vm.format =
                                                            if vm.format == ExportFormat::Png {
                                                                ExportFormat::Jpeg
                                                            } else {
                                                                ExportFormat::Png
                                                            };
                                                        cx.notify();
                                                    }
                                                })),
                                        ),
                                )
                                .child(
                                    div()
                                        .id(ids::EXPORT_STATUS)
                                        .test_support()
                                        .aria_label(status.clone())
                                        .text_color(theme.muted_foreground)
                                        .child(status),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .justify_end()
                                        .gap_2()
                                        .child(
                                            Button::new(ids::EXPORT_CANCEL)
                                                .label(if editing { "Cancel" } else { "Close" })
                                                .on_click(cx.listener(|this, _, window, cx| {
                                                    if let Overlay::Export { vm, .. } =
                                                        &mut this.overlay
                                                    {
                                                        vm.cancel();
                                                    }
                                                    this.close_overlay(window, cx);
                                                })),
                                        )
                                        .child(
                                            Button::new(ids::EXPORT_CONFIRM)
                                                .label("Export")
                                                .disabled(!can_start)
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.start_export(cx);
                                                })),
                                        ),
                                ),
                        )
                        .into_any_element(),
                )
            }
            Overlay::Settings => Some(self.render_settings_overlay(cx)),
        }
    }

    fn new_album_for_targets(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ids = self.vm.target_ids();
        let created = self.sidebar.create_album();
        if let Some((album, _)) = self.report(created, cx) {
            self.vm.add_to_album(album.id, &ids);
            self.after_intent(cx);
            if let Err(e) = self.sidebar.refresh() {
                tracing::warn!(error = %e, "sidebar refresh failed");
            }
        }
        self.close_overlay(window, cx);
    }

    fn render_menu(
        &mut self,
        photo: PhotoId,
        at: Point<Pixels>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme().clone();
        let item = |id: &'static str,
                    label: &'static str,
                    cx: &mut Context<Self>,
                    f: fn(&mut Self, PhotoId, &mut Window, &mut Context<Self>)| {
            div()
                .id(id)
                .test_support()
                .aria_label(label)
                .px_3()
                .py_1()
                .cursor_pointer()
                .hover(|s| s.bg(theme.list_hover))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        this.overlay = Overlay::None;
                        window.focus(&this.focus, cx);
                        f(this, photo, window, cx);
                        cx.notify();
                    }),
                )
                .child(label)
        };
        div()
            .absolute()
            .inset_0()
            .occlude()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, window, cx| this.close_overlay(window, cx)),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, _: &MouseDownEvent, window, cx| this.close_overlay(window, cx)),
            )
            .child(
                div()
                    .id(ids::CONTEXT_MENU)
                    .test_support()
                    .absolute()
                    .left(at.x)
                    .top(at.y)
                    .w(px(200.))
                    .py_1()
                    .rounded_md()
                    .bg(theme.popover)
                    .text_color(theme.popover_foreground)
                    .border_1()
                    .border_color(theme.border)
                    .flex()
                    .flex_col()
                    .child(item(ids::MENU_OPEN, "Open", cx, |this, id, _, cx| {
                        this.open_photo(id, cx)
                    }))
                    .child(item(ids::MENU_STAR, "Star", cx, |this, _, _, cx| {
                        this.toggle_star_targets(cx)
                    }))
                    .child(item(ids::MENU_HOLD, "Hold in tray", cx, |this, _, _, _| {
                        this.vm.hold_selected()
                    }))
                    .child(item(
                        ids::MENU_EXPORT,
                        "Export…",
                        cx,
                        |this, _, window, cx| this.open_export(window, cx),
                    ))
                    .child(item(
                        ids::MENU_REVEAL,
                        "Reveal in file manager",
                        cx,
                        |this, id, _, cx| {
                            let result = this.app.photos().reveal(id);
                            this.report(result, cx);
                        },
                    ))
                    .child(item(
                        ids::MENU_RENAME,
                        "Rename…",
                        cx,
                        |this, _, window, cx| this.open_rename(window, cx),
                    ))
                    .child(item(ids::MENU_HIDE, "Hide", cx, |this, _, _, cx| {
                        this.vm.set_hidden_selected(true);
                        this.after_intent(cx);
                    }))
                    .child(item(ids::MENU_DELETE, "Delete…", cx, |this, _, _, cx| {
                        this.ask_delete(cx)
                    })),
            )
            .into_any_element()
    }
}
