//! `app`
//!
//! Contains the entry point for the egui APP.

mod preview;
use oneshot::TryRecvError;
pub use preview::{render_task, RenderRequest};
use reqwest::StatusCode;

use std::{
    collections::HashMap,
    fs,
    hash::{self, DefaultHasher, Hash, Hasher},
    path::PathBuf,
    str::FromStr,
    sync::{Arc, Mutex, RwLock},
    time::Duration,
};

use egui::{
    ecolor::HexColor, Align, Color32, DragValue, FontId, Frame, Key, Label, Layout, Margin, Pos2,
    Rect, RichText, ScrollArea, Sense, Slider, Stroke, StrokeKind, TextEdit, UiBuilder, Vec2,
    Visuals, WidgetText,
};
use egui_dnd::{dnd, DragDropConfig};
use egui_extras::{Size, StripBuilder};
use preview::{DesignPreview, MAX_ZOOM_LEVEL, MIN_ZOOM_LEVEL};

use planchette::{
    seance::{
        default_passes,
        svg::{parse_svg, SVG_UNITS_PER_MM},
        DesignFile, DesignOffset, ToolPass, BED_HEIGHT_MM, BED_WIDTH_MM,
    },
    PrintJob,
};

/// `DesignFile` with a hash and original path attached.
type DesignWithMeta = (planchette::seance::DesignFile, u64, PathBuf);

/// Default URL of the Planchette server to send jobs to.
const DEFAULT_PLANCHETTE_URL: &str = "http://ouija.yhs:1789";
/// The minimum amount that a design can be moved by.
const MINIMUM_DEFAULT_DESIGN_MOVE_STEP_MM: f32 = 0.1;
/// The default amount that designs are moved by.
const DEFAULT_DESIGN_MOVE_STEP_MM: f32 = 10.0;
/// The maximum amount that designs can be moved by.
const MAXIMUM_DESIGN_MOVE_STEP_MM: f32 = 500.0;

/// Minimum power value that can be set, as a floating point value.
const MIN_POWER_VALUE_FLOAT: f32 = 0.0;
/// Maximum power value that can be set, as an integer value.
const MAX_POWER_VALUE_FLOAT: f32 = 100.0;
/// Minimum speed value that can be set, as a floating point value.
const MIN_SPEED_VALUE_FLOAT: f32 = 0.0;
/// Maximum speed value that can be set, as an integer value.
const MAX_SPEED_VALUE_FLOAT: f32 = 100.0;

/// Data that is saved between uses of Seance.
#[derive(serde::Deserialize, serde::Serialize)]
struct PersistentStorage {
    /// Whether the UI should be dark mode.
    dark_mode: bool,
    /// The tool passes to run on the machine.
    passes: Vec<ToolPass>,
    /// The URL of the planchette server to send jobs to.
    planchette_url: String,
    /// How much to move the design by each time a movement button is pressed.
    design_move_step_mm: f32,
}

/// A oneshot receiver that will receive the result of uploading a design to a
/// Planchette server.
type PlanchetteUploadResultReceiver = oneshot::Receiver<Result<(), PlanchetteError>>;

/// The status of an ongoing upload to a Planchette server, if any.
enum PlanchetteUploadStatus {
    /// No ongoing upload.
    None,
    /// Ongoing upload awaiting result.
    Uploading {
        /// Channel on which the result will be received.
        receiver: PlanchetteUploadResultReceiver,
    },
    /// An upload failed.
    Failed {
        /// When the upload failed.
        at: std::time::Instant,
    },
    /// An upload succeeded.
    Succeeded {
        /// When the upload succeeded.
        at: std::time::Instant,
    },
}

/// The Seance UI app.
pub struct Seance {
    /// Whether the UI should be dark mode.
    dark_mode: bool,
    /// The tool passes to run on the machine.
    passes: Vec<ToolPass>,
    /// The URL of the planchette server to send jobs to.
    planchette_url: reqwest::Url,

    /// The currently open design file, if any.
    design_file: Arc<RwLock<Option<DesignWithMeta>>>,

    /// The message channel that UI events will be sent into.
    ui_message_rx: UIMessageRx,
    /// Where to put requests to re-render the design preview.
    render_request: Arc<Mutex<Option<RenderRequest>>>,
    /// The hasher to use to calculate the hash of the design file.
    hasher: Box<dyn Hasher>,
    /// Amount to move the design by when moving.
    design_move_step_mm: f32,

    /// Context passed around for drawing.
    ui_context: UIContext,
    /// The states of all of the tool pass widgets.
    tool_pass_widget_states: Vec<ToolPassWidgetState>,
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
    /// Current state of uploading a design to a Planchette server.
    planchette_upload_status: PlanchetteUploadStatus,
}

/// Context that we're drawing into.
///
/// Use the methods implemented on this struct so that The Right Thing happens,
/// don't modify the values in this struct directly!
struct UIContext {
    /// The message channel that will receive UI events.
    ui_message_tx: UIMessageTx,
    /// The widgets that were created on the previous frame, used for
    /// handling tab/arrow-key/enter-key events.
    previous_frame_widgets: HashMap<egui::Id, SeanceUIElement>,
}

impl UIContext {
    /// Create a new [`UIContext`].
    ///
    /// # Arguments
    /// * `ui_message_tx`: Message channel for sending UI events.
    ///
    /// # Returns
    /// A new [`UIContext`].
    fn new(ui_message_tx: UIMessageTx) -> Self {
        Self {
            ui_message_tx,
            previous_frame_widgets: HashMap::default(),
        }
    }

    /// Reset this context before painting the next frame.
    /// Should be called after handling events and just before sarting to render a new frame.
    fn prepare_for_repaint(&mut self) {
        self.previous_frame_widgets = HashMap::default();
    }

    /// Send a [`UIMessage`].
    ///
    /// # Arguments
    /// * `message`: The message to send.
    fn send_ui_message(&mut self, message: UIMessage) {
        let _ = self.ui_message_tx.send(message);
    }

    /// Add a widget to the stored widgets for this frame.
    /// Should be called when creating widgets that are interacted with for text entry.
    ///
    /// # Arguments
    /// * `id`: The Id of the widget.
    /// * `element`: The element that was created.
    fn add_widget(&mut self, id: egui::Id, element: SeanceUIElement) {
        self.previous_frame_widgets.insert(id, element);
    }

    /// Get a widget that was created during this frame.
    ///
    /// # Arguments
    /// * `id`: The Id of the widget to retrieve.
    ///
    /// # Returns
    /// A reference to the stored [`SeanceUIElement`], if an element with the specified Id exists.
    fn get_widget(&self, id: &egui::Id) -> Option<&SeanceUIElement> {
        self.previous_frame_widgets.get(id)
    }
}

/// The state of the settings dialog. Data here is ephemiral and must explicitly be saved when required.
struct SettingsDialogState {
    /// The URL of the planchette server to send jobs to.
    planchette_url: String,
}

impl SettingsDialogState {
    /// Creates a new [`SettingsDialogState`].
    ///
    /// # Arguments
    /// * `print_device`: The URL of the planchette server to send jobs to.
    ///
    /// # Returns
    /// A new [`SettingsDialogState`].
    fn new(planchette_url: String) -> Self {
        Self { planchette_url }
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
                    planchette_url: DEFAULT_PLANCHETTE_URL.to_string(),
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
                .map(|_| ToolPassWidgetState::new(Default::default()))
                .collect::<Vec<_>>();

            return Seance {
                dark_mode: seance_storage.dark_mode,
                passes: seance_storage.passes,
                planchette_url: reqwest::Url::parse(&seance_storage.planchette_url).unwrap_or(
                    reqwest::Url::parse(DEFAULT_PLANCHETTE_URL)
                        .expect("Default URL is a valid URL"),
                ),

                design_file: Default::default(),
                ui_message_rx,
                render_request,
                hasher: Box::new(DefaultHasher::new()),
                design_move_step_mm: seance_storage.design_move_step_mm,

                ui_context: UIContext::new(ui_message_tx),
                tool_pass_widget_states: laser_pass_widget_states,
                preview_zoom_level: MIN_ZOOM_LEVEL,
                file_dialog: None,
                current_error: None,
                design_preview_image: None,
                settings_dialog: None,
                planchette_upload_status: PlanchetteUploadStatus::None,
            };
        }

        let laser_passes_widget_states: Vec<ToolPassWidgetState> = default_pens
            .iter()
            .map(|_| ToolPassWidgetState::new(Default::default()))
            .collect::<Vec<_>>();

        Seance {
            dark_mode: cc.egui_ctx.style().visuals.dark_mode,
            passes: default_pens,
            planchette_url: reqwest::Url::parse(DEFAULT_PLANCHETTE_URL)
                .expect("Default URL is a valid URL"),

            design_file: Default::default(),
            ui_message_rx,
            render_request,
            hasher: Box::new(DefaultHasher::new()),
            design_move_step_mm: DEFAULT_DESIGN_MOVE_STEP_MM,

            ui_context: UIContext::new(ui_message_tx),
            tool_pass_widget_states: laser_passes_widget_states,

            preview_zoom_level: MIN_ZOOM_LEVEL,
            file_dialog: None,
            current_error: None,
            design_preview_image: None,
            settings_dialog: None,
            planchette_upload_status: PlanchetteUploadStatus::None,
        }
    }

    /// Handle all UI messages from the previous frame.
    ///
    /// # Arguments
    /// * `ctx`: egui context.
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
                    let ui_message_tx = self.ui_context.ui_message_tx.clone();
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
                    self.settings_dialog =
                        Some(SettingsDialogState::new(self.planchette_url.to_string()))
                }
                UIMessage::PrinterSettingsChanged { planchette_url } => {
                    if let Some(dialog) = &mut self.settings_dialog {
                        dialog.planchette_url = planchette_url;
                    }
                }
                UIMessage::SaveSettings => {
                    if let Some(dialog) = &self.settings_dialog {
                        if let Ok(url) = reqwest::Url::parse(&dialog.planchette_url) {
                            self.planchette_url = url;
                        }
                    }
                }
                UIMessage::CloseSettingsDialog => {
                    self.settings_dialog = None;
                }
                UIMessage::DesignFileChanged { design_file } => {
                    let Ok(mut design_lock) = self.design_file.write() else {
                        self.ui_context.send_ui_message(UIMessage::ShowError {
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
                UIMessage::ToolPassColourClicked { index } => {
                    if let Some(pass) = self.passes.get(index) {
                        if let Some(state) = self.tool_pass_widget_states.get_mut(index) {
                            let [r, g, b] = pass.colour();
                            let colour_u32: u64 =
                                ((*r as u64) << 16) + ((*g as u64) << 8) + (*b as u64);
                            state.editing = ToolPassWidgetEditing::Colour {
                                value: format!("#{colour_u32:06X}"),
                            };
                        }
                    }
                }
                UIMessage::ToolPassColourLostFocus => {
                    focus_changing(
                        ctx,
                        &mut self.ui_context,
                        &mut self.tool_pass_widget_states,
                        &self.passes,
                        FocusChangingReason::ToolPassColourLostFocus,
                    );
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
                        &mut self.ui_context,
                        &mut self.tool_pass_widget_states,
                        &self.passes,
                        FocusChangingReason::ToolPassNameLostFocus,
                    );
                }
                UIMessage::ToolPassEnableChanged { index, enabled } => {
                    if let Some(pass) = self.passes.get_mut(index) {
                        pass.set_enabled(enabled);
                    }
                }
                UIMessage::PreviewZoomLevelChanged { zoom } => {
                    self.preview_zoom_level = zoom.clamp(MIN_ZOOM_LEVEL, MAX_ZOOM_LEVEL);
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
                UIMessage::DesignOffsetChanged { offset } => {
                    if let Some(preview) = &mut self.design_preview_image {
                        preview.set_design_offset(offset, &self.design_file);
                    }
                }
                UIMessage::ResetDesignPosition => {
                    if let Some(preview) = &mut self.design_preview_image {
                        preview.set_design_offset(Default::default(), &self.design_file);
                    }
                }
                UIMessage::PlanchetteUploadStarted { receiver } => {
                    // If we've started a new upload then we will replace the old upload as
                    // it is now irrelevant.
                    self.planchette_upload_status = PlanchetteUploadStatus::Uploading { receiver };
                }
                UIMessage::EnterKeyPressed => {
                    focus_changing(
                        ctx,
                        &mut self.ui_context,
                        &mut self.tool_pass_widget_states,
                        &self.passes,
                        FocusChangingReason::EnterKeyPressed,
                    );
                }
                UIMessage::TabKeyPressed => {
                    focus_changing(
                        ctx,
                        &mut self.ui_context,
                        &mut self.tool_pass_widget_states,
                        &self.passes,
                        FocusChangingReason::TabKeyPressed,
                    );
                }
                UIMessage::SpaceKeyPressed => {
                    focus_changing(
                        ctx,
                        &mut self.ui_context,
                        &mut self.tool_pass_widget_states,
                        &self.passes,
                        FocusChangingReason::SpaceKeyPressed,
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
                planchette_url: self.planchette_url.to_string(),
                design_move_step_mm: self.design_move_step_mm,
            },
        );
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_ui_messages(ctx);

        if !FileDialog::poll(&mut self.ui_context, &self.file_dialog, &mut self.hasher) {
            let _ = self.file_dialog.take();
        }

        if let Some((err, details)) = &self.current_error {
            error_dialog(ctx, &mut self.ui_context, err, details);
        }

        if let Some(settings) = &self.settings_dialog {
            settings_dialog(ctx, &mut self.ui_context, settings);
        }

        match &mut self.planchette_upload_status {
            PlanchetteUploadStatus::None => {}
            PlanchetteUploadStatus::Uploading { receiver } => match receiver.try_recv() {
                Ok(Ok(_)) => {
                    self.planchette_upload_status = PlanchetteUploadStatus::Succeeded {
                        at: std::time::Instant::now(),
                    }
                }
                Ok(Err(err)) => {
                    handle_planchette_error(&mut self.ui_context, err);
                    self.planchette_upload_status = PlanchetteUploadStatus::Failed {
                        at: std::time::Instant::now(),
                    };
                }
                Err(TryRecvError::Disconnected) => {
                    self.ui_context.send_ui_message(UIMessage::ShowError {
                        error: "Failed to confirm status of design upload".to_string(),
                        details: Some("Sending half of response channel was closed".to_string()),
                    });
                    self.planchette_upload_status = PlanchetteUploadStatus::Failed {
                        at: std::time::Instant::now(),
                    };
                }
                Err(TryRecvError::Empty) => {}
            },
            PlanchetteUploadStatus::Failed { at } => {
                if at.elapsed() >= Duration::from_secs(5) {
                    self.planchette_upload_status = PlanchetteUploadStatus::None;
                }
            }
            PlanchetteUploadStatus::Succeeded { at } => {
                if at.elapsed() >= Duration::from_secs(5) {
                    self.planchette_upload_status = PlanchetteUploadStatus::None;
                }
            }
        }

        self.ui_context.prepare_for_repaint();

        // Slow down key presses to make typing bearable.
        std::thread::sleep(Duration::from_millis(10));

        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                // NOTE: no File->Quit on web pages!
                let is_web = cfg!(target_arch = "wasm32");
                if !is_web {
                    ui.menu_button("File", |ui| {
                        if ui.button("Settings").clicked() {
                            self.ui_context
                                .send_ui_message(UIMessage::ShowSettingsDialog);
                        }

                        if ui.button("Quit").clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });
                    ui.add_space(16.0);
                }

                egui::global_theme_preference_buttons(ui);

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
                                left: 0,
                                right: 0,
                                top: 0,
                                bottom: ui.style().spacing.menu_margin.bottom,
                            })
                            .show(ui, |ui| {
                                let offset = self
                                    .design_preview_image
                                    .as_ref()
                                    .map(|preview| preview.get_design_offset())
                                    .cloned()
                                    .unwrap_or_default();

                                toolbar_widget(
                                    ui,
                                    &mut self.ui_context,
                                    &self.design_file,
                                    &self.passes,
                                    &self.planchette_url,
                                    &offset,
                                    &self.planchette_upload_status,
                                );
                            });
                    });
                    strip.cell(|ui| {
                        ui_main(
                            ui,
                            &mut self.ui_context,
                            &mut self.passes,
                            &mut self.tool_pass_widget_states,
                            &self.design_file,
                            &mut self.design_preview_image,
                            self.preview_zoom_level,
                            self.design_move_step_mm,
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
                            self.ui_context
                                .send_ui_message(UIMessage::DesignFileChanged {
                                    design_file: file,
                                });
                        }
                        Err(err) => {
                            self.ui_context.send_ui_message(UIMessage::ShowError {
                                error: "Failed to load design".to_string(),
                                details: Some(err),
                            });
                        }
                    }
                }
            }

            if i.key_pressed(Key::Enter) {
                self.ui_context.send_ui_message(UIMessage::EnterKeyPressed);
            }

            if i.key_pressed(Key::Tab) {
                self.ui_context.send_ui_message(UIMessage::TabKeyPressed);
            }

            if i.key_pressed(Key::Space) {
                self.ui_context.send_ui_message(UIMessage::SpaceKeyPressed);
            }
        });

        ctx.request_repaint_after(Duration::from_millis(20));
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
        /// URL of the Planchette server to send jobs to.
        planchette_url: String,
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
    /// The colour label has been clicked.
    ToolPassColourClicked {
        /// The index of the tool pass that has been clicked.
        index: usize,
    },
    /// The colour of a tool pass lost focus.
    ToolPassColourLostFocus,
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
    /// Whether the tool pass is enabled has changed.
    ToolPassEnableChanged {
        /// The index of the tool pass.
        index: usize,
        /// Whether the tool pass should be set to enabled.
        enabled: bool,
    },
    /// The zoom level of the design preview has changed.
    PreviewZoomLevelChanged {
        /// The new zoom level.
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
    /// Design offset has changed to a new position.
    DesignOffsetChanged {
        /// The new offset.
        offset: DesignOffset,
    },
    /// Reset the design to align with the top-left edge.
    ResetDesignPosition,
    /// A design has been sent to Planchette, we're waiting on a response.
    PlanchetteUploadStarted {
        /// Channel on which the response will be received.
        receiver: PlanchetteUploadResultReceiver,
    },
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
    /// The label for the colour value.
    ColourLabel {
        /// The index of the tool pass for which this is the colour label.
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
    pub fn apply(&self, current_offset: &DesignOffset, step_mm: f32) -> DesignOffset {
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
    /// * `ui_context`: The Seance UI context.
    /// * `dialog`: The file dialog to poll.
    /// * `hasher`: Hasher that can be used to get the hash of files.
    ///
    /// # Returns
    /// Whether the file dialog should be kept (`true`) or destroyed (`false`).
    fn poll(
        ui_context: &mut UIContext,
        dialog: &Option<FileDialog>,
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
                                    ui_context.send_ui_message(UIMessage::DesignFileChanged {
                                        design_file: file,
                                    });
                                }
                                Err(err) => {
                                    ui_context.send_ui_message(UIMessage::ShowError {
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
                                    ui_context.send_ui_message(UIMessage::ToolPassesListChanged {
                                        passes,
                                    });
                                }
                                Err(err) => {
                                    ui_context.send_ui_message(UIMessage::ShowError {
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
/// * `ui_context`: The Seance UI context.
/// * `design_file`: The currently loaded design file, if any.
/// * `tool_passes`: The current passes of the tool.
/// * `planchette_url`: The URL of the planchette server to send jobs to.
/// * `offset`: How much to move the design by relative to its starting position, in mm, where +x is more right and +y is more down.
/// * `planchette_upload_status`: The status of an ongoing upload to a Planchette server, if any.
///
/// # Returns
/// An [`egui::Response`].
fn toolbar_widget(
    ui: &mut egui::Ui,
    ui_context: &mut UIContext,
    design_file: &Arc<RwLock<Option<DesignWithMeta>>>,
    tool_passes: &[ToolPass],
    planchette_url: &reqwest::Url,
    offset: &DesignOffset,
    planchette_upload_status: &PlanchetteUploadStatus,
) -> egui::Response {
    StripBuilder::new(ui)
        .sizes(Size::remainder(), 2)
        .horizontal(|mut strip| {
            strip.cell(|ui| {
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    if ui.button("Open Design").clicked() {
                        ui_context.send_ui_message(UIMessage::ShowOpenFileDialog);
                    }

                    if ui.button("Import Laser Settings").clicked() {
                        ui_context.send_ui_message(UIMessage::ShowOpenToolPathSettingsDialog);
                    }

                    if ui.button("Export Laser Settings").clicked() {
                        ui_context.send_ui_message(UIMessage::ShowExportToolPathSettingsDialog);
                    }

                    if ui.button("Enable All").clicked() {
                        for (index, _) in tool_passes.iter().enumerate() {
                            ui_context.send_ui_message(UIMessage::ToolPassEnableChanged { index, enabled: true });
                        }
                    }

                    if ui.button("Disable All").clicked() {
                        for (index, _) in tool_passes.iter().enumerate() {
                            ui_context.send_ui_message(UIMessage::ToolPassEnableChanged { index, enabled: false });
                        }
                    }
                });
            });

            strip.cell(|ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let hover_text = "Sends your design to the laser cutter. You will need to press Start on the laser cutter after sending.";

                    let design_valid = {
                        let design_lock = design_file.read();
                        matches!(design_lock.map(|design| design.is_some()), Ok(true))
                    };
                    let enable_upload_button = design_valid && matches!(planchette_upload_status, PlanchetteUploadStatus::None);
                    let button = egui::Button::new("Send to Laser");
                    if ui.add_enabled(enable_upload_button, button).on_hover_text(hover_text).clicked() {
                        if let Ok(design_lock) = design_file.read() {
                            if let Some((file, _, _)) = &*design_lock {
                                let receiver = send_job_to_planchette(planchette_url, file, tool_passes, offset);
                                ui_context.send_ui_message(UIMessage::PlanchetteUploadStarted { receiver });
                            }
                        }
                    }

                    match planchette_upload_status {
                        PlanchetteUploadStatus::None => {},
                        PlanchetteUploadStatus::Uploading { .. } => {
                            ui.spinner();
                        },
                        PlanchetteUploadStatus::Failed { .. } => {
                            let text = RichText::new("❌")
                                .color(Color32::DARK_RED)
                                .font(FontId {
                                    size: 11.0,
                                    family: egui::FontFamily::Monospace,
                                });
                            ui.label(text);
                        },
                        PlanchetteUploadStatus::Succeeded { .. } => {
                            // Check mark:
                            let text = RichText::new("✅")
                                .color(Color32::DARK_GREEN)
                                .font(FontId {
                                    size: 11.0,
                                    family: egui::FontFamily::Monospace,
                                });
                            ui.label(text);
                        },
                    }
                });
            });
        })
}

/// Errors that can occur when communicating with Planchette.
#[derive(Debug)]
enum PlanchetteError {
    /// We were unable to construct the URL we want to send the request to.
    FailedToCreateRequest(String),
    /// Sending the request to the Planchette server failed.
    FailedToSendRequest(String),
    /// The server informed us that our request was bad and we should feel bad.
    BadRequest(String),
    /// Hah! We've caught the server misbehaving!
    ServerError(String),
}

/// Ask Planchette to send a design to the laser cutter.
///
/// # Arguments
/// * `planchette_url`: The URL of the Planchette server to send designs to. This is the
///   "root" URL, e.g. `http://ouija.yhs` as opposed to `http://ouija.yhs/jobs`. The appropriate
///   paths will be appended to the provided URL when constructing requests to send to the server.
/// * `design_file`: The design file to be sent to the laser cutter.
/// * `tool_passes`: The tool passes to use to cut the design.
/// * `offset`: The offset to apply to the design, relative to the top-left corner.
///
/// # Returns
/// A oneshot channel that will receive a message when the request has been handled by the
/// Planchette server.
fn send_job_to_planchette(
    planchette_url: &reqwest::Url,
    design_file: &DesignFile,
    tool_passes: &[ToolPass],
    offset: &DesignOffset,
) -> PlanchetteUploadResultReceiver {
    let (tx, rx) = oneshot::channel::<Result<(), PlanchetteError>>();

    let planchette_url = planchette_url.clone();
    let job = PrintJob {
        design_file: design_file.bytes.clone(),
        file_name: design_file.name.clone(),
        tool_passes: tool_passes.to_vec(),
        offset: offset.clone(),
    };

    std::thread::spawn(move || {
        let result = send_job_inner(planchette_url, job);
        let _ = tx.send(result);
    });

    rx
}

/// Send a job to Planchette.
/// This should be called outside of the UI thread as it could block for significant time.
///
/// # Arguments
/// * `planchette_url`: The URL of the Planchette server to send designs to. This is the
///   "root" URL, e.g. `http://ouija.yhs` as opposed to `http://ouija.yhs/jobs`. The appropriate
///   paths will be appended to the provided URL when constructing requests to send to the server.
/// * `job`: The [`PrintJob`] to send to the Planchette server.
///
/// # Returns
/// `Ok(())` if the design has successfully been sent all the way to the the laser cutter.
///
/// # Errors
/// A [`PlanchetteError`] will be provided describing what went wrong.
fn send_job_inner(planchette_url: reqwest::Url, job: PrintJob) -> Result<(), PlanchetteError> {
    let client = reqwest::blocking::Client::new();
    let url = planchette_url
        .join("/jobs")
        .map_err(|err| PlanchetteError::FailedToCreateRequest(err.to_string()))?;

    let response = client
        .post(url)
        .json(&job)
        .send()
        .map_err(|err| PlanchetteError::FailedToSendRequest(err.to_string()))?;

    match response.status() {
        StatusCode::BAD_REQUEST => {
            let response_body = response.text().unwrap_or("Unknown Error".to_string());
            Err(PlanchetteError::BadRequest(response_body))
        }
        StatusCode::INTERNAL_SERVER_ERROR => {
            let response_body = response.text().unwrap_or("Unknown Error".to_string());
            Err(PlanchetteError::ServerError(response_body))
        }
        _ => Ok(()),
    }
}

/// Handle an error produced when trying to cut a design file.
///
/// # Arguments
/// * `ui_context`: The Seance UI context.
/// * `err`: The error that was produced.
fn handle_planchette_error(ui_context: &mut UIContext, err: PlanchetteError) {
    log::error!("Error cutting design: {err:?}");
    let (error, details) = match err {
        PlanchetteError::FailedToCreateRequest(err) => {
            ("Failed to construct request to laser cutter server", err)
        }
        PlanchetteError::FailedToSendRequest(err) => {
            ("Failed to send request to laser cutter server", err)
        }
        PlanchetteError::BadRequest(err) => ("Server rejected the design file", err),
        PlanchetteError::ServerError(err) => ("Server encountered an error", err),
    };
    ui_context.send_ui_message(UIMessage::ShowError {
        error: error.to_string(),
        details: Some(details),
    });
}

/// Draws the main UI (tool paths and design preview).
///
/// # Arguments
/// * `ui`: The UI to draw the widget to.
/// * `ui_context`: The Seance UI context.
/// * `tool_passes`: The passes of the tool head.
/// * `tool_pass_widget_states`: Current states of tool pass widgets.
/// * `design_file`: The loaded design file, if any.
/// * `design_preview_image`: The preview image to draw to the UI.
/// * `preview_zoom_level`: How much the preview image is zoomed in.
/// * `design_move_step_mm`: The current amount to step the design by when moving it.
fn ui_main(
    ui: &mut egui::Ui,
    ui_context: &mut UIContext,
    tool_passes: &mut [ToolPass],
    tool_pass_widget_states: &mut [ToolPassWidgetState],
    design_file: &Arc<RwLock<Option<DesignWithMeta>>>,
    design_preview_image: &mut Option<DesignPreview>,
    preview_zoom_level: f32,
    design_move_step_mm: f32,
) {
    StripBuilder::new(ui)
        .size(Size::relative(0.2).at_least(525.0))
        .size(Size::remainder())
        .horizontal(|mut strip| {
            strip.cell(|ui| {
                tool_passes_widget(ui, ui_context, tool_passes, tool_pass_widget_states);
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
                                    ui_context,
                                    design_file,
                                    design_preview_image,
                                    egui::Vec2 {
                                        x: width,
                                        y: height,
                                    },
                                );
                            });
                        });
                        strip.cell(|ui| {
                            let current_offset = match design_preview_image {
                                Some(preview) => preview.get_design_offset().clone(),
                                None => DesignOffset::default(),
                            };

                            design_preview_navigation(
                                ui,
                                ui_context,
                                preview_zoom_level,
                                design_move_step_mm,
                                &current_offset,
                            );
                        });
                    });
            });
        });
}

/// Draws the navigation panel for the design preview.
///
/// # Arguments
/// * `ui`: The UI to draw the widget to.
/// * `ui_context`: The Seance UI context.
/// * `preview_zoom_level`: How much the preview image is zoomed in.
/// * `design_move_step_mm`: The current amount to step the design by when moving it.
/// * `current_offset`: The current offset of the design.
fn design_preview_navigation(
    ui: &mut egui::Ui,
    ui_context: &mut UIContext,
    preview_zoom_level: f32,
    design_move_step_mm: f32,
    current_offset: &DesignOffset,
) {
    ui.horizontal(|ui| {
        let mut zoom_value = preview_zoom_level;
        let zoom_widget = Slider::new(&mut zoom_value, MIN_ZOOM_LEVEL..=MAX_ZOOM_LEVEL);
        ui.label("Zoom");
        if ui.add(zoom_widget).changed() {
            ui_context.send_ui_message(UIMessage::PreviewZoomLevelChanged { zoom: zoom_value });
        }
    });
    ui.separator();
    ui.label("Position Design");
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            /// How many items in the button grid horizontally.
            const GRID_WIDTH: usize = 3;
            /// How many items in the button grid vertically.
            const GRID_HEIGHT: usize = 3;
            // Buttons to be displayed along with their tooltips and associated events.
            let buttons: [(&str, &str, UIMessage); GRID_WIDTH * GRID_HEIGHT] = [
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
                        let (button_text, tooltip, event) =
                            buttons_iter.next().expect("There must be a button");
                        if ui
                            .button(RichText::new(button_text).font(FontId {
                                size: 24.0,
                                family: egui::FontFamily::Monospace,
                            }))
                            .on_hover_text(tooltip)
                            .clicked()
                        {
                            ui_context.send_ui_message(event);
                        }
                    }
                });
            }
        });
        ui.vertical(|ui| {
            let mut step_value = design_move_step_mm;
            let step_by_widget = Slider::new(
                &mut step_value,
                MINIMUM_DEFAULT_DESIGN_MOVE_STEP_MM..=MAXIMUM_DESIGN_MOVE_STEP_MM,
            );
            ui.label("Step By (mm)");
            if ui.add(step_by_widget).changed() {
                ui_context.send_ui_message(UIMessage::DesignMoveStepChanged { step: step_value });
            }
            ui.label("Position");
            ui.horizontal(|ui| {
                ui.label("X");
                let mut offset_x = current_offset.x;
                let offset_x_slider = DragValue::new(&mut offset_x)
                    .max_decimals(2)
                    .range(0.0..=BED_WIDTH_MM)
                    .clamp_existing_to_range(true);
                if ui.add(offset_x_slider).changed() {
                    ui_context.send_ui_message(UIMessage::DesignOffsetChanged {
                        offset: DesignOffset {
                            x: offset_x,
                            y: current_offset.y,
                        },
                    });
                }

                ui.label("Y");
                let mut offset_y = current_offset.y;
                let offset_y_slider = DragValue::new(&mut offset_y)
                    .max_decimals(2)
                    .range(0.0..=BED_HEIGHT_MM)
                    .clamp_existing_to_range(true);
                if ui.add(offset_y_slider).changed() {
                    ui_context.send_ui_message(UIMessage::DesignOffsetChanged {
                        offset: DesignOffset {
                            x: current_offset.x,
                            y: offset_y,
                        },
                    });
                }
            });
        });
    });
}

/// Draws a widget for displaying/editing the tool passes.
///
/// # Arguments
/// * `ui`: The UI to draw the widget into.
/// * `ui_context`: The Seance UI context.
/// * `tool_passes`: The tool passes to draw.
/// * `tool_pass_widget_states`: The states of the tool pass widgets that we're drawing, should be persistent across frames.
fn tool_passes_widget(
    ui: &mut egui::Ui,
    ui_context: &mut UIContext,
    tool_passes: &mut [ToolPass],
    tool_pass_widget_states: &mut [ToolPassWidgetState],
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
                    let mut widget_size = ui.available_size_before_wrap();
                    widget_size.y = 60.0;
                    let (_, widget_rect) = ui.allocate_space(widget_size);
                    ui.painter()
                        .rect_filled(widget_rect, 2.0, ui.style().visuals.panel_fill);
                    ui.painter().rect_stroke(
                        widget_rect,
                        2.0,
                        Stroke::new(2.0, Color32::DARK_GRAY),
                        StrokeKind::Inside,
                    );

                    let mut child_ui = ui.new_child(
                        UiBuilder::new()
                            .max_rect(widget_rect)
                            .layout(Layout::left_to_right(Align::Center)),
                    );
                    tool_pass_widget(
                        &mut child_ui,
                        ui_context,
                        handle,
                        pass,
                        state.index,
                        &mut tool_pass_widget_states[state.index], // TODO: BAD!
                    );
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
}

impl ToolPassWidgetState {
    /// Creates a new [`ToolPassWidgetState`].
    ///
    /// # Arguments
    /// * `editing`: The aspect of the tool pass that is being edited.
    fn new(editing: ToolPassWidgetEditing) -> Self {
        Self { editing }
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
    /// Tool pass colour is being edited.
    Colour {
        /// The current colour value.
        value: String,
    },
}

/// A single tool pass widget.
///
/// # Arguments
/// * `ui`: The UI to draw the widget into.
/// * `ui_context`: The Seance UI context.
/// * `tool_pass`: The tool pass to draw.
/// * `pass_index`: The index into the tool passes array that is being drawn.
/// * `state`: The state of the widget.
///
/// # Returns
/// An [`egui::Response`].
fn tool_pass_widget(
    ui: &mut egui::Ui,
    ui_context: &mut UIContext,
    handle: egui_dnd::Handle<'_>,
    tool_pass: &ToolPass,
    pass_index: usize,
    state: &mut ToolPassWidgetState,
) -> egui::Response {
    StripBuilder::new(ui)
        .size(Size::exact(30.0))
        .size(Size::remainder())
        .horizontal(|mut strip| {
            // Drag Handle
            strip.cell(|ui| {
                handle.show_drag_cursor_on_hover(true).ui(ui, |ui| {
                    Frame::default().inner_margin(10.0).show(ui, |ui| {
                        ui.label("☰").on_hover_cursor(egui::CursorIcon::Grab);
                    });
                });
            });
            strip.cell(|ui| {
                let margin = Margin {
                    left: 10,
                    right: 10,
                    top: 5,
                    bottom: 5,
                };
                Frame::default().inner_margin(margin).show(ui, |ui| {
                    tool_pass_details_widget(ui, ui_context, tool_pass, pass_index, state);
                });
            });
        })
}

/// Draws the editable details of a tool pass.
///
/// # Arguments
/// * `ui`: The UI to draw to.
/// * `ui_context`: The Seance UI context.
/// * `tool_pass`: The pass to draw.
/// * `pass_index`: The index of the tool pass.
/// * `state`: The state of this tool pass widget.
fn tool_pass_details_widget(
    ui: &mut egui::Ui,
    ui_context: &mut UIContext,
    tool_pass: &ToolPass,
    pass_index: usize,
    state: &mut ToolPassWidgetState,
) {
    StripBuilder::new(ui)
        .sizes(Size::remainder(), 2)
        .vertical(|mut strip| {
            strip.cell(|ui| {
                StripBuilder::new(ui)
                    .sizes(Size::remainder(), 2)
                    .horizontal(|mut strip| {
                        strip.cell(|ui| {
                            tool_pass_name_widget(ui, ui_context, tool_pass, pass_index, state)
                        });
                        strip.cell(|ui| {
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                tool_pass_colour_widget(
                                    ui, ui_context, tool_pass, pass_index, state,
                                );
                            });
                        });
                    });
            });
            strip.cell(|ui| {
                StripBuilder::new(ui)
                    .sizes(Size::remainder(), 2)
                    .horizontal(|mut strip| {
                        strip.cell(|ui| {
                            tool_pass_enable_button_widget(ui, ui_context, tool_pass, pass_index);
                        });
                        strip.cell(|ui| {
                            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                                tool_pass_power_widget(ui, ui_context, tool_pass, pass_index);
                                tool_pass_speed_widget(ui, ui_context, tool_pass, pass_index)
                            });
                        });
                    });
            });
        });
}

/// Draws the name of a tool pass.
///
/// # Arguments
/// * `ui`: The UI to draw to.
/// * `ui_context`: The Seance UI context.
/// * `tool_pass`: The pass to draw.
/// * `pass_index`: The index of the tool pass.
/// * `state`: The state of this tool pass widget.
fn tool_pass_name_widget(
    ui: &mut egui::Ui,
    ui_context: &mut UIContext,
    tool_pass: &ToolPass,
    pass_index: usize,
    state: &mut ToolPassWidgetState,
) {
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
            ui_context.send_ui_message(UIMessage::ToolPassNameChanged {
                index: pass_index,
                name: pen_name.to_string(),
            });
        }

        if text_edit.clicked_elsewhere() {
            ui_context.send_ui_message(UIMessage::ToolPassNameLostFocus);
        }
    } else {
        let text = RichText::new(pen_name).strong().size(20.0);
        let pen_name_label = Label::new(text).truncate().sense(Sense::click());
        let pen_name_widget = ui
            .add(pen_name_label)
            .on_hover_cursor(egui::CursorIcon::Text);
        ui_context.add_widget(
            pen_name_widget.id,
            SeanceUIElement::NameLabel { index: pass_index },
        );

        if pen_name_widget.clicked() {
            ui_context.send_ui_message(UIMessage::ToolPassNameClicked { index: pass_index });
        }
    }
}

/// Draws the editable colour of a tool pass.
///
/// # Arguments
/// * `ui`: The UI to draw to.
/// * `ui_context`: The Seance UI context.
/// * `tool_pass`: The pass to draw.
/// * `pass_index`: The index of the tool pass.
fn tool_pass_colour_widget(
    ui: &mut egui::Ui,
    ui_context: &mut UIContext,
    tool_pass: &ToolPass,
    pass_index: usize,
    state: &mut ToolPassWidgetState,
) {
    let mut colour = *tool_pass.colour();
    if ui.color_edit_button_srgb(&mut colour).changed() {
        ui_context.send_ui_message(UIMessage::ToolPassColourChanged {
            index: pass_index,
            colour,
        });
    };

    let [r, g, b] = tool_pass.colour();
    let colour_u32: u64 = ((*r as u64) << 16) + ((*g as u64) << 8) + (*b as u64);
    if let ToolPassWidgetEditing::Colour { value } = &mut state.editing {
        let text_edit = ui.add(
            TextEdit::singleline(value)
                .horizontal_align(Align::RIGHT)
                .vertical_align(Align::Center),
        );

        ui.ctx()
            .memory_mut(|memory| memory.request_focus(text_edit.id));

        if text_edit.changed() || text_edit.lost_focus() {
            if let Ok(parsed_colour) =
                HexColor::from_str(value).or(HexColor::from_str_without_hash(value))
            {
                ui_context.send_ui_message(UIMessage::ToolPassColourChanged {
                    index: pass_index,
                    colour: [
                        parsed_colour.color().r(),
                        parsed_colour.color().g(),
                        parsed_colour.color().b(),
                    ],
                });
            };
        }

        if text_edit.clicked_elsewhere() {
            ui_context.send_ui_message(UIMessage::ToolPassColourLostFocus);
        }
    } else {
        let colour_label = ui.label(format!("#{colour_u32:06X}"));
        ui_context.add_widget(
            colour_label.id,
            SeanceUIElement::ColourLabel { index: pass_index },
        );
        if colour_label.clicked() {
            ui_context.send_ui_message(UIMessage::ToolPassColourClicked { index: pass_index });
        }
    }
}

/// Draws the Enabled/Disabled status/button for a tool pass.
///
/// # Arguments
/// * `ui`: The UI to draw to.
/// * `ui_context`: The Seance UI context.
/// * `tool_pass`: The tool pass to draw.
/// * `pass_index`: The index of the tool pass.
fn tool_pass_enable_button_widget(
    ui: &mut egui::Ui,
    ui_context: &mut UIContext,
    tool_pass: &ToolPass,
    pass_index: usize,
) {
    let is_dark_mode = ui.ctx().style().visuals.dark_mode;

    let (label, colour) = if *tool_pass.enabled() {
        (
            "Enabled",
            if is_dark_mode {
                Color32::DARK_GREEN
            } else {
                Color32::LIGHT_GREEN
            },
        )
    } else {
        (
            "Disabled",
            if is_dark_mode {
                Color32::DARK_RED
            } else {
                Color32::LIGHT_RED
            },
        )
    };
    let button = egui::Button::new(label).fill(colour);

    if ui.add(button).clicked() {
        ui_context.send_ui_message(UIMessage::ToolPassEnableChanged {
            index: pass_index,
            enabled: !tool_pass.enabled(),
        });
    }
}

/// Draws the editable power of a tool pass.
///
/// # Arguments
/// * `ui`: The UI to draw to.
/// * `ui_context`: The Seance UI context.
/// * `tool_pass`: The tool pass to draw.
/// * `pass_index`: The index of the tool pass.
fn tool_pass_power_widget(
    ui: &mut egui::Ui,
    ui_context: &mut UIContext,
    tool_pass: &ToolPass,
    pass_index: usize,
) {
    let mut power = (*tool_pass.power() as f32) / 10.0;
    let power_slider = DragValue::new(&mut power)
        .max_decimals(1)
        .range(MIN_POWER_VALUE_FLOAT..=MAX_POWER_VALUE_FLOAT)
        .clamp_existing_to_range(true);
    if ui.add(power_slider).changed() {
        ui_context.send_ui_message(UIMessage::ToolPassPowerChanged {
            index: pass_index,
            power: (power * 10.0).round() as u64,
        });
    }
    ui.label("Power %");
}

/// Draws the editable speed of a tool pass.
///
/// # Arguments
/// * `ui`: The UI to draw to.
/// * `ui_context`: The Seance UI context.
/// * `tool_pass`: The tool pass to draw.
/// * `pass_index`: The index of the tool pass.
fn tool_pass_speed_widget(
    ui: &mut egui::Ui,
    ui_context: &mut UIContext,
    tool_pass: &ToolPass,
    pass_index: usize,
) {
    let mut speed = (*tool_pass.speed() as f32) / 10.0;
    let speed_slider = DragValue::new(&mut speed)
        .max_decimals(1)
        .range(MIN_SPEED_VALUE_FLOAT..=MAX_SPEED_VALUE_FLOAT)
        .clamp_existing_to_range(true);
    if ui.add(speed_slider).changed() {
        ui_context.send_ui_message(UIMessage::ToolPassSpeedChanged {
            index: pass_index,
            speed: (speed * 10.0).round() as u64,
        });
    }
    ui.label("Speed %");
}

/// A widget for drawing the preview of a design.
///
/// # Arguments
/// * `ui`: The UI to draw the preview into.
/// * `ui_context`: The Seance UI context.
/// * `design_file`: The design file to draw.
/// * `design_file_preview`: The generated preview.
/// * `size`: How big to draw the preview.
///
/// # Returns
/// An [`egui::Response`].
fn design_file_widget(
    ui: &mut egui::Ui,
    ui_context: &mut UIContext,
    design_file: &Arc<RwLock<Option<DesignWithMeta>>>,
    design_preview: &mut Option<DesignPreview>,
    size: egui::Vec2,
) -> egui::Response {
    ui_context.send_ui_message(UIMessage::DesignPreviewSize {
        size_before_wrap: size,
    });

    let (_, widget_rect) = ui.allocate_space(size);
    ui.painter().rect_stroke(
        widget_rect,
        2.0,
        Stroke::new(2.0, Color32::DARK_GRAY),
        StrokeKind::Inside,
    );

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

    let mut child_ui = ui.new_child(
        UiBuilder::new()
            .max_rect(widget_rect)
            .layout(Layout::left_to_right(Align::Min)),
    );

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
/// * `ui_context`: The Seance UI context.
/// * `err`: The error to display.
/// * `details`: Any details that can be shown along with the error message.
fn error_dialog(
    ctx: &egui::Context,
    ui_context: &mut UIContext,
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
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.label(err);
                if let Some(details) = details {
                    ui.collapsing("Details", |ui| ui.label(details));
                }
                ui.with_layout(Layout::right_to_left(Align::BOTTOM), |ui| {
                    if ui.button("ok").clicked() {
                        ui_context.send_ui_message(UIMessage::CloseErrorDialog);
                    }
                });
            });
            ctx.input(|i| {
                if i.viewport().close_requested()
                    || i.key_pressed(Key::Escape)
                    || i.key_pressed(Key::Enter)
                {
                    // Tell parent to close us.
                    ui_context.send_ui_message(UIMessage::CloseErrorDialog);
                }
            });
        },
    );
}

/// Shows the settings dialog.
///
/// # Arguments
/// * `ctx`: The egui context.
/// * `ui_context`: The Seance UI context.
/// * `settings`: The state of the settings dialog.
fn settings_dialog(
    ctx: &egui::Context,
    ui_context: &mut UIContext,
    settings: &SettingsDialogState,
) {
    let url_valid = reqwest::Url::parse(&settings.planchette_url).is_ok();
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
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("URL to send jobs to");

                    let mut planchette_url = settings.planchette_url.clone();
                    if ui.text_edit_singleline(&mut planchette_url).changed() {
                        ui_context
                            .send_ui_message(UIMessage::PrinterSettingsChanged { planchette_url });
                    }

                    if !url_valid {
                        ui.label("URL is invalid");
                    }
                });

                ui.with_layout(Layout::right_to_left(Align::BOTTOM), |ui| {
                    let save_button = egui::Button::new("Save and Close");
                    if ui.add_enabled(url_valid, save_button).clicked() {
                        ui_context.send_ui_message(UIMessage::SaveSettings);
                        ui_context.send_ui_message(UIMessage::CloseSettingsDialog);
                    }
                    if ui.button("Discard and Close").clicked() {
                        ui_context.send_ui_message(UIMessage::CloseSettingsDialog);
                    }
                });
            });
            ctx.input(|i| {
                if i.viewport().close_requested() || i.key_pressed(Key::Escape) {
                    // Tell parent to close us.
                    ui_context.send_ui_message(UIMessage::CloseSettingsDialog);
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
            let svg = parse_svg(&bytes).map_err(|err| {
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
                    bytes,
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

/// The reason that we're changing focus.
enum FocusChangingReason {
    /// The enter key has been pressed.
    EnterKeyPressed,
    /// The tab key has been pressed.
    TabKeyPressed,
    /// The space key has been pressed.
    SpaceKeyPressed,
    /// The tool pass name has lost focus e.g. due to a click.
    ToolPassNameLostFocus,
    /// The tool pass colour has lost focus e.g. due to a click.
    ToolPassColourLostFocus,
}

/// The focus is changing from one UI element to another.
/// Makes decisions about whether to allow the focus to change and what to do about it.
///
/// # Arguments
/// * `ctx`: The egui context.
/// * `ui_context`: The Seance UI context.
/// * `tool_pass_widgets_states`: The states of all of the tool pass widgets.
/// * `reason`: The reason that focus is changing.
fn focus_changing(
    ctx: &egui::Context,
    ui_context: &mut UIContext,
    tool_pass_widget_states: &mut [ToolPassWidgetState],
    tool_passes: &[ToolPass],
    reason: FocusChangingReason,
) {
    let mut allow_move = true;
    for pen_widget in tool_pass_widget_states.iter_mut() {
        match pen_widget.editing {
            ToolPassWidgetEditing::None => {}
            ToolPassWidgetEditing::Name | ToolPassWidgetEditing::Colour { .. } => match reason {
                FocusChangingReason::SpaceKeyPressed => allow_move = false,
                FocusChangingReason::EnterKeyPressed
                | FocusChangingReason::TabKeyPressed
                | FocusChangingReason::ToolPassNameLostFocus
                | FocusChangingReason::ToolPassColourLostFocus => allow_move = true,
            },
        }

        if allow_move {
            pen_widget.editing = ToolPassWidgetEditing::None;
        }
    }

    if allow_move {
        ctx.memory_mut(|memory| {
            if let Some(id) = memory.focused() {
                if let Some(widget) = ui_context.get_widget(&id) {
                    match widget {
                        SeanceUIElement::NameLabel { index } => {
                            if let Some(pass) = tool_pass_widget_states.get_mut(*index) {
                                pass.editing = ToolPassWidgetEditing::Name;
                            }
                        }
                        SeanceUIElement::ColourLabel { index } => {
                            if let Some(state) = tool_pass_widget_states.get_mut(*index) {
                                if let Some(pass) = tool_passes.get(*index) {
                                    let [r, g, b] = pass.colour();
                                    let colour_u32: u64 =
                                        ((*r as u64) << 16) + ((*g as u64) << 8) + (*b as u64);
                                    state.editing = ToolPassWidgetEditing::Colour {
                                        value: format!("#{colour_u32:06X}"),
                                    };
                                }
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
                    .unwrap_or_else(|| panic!("Could not get character {i}"))
                    .to_uppercase()
                    .to_string();
            } else {
                new_str += &input
                    .chars()
                    .nth(i)
                    .unwrap_or_else(|| panic!("Could not get character {i}"))
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
