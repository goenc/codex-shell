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
struct UiAssets {
    base_dir: String,
    images: HashMap<String, String>,
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
struct UiObject {
    id: String,
    #[serde(rename = "type")]
    object_type: String,
    z_index: i32,
    checked: bool,
    position: UiPosition,
    size: UiSize,
    visible: bool,
    enabled: bool,
    bind: UiBind,
    visual: UiVisual,
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
struct UiPosition {
    x: f32,
    y: f32,
}

impl Default for UiPosition {
    fn default() -> Self {
        Self { x: 0.0, y: 0.0 }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default)]
struct UiSize {
    w: f32,
    h: f32,
}

impl Default for UiSize {
    fn default() -> Self {
        Self { w: 120.0, h: 32.0 }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default)]
struct UiBind {
    command: String,
    group: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
struct UiVisual {
    background: UiBackground,
    icon: UiIcon,
    text: UiText,
    states: UiStates,
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
struct UiBackground {
    image: String,
    fit: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
struct UiIcon {
    image: String,
    anchor: String,
    offset: UiPosition,
    size: UiSize,
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
struct UiText {
    value: String,
    align: String,
    font_size: f32,
    font_family: String,
    bold: bool,
    italic: bool,
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
struct UiStates {
    hover: UiStateVisual,
    pressed: UiStateVisual,
    disabled: UiStateVisual,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default)]
struct UiStateVisual {
    background: UiBackground,
}

#[derive(Clone, Debug)]
struct ProjectDeclarationEntry {
    name: String,
    path: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CodexRuntimeState {
    Calculating,
    Stopped,
}

impl CodexRuntimeState {
    fn label(self) -> &'static str {
        match self {
            Self::Calculating => "計算中",
            Self::Stopped => "停止中",
        }
    }
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
    codex_runtime_state: CodexRuntimeState,
    codex_runtime_state_b: CodexRuntimeState,
    history: Vec<String>,
    window_size: egui::Vec2,
    input_area_size: egui::Vec2,
    ui_font_names: Vec<String>,
    resize_enabled: bool,
    voice_input_active: bool,
    pending_input_focus: bool,
    ui_resize_locked_by_save: bool,
    project_runtime_active: bool,
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
