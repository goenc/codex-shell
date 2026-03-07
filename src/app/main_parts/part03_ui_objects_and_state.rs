fn create_label_object(
    id: &str,
    text: &str,
    z_index: i32,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    align: &str,
) -> UiObject {
    UiObject {
        id: id.to_string(),
        object_type: "label".to_string(),
        z_index,
        checked: false,
        position: UiPosition { x, y },
        size: UiSize { w, h },
        visible: true,
        enabled: true,
        bind: UiBind::default(),
        visual: UiVisual {
            text: UiText {
                value: text.to_string(),
                align: align.to_string(),
                font_size: 16.0,
                font_family: "noto_sans_jp".to_string(),
                bold: false,
                italic: false,
            },
            ..UiVisual::default()
        },
    }
}

fn create_input_object(
    id: &str,
    command: &str,
    z_index: i32,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) -> UiObject {
    UiObject {
        id: id.to_string(),
        object_type: "input".to_string(),
        z_index,
        checked: false,
        position: UiPosition { x, y },
        size: UiSize { w, h },
        visible: true,
        enabled: true,
        bind: UiBind {
            command: command.to_string(),
            group: String::new(),
        },
        visual: UiVisual::default(),
    }
}

fn create_button_object(
    id: &str,
    text: &str,
    command: &str,
    z_index: i32,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) -> UiObject {
    UiObject {
        id: id.to_string(),
        object_type: "button".to_string(),
        z_index,
        checked: false,
        position: UiPosition { x, y },
        size: UiSize { w, h },
        visible: true,
        enabled: true,
        bind: UiBind {
            command: command.to_string(),
            group: String::new(),
        },
        visual: UiVisual {
            text: UiText {
                value: text.to_string(),
                align: "center".to_string(),
                font_size: 16.0,
                font_family: "noto_sans_jp".to_string(),
                bold: false,
                italic: false,
            },
            ..UiVisual::default()
        },
    }
}

fn create_checkbox_object(
    id: &str,
    text: &str,
    command: &str,
    z_index: i32,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) -> UiObject {
    UiObject {
        id: id.to_string(),
        object_type: "checkbox".to_string(),
        z_index,
        checked: false,
        position: UiPosition { x, y },
        size: UiSize { w, h },
        visible: true,
        enabled: true,
        bind: UiBind {
            command: command.to_string(),
            group: String::new(),
        },
        visual: UiVisual {
            text: UiText {
                value: text.to_string(),
                align: "left".to_string(),
                font_size: 16.0,
                font_family: "noto_sans_jp".to_string(),
                bold: false,
                italic: false,
            },
            ..UiVisual::default()
        },
    }
}

fn create_radio_object(
    id: &str,
    text: &str,
    command: &str,
    group: &str,
    checked: bool,
    z_index: i32,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) -> UiObject {
    UiObject {
        id: id.to_string(),
        object_type: "radio".to_string(),
        z_index,
        checked,
        position: UiPosition { x, y },
        size: UiSize { w, h },
        visible: true,
        enabled: true,
        bind: UiBind {
            command: command.to_string(),
            group: group.to_string(),
        },
        visual: UiVisual {
            text: UiText {
                value: text.to_string(),
                align: "center".to_string(),
                font_size: 16.0,
                font_family: "noto_sans_jp".to_string(),
                bold: false,
                italic: false,
            },
            ..UiVisual::default()
        },
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct UiAssets {
    pub(crate) base_dir: String,
    pub(crate) images: HashMap<String, String>,
}

impl Default for UiAssets {
    fn default() -> Self {
        Self {
            base_dir: "assets/ui".to_string(),
            images: HashMap::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct UiObject {
    pub(crate) id: String,
    #[serde(rename = "type")]
    pub(crate) object_type: String,
    pub(crate) z_index: i32,
    pub(crate) checked: bool,
    pub(crate) position: UiPosition,
    pub(crate) size: UiSize,
    pub(crate) visible: bool,
    pub(crate) enabled: bool,
    pub(crate) bind: UiBind,
    pub(crate) visual: UiVisual,
}

impl Default for UiObject {
    fn default() -> Self {
        Self {
            id: String::new(),
            object_type: "button".to_string(),
            z_index: 0,
            checked: false,
            position: UiPosition::default(),
            size: UiSize::default(),
            visible: true,
            enabled: true,
            bind: UiBind::default(),
            visual: UiVisual::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct UiPosition {
    pub(crate) x: f32,
    pub(crate) y: f32,
}

impl Default for UiPosition {
    fn default() -> Self {
        Self { x: 0.0, y: 0.0 }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct UiSize {
    pub(crate) w: f32,
    pub(crate) h: f32,
}

impl Default for UiSize {
    fn default() -> Self {
        Self { w: 120.0, h: 32.0 }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default)]
pub(crate) struct UiBind {
    pub(crate) command: String,
    pub(crate) group: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct UiVisual {
    pub(crate) background: UiBackground,
    pub(crate) icon: UiIcon,
    pub(crate) text: UiText,
    pub(crate) states: UiStates,
}

impl Default for UiVisual {
    fn default() -> Self {
        Self {
            background: UiBackground::default(),
            icon: UiIcon::default(),
            text: UiText::default(),
            states: UiStates::default(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default)]
pub(crate) struct UiBackground {
    pub(crate) image: String,
    pub(crate) fit: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct UiIcon {
    pub(crate) image: String,
    pub(crate) anchor: String,
    pub(crate) offset: UiPosition,
    pub(crate) size: UiSize,
}

impl Default for UiIcon {
    fn default() -> Self {
        Self {
            image: String::new(),
            anchor: "center".to_string(),
            offset: UiPosition::default(),
            size: UiSize::default(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct UiText {
    pub(crate) value: String,
    pub(crate) align: String,
    pub(crate) font_size: f32,
    pub(crate) font_family: String,
    pub(crate) bold: bool,
    pub(crate) italic: bool,
}

impl Default for UiText {
    fn default() -> Self {
        Self {
            value: String::new(),
            align: "center".to_string(),
            font_size: 16.0,
            font_family: "noto_sans_jp".to_string(),
            bold: false,
            italic: false,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default)]
pub(crate) struct UiStates {
    pub(crate) hover: UiStateVisual,
    pub(crate) pressed: UiStateVisual,
    pub(crate) disabled: UiStateVisual,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default)]
pub(crate) struct UiStateVisual {
    pub(crate) background: UiBackground,
}

#[derive(Clone, Debug)]
struct ProjectDeclarationEntry {
    name: String,
    path: Option<PathBuf>,
}

struct CodexShellApp {
    config: AppConfig,
    ui_definition: UiDefinition,
    ui_live_path: PathBuf,
    ui_last_modified: Option<SystemTime>,
    ui_last_reload_check: Instant,
    ui_edit_mode: bool,
    ui_edit_grid_visible: bool,
    ui_has_unsaved_changes: bool,
    ui_current_screen_id: String,
    ui_selected_screen_id: String,
    ui_selected_object_id: String,
    ui_selected_object_ids: Vec<String>,
    selected_reasoning_effort: String,
    input_command: String,
    status_message: String,
    history: Vec<String>,
    window_size: egui::Vec2,
    input_area_size: egui::Vec2,
    ui_font_names: Vec<String>,
    resize_enabled: bool,
    voice_input_active: bool,
    pending_input_focus: bool,
    codex_exec_in_progress: bool,
    codex_exec_result_rx: Option<Receiver<CodexExecResult>>,
    ui_resize_locked_by_save: bool,
    target_project_dir_path: Option<PathBuf>,
    project_declarations: Vec<ProjectDeclarationEntry>,
    project_selected_index: Option<usize>,
    moved_project_highlight_key: Option<String>,
}

struct RenderObjCtx<'a> {
    ui: &'a mut egui::Ui,
    object: &'a UiObject,
    object_id: &'a str,
    object_type: &'a str,
    object_command: &'a str,
    object_size: egui::Vec2,
    text_font: &'a egui::FontId,
    controls_enabled: bool,
}

struct CodexExecResult {
    input: String,
    status_code: Option<i32>,
    stdout: String,
    stderr: String,
    launch_error: Option<String>,
}
