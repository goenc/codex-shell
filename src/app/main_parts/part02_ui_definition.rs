#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
struct AppConfig {
    working_dir: String,
    build_command: String,
    build_root_dir: String,
    startup_exe_1: String,
    startup_exe_2: String,
    startup_exe_3: String,
    startup_exe_4: String,
    show_size_overlay: bool,
    main_window_width: f32,
    main_window_height: f32,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            working_dir: std::env::current_dir()
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_else(|_| ".".to_string()),
            build_command: DEFAULT_BUILD_COMMAND.to_string(),
            build_root_dir: std::env::current_dir()
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_else(|_| ".".to_string()),
            startup_exe_1: String::new(),
            startup_exe_2: String::new(),
            startup_exe_3: String::new(),
            startup_exe_4: String::new(),
            show_size_overlay: true,
            main_window_width: FIXED_WINDOW_WIDTH,
            main_window_height: FIXED_WINDOW_HEIGHT,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
struct UiDefinition {
    version: u32,
    assets: UiAssets,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    objects: Vec<UiObject>,
    screens: Vec<UiScreen>,
}

impl Default for UiDefinition {
    fn default() -> Self {
        Self {
            version: 1,
            assets: UiAssets::default(),
            objects: Vec::new(),
            screens: Vec::new(),
        }
    }
}

impl UiDefinition {
    fn normalize_screens(&mut self) {
        let legacy_objects = std::mem::take(&mut self.objects);
        if self.screens.is_empty() {
            self.screens.push(UiScreen {
                id: UI_MAIN_SCREEN_ID.to_string(),
                objects: legacy_objects,
            });
        } else if !legacy_objects.is_empty() {
            if let Some(main_screen_index) = self.screen_index(UI_MAIN_SCREEN_ID) {
                if self.screens[main_screen_index].objects.is_empty() {
                    self.screens[main_screen_index].objects = legacy_objects;
                } else {
                    self.objects.clear();
                }
            } else {
                self.screens.push(UiScreen {
                    id: UI_MAIN_SCREEN_ID.to_string(),
                    objects: legacy_objects,
                });
            }
        }

        if self.screen(UI_MAIN_SCREEN_ID).is_none() {
            self.screens.push(UiScreen {
                id: UI_MAIN_SCREEN_ID.to_string(),
                objects: Vec::new(),
            });
        }
        if self.screen(UI_SETTINGS_SCREEN_ID).is_none() {
            self.screens.push(default_settings_screen());
        }
        self.remove_deprecated_persistent_shell_objects();
        self.remove_legacy_runtime_status_labels();
        self.ensure_settings_build_root_field();
        self.ensure_input_send_button();
        self.relocate_reasoning_controls_to_settings();
        ensure_project_target_move_button(self);
        self.objects.clear();
    }

    fn remove_deprecated_persistent_shell_objects(&mut self) {
        const DEPRECATED_COMMANDS: &[&str] = &[
            ui_tool::MODE_CODEX_START,
            ui_tool::MODE_CODEX_START_B,
            ui_tool::MODE_STOP,
            ui_tool::MODE_STOP_B,
            ui_tool::MODE_BUILD,
            ui_tool::CONFIG_CODEX_COMMAND,
            ui_tool::CONFIG_CODEX_COMMAND_A,
            ui_tool::CONFIG_CODEX_COMMAND_B,
            ui_tool::CONFIG_INPUT_PREFIX,
            ui_tool::CONFIG_OPEN_CONSULTATION_WINDOW_ON_STARTUP,
            ui_tool::CONFIG_OPEN_IMPLEMENTATION_WINDOW_ON_STARTUP,
        ];
        const DEPRECATED_IDS: &[&str] = &[
            "btn_codex_start",
            "btn_codex_start_b",
            "btn_stop",
            "btn_stop_b",
            "btn_build",
            "lbl_settings_codex",
            "input_settings_codex",
            "lbl_settings_codex_b",
            "input_settings_codex_b",
            "lbl_settings_input_prefix",
            "input_settings_input_prefix",
            "chk_settings_open_consultation_window_on_startup",
            "chk_settings_open_implementation_window_on_startup",
            "btn_settings_restart",
        ];
        for screen in &mut self.screens {
            screen.objects.retain(|object| {
                !DEPRECATED_IDS.contains(&object.id.as_str())
                    && !DEPRECATED_COMMANDS.contains(&object.bind.command.trim())
            });
        }
    }

    fn ensure_input_send_button(&mut self) {
        let Some(main_objects) = self.screen_objects_mut(UI_MAIN_SCREEN_ID) else {
            return;
        };
        if main_objects
            .iter()
            .any(|object| {
                object.id == "btn_input_send"
                    || object.bind.command.trim() == ui_tool::INPUT_SEND
            })
        {
            return;
        }
        let input_rect = main_objects
            .iter()
            .find(|object| object.id == "input_command")
            .map(|object| (object.position.x, object.position.y, object.size.w, object.size.h));
        let (input_x, input_y, input_w, input_h) = input_rect.unwrap_or((37.0, 103.0, 763.0, 220.0));
        main_objects.push(create_button_object(
            "btn_input_send",
            "送信",
            ui_tool::INPUT_SEND,
            70,
            input_x + input_w + 8.0,
            input_y,
            96.0,
            input_h.min(50.0).max(40.0),
        ));
    }

    fn remove_legacy_runtime_status_labels(&mut self) {
        let Some(main_objects) = self.screen_objects_mut(UI_MAIN_SCREEN_ID) else {
            return;
        };
        main_objects.retain(|object| {
            object.id != "lbl_codex_state"
                && object.id != "lbl_codex_state_b"
                && object.id != "lbl_project_target"
        });
    }

    fn ensure_settings_build_root_field(&mut self) {
        let Some(settings_objects) = self.screen_objects_mut(UI_SETTINGS_SCREEN_ID) else {
            return;
        };
        let mut input_rect: Option<(f32, f32, f32, f32, i32)> = None;
        let mut has_browse = false;
        for object in settings_objects.iter_mut() {
            if object.id == "lbl_settings_build" || object.id == "lbl_settings_build_root" {
                object.id = "lbl_settings_build_root".to_string();
                object.visual.text.value = "ビルルート".to_string();
            }
            if object.id == "input_settings_build" || object.id == "input_settings_build_root" {
                object.id = "input_settings_build_root".to_string();
                object.bind.command = ui_tool::CONFIG_BUILD_ROOT_DIR.to_string();
                input_rect = Some((
                    object.position.x,
                    object.position.y,
                    object.size.w,
                    object.size.h,
                    object.z_index,
                ));
            }
            if object.id == "btn_settings_build_root_browse"
                || object.bind.command.trim() == ui_tool::CONFIG_BUILD_ROOT_DIR_BROWSE
            {
                object.id = "btn_settings_build_root_browse".to_string();
                object.bind.command = ui_tool::CONFIG_BUILD_ROOT_DIR_BROWSE.to_string();
                object.visual.text.value = "参照".to_string();
                has_browse = true;
            }
        }
        if has_browse {
            return;
        }
        let (input_x, input_y, input_w, input_h, input_z) =
            input_rect.unwrap_or((156.0, 96.0, 640.0, 24.0, 110));
        settings_objects.push(create_button_object(
            "btn_settings_build_root_browse",
            "参照",
            ui_tool::CONFIG_BUILD_ROOT_DIR_BROWSE,
            input_z + 10,
            input_x + input_w + 8.0,
            input_y,
            72.0,
            input_h,
        ));
    }

    fn relocate_reasoning_controls_to_settings(&mut self) {
        let reasoning_commands = [
            ui_tool::REASONING_MEDIUM,
            ui_tool::REASONING_HIGH,
            ui_tool::REASONING_XHIGH,
        ];
        let reasoning_ids = [
            "lbl_reasoning_effort",
            "lbl_settings_reasoning_effort",
            "radio_reasoning_medium",
            "radio_reasoning_high",
            "radio_reasoning_xhigh",
            "radio_settings_reasoning_medium",
            "radio_settings_reasoning_high",
            "radio_settings_reasoning_xhigh",
        ];
        if let Some(main_objects) = self.screen_objects_mut(UI_MAIN_SCREEN_ID) {
            main_objects.retain(|object| {
                !reasoning_ids.contains(&object.id.as_str())
                    && !reasoning_commands.contains(&object.bind.command.trim())
            });
        }

        let Some(settings_objects) = self.screen_objects_mut(UI_SETTINGS_SCREEN_ID) else {
            return;
        };
        let has_label = settings_objects.iter().any(|object| {
            object.id == "lbl_settings_reasoning_effort" || object.id == "lbl_reasoning_effort"
        });
        let has_medium = settings_objects
            .iter()
            .any(|object| object.bind.command.trim() == ui_tool::REASONING_MEDIUM);
        let has_high = settings_objects
            .iter()
            .any(|object| object.bind.command.trim() == ui_tool::REASONING_HIGH);
        let has_xhigh = settings_objects
            .iter()
            .any(|object| object.bind.command.trim() == ui_tool::REASONING_XHIGH);

        if !has_label {
            settings_objects.push(create_label_object(
                "lbl_settings_reasoning_effort",
                "思考深度",
                100,
                320.0,
                18.0,
                72.0,
                28.0,
                "left",
            ));
        }
        if !has_medium {
            settings_objects.push(create_radio_object(
                "radio_settings_reasoning_medium",
                "medium",
                ui_tool::REASONING_MEDIUM,
                "reasoning_effort",
                true,
                110,
                396.0,
                18.0,
                78.0,
                28.0,
            ));
        }
        if !has_high {
            settings_objects.push(create_radio_object(
                "radio_settings_reasoning_high",
                "high",
                ui_tool::REASONING_HIGH,
                "reasoning_effort",
                false,
                110,
                482.0,
                18.0,
                66.0,
                28.0,
            ));
        }
        if !has_xhigh {
            settings_objects.push(create_radio_object(
                "radio_settings_reasoning_xhigh",
                "xhigh",
                ui_tool::REASONING_XHIGH,
                "reasoning_effort",
                false,
                110,
                556.0,
                18.0,
                72.0,
                28.0,
            ));
        }
    }

    fn screen_ids(&self) -> Vec<String> {
        self.screens.iter().map(|screen| screen.id.clone()).collect()
    }

    fn screen(&self, screen_id: &str) -> Option<&UiScreen> {
        self.screens.iter().find(|screen| screen.id == screen_id)
    }

    fn screen_index(&self, screen_id: &str) -> Option<usize> {
        self.screens.iter().position(|screen| screen.id == screen_id)
    }

    fn screen_mut(&mut self, screen_id: &str) -> Option<&mut UiScreen> {
        self.screens
            .iter_mut()
            .find(|screen| screen.id == screen_id)
    }

    fn object_index_in_screen(&self, screen_id: &str, object_id: &str) -> Option<usize> {
        self.screen(screen_id)?
            .objects
            .iter()
            .position(|object| object.id == object_id)
    }

    fn screen_objects(&self, screen_id: &str) -> Option<&Vec<UiObject>> {
        Some(&self.screen(screen_id)?.objects)
    }

    fn screen_objects_mut(&mut self, screen_id: &str) -> Option<&mut Vec<UiObject>> {
        Some(&mut self.screen_mut(screen_id)?.objects)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
struct UiScreen {
    id: String,
    objects: Vec<UiObject>,
}

impl Default for UiScreen {
    fn default() -> Self {
        Self {
            id: String::new(),
            objects: Vec::new(),
        }
    }
}

fn ensure_project_target_move_button(definition: &mut UiDefinition) {
    let Some(objects) = definition.screen_objects_mut(UI_MAIN_SCREEN_ID) else {
        return;
    };
    let combo_rect = objects.iter_mut().find(|object| object.id == "cmb_project_selector");
    if let Some(combo) = combo_rect {
        combo.bind.command = ui_tool::MODE_PROJECT_TARGET_MOVE.to_string();
    }
    let combo_rect = objects
        .iter()
        .find(|object| object.id == "cmb_project_selector")
        .map(|object| (object.position.x, object.position.y, object.size.w, object.size.h));
    let Some((combo_x, combo_y, combo_w, combo_h)) = combo_rect else {
        return;
    };

    if let Some(button) = objects
        .iter_mut()
        .find(|object| object.id == "btn_project_target_move")
    {
        if button.bind.command.trim().is_empty() {
            button.bind.command = ui_tool::MODE_PROJECT_TARGET_MOVE.to_string();
        }
        return;
    }

    let button_w = 96.0;
    let mut button = create_button_object(
        "btn_project_target_move",
        "このフォルダへ移動",
        ui_tool::MODE_PROJECT_TARGET_MOVE,
        93,
        combo_x + combo_w + 8.0,
        combo_y,
        button_w,
        combo_h.max(24.0),
    );
    button.visual.text.font_size = 14.0;
    objects.push(button);
}

fn default_settings_screen() -> UiScreen {
    let mut objects = vec![
        create_label_object("lbl_settings_title", "設定", 100, 24.0, 18.0, 240.0, 28.0, "left"),
        create_label_object(
            "lbl_settings_working_dir",
            "起動フォルダ",
            100,
            24.0,
            64.0,
            120.0,
            24.0,
            "left",
        ),
        create_input_object(
            "input_settings_working_dir",
            ui_tool::CONFIG_WORKING_DIR,
            110,
            156.0,
            64.0,
            640.0,
            24.0,
        ),
        create_label_object("lbl_settings_build_root", "ビルルート", 100, 24.0, 96.0, 120.0, 24.0, "left"),
        create_input_object(
            "input_settings_build_root",
            ui_tool::CONFIG_BUILD_ROOT_DIR,
            110,
            156.0,
            96.0,
            640.0,
            24.0,
        ),
        create_button_object(
            "btn_settings_build_root_browse",
            "参照",
            ui_tool::CONFIG_BUILD_ROOT_DIR_BROWSE,
            120,
            804.0,
            96.0,
            72.0,
            24.0,
        ),
        create_label_object(
            "lbl_settings_startup_exe_1",
            "自動起動EXE1",
            100,
            24.0,
            128.0,
            120.0,
            24.0,
            "left",
        ),
        create_input_object(
            "input_settings_startup_exe_1",
            ui_tool::CONFIG_STARTUP_EXE_1,
            110,
            156.0,
            128.0,
            640.0,
            24.0,
        ),
        create_button_object(
            "btn_settings_startup_exe_1_browse",
            "参照",
            ui_tool::CONFIG_STARTUP_EXE_1_BROWSE,
            120,
            804.0,
            128.0,
            72.0,
            24.0,
        ),
        create_label_object(
            "lbl_settings_startup_exe_2",
            "自動起動EXE2",
            100,
            24.0,
            156.0,
            120.0,
            24.0,
            "left",
        ),
        create_input_object(
            "input_settings_startup_exe_2",
            ui_tool::CONFIG_STARTUP_EXE_2,
            110,
            156.0,
            156.0,
            640.0,
            24.0,
        ),
        create_button_object(
            "btn_settings_startup_exe_2_browse",
            "参照",
            ui_tool::CONFIG_STARTUP_EXE_2_BROWSE,
            120,
            804.0,
            156.0,
            72.0,
            24.0,
        ),
        create_label_object(
            "lbl_settings_startup_exe_3",
            "自動起動EXE3",
            100,
            24.0,
            184.0,
            120.0,
            24.0,
            "left",
        ),
        create_input_object(
            "input_settings_startup_exe_3",
            ui_tool::CONFIG_STARTUP_EXE_3,
            110,
            156.0,
            184.0,
            640.0,
            24.0,
        ),
        create_button_object(
            "btn_settings_startup_exe_3_browse",
            "参照",
            ui_tool::CONFIG_STARTUP_EXE_3_BROWSE,
            120,
            804.0,
            184.0,
            72.0,
            24.0,
        ),
        create_label_object(
            "lbl_settings_startup_exe_4",
            "自動起動EXE4",
            100,
            24.0,
            212.0,
            120.0,
            24.0,
            "left",
        ),
        create_input_object(
            "input_settings_startup_exe_4",
            ui_tool::CONFIG_STARTUP_EXE_4,
            110,
            156.0,
            212.0,
            640.0,
            24.0,
        ),
        create_button_object(
            "btn_settings_startup_exe_4_browse",
            "参照",
            ui_tool::CONFIG_STARTUP_EXE_4_BROWSE,
            120,
            804.0,
            212.0,
            72.0,
            24.0,
        ),
        create_checkbox_object(
            "chk_settings_show_size_overlay",
            "サイズ表示を表示",
            ui_tool::CONFIG_SHOW_SIZE_OVERLAY,
            110,
            24.0,
            248.0,
            280.0,
            28.0,
        ),
        create_button_object(
            "btn_settings_save",
            "設定保存",
            ui_tool::CONFIG_SAVE,
            120,
            24.0,
            288.0,
            120.0,
            28.0,
        ),
        create_button_object(
            "btn_settings_back",
            "閉じる",
            ui_tool::NAV_BACK_MAIN,
            120,
            152.0,
            288.0,
            120.0,
            28.0,
        ),
        create_checkbox_object(
            "chk_settings_ui_edit",
            "UI編集",
            ui_tool::UI_EDIT_TOGGLE,
            130,
            280.0,
            288.0,
            120.0,
            28.0,
        ),
    ];
    for object in &mut objects {
        if object.id == "chk_settings_ui_edit" {
            object.checked = false;
        }
    }
    UiScreen {
        id: UI_SETTINGS_SCREEN_ID.to_string(),
        objects,
    }
}
