//! Photo viewer settings and the first-run default-app banner (SPEC-VIEWER §V10.6, §V11).

use crate::ids;
use crate::window::{LightboxWindow, Overlay};
use gpui_kit::component::button::Button;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::{ActiveTheme as _, Disableable as _};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, Context, FontWeight, IntoElement, TestSupportExt as _, Window, div, px,
};
use lb_app::viewer::FolderSort;
use lb_app::{
    AppInstall, AssocService, AssocStatus, BackdropMode, FormatAssoc, MakeDefaultOutcome,
    MotionLevel, SavedDefaults, ViewerSettings, WheelAction,
};
use std::path::PathBuf;

/// One file type in the settings list.
#[derive(Clone)]
pub(crate) struct TypeRow {
    ext: String,
    status: String,
    checked: bool,
}

/// Draft shown by the settings overlay.
#[derive(Clone)]
pub(crate) struct ViewerSettingsState {
    draft: ViewerSettings,
    rows: Vec<TypeRow>,
    note: Option<String>,
}

const PROMPT: &str = "Open photos from your file manager with Lightbox?";

impl LightboxWindow {
    pub(crate) fn show_default_viewer_banner(&self) -> bool {
        if self.standalone {
            return false;
        }
        let Ok(settings) = self.app.settings().load() else {
            return false;
        };
        if settings.viewer.default_prompt_dismissed {
            return false;
        }
        self.app.force_default_prompt() || lb_app::running_as_installed_app()
    }

    pub(crate) fn render_default_viewer_banner(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        div()
            .id(ids::DEFAULT_VIEWER_BANNER)
            .test_support()
            .aria_label(PROMPT)
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .py_2()
            .bg(theme.muted)
            .text_color(theme.foreground)
            .child(PROMPT)
            .child(
                Button::new(ids::BANNER_MAKE_DEFAULT)
                    .label("Make default")
                    .compact()
                    .on_click(cx.listener(|this, _, _, cx| this.banner_make_default(cx))),
            )
            .child(
                Button::new(ids::BANNER_DISMISS)
                    .label("Not now")
                    .compact()
                    .on_click(cx.listener(|this, _, _, cx| this.dismiss_default_prompt(cx))),
            )
            .into_any_element()
    }

    pub(crate) fn open_viewer_settings(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let draft = self
            .app
            .settings()
            .load()
            .map(|settings| settings.viewer)
            .unwrap_or_default();
        let rows = self.type_rows(&draft);
        self.settings_panel = Some(ViewerSettingsState {
            draft,
            rows,
            note: None,
        });
        self.overlay = Overlay::Settings;
        cx.notify();
    }

    pub(crate) fn render_settings_overlay(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let Some(state) = self.settings_panel.clone() else {
            return div().into_any_element();
        };
        let can_restore = state.draft.saved_defaults.is_some();
        let rows = state.rows.clone();
        let note = state.note.clone();
        let scrim = div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(gpui_kit::black().opacity(0.45))
            .occlude()
            .child(
                div()
                    .id(ids::SETTINGS_VIEWER)
                    .test_support()
                    .w(px(560.))
                    .max_h(px(640.))
                    .p_4()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .rounded_lg()
                    .bg(theme.popover)
                    .text_color(theme.popover_foreground)
                    .border_1()
                    .border_color(theme.border)
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Photo viewer & file types"),
                    )
                    .child(self.viewer_option_row(&state.draft, cx))
                    .child(self.viewer_flag_row(&state.draft, cx))
                    .child(type_list(rows, cx))
                    .children(note.map(|text| {
                        div()
                            .id(ids::SETTINGS_NOTE)
                            .test_support()
                            .aria_label(text.clone())
                            .child(text)
                    }))
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap_2()
                            .child(
                                Button::new(ids::SETTINGS_RESTORE_DEFAULTS)
                                    .label("Restore previous defaults")
                                    .disabled(!can_restore)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.restore_viewer_defaults(cx);
                                    })),
                            )
                            .child(
                                Button::new(ids::SETTINGS_MAKE_DEFAULT)
                                    .label("Make Lightbox the default photo viewer")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.make_checked_types_default(cx);
                                    })),
                            )
                            .child(Button::new(ids::SETTINGS_CLOSE).label("Close").on_click(
                                cx.listener(|this, _, window, cx| {
                                    this.close_overlay(window, cx);
                                }),
                            )),
                    ),
            );
        scrim.into_any_element()
    }

    fn viewer_option_row(&mut self, draft: &ViewerSettings, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex()
            .flex_wrap()
            .gap_2()
            .child(
                Button::new(ids::SETTINGS_BACKDROP)
                    .compact()
                    .label(format!("Backdrop: {}", backdrop_name(draft.backdrop)))
                    .on_click(cx.listener(|this, _, _, cx| this.cycle_backdrop(cx))),
            )
            .child(
                Button::new(ids::SETTINGS_MOTION)
                    .compact()
                    .label(format!("Motion: {}", motion_name(draft.motion)))
                    .on_click(cx.listener(|this, _, _, cx| this.cycle_motion(cx))),
            )
            .child(
                Button::new(ids::SETTINGS_WHEEL)
                    .compact()
                    .label(format!("Wheel: {}", wheel_name(draft.wheel)))
                    .on_click(cx.listener(|this, _, _, cx| this.cycle_wheel(cx))),
            )
            .child(
                Button::new(ids::SETTINGS_SORT)
                    .compact()
                    .label(format!("Folder sort: {}", sort_name(draft.folder_sort)))
                    .on_click(cx.listener(|this, _, _, cx| this.cycle_sort(cx))),
            )
            .into_any_element()
    }

    fn viewer_flag_row(&mut self, draft: &ViewerSettings, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex()
            .flex_wrap()
            .gap_3()
            .child(flag(
                ids::SETTINGS_ENLARGE,
                "Enlarge small photos",
                draft.enlarge_small,
                cx,
                |draft, checked| draft.enlarge_small = checked,
            ))
            .child(flag(
                ids::SETTINGS_WRAP,
                "Wrap at ends",
                draft.wrap,
                cx,
                |draft, checked| draft.wrap = checked,
            ))
            .child(flag(
                ids::SETTINGS_FILMSTRIP,
                "Show filmstrip",
                draft.show_filmstrip,
                cx,
                |draft, checked| draft.show_filmstrip = checked,
            ))
            .child(flag(
                ids::SETTINGS_SINGLE_CLICK,
                "Single click opens",
                draft.single_click_opens,
                cx,
                |draft, checked| draft.single_click_opens = checked,
            ))
            .into_any_element()
    }

    fn cycle_backdrop(&mut self, cx: &mut Context<Self>) {
        self.update_draft(
            |draft| {
                draft.backdrop = match draft.backdrop {
                    BackdropMode::Auto => BackdropMode::DesktopBlur,
                    BackdropMode::DesktopBlur => BackdropMode::Ambient,
                    BackdropMode::Ambient => BackdropMode::Dim,
                    BackdropMode::Dim => BackdropMode::Auto,
                };
            },
            cx,
        );
    }

    fn cycle_motion(&mut self, cx: &mut Context<Self>) {
        self.update_draft(
            |draft| {
                draft.motion = match draft.motion {
                    MotionLevel::Auto => MotionLevel::Full,
                    MotionLevel::Full => MotionLevel::Reduced,
                    MotionLevel::Reduced => MotionLevel::Off,
                    MotionLevel::Off => MotionLevel::Auto,
                };
            },
            cx,
        );
    }

    fn cycle_wheel(&mut self, cx: &mut Context<Self>) {
        self.update_draft(
            |draft| {
                draft.wheel = match draft.wheel {
                    WheelAction::Zoom => WheelAction::Navigate,
                    WheelAction::Navigate => WheelAction::Zoom,
                };
            },
            cx,
        );
    }

    fn cycle_sort(&mut self, cx: &mut Context<Self>) {
        self.update_draft(
            |draft| {
                draft.folder_sort = match draft.folder_sort {
                    FolderSort::NameNatural => FolderSort::DateModified,
                    FolderSort::DateModified => FolderSort::DateTaken,
                    FolderSort::DateTaken => FolderSort::NameNatural,
                };
            },
            cx,
        );
    }

    fn update_draft(&mut self, change: impl FnOnce(&mut ViewerSettings), cx: &mut Context<Self>) {
        let Some(draft) = (match &mut self.settings_panel {
            Some(state) => {
                change(&mut state.draft);
                Some(state.draft.clone())
            }
            None => None,
        }) else {
            return;
        };
        self.persist_viewer(draft, cx);
    }

    fn set_type_checked(&mut self, ext: &str, checked: bool, cx: &mut Context<Self>) {
        let Some(draft) = (match &mut self.settings_panel {
            Some(state) => {
                if checked
                    && !state
                        .draft
                        .file_types
                        .iter()
                        .any(|item| item.eq_ignore_ascii_case(ext))
                {
                    state.draft.file_types.push(ext.to_owned());
                } else if !checked {
                    state
                        .draft
                        .file_types
                        .retain(|item| !item.eq_ignore_ascii_case(ext));
                }
                if let Some(row) = state
                    .rows
                    .iter_mut()
                    .find(|row| row.ext.eq_ignore_ascii_case(ext))
                {
                    row.checked = checked;
                }
                Some(state.draft.clone())
            }
            None => None,
        }) else {
            return;
        };
        self.persist_viewer(draft, cx);
    }

    fn persist_viewer(&self, draft: ViewerSettings, cx: &mut Context<Self>) {
        if let Err(error) = self
            .app
            .settings()
            .update(|settings| settings.viewer = draft)
        {
            tracing::warn!(%error, "could not save viewer settings");
        }
        cx.notify();
    }

    fn dismiss_default_prompt(&mut self, cx: &mut Context<Self>) {
        if let Err(error) = self
            .app
            .settings()
            .update(|settings| settings.viewer.default_prompt_dismissed = true)
        {
            tracing::warn!(%error, "could not dismiss the viewer prompt");
        }
        if let Some(state) = &mut self.settings_panel {
            state.draft.default_prompt_dismissed = true;
        }
        cx.notify();
    }

    fn banner_make_default(&mut self, cx: &mut Context<Self>) {
        let extensions = ViewerSettings::default().file_types;
        self.make_viewer_default(&extensions, cx);
        self.dismiss_default_prompt(cx);
    }

    fn make_checked_types_default(&mut self, cx: &mut Context<Self>) {
        let Some(extensions) = self
            .settings_panel
            .as_ref()
            .map(|state| state.draft.file_types.clone())
        else {
            return;
        };
        self.make_viewer_default(&extensions, cx);
    }

    fn make_viewer_default(&mut self, extensions: &[String], cx: &mut Context<Self>) {
        let formats = selected_formats(extensions);
        let outcome = if let Some(backend) = self.app.associations() {
            backend
                .make_default(&current_install(), &formats)
                .map(|result| (result.outcome, result.saved))
                .map_err(|error| error.to_string())
        } else {
            AssocService::make_default(extensions).map_err(|error| error.to_string())
        };
        match outcome {
            Ok((outcome, saved)) => {
                let note = outcome_note(outcome);
                self.store_saved_defaults(saved, false, Some(note), cx);
            }
            Err(message) => self.set_note(message, cx),
        }
    }

    fn restore_viewer_defaults(&mut self, cx: &mut Context<Self>) {
        let Some(saved) = self
            .settings_panel
            .as_ref()
            .and_then(|state| state.draft.saved_defaults.clone())
        else {
            return;
        };
        let outcome = if let Some(backend) = self.app.associations() {
            backend
                .restore_previous(&saved)
                .map_err(|error| error.to_string())
        } else {
            AssocService::restore(&saved).map_err(|error| error.to_string())
        };
        match outcome {
            Ok(()) => self.store_saved_defaults(
                None,
                true,
                Some("Previous defaults restored.".into()),
                cx,
            ),
            Err(message) => self.set_note(message, cx),
        }
    }

    fn store_saved_defaults(
        &mut self,
        saved: Option<SavedDefaults>,
        clear: bool,
        note: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if let Err(error) = self.app.settings().update(|settings| {
            if clear {
                settings.viewer.saved_defaults = None;
            } else if let Some(saved) = &saved {
                settings.viewer.saved_defaults = Some(saved.clone());
            }
        }) {
            tracing::warn!(%error, "could not store file-type defaults");
        }
        let draft = self.settings_panel.as_ref().map(|state| {
            let mut draft = state.draft.clone();
            if clear {
                draft.saved_defaults = None;
            } else if let Some(saved) = &saved {
                draft.saved_defaults = Some(saved.clone());
            }
            draft
        });
        let rows = draft.as_ref().map(|draft| self.type_rows(draft));
        if let Some(state) = &mut self.settings_panel {
            if let Some(draft) = draft {
                state.draft = draft;
            }
            if let Some(rows) = rows {
                state.rows = rows;
            }
            if note.is_some() {
                state.note = note;
            }
        }
        cx.notify();
    }

    fn set_note(&mut self, note: String, cx: &mut Context<Self>) {
        if let Some(state) = &mut self.settings_panel {
            state.note = Some(note);
        }
        cx.notify();
    }

    fn type_rows(&self, draft: &ViewerSettings) -> Vec<TypeRow> {
        let formats = AssocService::formats();
        let statuses = self.statuses(&formats);
        formats
            .into_iter()
            .map(|fmt| {
                let status = statuses
                    .iter()
                    .find(|(known, _)| known.ext == fmt.ext)
                    .map(|(_, status)| status_text(status))
                    .unwrap_or_else(|| "Not registered".to_owned());
                TypeRow {
                    ext: fmt.ext.to_owned(),
                    status,
                    checked: draft
                        .file_types
                        .iter()
                        .any(|ext| ext.eq_ignore_ascii_case(fmt.ext)),
                }
            })
            .collect()
    }

    fn statuses(&self, formats: &[FormatAssoc]) -> Vec<(FormatAssoc, AssocStatus)> {
        if let Some(backend) = self.app.associations() {
            return backend.status(formats).unwrap_or_else(|error| {
                tracing::warn!(%error, "could not read file-type status");
                Vec::new()
            });
        }
        AssocService::status().unwrap_or_else(|error| {
            tracing::warn!(%error, "could not read file-type status");
            Vec::new()
        })
    }
}

fn type_list(rows: Vec<TypeRow>, cx: &mut Context<LightboxWindow>) -> AnyElement {
    let mut list = div()
        .id(ids::SETTINGS_TYPE_LIST)
        .test_support()
        .flex()
        .flex_col()
        .gap_1()
        .max_h(px(240.))
        .overflow_y_scroll();
    for row in rows {
        let ext = row.ext.clone();
        let label = format!("{}  {}", row.ext, row.status);
        list = list.child(
            Checkbox::new(ids::assoc_type(&row.ext))
                .label(label)
                .checked(row.checked)
                .on_change(cx.listener(move |this, checked, _, cx| {
                    this.set_type_checked(&ext, *checked, cx);
                })),
        );
    }
    list.into_any_element()
}

fn flag(
    id: &'static str,
    label: &'static str,
    checked: bool,
    cx: &mut Context<LightboxWindow>,
    write: fn(&mut ViewerSettings, bool),
) -> Checkbox {
    Checkbox::new(id)
        .label(label)
        .checked(checked)
        .on_change(cx.listener(move |this, checked, _, cx| {
            let checked = *checked;
            this.update_draft(|draft| write(draft, checked), cx);
        }))
}

fn selected_formats(extensions: &[String]) -> Vec<FormatAssoc> {
    AssocService::formats()
        .into_iter()
        .filter(|fmt| {
            extensions
                .iter()
                .any(|ext| ext.eq_ignore_ascii_case(fmt.ext))
        })
        .collect()
}

fn current_install() -> AppInstall {
    AppInstall::current().unwrap_or_else(|_| AppInstall {
        exe: PathBuf::from("lightbox"),
        bundle_id: "app.lightbox.Lightbox".into(),
        display_name: "Lightbox Photo Viewer".into(),
        icon: None,
    })
}

fn outcome_note(outcome: MakeDefaultOutcome) -> String {
    match outcome {
        MakeDefaultOutcome::OpenedSystemSettings => {
            "Choose Lightbox Photo Viewer for the types you want.".to_owned()
        }
        MakeDefaultOutcome::UserConfirmationShown => {
            "Confirm Lightbox in the system dialog.".to_owned()
        }
        MakeDefaultOutcome::Done => {
            "Lightbox is now the default for the selected types.".to_owned()
        }
    }
}

fn status_text(status: &AssocStatus) -> String {
    match status {
        AssocStatus::Default => "Default".to_owned(),
        AssocStatus::Available => "Available".to_owned(),
        AssocStatus::OtherDefault(name) => format!("Default: {name}"),
        AssocStatus::Unregistered => "Not registered".to_owned(),
    }
}

fn backdrop_name(mode: BackdropMode) -> &'static str {
    match mode {
        BackdropMode::Auto => "auto",
        BackdropMode::DesktopBlur => "desktop_blur",
        BackdropMode::Ambient => "ambient",
        BackdropMode::Dim => "dim",
    }
}

fn motion_name(level: MotionLevel) -> &'static str {
    match level {
        MotionLevel::Auto => "auto",
        MotionLevel::Full => "full",
        MotionLevel::Reduced => "reduced",
        MotionLevel::Off => "off",
    }
}

fn wheel_name(action: WheelAction) -> &'static str {
    match action {
        WheelAction::Zoom => "zoom",
        WheelAction::Navigate => "navigate",
    }
}

fn sort_name(sort: FolderSort) -> &'static str {
    match sort {
        FolderSort::NameNatural => "name_natural",
        FolderSort::DateModified => "date_modified",
        FolderSort::DateTaken => "date_taken",
    }
}
