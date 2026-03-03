
fn is_radio_object_type(object_type: &str) -> bool {
    matches!(object_type.trim(), "radio" | "radio_button")
}

fn resolve_target_window_size(
    selected_screen_id: &str,
    screen_objects: &[UiObject],
    main_window_size: egui::Vec2,
    created_window_size: egui::Vec2,
) -> egui::Vec2 {
    if selected_screen_id.trim() == UI_MAIN_SCREEN_ID {
        return main_window_size;
    }
    if is_custom_windows_screen(selected_screen_id) {
        return created_window_size;
    }
    detect_modal_inner_window_size(screen_objects).unwrap_or(created_window_size)
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

fn detect_modal_inner_window_size(screen_objects: &[UiObject]) -> Option<egui::Vec2> {
    let panel_size = screen_objects
        .iter()
        .filter(|object| object.visible && object.object_type.trim() == "panel")
        .max_by(|left, right| {
            let left_area = left.size.w * left.size.h;
            let right_area = right.size.w * right.size.h;
            left_area
                .partial_cmp(&right_area)
                .unwrap_or(Ordering::Equal)
        })
        .map(|panel| egui::vec2(panel.size.w.max(0.0), panel.size.h.max(0.0)));
    if panel_size.is_some() {
        return panel_size;
    }

    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for object in screen_objects.iter().filter(|object| object.visible) {
        min_x = min_x.min(object.position.x);
        min_y = min_y.min(object.position.y);
        max_x = max_x.max(object.position.x + object.size.w);
        max_y = max_y.max(object.position.y + object.size.h);
    }
    if !min_x.is_finite() || !min_y.is_finite() || !max_x.is_finite() || !max_y.is_finite() {
        return None;
    }
    Some(egui::vec2((max_x - min_x).max(0.0), (max_y - min_y).max(0.0)))
}

#[derive(Clone, Copy)]
enum AlignMode {
    Top,
    Bottom,
    MiddleVertical,
    DistributeVertical,
    Left,
    Right,
    MiddleHorizontal,
    DistributeHorizontal,
}

fn apply_alignment(
    ui_definition: &mut UiDefinition,
    selected_screen_id: &str,
    selected_object_ids: &[String],
    mode: AlignMode,
) -> bool {
    if selected_object_ids.len() < 2 {
        return false;
    }
    let Some(primary_id) = selected_object_ids.first() else {
        return false;
    };
    let Some(screen_objects) = ui_definition.screen_objects(selected_screen_id) else {
        return false;
    };
    let Some(primary) = screen_objects
        .iter()
        .find(|object| object.id.as_str() == primary_id.as_str())
    else {
        return false;
    };
    let reference_left = primary.position.x;
    let reference_center_x = primary.position.x + (primary.size.w * 0.5);
    let reference_right = primary.position.x + primary.size.w;
    let reference_top = primary.position.y;
    let reference_center_y = primary.position.y + (primary.size.h * 0.5);
    let reference_bottom = primary.position.y + primary.size.h;
    let selected_set: HashSet<&str> = selected_object_ids.iter().map(|id| id.as_str()).collect();

    let Some(screen_objects) = ui_definition.screen_objects_mut(selected_screen_id) else {
        return false;
    };
    let mut changed = false;
    for object in screen_objects.iter_mut() {
        if !selected_set.contains(object.id.as_str()) || object.id == *primary_id {
            continue;
        }
        let before_x = object.position.x;
        let before_y = object.position.y;
        match mode {
            AlignMode::Top => object.position.y = reference_top,
            AlignMode::Bottom => object.position.y = reference_bottom - object.size.h,
            AlignMode::MiddleVertical => {
                object.position.y = reference_center_y - (object.size.h * 0.5)
            }
            AlignMode::Left => object.position.x = reference_left,
            AlignMode::Right => object.position.x = reference_right - object.size.w,
            AlignMode::MiddleHorizontal => {
                object.position.x = reference_center_x - (object.size.w * 0.5);
            }
            AlignMode::DistributeVertical | AlignMode::DistributeHorizontal => continue,
        }
        if object.position.x != before_x || object.position.y != before_y {
            changed = true;
        }
    }
    if matches!(mode, AlignMode::DistributeVertical) {
        let mut selected_objects: Vec<(usize, f32, f32)> = screen_objects
            .iter()
            .enumerate()
            .filter(|(_, object)| selected_set.contains(object.id.as_str()))
            .map(|(index, object)| {
                (
                    index,
                    object.position.y + (object.size.h * 0.5),
                    object.size.h,
                )
            })
            .collect();
        if selected_objects.len() < 3 {
            return changed;
        }
        selected_objects.sort_by(|left, right| {
            left.1
                .partial_cmp(&right.1)
                .unwrap_or(Ordering::Equal)
        });
        let min_center = selected_objects.first().map(|value| value.1).unwrap_or(0.0);
        let max_center = selected_objects.last().map(|value| value.1).unwrap_or(0.0);
        let step = (max_center - min_center) / ((selected_objects.len() - 1) as f32);
        for (order, (index, _center, size_h)) in selected_objects.iter().enumerate() {
            let target_center = min_center + step * order as f32;
            let target_y = target_center - (size_h * 0.5);
            if screen_objects[*index].position.y != target_y {
                screen_objects[*index].position.y = target_y;
                changed = true;
            }
        }
    } else if matches!(mode, AlignMode::DistributeHorizontal) {
        let mut selected_objects: Vec<(usize, f32, f32)> = screen_objects
            .iter()
            .enumerate()
            .filter(|(_, object)| selected_set.contains(object.id.as_str()))
            .map(|(index, object)| {
                (
                    index,
                    object.position.x + (object.size.w * 0.5),
                    object.size.w,
                )
            })
            .collect();
        if selected_objects.len() < 3 {
            return changed;
        }
        selected_objects.sort_by(|left, right| {
            left.1
                .partial_cmp(&right.1)
                .unwrap_or(Ordering::Equal)
        });
        let min_center = selected_objects.first().map(|value| value.1).unwrap_or(0.0);
        let max_center = selected_objects.last().map(|value| value.1).unwrap_or(0.0);
        let step = (max_center - min_center) / ((selected_objects.len() - 1) as f32);
        for (order, (index, _center, size_w)) in selected_objects.iter().enumerate() {
            let target_center = min_center + step * order as f32;
            let target_x = target_center - (size_w * 0.5);
            if screen_objects[*index].position.x != target_x {
                screen_objects[*index].position.x = target_x;
                changed = true;
            }
        }
    }
    changed
}
