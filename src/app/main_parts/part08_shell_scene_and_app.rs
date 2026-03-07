impl CodexShellApp {

    fn render_runtime_ui_objects(&mut self, ctx: &egui::Context) {
        let mut clicked_commands = Vec::new();
        let mut position_changed = false;
        let mut state_changed = self.sync_runtime_bound_states();
        let controls_enabled = !self.ui_edit_mode;
        let object_layer_order = egui::Order::Foreground;
        let mut rendered_layers = Vec::new();
        let current_screen_id = self.ui_current_screen_id.clone();
        let Some(screen_snapshot) = self
            .ui_definition
            .screen_objects(current_screen_id.as_str())
            .cloned()
        else {
            return;
        };
        self.ensure_selected_objects_valid(current_screen_id.as_str());
        if self.ui_edit_mode && self.ui_edit_grid_visible {
            self.render_edit_grid(ctx);
        }
        self.render_modal_screen_tint(ctx, current_screen_id.as_str());
        let mut ordered_indices: Vec<usize> = (0..screen_snapshot.len()).collect();
        ordered_indices.sort_by(|left, right| {
            screen_snapshot[*left]
                .z_index
                .cmp(&screen_snapshot[*right].z_index)
                .then(left.cmp(right))
        });

        for index in ordered_indices {
            let object = screen_snapshot[index].clone();
            if !self.is_object_runtime_visible(&object) {
                continue;
            }

            let object_type = object.object_type.trim().to_string();
            let object_id = object.id.clone();
            let object_command = object.bind.command.trim().to_string();
            let object_size = egui::vec2(object.size.w.max(12.0), object.size.h.max(12.0));
            let text_size = object.visual.text.font_size.max(1.0);
            let requested_family = object.visual.text.font_family.trim();
            let text_font = if !requested_family.is_empty()
                && self.ui_font_names.iter().any(|name| name == requested_family)
            {
                egui::FontId::new(
                    text_size,
                    egui::FontFamily::Name(Arc::from(requested_family.to_string())),
                )
            } else {
                egui::FontId::new(text_size, egui::FontFamily::Proportional)
            };
            let area_interactable = true;
            let mut clicked = false;
            let mut checkbox_changed: Option<bool> = None;
            let mut radio_selected = false;
            let layer_id = egui::LayerId::new(
                object_layer_order,
                egui::Id::new(("ui_object", object_id.clone())),
            );
            rendered_layers.push(layer_id);

            let area_response = egui::Area::new(layer_id.id)
                .order(object_layer_order)
                .interactable(area_interactable)
                .current_pos(egui::pos2(object.position.x, object.position.y))
                .sense(if self.ui_edit_mode {
                    egui::Sense::click_and_drag()
                } else {
                    egui::Sense::hover()
                })
                .show(ctx, |ui| {
                    let mut render_ctx = RenderObjCtx {
                        ui,
                        object: &object,
                        object_id: object_id.as_str(),
                        object_type: object_type.as_str(),
                        object_command: object_command.as_str(),
                        object_size,
                        text_font: &text_font,
                        controls_enabled,
                    };
                    self.render_obj_by_type(
                        &mut render_ctx,
                        &mut state_changed,
                        &mut clicked,
                        &mut checkbox_changed,
                        &mut radio_selected,
                    );
                });

            if object_command == ui_tool::MODE_PROJECT_DEBUG_RUN
                && let Some(modified_hhmm) = self.active_project_debug_modified_hhmm()
            {
                let debug_time_layer = egui::LayerId::new(
                    egui::Order::Foreground,
                    egui::Id::new(("debug_button_time", object_id.clone())),
                );
                let painter = ctx.layer_painter(debug_time_layer);
                painter.text(
                    egui::pos2(
                        area_response.response.rect.left(),
                        area_response.response.rect.bottom() + 2.0,
                    ),
                    egui::Align2::LEFT_TOP,
                    format!("更新日時 {modified_hhmm}"),
                    egui::FontId::new(12.0, egui::FontFamily::Proportional),
                    Color32::BLACK,
                );
            }

            let pointer_clicked_on_area = ctx.input(|input| {
                input.pointer.primary_clicked()
                    && input
                        .pointer
                        .interact_pos()
                        .is_some_and(|pos| area_response.response.rect.contains(pos))
            });
            if self.ui_edit_mode
                && (area_response.response.clicked()
                    || area_response.response.drag_started()
                    || pointer_clicked_on_area)
            {
                self.ui_selected_screen_id = current_screen_id.clone();
                let additive_select = ctx.input(|input| {
                    input.modifiers.ctrl || input.modifiers.command || input.modifiers.shift
                });
                if additive_select {
                    if self.ui_selected_object_ids.is_empty() && !self.ui_selected_object_id.is_empty()
                    {
                        self.ui_selected_object_ids
                            .push(self.ui_selected_object_id.clone());
                    }
                    if !self
                        .ui_selected_object_ids
                        .iter()
                        .any(|selected_id| selected_id == &object_id)
                    {
                        self.ui_selected_object_ids.push(object_id.clone());
                    }
                    if self.ui_selected_object_id.is_empty() {
                        self.ui_selected_object_id = object_id.clone();
                    }
                } else {
                    self.set_primary_selected_object(object_id.clone());
                }
            }

            if let Some(next_checked) = checkbox_changed {
                let Some(screen_objects) = self
                    .ui_definition
                    .screen_objects_mut(current_screen_id.as_str())
                else {
                    continue;
                };
                let target = &mut screen_objects[index];
                if target.checked != next_checked {
                    target.checked = next_checked;
                    state_changed = true;
                    if object_command == ui_tool::CONFIG_SHOW_SIZE_OVERLAY {
                        self.config.show_size_overlay = next_checked;
                    } else if !object_command.is_empty() {
                        clicked_commands.push(object_command.clone());
                    }
                }
            }

            if radio_selected {
                let group_key = Self::radio_group_key(&object);
                let mut group_changed = false;
                let Some(screen_objects) = self
                    .ui_definition
                    .screen_objects_mut(current_screen_id.as_str())
                else {
                    continue;
                };
                for (other_index, other) in screen_objects.iter_mut().enumerate() {
                    if Self::is_radio_object_type(&other.object_type)
                        && Self::radio_group_key(other) == group_key
                    {
                        let next_checked = other_index == index;
                        if other.checked != next_checked {
                            other.checked = next_checked;
                            group_changed = true;
                        }
                    }
                }
                if group_changed {
                    state_changed = true;
                    if !object_command.is_empty() {
                        clicked_commands.push(object_command.clone());
                    }
                }
            }

            if self.ui_edit_mode
                && self
                    .ui_selected_object_ids
                    .iter()
                    .any(|selected_id| selected_id == &object_id)
            {
                let highlight_rect = area_response.response.rect.expand(2.0);
                let painter = ctx.layer_painter(egui::LayerId::new(
                    egui::Order::Tooltip,
                    egui::Id::new(("ui_selected_highlight", object_id.clone())),
                ));
                let is_primary = self.ui_selected_object_id == object_id;
                let (fill_color, stroke_color) = if is_primary {
                    (
                        Color32::from_rgba_unmultiplied(255, 0, 0, 26),
                        Color32::from_rgba_unmultiplied(255, 0, 0, 180),
                    )
                } else {
                    (
                        Color32::from_rgba_unmultiplied(180, 220, 255, 28),
                        Color32::from_rgba_unmultiplied(115, 175, 235, 200),
                    )
                };
                painter.rect(
                    highlight_rect,
                    egui::CornerRadius::same(2),
                    fill_color,
                    egui::Stroke::new(2.0, stroke_color),
                    egui::StrokeKind::Outside,
                );
            }

            if self.ui_edit_mode {
                let drag_delta = area_response.response.drag_delta();
                if drag_delta != egui::Vec2::ZERO
                    && self
                        .ui_selected_object_ids
                        .iter()
                        .any(|selected_id| selected_id == &object_id)
                {
                    let selected_ids = self.ui_selected_object_ids.clone();
                    let Some(screen_objects) = self
                        .ui_definition
                        .screen_objects_mut(current_screen_id.as_str())
                    else {
                        continue;
                    };
                    for target in screen_objects.iter_mut() {
                        if selected_ids
                            .iter()
                            .any(|selected_id| selected_id == &target.id)
                        {
                            target.position.x += drag_delta.x;
                            target.position.y += drag_delta.y;
                            position_changed = true;
                        }
                    }
                }
            }

            if clicked && !object_command.is_empty() {
                clicked_commands.push(object_command);
            }
        }

        if position_changed {
            self.mark_ui_definition_dirty();
        }
        if state_changed {
            // ランタイム同期で変わる checked は即保存しない。
        }
        let popup_open = egui::Popup::is_any_open(ctx);
        if !popup_open {
            ctx.memory_mut(|memory| {
                let areas = memory.areas_mut();
                for layer in rendered_layers {
                    areas.move_to_top(layer);
                }
            });
        }

        if controls_enabled {
            for command in clicked_commands {
                self.dispatch_ui_command(&command);
            }
        }
    }

    fn render_ui_editor(&mut self, ctx: &egui::Context) {
        if !self.ui_edit_mode {
            return;
        }

        let before_screen_id = self.ui_selected_screen_id.clone();
        let before_object_id = self.ui_selected_object_id.clone();
        let events = ui_tool::render_ui_editor_viewport(
            ctx,
            &mut self.ui_definition,
            &mut self.ui_selected_screen_id,
            &mut self.ui_selected_object_id,
            &mut self.ui_selected_object_ids,
            &mut self.ui_edit_grid_visible,
            &self.ui_font_names,
            self.config.show_size_overlay,
            self.window_size,
            self.ui_has_unsaved_changes,
        );
        if self.ui_selected_screen_id != before_screen_id {
            self.ui_current_screen_id = self.ui_selected_screen_id.clone();
        }
        if self.ui_selected_screen_id != before_screen_id
            || self.ui_selected_object_id != before_object_id
        {
            self.set_primary_selected_object(self.ui_selected_object_id.clone());
        } else {
            let selected_screen_id = self.ui_selected_screen_id.clone();
            self.ensure_selected_objects_valid(selected_screen_id.as_str());
        }
        if events.changed {
            self.mark_ui_definition_dirty();
        }
        if events.save_requested {
            let current_size = ctx.content_rect().size();
            if current_size.x > 1.0 && current_size.y > 1.0 {
                self.config.main_window_width = current_size.x;
                self.config.main_window_height = current_size.y;
                self.save_config();
                self.ui_resize_locked_by_save = true;
            }
            self.save_live_ui_definition("UI編集内容を保存しました");
            self.update_status("UI編集内容を保存しました");
        }
        if events.closed {
            self.ui_edit_mode = false;
            self.update_status("UI編集モードを無効化しました");
            self.push_history("UI編集ウィンドウを閉じました");
        }
    }

    fn render_edit_grid(&self, ctx: &egui::Context) {
        let grid_step_px = 10;
        let major_step_px = 50;
        let rect = ctx.content_rect();
        let max_x = rect.right().max(0.0).floor() as i32;
        let max_y = rect.bottom().max(0.0).floor() as i32;
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Tooltip,
            egui::Id::new("ui_edit_grid"),
        ));
        let minor_color = Color32::from_rgba_unmultiplied(190, 160, 220, 70);
        let major_color = Color32::from_rgba_unmultiplied(170, 130, 210, 120);

        let mut x = 0;
        while x <= max_x {
            let is_major = x % major_step_px == 0;
            painter.line_segment(
                [egui::pos2(x as f32, 0.0), egui::pos2(x as f32, max_y as f32)],
                egui::Stroke::new(if is_major { 1.6 } else { 1.0 }, if is_major { major_color } else { minor_color }),
            );
            x += grid_step_px;
        }

        let mut y = 0;
        while y <= max_y {
            let is_major = y % major_step_px == 0;
            painter.line_segment(
                [egui::pos2(0.0, y as f32), egui::pos2(max_x as f32, y as f32)],
                egui::Stroke::new(if is_major { 1.6 } else { 1.0 }, if is_major { major_color } else { minor_color }),
            );
            y += grid_step_px;
        }
    }

    fn render_modal_screen_tint(&self, ctx: &egui::Context, screen_id: &str) {
        if !Self::is_modal_screen(screen_id) {
            return;
        }
        let content_rect = ctx.content_rect();
        let inset_x = (content_rect.width() * 0.05).max(0.0);
        let inset_y = (content_rect.height() * 0.05).max(0.0);
        let overlay_rect = content_rect.shrink2(egui::vec2(inset_x, inset_y));
        let overlay_layer = egui::LayerId::new(
            egui::Order::Middle,
            egui::Id::new("runtime_modal_screen_tint"),
        );
        let painter = ctx.layer_painter(overlay_layer);
        painter.rect(
            overlay_rect,
            egui::CornerRadius::ZERO,
            Color32::from_rgba_unmultiplied(196, 170, 224, 42),
            egui::Stroke::NONE,
            egui::StrokeKind::Middle,
        );
    }

    fn is_modal_screen(screen_id: &str) -> bool {
        let normalized = screen_id.trim();
        if normalized == UI_MAIN_SCREEN_ID {
            return false;
        }
        !Self::is_custom_windows_screen(normalized)
    }

    fn is_custom_windows_screen(screen_id: &str) -> bool {
        let normalized = screen_id.trim().to_ascii_lowercase();
        if normalized.is_empty() {
            return false;
        }
        let looks_like_window_screen = normalized.contains("window")
            || normalized.starts_with("win_")
            || normalized.ends_with("_win");
        looks_like_window_screen && !normalized.contains("modal")
    }

}

impl eframe::App for CodexShellApp {
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        raw_input.events.retain(|event| {
            !matches!(
                event,
                egui::Event::Key {
                    key: egui::Key::ArrowRight,
                    modifiers,
                    ..
                } if modifiers.ctrl && modifiers.alt
            )
        });
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.apply_window_resize_policy(ctx);
        self.drain_codex_exec_result();
        self.reload_ui_definition_if_changed(ctx);
        let next_window_size = ctx.content_rect().size();
        if self.ui_edit_mode {
            let width_changed = (next_window_size.x - self.window_size.x).abs() >= 1.0;
            let height_changed = (next_window_size.y - self.window_size.y).abs() >= 1.0;
            if self.window_size.x > 1.0 && self.window_size.y > 1.0 && (width_changed || height_changed) {
                self.mark_ui_definition_dirty();
            }
        }
        self.window_size = next_window_size;
        self.apply_runtime_background(ctx);

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.allocate_space(egui::Vec2::ZERO);
        });
        self.render_runtime_ui_objects(ctx);

        self.render_ui_editor(ctx);
        ctx.request_repaint_after(Duration::from_millis(UI_RELOAD_CHECK_INTERVAL_MS));
    }

}
