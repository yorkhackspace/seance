//! `app`
//!
//! Contains the entry point for the egui APP.

mod preview;
pub use preview::{render_task, RenderRequest};

use std::{
    collections::HashMap,
    fs,
    hash::{self, DefaultHasher, Hash, Hasher},
    path::PathBuf,
    sync::{Arc, Mutex, RwLock},
    time::Duration,
};

use egui::{
    Align, Color32, Frame, Key, Label, Layout, Margin, Pos2, Rect, RichText, ScrollArea, Sense,
    Slider, Stroke, TextEdit, Vec2, Visuals, WidgetText,
};
use egui_dnd::{dnd, DragDropConfig};
use egui_extras::{Size, StripBuilder};
use preview::{DesignPreview, MAX_ZOOM_LEVEL, MIN_ZOOM_LEVEL};

use seance::{
    cut_file, default_passes,
    svg::{parse_svg, SVG_UNITS_PER_MM},
    DesignFile, PrintDevice, SendToDeviceError, ToolPass, BED_HEIGHT_MM, BED_WIDTH_MM,
};

/// `DesignFile` with a hash and original path attached.
type DesignWithMeta = (seance::DesignFile, u64, PathBuf);

/// The minimum amount that a design can be moved by.
const MINIMUM_DEFAULT_DESIGN_MOVE_STEP_MM: f32 = 0.1;
/// The default amount that designs are moved by.
const DEFAULT_DESIGN_MOVE_STEP_MM: f32 = 10.0;
/// The maximum amount that designs can be moved by.
const MAXIMUM_DESIGN_MOVE_STEP_MM: f32 = 500.0;

#[cfg(target_os = "windows")]
use crate::USBPort;

/// Data that is saved between uses of Seance.
#[derive(serde::Deserialize, serde::Serialize)]
struct PersistentStorage {
    /// Whether the UI should be dark mode.
    dark_mode: bool,
    /// The tool passes to run on the machine.
    passes: Vec<ToolPass>,
    /// The print device configuration.
    print_device: PrintDevice,
    /// How much to move the design by each time a movement button is pressed.
    design_move_step_mm: f32,
}

/// The Seance UI app.
pub struct Seance {
    /// Whether the UI should be dark mode.
    dark_mode: bool,
    /// The tool passes to run on the machine.
    passes: Vec<ToolPass>,
    /// The print device configuration.
    print_device: PrintDevice,

    /// The currently open design file, if any.
    design_file: Arc<RwLock<Option<DesignWithMeta>>>,
    /// The message channel that will receive UI events.
    ui_message_tx: UIMessageTx,
    /// The message channel that UI events will be sent into.
    ui_message_rx: UIMessageRx,
    /// Where to put requests to re-render the design preview.
    render_request: Arc<Mutex<Option<RenderRequest>>>,
    /// The hasher to use to calculate the hash of the design file.
    hasher: Box<dyn Hasher>,
    /// Amount to move the design by when moving.
    design_move_step_mm: f32,

    /// The states of all of the tool pass widgets.
    tool_pass_widget_states: Vec<ToolPassWidgetState>,
    /// The widgets that were created on the previous frame, used for
    /// handling tab/arrow-key/enter-key events.
    previous_frame_widgets: HashMap<egui::Id, SeanceUIElement>,
    /// The zoom level of the design preview.
    preview_zoom_level: f32,

    /// The file dialog that is currently open, if any.
    /// Used for e.g. opening files/saving files.
    file_dialog: Option<FileDialog>,
    /// The error that is currently being displayed.
    /// (Error, Details).
    current_error: Option<(String, Option<String>)>,
    /// The current preview for the design. Used to cache the previews
    /// across draws of the UI.
    design_preview_image: Option<DesignPreview>,
    /// The settings dialog, if it is currently open.
    settings_dialog: Option<SettingsDialogState>,
}

/// The state of the settings dialog. Data here is ephemiral and must explicitly be saved when required.
struct SettingsDialogState {
    /// The device that we will be using to "print" the design.
    print_device: PrintDevice,
}

impl SettingsDialogState {
    /// Creates a new [`SettingsDialogState`].
    ///
    /// # Arguments
    /// * `print_device`: The device to print to.
    ///
    /// # Returns
    /// A new [`SettingsDialogState`].
    fn new(print_device: PrintDevice) -> Self {
        Self { print_device }
    }
}

/// A message channel that UI events are sent into.
type UIMessageTx = std::sync::mpsc::Sender<UIMessage>;
/// A message channel that UI events can be received from.
type UIMessageRx = std::sync::mpsc::Receiver<UIMessage>;

impl Seance {
    /// Creates a new instance of the [`Seance`] UI.
    ///
    /// # Arguments
    /// * `cc`: An eframe creation context.
    /// * `render_request`: Where to put requests to re-render the design preview.
    ///
    /// # Returns
    /// A new instance of the [`Seance`] UI.
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        render_request: Arc<Mutex<Option<RenderRequest>>>,
    ) -> Self {
        let default_pens = default_passes::default_passes();
        let (ui_message_tx, ui_message_rx) = std::sync::mpsc::channel();

        if let Some(storage) = cc.storage {
            let seance_storage: PersistentStorage = eframe::get_value(storage, eframe::APP_KEY)
                .unwrap_or(PersistentStorage {
                    dark_mode: cc.egui_ctx.style().visuals.dark_mode,
                    passes: default_pens,
                    print_device: PrintDevice::default(),
                    design_move_step_mm: DEFAULT_DESIGN_MOVE_STEP_MM,
                });
            if seance_storage.dark_mode {
                cc.egui_ctx.set_visuals(Visuals::dark());
            } else {
                cc.egui_ctx.set_visuals(Visuals::light());
            }

            let laser_pass_widget_states: Vec<ToolPassWidgetState> = seance_storage
                .passes
                .iter()
                .map(|pass| {
                    ToolPassWidgetState::new(Default::default(), pass.power(), pass.speed())
                })
                .collect::<Vec<_>>();

            return Seance {
                dark_mode: seance_storage.dark_mode,
                passes: seance_storage.passes,
                print_device: seance_storage.print_device,

                design_file: Default::default(),
                ui_message_tx,
                ui_message_rx,
                render_request,
                hasher: Box::new(DefaultHasher::new()),
                design_move_step_mm: seance_storage.design_move_step_mm,

                tool_pass_widget_states: laser_pass_widget_states,
                previous_frame_widgets: Default::default(),
                preview_zoom_level: MIN_ZOOM_LEVEL,
                file_dialog: None,
                current_error: None,
                design_preview_image: None,
                settings_dialog: None,
            };
        }

        let laser_passes_widget_states: Vec<ToolPassWidgetState> = default_pens
            .iter()
            .map(|pass| ToolPassWidgetState::new(Default::default(), pass.power(), pass.speed()))
            .collect::<Vec<_>>();

        Seance {
            dark_mode: cc.egui_ctx.style().visuals.dark_mode,
            passes: default_pens,
            print_device: PrintDevice::default(),

            design_file: Default::default(),
            ui_message_tx,
            ui_message_rx,
            render_request,
            hasher: Box::new(DefaultHasher::new()),
            design_move_step_mm: DEFAULT_DESIGN_MOVE_STEP_MM,

            tool_pass_widget_states: laser_passes_widget_states,
            previous_frame_widgets: Default::default(),
            preview_zoom_level: MIN_ZOOM_LEVEL,
            file_dialog: None,
            current_error: None,
            design_preview_image: None,
            settings_dialog: None,
        }
    }

    fn handle_ui_messages(&mut self, ctx: &egui::Context) {
        while let Ok(msg) = self.ui_message_rx.try_recv() {
            match msg {
                UIMessage::ShowOpenFileDialog => {
                    if self.file_dialog.is_none() {
                        let (tx, rx) = oneshot::channel();
                        let _ = std::thread::spawn(|| {
                            let file = rfd::FileDialog::new()
                                .set_title("Select Design File")
                                .add_filter("Supported Files", &all_capitalisations_of("svg"))
                                .add_filter("All Files", &["*"])
                                .pick_file();
                            let _ = tx.send(file);
                        });
                        self.file_dialog = Some(FileDialog::OpenDesign { rx });
                    }
                }
                UIMessage::ShowOpenToolPathSettingsDialog => {
                    if self.file_dialog.is_none() {
                        let (tx, rx) = oneshot::channel();
                        let _ = std::thread::spawn(|| {
                            let file = rfd::FileDialog::new()
                                .set_title("Select Settings File")
                                .add_filter("Supported Files", &all_capitalisations_of("json"))
                                .add_filter("All Files", &["*"])
                                .pick_file();
                            let _ = tx.send(file);
                        });
                        self.file_dialog = Some(FileDialog::OpenToolPaths { rx });
                    }
                }
                UIMessage::ShowExportToolPathSettingsDialog => {
                    let passes = self.passes.clone();
                    let (tx, rx) = oneshot::channel();
                    let ui_message_tx = self.ui_message_tx.clone();
                    let _ = std::thread::spawn(move || {
                        if let Some(mut path) = rfd::FileDialog::new()
                            .set_title("Export Laser Settings")
                            .add_filter("Supported Files", &all_capitalisations_of("json"))
                            .add_filter("All Files", &["*"])
                            .save_file()
                        {
                            if let Some(ext) = path.extension() {
                                if !ext.eq_ignore_ascii_case("json") {
                                    path.set_extension("json");
                                }
                            } else {
                                path.set_extension("json");
                            }

                            if let Ok(json_string) = serde_json::to_string(&passes) {
                                if let Err(err) = fs::write(path, json_string) {
                                    let _ = ui_message_tx.send(UIMessage::ShowError {
                                        error: "Could not open export dialog".to_string(),
                                        details: Some(format!("I/O error: {err:?}")),
                                    });
                                }
                            }
                        }

                        let _ = tx.send(());
                    });
                    self.file_dialog = Some(FileDialog::ExportToolPaths { rx });
                }
                UIMessage::ShowError { error, details } => {
                    self.current_error = Some((error, details));
                }
                UIMessage::CloseErrorDialog => {
                    let _ = self.current_error.take();
                }
                UIMessage::ShowSettingsDialog => {
                    self.settings_dialog = Some(SettingsDialogState::new(self.print_device.clone()))
                }
                UIMessage::PrinterSettingsChanged { printer } => {
                    if let Some(dialog) = &mut self.settings_dialog {
                        dialog.print_device = printer;
                    }
                }
                UIMessage::SaveSettings => {
                    if let Some(dialog) = &self.settings_dialog {
                        self.print_device = dialog.print_device.clone();
                    }
                }
                UIMessage::CloseSettingsDialog => {
                    self.settings_dialog = None;
                }
                UIMessage::DesignFileChanged { design_file } => {
                    let Ok(mut design_lock) = self.design_file.write() else {
                        let _ = self.ui_message_tx.send(UIMessage::ShowError {
                            error: "Could not store design file".to_string(),
                            details: Some("Unable to write to design file store".to_string()),
                        });
                        continue;
                    };

                    *design_lock = Some(design_file);
                    if let Some(preview) = &mut self.design_preview_image {
                        preview.render(&self.design_file);
                    }
                }
                UIMessage::ToolPassesListChanged { passes } => {
                    self.passes = passes;
                }
                UIMessage::ToolPassNameChanged { index, name } => {
                    if let Some(pass) = self.passes.get_mut(index) {
                        pass.set_name(name);
                    }
                }
                UIMessage::ToolPassPowerChanged { index, power } => {
                    if let Some(pass) = self.passes.get_mut(index) {
                        pass.set_power(power);
                    }
                }
                UIMessage::ToolPassSpeedChanged { index, speed } => {
                    if let Some(pass) = self.passes.get_mut(index) {
                        pass.set_speed(speed);
                    }
                }
                UIMessage::ToolPassColourChanged { index, colour } => {
                    if let Some(pass) = self.passes.get_mut(index) {
                        pass.set_colour(colour);
                    }
                }
                UIMessage::ToolPassNameClicked { index } => {
                    if let Some(pass) = self.tool_pass_widget_states.get_mut(index) {
                        pass.editing = ToolPassWidgetEditing::Name;
                    }
                }
                UIMessage::ToolPassNameLostFocus => {
                    focus_changing(
                        ctx,
                        &self.previous_frame_widgets,
                        &mut self.tool_pass_widget_states,
                        &self.ui_message_tx,
                    );
                }
                UIMessage::ToolPassPowerClicked { index } => {
                    if let Some(pass) = self.tool_pass_widget_states.get_mut(index) {
                        pass.editing = ToolPassWidgetEditing::Power;
                    }
                }
                UIMessage::ToolPassPowerLostFocus => {
                    focus_changing(
                        ctx,
                        &self.previous_frame_widgets,
                        &mut self.tool_pass_widget_states,
                        &self.ui_message_tx,
                    );
                }
                UIMessage::ToolPassSpeedClicked { index } => {
                    if let Some(pass) = self.tool_pass_widget_states.get_mut(index) {
                        pass.editing = ToolPassWidgetEditing::Speed;
                    }
                }
                UIMessage::ToolPassSpeedLostFocus => {
                    focus_changing(
                        ctx,
                        &self.previous_frame_widgets,
                        &mut self.tool_pass_widget_states,
                        &self.ui_message_tx,
                    );
                }
                UIMessage::ToolPassEnableChanged { index, enabled } => {
                    if let Some(pass) = self.passes.get_mut(index) {
                        pass.set_enabled(enabled);
                    }
                }
                UIMessage::PreviewZoomLevelChanged { zoom } => {
                    self.preview_zoom_level = zoom.min(MAX_ZOOM_LEVEL).max(MIN_ZOOM_LEVEL);
                    if let Some(preview) = &mut self.design_preview_image {
                        preview.zoom(self.preview_zoom_level);
                    }
                }
                UIMessage::DesignPreviewSize { size_before_wrap } => {
                    let resize = self.design_preview_image.is_some();
                    let preview = self.design_preview_image.get_or_insert_with(|| {
                        DesignPreview::new(
                            size_before_wrap,
                            self.preview_zoom_level,
                            &self.design_file,
                            self.render_request.clone(),
                        )
                    });
                    if resize {
                        preview.resize(size_before_wrap, &self.design_file);
                    }
                }
                UIMessage::DesignMoveStepChanged { step } => {
                    self.design_move_step_mm = step;
                }
                UIMessage::MoveDesign { direction, step } => {
                    if let Some(preview) = &mut self.design_preview_image {
                        let new_offset = direction.apply(preview.get_design_offset(), step);
                        preview.set_design_offset(new_offset, &self.design_file);
                    }
                }
                UIMessage::ResetDesignPosition => {
                    if let Some(preview) = &mut self.design_preview_image {
                        preview.set_design_offset(Default::default(), &self.design_file);
                    }
                }
                UIMessage::EnterKeyPressed => {
                    focus_changing(
                        ctx,
                        &self.previous_frame_widgets,
                        &mut self.tool_pass_widget_states,
                        &self.ui_message_tx,
                    );
                }
                UIMessage::TabKeyPressed => {
                    focus_changing(
                        ctx,
                        &self.previous_frame_widgets,
                        &mut self.tool_pass_widget_states,
                        &self.ui_message_tx,
                    );
                }
                UIMessage::SpaceKeyPressed => {
                    focus_changing(
                        ctx,
                        &self.previous_frame_widgets,
                        &mut self.tool_pass_widget_states,
                        &self.ui_message_tx,
                    );
                }
            }
        }
    }
}

impl eframe::App for Seance {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(
            storage,
            eframe::APP_KEY,
            &PersistentStorage {
                dark_mode: self.dark_mode,
                passes: self.passes.clone(),
                print_device: self.print_device.clone(),
                design_move_step_mm: self.design_move_step_mm,
            },
        );
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_ui_messages(ctx);

        if !FileDialog::poll(&self.file_dialog, &self.ui_message_tx, &mut self.hasher) {
            let _ = self.file_dialog.take();
        }

        if let Some((err, details)) = &self.current_error {
            error_dialog(ctx, &self.ui_message_tx, err, details);
        }

        if let Some(settings) = &self.settings_dialog {
            settings_dialog(ctx, &self.ui_message_tx, settings);
        }

        self.previous_frame_widgets = Default::default();

        // Slow down key presses to make typing bearable.
        std::thread::sleep(Duration::from_millis(10));

        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                // NOTE: no File->Quit on web pages!
                let is_web = cfg!(target_arch = "wasm32");
                if !is_web {
                    ui.menu_button("File", |ui| {
                        if ui.button("Settings").clicked() {
                            let _ = self.ui_message_tx.send(UIMessage::ShowSettingsDialog);
                        }

                        if ui.button("Quit").clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });
                    ui.add_space(16.0);
                }

                egui::global_dark_light_mode_buttons(ui);

                if ui.style().visuals.dark_mode != self.dark_mode {
                    self.dark_mode = ui.style().visuals.dark_mode;
                }
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            StripBuilder::new(ui)
                .size(Size::exact(20.0))
                .size(Size::remainder())
                .vertical(|mut strip| {
                    strip.cell(|ui| {
                        Frame::default()
                            .outer_margin(Margin {
                                left: 0.0,
                                right: 0.0,
                                top: 0.0,
                                bottom: ui.style().spacing.menu_margin.bottom,
                            })
                            .show(ui, |ui| {
                                toolbar_widget(
                                    ui,
                                    &self.design_file,
                                    &self.passes,
                                    &self.print_device,
                                    &self
                                        .design_preview_image
                                        .as_ref()
                                        .map(|preview| preview.get_design_offset())
                                        .cloned()
                                        .unwrap_or_default(),
                                    &self.ui_message_tx,
                                );
                            });
                    });
                    strip.cell(|ui| {
                        ui_main(
                            ui,
                            &mut self.passes,
                            &mut self.tool_pass_widget_states,
                            &mut self.previous_frame_widgets,
                            &self.design_file,
                            &mut self.design_preview_image,
                            self.preview_zoom_level,
                            self.design_move_step_mm,
                            &self.ui_message_tx,
                        );
                    });
                });
        });

        // Handle events.
        ctx.input(|i| {
            // Handle dropped files.
            if !i.raw.dropped_files.is_empty() {
                if let Some(path) = &i.raw.dropped_files[0].path {
                    match load_design(path, &mut self.hasher) {
                        Ok(file) => {
                            let _ = self
                                .ui_message_tx
                                .send(UIMessage::DesignFileChanged { design_file: file });
                        }
                        Err(err) => {
                            let _ = self.ui_message_tx.send(UIMessage::ShowError {
                                error: "Failed to load design".to_string(),
                                details: Some(err),
                            });
                        }
                    }
                }
            }

            if i.key_pressed(Key::Enter) {
                let _ = self.ui_message_tx.send(UIMessage::EnterKeyPressed);
            }

            if i.key_pressed(Key::Tab) {
                let _ = self.ui_message_tx.send(UIMessage::TabKeyPressed);
            }

            if i.key_pressed(Key::Space) {
                let _ = self.ui_message_tx.send(UIMessage::SpaceKeyPressed);
            }
        });

        // We need to redraw the UI until the design preview has finished rendering,
        // otherwise the user may be left very frustrated that it is taking a while to render.
        if let Some(preview) = &self.design_preview_image {
            if preview.is_rendering() {
                ctx.request_repaint();
            }
        }
    }
}

/// Events that can be sent by UI components.
enum UIMessage {
    /// We want to show the dialog to open a design file.
    ShowOpenFileDialog,
    /// We want to show the dialog to open a tool path settings file.
    ShowOpenToolPathSettingsDialog,
    /// We want to show the dialog to export tool path settings.
    ShowExportToolPathSettingsDialog,
    /// An error has occurred and should be shown to the user.
    /// This only needs to be sent when the error initially occurrs,
    /// it should not be sent on re-render of the app.
    ShowError {
        /// The error to display to the user.
        error: String,
        /// Details about the error that may help to diagnose the issue.
        details: Option<String>,
    },
    /// The error dialog should be closed.
    CloseErrorDialog,
    /// We want to show the settings dialog.
    ShowSettingsDialog,
    /// The printer settings have changed.
    /// This only affects the state of the settings dialog, it does not save the settings.
    PrinterSettingsChanged {
        /// The device we should use to as our printer-like device.
        printer: PrintDevice,
    },
    /// The current state of the settings dialog should be applied to the app state.
    SaveSettings,
    /// The settings dialog should be closed.
    CloseSettingsDialog,
    /// A new design file has been loaded.
    DesignFileChanged {
        /// The design file that has been loaded.
        design_file: DesignWithMeta,
    },
    /// The list of tool passes have changed.
    /// This is used when the tool passes are imported, for example.
    /// It is not used for changes to individual options made on individual tool passes.
    ToolPassesListChanged {
        /// The new list of tool passes.
        passes: Vec<ToolPass>,
    },
    /// The name of a tool pass has changed.
    ToolPassNameChanged {
        /// The index of the tool pass that has changed.
        index: usize,
        /// The new name of the tool pass.
        name: String,
    },
    /// The power of a tool pass has changed.
    ToolPassPowerChanged {
        /// The index of the tool pass that has changed.
        index: usize,
        /// The new power of the tool pass.
        power: u64,
    },
    /// The speed of a tool pass has changed.
    ToolPassSpeedChanged {
        /// The index of the tool pass that has changed.
        index: usize,
        /// The new speed of the tool pass.
        speed: u64,
    },
    /// The colour associated with a tool pass has changed.
    ToolPassColourChanged {
        /// The index of the tool pass that has changed.
        index: usize,
        /// The new colour of associated with the tool pass.
        colour: [u8; 3],
    },
    /// The name of a tool pass has been clicked.
    ToolPassNameClicked {
        /// The index of the tool pass that was clicked.
        index: usize,
    },
    /// The name of a tool pass has lost focus.
    ToolPassNameLostFocus,
    /// The power of a tool pass has been clicked.
    ToolPassPowerClicked {
        /// The index of the tool pass that was clicked.
        index: usize,
    },
    /// The power of a tool pass has lost focus.
    ToolPassPowerLostFocus,
    /// The speed of a tool pass has been clicked.
    ToolPassSpeedClicked {
        /// The index of the tool pass that was clicked.
        index: usize,
    },
    /// The speed of a tool pass has lost focus.
    ToolPassSpeedLostFocus,
    ToolPassEnableChanged {
        index: usize,
        enabled: bool,
    },
    /// The zoom level of the design preview has changed.
    PreviewZoomLevelChanged {
        zoom: f32,
    },
    /// This event is emitted when we know how large the design preview area is (e.g. after UI resize).
    DesignPreviewSize {
        /// The size available for the design preview.
        size_before_wrap: egui::Vec2,
    },
    /// The amount to move the design by has changed.
    DesignMoveStepChanged {
        /// The new step amount, in mm.
        step: f32,
    },
    /// Move the design around the bed.
    ///
    /// In the case of diagonal moves, the step specifies the amount of diagonal distance that will be moved.
    MoveDesign {
        /// The direction in which to move the design.
        direction: DesignMoveDirection,
        /// The amount to move the design in mm.
        step: f32,
    },
    /// Reset the design to align with the top-left edge.
    ResetDesignPosition,
    /// The enter key has been pressed.
    EnterKeyPressed,
    /// The tab key has been pressed.
    TabKeyPressed,
    /// The space key has been pressed.
    SpaceKeyPressed,
}

/// Types of UI element that we want to track interactivity for.
enum SeanceUIElement {
    /// The label for a tool pass name.
    NameLabel {
        /// The index of the tool pass for which this is the name label.
        index: usize,
    },
    /// The label for a tool pass power.
    PowerLabel {
        /// The index of the tool pass for which this is the power label.
        index: usize,
    },
    /// The label for a tool pass speed.
    SpeedLabel {
        /// The index of the tool pass for which this is the speed label.
        index: usize,
    },
}

/// The directions in which the design can be moved.
enum DesignMoveDirection {
    /// Up and then left.
    UpAndLeft,
    /// Up.
    Up,
    /// Up and then right.
    UpAndRight,
    /// Left.
    Left,
    /// Right.
    Right,
    /// Down and then left.
    DownAndLeft,
    /// Down.
    Down,
    /// Down and then right.
    DownAndRight,
}

impl DesignMoveDirection {
    /// Apply this move to an offset (in mm), returning the new offset.
    ///
    /// Offsets are defined such that +x is more right and +y is more down.
    ///
    /// # Arguments
    /// * `current_offset`: The offset to apply the move to.
    /// * `step_mm`: The amount to move by, in mm.
    ///
    /// # Returns
    /// A new offset, in mm.
    pub fn apply(&self, current_offset: &egui::Vec2, step_mm: f32) -> egui::Vec2 {
        let mut offset = current_offset.clone();
        match self {
            DesignMoveDirection::UpAndLeft => {
                let step_each_side = step_mm / (2.0f32.sqrt());
                offset.x -= step_each_side;
                offset.y -= step_each_side;
            }
            DesignMoveDirection::Up => {
                offset.y -= step_mm;
            }
            DesignMoveDirection::UpAndRight => {
                let step_each_side = step_mm / (2.0f32.sqrt());
                offset.x += step_each_side;
                offset.y -= step_each_side;
            }
            DesignMoveDirection::Left => {
                offset.x -= step_mm;
            }
            DesignMoveDirection::Right => {
                offset.x += step_mm;
            }
            DesignMoveDirection::DownAndLeft => {
                let step_each_side = step_mm / (2.0f32.sqrt());
                offset.x -= step_each_side;
                offset.y += step_each_side;
            }
            DesignMoveDirection::Down => {
                offset.y += step_mm;
            }
            DesignMoveDirection::DownAndRight => {
                let step_each_side = step_mm / (2.0f32.sqrt());
                offset.x += step_each_side;
                offset.y += step_each_side;
            }
        }

        offset
    }
}

/// The types of file dialog that can be opened.
enum FileDialog {
    /// A file dialog for opening design files.
    OpenDesign {
        /// The channel that the selected file will be received from, or `None` if no file was selected.
        rx: oneshot::Receiver<Option<PathBuf>>,
    },
    /// A file dialog for opening a tool path settings file.
    OpenToolPaths {
        /// The channel that the selected file will be received from, or `None` if no file was selected.
        rx: oneshot::Receiver<Option<PathBuf>>,
    },
    /// A file dialog for exporting tool path settings to a file.
    ExportToolPaths {
        /// The channel that the selected file will be received from, or `None` if no file was selected.
        rx: oneshot::Receiver<()>,
    },
}

impl FileDialog {
    /// Poll the file dialog, to see whether a file has been selected or whether the dialog has been cancelled.
    ///
    /// # Arguments
    /// * `dialog`: The file dialog to poll.
    /// * `ui_message_tx`: The channel that messages will be sent into according to the interaction the user has with the file dialog.
    /// * `hasher`: Hasher that can be used to get the hash of files.
    ///
    /// # Returns
    /// Whether the file dialog should be kept (`true`) or destroyed (`false`).
    fn poll(
        dialog: &Option<FileDialog>,
        ui_message_tx: &UIMessageTx,
        hasher: &mut Box<dyn hash::Hasher>,
    ) -> bool {
        let mut keep_dialog = true;
        if let Some(dialog) = dialog {
            match dialog {
                FileDialog::OpenDesign { rx } => match rx.try_recv() {
                    Ok(path) => {
                        keep_dialog = false;

                        if let Some(path) = path {
                            match load_design(&path, hasher) {
                                Ok(file) => {
                                    let _ = ui_message_tx
                                        .send(UIMessage::DesignFileChanged { design_file: file });
                                }
                                Err(err) => {
                                    let _ = ui_message_tx.send(UIMessage::ShowError {
                                        error: "Failed to load design".to_string(),
                                        details: Some(err),
                                    });
                                }
                            }
                        }
                    }
                    Err(oneshot::TryRecvError::Disconnected) => {
                        keep_dialog = false;
                    }
                    Err(oneshot::TryRecvError::Empty) => {}
                },
                FileDialog::OpenToolPaths { rx } => match rx.try_recv() {
                    Ok(path) => {
                        keep_dialog = false;

                        if let Some(path) = path {
                            match Self::handle_open_tool_paths(&path) {
                                Ok(passes) => {
                                    let _ = ui_message_tx
                                        .send(UIMessage::ToolPassesListChanged { passes });
                                }
                                Err(err) => {
                                    let _ = ui_message_tx.send(UIMessage::ShowError {
                                        error: err,
                                        details: None,
                                    });
                                }
                            }
                        }
                    }
                    Err(oneshot::TryRecvError::Disconnected) => {
                        keep_dialog = false;
                    }
                    Err(oneshot::TryRecvError::Empty) => {}
                },
                FileDialog::ExportToolPaths { rx } => match rx.try_recv() {
                    Ok(_) | Err(oneshot::TryRecvError::Disconnected) => {
                        keep_dialog = false;
                    }
                    Err(oneshot::TryRecvError::Empty) => {}
                },
            }
        }

        keep_dialog
    }

    /// Handle opening a settings file.
    ///
    /// # Arguments
    /// * `path`: The path to the settings file to open.
    ///
    /// # Returns
    /// Loaded tool passes, otherwise an error string.
    fn handle_open_tool_paths(path: &PathBuf) -> Result<Vec<ToolPass>, String> {
        let Some(extension) = path.extension() else {
            return Err("File does not have a file extension".to_string());
        };

        if !extension.eq_ignore_ascii_case("json") {
            return Err(format!(
                "Unrecognised extension {}",
                extension.to_string_lossy()
            ));
        }

        let Ok(bytes) = fs::read(path) else {
            return Err("Could not load file".to_string());
        };

        let Ok(json_string) = String::from_utf8(bytes) else {
            return Err("Could not decode file".to_string());
        };

        let Ok(passes) = serde_json::from_str::<Vec<ToolPass>>(&json_string) else {
            return Err("Could not load tool passes from file".to_string());
        };

        Ok(passes)
    }
}

/// Renders the toolbar widget.
///
/// # Arguments
/// * `ui`: The UI to draw the widget into.
/// * `design_file`: The currently loaded design file, if any.
/// * `tool_passes`: The current passes of the tool.
/// * `print_device`: The device to use as our "printer".
/// * `offset`: How much to move the design by relative to its starting position, in mm, where +x is more right and +y is more down.
/// * `ui_message_tx`: Channel that can be used to send events.
///
/// # Returns
/// An [`egui::Response`].
fn toolbar_widget(
    ui: &mut egui::Ui,
    design_file: &Arc<RwLock<Option<(DesignFile, u64, PathBuf)>>>,
    tool_passes: &Vec<ToolPass>,
    print_device: &PrintDevice,
    offset: &Vec2,
    ui_message_tx: &UIMessageTx,
) -> egui::Response {
    StripBuilder::new(ui)
        .sizes(Size::remainder(), 2)
        .horizontal(|mut strip| {
            strip.cell(|ui| {
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    if ui.button("Open Design").clicked() {
                        let _ = ui_message_tx.send(UIMessage::ShowOpenFileDialog);
                    }

                    if ui.button("Import Laser Settings").clicked() {
                        let _ = ui_message_tx.send(UIMessage::ShowOpenToolPathSettingsDialog);
                    }

                    if ui.button("Export Laser Settings").clicked() {
                        let _ = ui_message_tx.send(UIMessage::ShowExportToolPathSettingsDialog);
                    }
                });
            });

            strip.cell(|ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let hover_text = if print_device.is_valid() {
                        "Sends your design to the laser cutter. You will need to press Start on the laser cutter after sending."
                    } else {
                        "No valid laser cutter has been configured, please configure in settings. Note: This button may be disabled due to being unable to access the configured device."
                    };
                    let button = egui::Button::new("Send to Laser");
                    if ui.add_enabled(print_device.is_valid(), button).on_hover_text(hover_text).clicked() {
                        if let Ok(design_lock) = design_file.read() {
                            if let Some(file) = &*design_lock {
                                if let Err(err) = cut_file(&file.0, tool_passes, print_device, (offset.x, offset.y)) {
                                    handle_cut_file_error(err, ui_message_tx);
                                }
                            }
                        }
                    }
                });
            });
        })
}

/// Handle an error produced when trying to cut a design file.
///
/// # Arguments
/// * `err`: The error that was produced.
/// * `ui_message_tx`: Channel into which UI events can be sent.
fn handle_cut_file_error(err: SendToDeviceError, ui_message_tx: &UIMessageTx) {
    log::error!("Error cutting design: {err:?}");
    let (error, details) = match err {
        SendToDeviceError::ErrorParsingSvg(error) => (
            "Error processing design".to_string(),
            format!("Error from SVG parsing library: {error}"),
        ),
        SendToDeviceError::FailedToOpenPrinter(err) => (
            "Error opening printer".to_string(),
            format!("I/O error: {err:?}"),
        ),
        SendToDeviceError::FailedToWriteToPrinter(err) => (
            "Error writing to printer".to_string(),
            format!("I/O error: {err:?}"),
        ),
    };
    let _ = ui_message_tx.send(UIMessage::ShowError {
        error,
        details: Some(details),
    });
}

/// Draws the main UI (tool paths and design preview).
///
/// # Arguments
/// * `ui`: The UI to draw the widget to.
/// * `tool_passes`: The passes of the tool head.
/// * `tool_pass_widget_states`: Current states of tool pass widgets.
/// * `frame_widgets`: Map of widgets being drawn this frame.
/// * `design_file`: The loaded design file, if any.
/// * `design_preview_image`: The preview image to draw to the UI.
/// * `preview_zoom_level`: How much the preview image is zoomed in.
/// * `design_move_step_mm`: The current amount to step the design by when moving it.
/// * `ui_message_tx`: Channel into which UI events can be sent.
fn ui_main(
    ui: &mut egui::Ui,
    tool_passes: &mut Vec<ToolPass>,
    tool_pass_widget_states: &mut Vec<ToolPassWidgetState>,
    frame_widgets: &mut HashMap<egui::Id, SeanceUIElement>,
    design_file: &Arc<RwLock<Option<DesignWithMeta>>>,
    design_preview_image: &mut Option<DesignPreview>,
    preview_zoom_level: f32,
    design_move_step_mm: f32,
    ui_message_tx: &UIMessageTx,
) {
    StripBuilder::new(ui)
        .size(Size::relative(0.2).at_least(525.0))
        .size(Size::remainder())
        .horizontal(|mut strip| {
            strip.cell(|ui| {
                tool_passes_widget(
                    ui,
                    tool_passes,
                    tool_pass_widget_states,
                    frame_widgets,
                    ui_message_tx,
                );
            });
            strip.cell(|ui| {
                let ratio = BED_HEIGHT_MM / BED_WIDTH_MM;
                let mut width = ui.available_width();
                let mut height = width * ratio;
                let max_height = ui.available_height() * 0.8;
                if height > max_height {
                    height = max_height;
                    width = height / ratio;
                }

                StripBuilder::new(ui)
                    .size(Size::exact(height))
                    .size(Size::remainder())
                    .vertical(|mut strip| {
                        strip.cell(|ui| {
                            // Design Preview.
                            Frame::default().fill(Color32::LIGHT_GRAY).show(ui, |ui| {
                                design_file_widget(
                                    ui,
                                    design_file,
                                    design_preview_image,
                                    ui_message_tx,
                                    egui::Vec2 {
                                        x: width,
                                        y: height,
                                    },
                                );
                            });
                        });
                        strip.cell(|ui| {
                            ui.horizontal(|ui| {
                                let mut zoom_value = preview_zoom_level;
                                let zoom_widget =
                                    Slider::new(&mut zoom_value, MIN_ZOOM_LEVEL..=MAX_ZOOM_LEVEL);
                                ui.label("Zoom");
                                if ui.add(zoom_widget).changed() {
                                    let _ =
                                        ui_message_tx.send(UIMessage::PreviewZoomLevelChanged {
                                            zoom: zoom_value,
                                        });
                                }
                            });
                            ui.separator();
                            ui.label("Position Design");
                            ui.horizontal(|ui| {
                                ui.vertical(|ui| {
                                    const GRID_WIDTH: usize = 3;
                                    const GRID_HEIGHT: usize = 3;
                                    // Buttons to be displayed along with their tooltips and associated events.
                                    let buttons: [(&str, &str, UIMessage);
                                        GRID_WIDTH * GRID_HEIGHT] = [
                                        (
                                            "↖",
                                            "Up and to the left",
                                            UIMessage::MoveDesign {
                                                direction: DesignMoveDirection::UpAndLeft,
                                                step: design_move_step_mm,
                                            },
                                        ),
                                        (
                                            "↑",
                                            "Up",
                                            UIMessage::MoveDesign {
                                                direction: DesignMoveDirection::Up,
                                                step: design_move_step_mm,
                                            },
                                        ),
                                        (
                                            "↗",
                                            "Up and to the right",
                                            UIMessage::MoveDesign {
                                                direction: DesignMoveDirection::UpAndRight,
                                                step: design_move_step_mm,
                                            },
                                        ),
                                        (
                                            "←",
                                            "Left",
                                            UIMessage::MoveDesign {
                                                direction: DesignMoveDirection::Left,
                                                step: design_move_step_mm,
                                            },
                                        ),
                                        ("⇱", "Reset to top-left", UIMessage::ResetDesignPosition),
                                        (
                                            "→",
                                            "Right",
                                            UIMessage::MoveDesign {
                                                direction: DesignMoveDirection::Right,
                                                step: design_move_step_mm,
                                            },
                                        ),
                                        (
                                            "↙",
                                            "Down and to the left",
                                            UIMessage::MoveDesign {
                                                direction: DesignMoveDirection::DownAndLeft,
                                                step: design_move_step_mm,
                                            },
                                        ),
                                        (
                                            "↓",
                                            "Down",
                                            UIMessage::MoveDesign {
                                                direction: DesignMoveDirection::Down,
                                                step: design_move_step_mm,
                                            },
                                        ),
                                        (
                                            "↘",
                                            "Down and to the right",
                                            UIMessage::MoveDesign {
                                                direction: DesignMoveDirection::DownAndRight,
                                                step: design_move_step_mm,
                                            },
                                        ),
                                    ];

                                    let mut buttons_iter = buttons.into_iter();

                                    for _ in 0..GRID_HEIGHT {
                                        ui.horizontal(|ui| {
                                            for _ in 0..GRID_WIDTH {
                                                let (button_text, tooltip, event) = buttons_iter
                                                    .next()
                                                    .expect("There must be a button");
                                                if ui
                                                    .button(RichText::new(button_text).text_style(
                                                        egui::TextStyle::Name(
                                                            "Movement Buttons".into(),
                                                        ),
                                                    ))
                                                    .on_hover_text(tooltip)
                                                    .clicked()
                                                {
                                                    let _ = ui_message_tx.send(event);
                                                }
                                            }
                                        });
                                    }
                                });
                                ui.vertical(|ui| {
                                    let mut step_value = design_move_step_mm;
                                    let step_by_widget = Slider::new(
                                        &mut step_value,
                                        MINIMUM_DEFAULT_DESIGN_MOVE_STEP_MM
                                            ..=MAXIMUM_DESIGN_MOVE_STEP_MM,
                                    );
                                    ui.label("Step By (mm)");
                                    if ui.add(step_by_widget).changed() {
                                        let _ =
                                            ui_message_tx.send(UIMessage::DesignMoveStepChanged {
                                                step: step_value,
                                            });
                                    }
                                });
                            });
                        });
                    });
            });
        });
}

/// Draws a widget for displaying/editing the tool passes.
///
/// # Arguments
/// * `ui`: The UI to draw the widget into.
/// * `tool_passes`: The tool passes to draw.
/// * `tool_pass_widget_states`: The states of the tool pass widgets that we're drawing, should be persistent across frames.
/// * `frame_widgets`: The map that created widgets should be added to.
/// * `ui_message_tx`: A channel for sending UI messages into.
fn tool_passes_widget(
    ui: &mut egui::Ui,
    tool_passes: &mut Vec<ToolPass>,
    tool_pass_widget_states: &mut Vec<ToolPassWidgetState>,
    frame_widgets: &mut HashMap<egui::Id, SeanceUIElement>,
    ui_message_tx: &UIMessageTx,
) {
    // List of laser passes.
    ScrollArea::vertical().show(ui, |ui| {
        let drag_area = dnd(ui, "seance_laser_passes")
            .with_mouse_config(DragDropConfig {
                click_tolerance: 25.0,
                drag_delay: Duration::from_millis(0),
                scroll_tolerance: None,
                click_tolerance_timeout: Duration::from_secs(2),
            })
            .show_vec::<ToolPass>(tool_passes, |ui, pass, handle, state| {
                ui.horizontal(|ui| {
                    handle.show_drag_cursor_on_hover(false).ui(ui, |ui| {
                        let mut widget_size = ui.available_size_before_wrap();
                        widget_size.y = 40.0;
                        let (_, widget_rect) = ui.allocate_space(widget_size);
                        ui.painter()
                            .rect_filled(widget_rect, 2.0, ui.style().visuals.panel_fill);
                        ui.painter().rect_stroke(
                            widget_rect,
                            2.0,
                            Stroke::new(2.0, Color32::DARK_GRAY),
                        );

                        let mut child_ui =
                            ui.child_ui(widget_rect, Layout::left_to_right(Align::Center), None);
                        tool_pass_widget(
                            &mut child_ui,
                            pass,
                            state.index,
                            &mut tool_pass_widget_states[state.index], // TODO: BAD!
                            frame_widgets,
                            ui_message_tx,
                        );
                    });
                });
            });

        if drag_area.is_drag_finished() {
            drag_area.update_vec(tool_passes);
        }
    });
}

/// The state of a tool pass widget.
struct ToolPassWidgetState {
    /// Which aspect of the tool pass that is being edited.
    editing: ToolPassWidgetEditing,
    /// The text being edited into the power field.
    power_editing_text: String,
    /// The text being edited into the speed field.
    speed_editing_text: String,
}

impl ToolPassWidgetState {
    /// Creates a new [`ToolPassWidgetState`].
    ///
    /// # Arguments
    /// * `editing`: The apect of the tool pass that is being edited.
    /// * `power`: The power of the tool pass.
    /// * `speed`: The speed of the tool pass.
    fn new(editing: ToolPassWidgetEditing, power: &u64, speed: &u64) -> Self {
        Self {
            editing,
            power_editing_text: power.to_string(),
            speed_editing_text: speed.to_string(),
        }
    }
}

/// Which aspect of a tool pass is currently being edited.
#[derive(Default)]
enum ToolPassWidgetEditing {
    /// Nothing is being edited.
    #[default]
    None,
    /// The name is being edited.
    Name,
    /// The power is being edited.
    Power,
    /// The speed is being edited.
    Speed,
}

/// A single tool pass widget.
///
/// # Arguments
/// * `ui`: The UI to draw the widget into.
/// * `tool_pass`: The tool pass to draw.
/// * `pass_index`: The index into the tool passes array that is being drawn.
/// * `state`: The state of the widget.
/// * `frame_widgets`: The map of widgets to add drawn widgets to.
/// * `ui_message_tx`: The channel to send UI events into.
///
/// # Returns
/// An [`egui::Response`].
fn tool_pass_widget(
    ui: &mut egui::Ui,
    tool_pass: &ToolPass,
    pass_index: usize,
    state: &mut ToolPassWidgetState,
    frame_widgets: &mut HashMap<egui::Id, SeanceUIElement>,
    ui_message_tx: &UIMessageTx,
) -> egui::Response {
    StripBuilder::new(ui)
        .size(Size::exact(20.0))
        .sizes(Size::remainder(), 6)
        .horizontal(|mut strip| {
            // Drag Handle
            strip.cell(|ui| {
                Frame::default().inner_margin(2.0).show(ui, |ui| {
                    ui.label("☰").on_hover_cursor(egui::CursorIcon::Grab);
                });
            });
            // ToolPass Name
            strip.cell(|ui| {
                Frame::default().inner_margin(5.0).show(ui, |ui| {
                    let mut pen_name = tool_pass.name().to_string();
                    if matches!(state.editing, ToolPassWidgetEditing::Name) {
                        let text_edit = ui.add(
                            TextEdit::singleline(&mut pen_name)
                                .horizontal_align(Align::RIGHT)
                                .vertical_align(Align::Center),
                        );

                        ui.ctx()
                            .memory_mut(|memory| memory.request_focus(text_edit.id));

                        if text_edit.changed() || text_edit.lost_focus() {
                            let _ = ui_message_tx.send(UIMessage::ToolPassNameChanged {
                                index: pass_index,
                                name: pen_name.to_string(),
                            });
                        }

                        if text_edit.lost_focus() {
                            let _ = ui_message_tx.send(UIMessage::ToolPassNameLostFocus);
                        }
                    } else {
                        let pen_name_label = Label::new(pen_name).truncate().sense(Sense::click());
                        let pen_name_widget = ui
                            .add(pen_name_label)
                            .on_hover_cursor(egui::CursorIcon::Text);
                        frame_widgets.insert(
                            pen_name_widget.id,
                            SeanceUIElement::NameLabel { index: pass_index },
                        );

                        if pen_name_widget.clicked() {
                            let _ = ui_message_tx
                                .send(UIMessage::ToolPassNameClicked { index: pass_index });
                        }
                    }
                });
            });
            // Power Field
            strip.cell(|ui| {
                Frame::default().inner_margin(10.0).show(ui, |ui| {
                    let pen_power_str = &mut state.power_editing_text;
                    if matches!(state.editing, ToolPassWidgetEditing::Power) {
                        let text_edit = ui.add(
                            TextEdit::singleline(pen_power_str)
                                .horizontal_align(Align::RIGHT)
                                .vertical_align(Align::Center),
                        );

                        ui.ctx()
                            .memory_mut(|memory| memory.request_focus(text_edit.id));

                        if text_edit.clicked_elsewhere() {
                            let _ = ui_message_tx.send(UIMessage::ToolPassPowerLostFocus);
                        }
                    } else {
                        let pen_power_label =
                            Label::new(format!("Power: {pen_power_str}")).sense(Sense::click());
                        let pen_power_widget = ui
                            .add(pen_power_label)
                            .on_hover_cursor(egui::CursorIcon::Text);
                        frame_widgets.insert(
                            pen_power_widget.id,
                            SeanceUIElement::PowerLabel { index: pass_index },
                        );

                        if pen_power_widget.clicked() {
                            let _ = ui_message_tx
                                .send(UIMessage::ToolPassPowerClicked { index: pass_index });
                        }
                    }
                });
            });
            // Speed Field
            strip.cell(|ui| {
                Frame::default().inner_margin(10.0).show(ui, |ui| {
                    let pen_speed_str = &mut state.speed_editing_text;
                    if matches!(state.editing, ToolPassWidgetEditing::Speed) {
                        let text_edit = ui.add(
                            TextEdit::singleline(pen_speed_str)
                                .horizontal_align(Align::RIGHT)
                                .vertical_align(Align::Center),
                        );

                        ui.ctx()
                            .memory_mut(|memory| memory.request_focus(text_edit.id));

                        if text_edit.clicked_elsewhere() {
                            let _ = ui_message_tx.send(UIMessage::ToolPassSpeedLostFocus);
                        }
                    } else {
                        let pen_speed_label =
                            Label::new(format!("Speed: {pen_speed_str}")).sense(Sense::click());
                        let pen_speed_widget = ui
                            .add(pen_speed_label)
                            .on_hover_cursor(egui::CursorIcon::Text);
                        frame_widgets.insert(
                            pen_speed_widget.id,
                            SeanceUIElement::SpeedLabel { index: pass_index },
                        );

                        if pen_speed_widget.clicked() {
                            let _ = ui_message_tx
                                .send(UIMessage::ToolPassSpeedClicked { index: pass_index });
                        }
                    }
                });
            });
            // Colour Hex-code
            strip.cell(|ui| {
                Frame::default().inner_margin(6.0).show(ui, |ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let [r, g, b] = tool_pass.colour();
                        let colour_u32: u64 =
                            ((*r as u64) << 16) + ((*g as u64) << 8) + (*b as u64);
                        ui.label(format!("#{colour_u32:06X}"));
                    });
                });
            });
            // Colour Swatch
            strip.cell(|ui| {
                let mut colour = tool_pass.colour().clone();
                if ui.color_edit_button_srgb(&mut colour).changed() {
                    let _ = ui_message_tx.send(UIMessage::ToolPassColourChanged {
                        index: pass_index,
                        colour: colour.clone(),
                    });
                };
            });
            // Enable Checkbox
            strip.cell(|ui| {
                let mut enabled_val = tool_pass.enabled().clone();
                let enable_box = ui.checkbox(&mut enabled_val, "");
                if enable_box.changed() {
                    let _ = ui_message_tx.send(UIMessage::ToolPassEnableChanged {
                        index: pass_index,
                        enabled: enabled_val.clone(),
                    });
                }
            });
        })
}

/// A widget for drawing the preview of a design.
///
/// # Arguments
/// * `ui`: The UI to draw the preview into.
/// * `design_file`: The design file to draw.
/// * `design_file_preview`: The generated preview.
/// * `ui_message_tx`: A channel that UI events can be sent into.
/// * `size`: How big to draw the preview.
///
/// # Returns
/// An [`egui::Response`].
fn design_file_widget(
    ui: &mut egui::Ui,
    design_file: &Arc<RwLock<Option<DesignWithMeta>>>,
    design_preview: &mut Option<DesignPreview>,
    ui_message_tx: &UIMessageTx,
    size: egui::Vec2,
) -> egui::Response {
    let _ = ui_message_tx.send(UIMessage::DesignPreviewSize {
        size_before_wrap: size,
    });

    let (_, widget_rect) = ui.allocate_space(size);
    ui.painter()
        .rect_stroke(widget_rect, 2.0, Stroke::new(2.0, Color32::DARK_GRAY));

    {
        let Ok(design_file_lock) = design_file.read() else {
            return design_file_placeholder(ui, widget_rect);
        };

        if design_file_lock.is_none() {
            return design_file_placeholder(ui, widget_rect);
        }
    }

    let Some(design_preview) = design_preview else {
        return design_file_placeholder(ui, widget_rect);
    };

    let Some(image) = design_preview.image(ui.ctx(), design_file) else {
        return design_file_placeholder(ui, widget_rect);
    };

    let mut child_ui = ui.child_ui(widget_rect, Layout::left_to_right(Align::Min), None);

    let response = ScrollArea::both()
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
        .animated(false)
        .min_scrolled_width(widget_rect.size().x)
        .min_scrolled_height(widget_rect.size().y)
        .max_width(widget_rect.size().x)
        .max_height(widget_rect.size().y)
        .show(&mut child_ui, |ui| ui.add(image));
    preview_files_being_dropped(ui, widget_rect);
    response.inner
}

/// A placeholder to display when there is no design file loaded.
///
/// # Arguments
/// * `ui`: The UI to draw the placeholder into.
/// * `widget_rect`: Where to draw the placeholder.
///
/// # Returns
/// An [`egui::Response`].
fn design_file_placeholder(ui: &mut egui::Ui, widget_rect: Rect) -> egui::Response {
    let label: WidgetText = "Design Preview".into();
    let response = ui.put(widget_rect, Label::new(label));
    preview_files_being_dropped(ui, widget_rect);
    response
}

/// A widget drawn whn a file is being hovered over the UI.
///
/// # Arguments
/// * `ui`: The UI to draw the preview into.
/// * `rect`: Where to draw the preview.
///
/// # Returns
/// An [`egui::Response`].
fn preview_files_being_dropped(ui: &mut egui::Ui, rect: Rect) {
    use egui::*;
    use std::fmt::Write as _;

    if !ui.ctx().input(|i| i.raw.hovered_files.is_empty()) {
        let mut show_preview = false;

        let text = ui.ctx().input(|i| {
            let mut text = "Open Design: ".to_owned();
            for file in &i.raw.hovered_files {
                if let Some(path) = &file.path {
                    if let Some(ext) = path.extension() {
                        if let Some(name) = path.file_name() {
                            if ext.eq_ignore_ascii_case("svg") {
                                show_preview = true;
                                write!(text, "{}", name.to_string_lossy()).ok();
                            }
                        }
                    }
                }
            }
            text
        });

        if show_preview {
            let painter = ui
                .ctx()
                .layer_painter(LayerId::new(Order::Foreground, Id::new("file_drop_target")));

            painter.rect_filled(rect, 0.0, Color32::from_black_alpha(192));
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                text,
                TextStyle::Heading.resolve(&ui.ctx().style()),
                Color32::WHITE,
            );
        }
    }
}

/// Shows an error dialog.
///
/// # Arguments
/// * `ctx`: The egui context.
/// * `ui_message_tx`: The channel that events can be sent into.
/// * `err`: The error to display.
/// * `details`: Any details that can be shown along with the error message.
fn error_dialog(
    ctx: &egui::Context,
    ui_message_tx: &UIMessageTx,
    err: &str,
    details: &Option<String>,
) {
    let window_size = ctx.screen_rect().max;
    let error_dialog_size = Vec2 {
        x: (window_size.x * 0.2).max(320.0),
        y: (window_size.y * 0.2).max(180.0),
    };
    ctx.show_viewport_immediate(
        egui::ViewportId::from_hash_of("error_dialog"),
        egui::ViewportBuilder::default()
            .with_title("Error")
            .with_inner_size([error_dialog_size.x, error_dialog_size.y])
            .with_position(Pos2 {
                x: (window_size.x / 2.0) - (error_dialog_size.x / 2.0),
                y: (window_size.y / 2.0) - (error_dialog_size.y / 2.0),
            })
            .with_resizable(false),
        move |ctx, _| {
            let ui_message_tx = ui_message_tx.clone();
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.label(err);
                if let Some(details) = details {
                    ui.collapsing("Details", |ui| ui.label(details));
                }
                ui.with_layout(Layout::right_to_left(Align::BOTTOM), |ui| {
                    if ui.button("ok").clicked() {
                        let _ = ui_message_tx.send(UIMessage::CloseErrorDialog);
                    }
                });
            });
            ctx.input(|i| {
                if i.viewport().close_requested()
                    || i.key_pressed(Key::Escape)
                    || i.key_pressed(Key::Enter)
                {
                    // Tell parent to close us.
                    let _ = ui_message_tx.send(UIMessage::CloseErrorDialog);
                }
            });
        },
    );
}

/// Shows the settings dialog.
///
/// # Arguments
/// * `ctx`: The egui context.
/// * `ui_message_tx`: A message channel that events can be sent into.
/// * `settings`: The state of the settings dialog.
fn settings_dialog(
    ctx: &egui::Context,
    ui_message_tx: &UIMessageTx,
    settings: &SettingsDialogState,
) {
    let window_size = ctx.screen_rect().max;
    let settings_dialog_size = Vec2 { x: 640.0, y: 480.0 };
    ctx.show_viewport_immediate(
        egui::ViewportId::from_hash_of("settings_dialog"),
        egui::ViewportBuilder::default()
            .with_title("Settings")
            .with_inner_size([settings_dialog_size.x, settings_dialog_size.y])
            .with_position(Pos2 {
                x: (window_size.x / 2.0) - (settings_dialog_size.x / 2.0),
                y: (window_size.y / 2.0) - (settings_dialog_size.y / 2.0),
            })
            .with_resizable(true),
        move |ctx, _| {
            let ui_message_tx = ui_message_tx.clone();
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let mut printer = settings.print_device.clone();
                    match &mut printer {
                        #[cfg(not(target_os = "windows"))]
                        PrintDevice::Path { path } => {
                            ui.label("Print Device");
                            let printer_edit = ui
                                .text_edit_singleline(path)
                                .on_hover_text(r#"This is the device that will be used to print."#);
                            if printer_edit.changed() || printer_edit.lost_focus() {
                                let _ = ui_message_tx
                                    .send(UIMessage::PrinterSettingsChanged { printer });
                            }
                        }
                        #[cfg(target_os = "windows")]
                        PrintDevice::USBPort { port: current_port } => {
                            let ports = usb_enumeration::enumerate(None, None);
                            let mut selected: Option<usb_enumeration::UsbDevice> = None;
                            if let Some(port) = current_port {
                                selected = ports
                                    .iter()
                                    .find(|p| {
                                        p.vendor_id == port.vendor_id
                                            && p.product_id == port.product_id
                                    })
                                    .cloned();
                            }

                            let original_selected = current_port.clone();

                            match &selected {
                                Some(head) => {
                                    let label = head.description.clone().unwrap();
                                    egui::ComboBox::from_label("Print Device").selected_text(label)
                                }
                                None => egui::ComboBox::from_label("Print Device"),
                            }
                            .show_ui(ui, |ui| {
                                for port in ports {
                                    if let Some(label) = port.description {
                                        ui.selectable_value(
                                            current_port,
                                            Some(USBPort {
                                                vendor_id: port.vendor_id,
                                                product_id: port.product_id,
                                            }),
                                            label,
                                        );
                                    }
                                }
                            });

                            if *current_port != original_selected {
                                let _ = ui_message_tx.send(UIMessage::PrinterSettingsChanged {
                                    printer: PrintDevice::USBPort {
                                        port: current_port.clone(),
                                    },
                                });
                            }
                        }
                    }
                });

                ui.with_layout(Layout::right_to_left(Align::BOTTOM), |ui| {
                    if ui.button("Save and Close").clicked() {
                        let _ = ui_message_tx.send(UIMessage::SaveSettings);
                        let _ = ui_message_tx.send(UIMessage::CloseSettingsDialog);
                    }
                    if ui.button("Discard and Close").clicked() {
                        let _ = ui_message_tx.send(UIMessage::CloseSettingsDialog);
                    }
                });
            });
            ctx.input(|i| {
                if i.viewport().close_requested()
                    || i.key_pressed(Key::Escape)
                    || i.key_pressed(Key::Enter)
                {
                    // Tell parent to close us.
                    let _ = ui_message_tx.send(UIMessage::CloseSettingsDialog);
                }
            });
        },
    );
}

/// Attempts to load a design from a path.
///
/// # Arguments
/// * `path`: The path to attempt to load from.
/// * `hasher`: Hasher to use to get the hash of the design file.
///
/// # Returns
/// The design file, if successfully loaded, otherwise an error string.
fn load_design(
    path: &PathBuf,
    hasher: &mut Box<dyn hash::Hasher>,
) -> Result<DesignWithMeta, String> {
    let mut path_without_extension = path.clone();
    path_without_extension.set_extension("");

    let Some(file_name) = path_without_extension.file_name() else {
        return Err("Failed to read file name".to_string());
    };

    let Some(file_name) = file_name.to_str() else {
        return Err("Failed to read file name".to_string());
    };

    let Some(extension) = path.extension() else {
        return Err("Unrecognised file extenstion".to_string());
    };

    if !extension.eq_ignore_ascii_case("svg") {
        return Err(format!(
            "Unrecognised file extension: '{}'",
            extension.to_string_lossy()
        ));
    }

    match fs::read(path) {
        Ok(bytes) => {
            let svg = parse_svg(&path, &bytes).map_err(|err| {
                let error_string = format!("Error reading SVG file: {err}");
                log::error!("{error_string}");
                error_string
            })?;
            let width = svg.size().width() / SVG_UNITS_PER_MM;
            let height = svg.size().height() / SVG_UNITS_PER_MM;

            bytes.hash(hasher);
            let hash = hasher.finish();

            Ok((
                DesignFile {
                    name: file_name.to_string(),
                    tree: svg,
                    width_mm: width,
                    height_mm: height,
                },
                hash,
                path.clone(),
            ))
        }
        Err(err) => Err(format!("Failed to read file: {}", err)),
    }
}

/// The focus is changing from one UI element to another.
/// Makes decisions about whether to allow the focus to change and what to do about it.
///
/// # Arguments
/// * `ctx`: The egui context.
/// * `previous_frame_widgets`: The widgets that were drawn on the previous frame.
/// * `tool_pass_widgets_states`: The states of all of the tool pass widgets.
/// * `ui_message_tx`: A channel that can be used to send UI events.
fn focus_changing(
    ctx: &egui::Context,
    previous_frame_widgets: &HashMap<egui::Id, SeanceUIElement>,
    tool_pass_widget_states: &mut Vec<ToolPassWidgetState>,
    ui_message_tx: &UIMessageTx,
) {
    let mut allow_move = true;
    for (index, pen_widget) in tool_pass_widget_states.iter_mut().enumerate() {
        match pen_widget.editing {
            ToolPassWidgetEditing::None => {}
            ToolPassWidgetEditing::Name => {}
            ToolPassWidgetEditing::Power => {
                if let Ok(power) = pen_widget.power_editing_text.parse::<u64>() {
                    let _ = ui_message_tx.send(UIMessage::ToolPassPowerChanged { index, power });
                } else {
                    // TODO: Flash red
                    allow_move = false;
                    pen_widget.editing = ToolPassWidgetEditing::Power;
                }
            }
            ToolPassWidgetEditing::Speed => {
                if let Ok(speed) = pen_widget.speed_editing_text.parse::<u64>() {
                    let _ = ui_message_tx.send(UIMessage::ToolPassSpeedChanged { index, speed });
                } else {
                    // TODO: Flash red
                    allow_move = false;
                    pen_widget.editing = ToolPassWidgetEditing::Speed;
                }
            }
        }

        if allow_move {
            pen_widget.editing = ToolPassWidgetEditing::None;
        }
    }

    if allow_move {
        ctx.memory_mut(|memory| {
            if let Some(id) = memory.focused() {
                if let Some(widget) = previous_frame_widgets.get(&id) {
                    match widget {
                        SeanceUIElement::NameLabel { index } => {
                            if let Some(pass) = tool_pass_widget_states.get_mut(*index) {
                                pass.editing = ToolPassWidgetEditing::Name;
                            }
                        }
                        SeanceUIElement::PowerLabel { index } => {
                            if let Some(pass) = tool_pass_widget_states.get_mut(*index) {
                                pass.editing = ToolPassWidgetEditing::Power;
                            }
                        }
                        SeanceUIElement::SpeedLabel { index } => {
                            if let Some(pass) = tool_pass_widget_states.get_mut(*index) {
                                pass.editing = ToolPassWidgetEditing::Speed;
                            }
                        }
                    }
                }
            }
        });
    }
}

/// Gets all of the possible capitalisations of a string.
/// We need this because the library we use for showig file dialogs is not very clever,
/// it does not match against file extensions case-insensitively. Therefore, we provide
/// the file dialog library with all of the possible capitalisations of the file extensions
/// we care about, just in case folks have bizarrely capitalised file extensions.
///
/// # Arguments
/// * `input`: The string to generate all the capitalisations of.
///
/// # Returns
/// An array of strings containing all of the possible capitalisations of the input string.
pub fn all_capitalisations_of(input: &str) -> Vec<String> {
    let mut result = vec![];

    let bitmask = ((2_u32.pow(input.len() as u32)) - 1) as usize;

    for mask in 0..=bitmask {
        let mut new_str = String::new();
        for i in 0..input.len() {
            if mask & (1 << i) > 0 {
                new_str += &input
                    .chars()
                    .nth(i)
                    .expect(&format!("Could not get character {i}"))
                    .to_uppercase()
                    .to_string();
            } else {
                new_str += &input
                    .chars()
                    .nth(i)
                    .expect(&format!("Could not get character {i}"))
                    .to_lowercase()
                    .to_string();
            }
        }
        result.push(new_str);
    }

    result
}

#[cfg(test)]
mod test {
    use super::all_capitalisations_of;

    #[test]
    fn capitalisations() {
        let mut result = all_capitalisations_of("svg");
        result.sort();
        assert_eq!(result.len(), 8);
        assert_eq!(
            result,
            vec!["SVG", "SVg", "SvG", "Svg", "sVG", "sVg", "svG", "svg"]
        )
    }
}
