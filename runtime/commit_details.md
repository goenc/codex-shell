日時: 2026-03-05 20:10:19 JST
対象: src/app/main_parts/part07_shell_widgets.rs
summary: 既に移動済みで緑背景のプロジェクト選択時はプロジェクト移動ボタンを無効表示にする
code_changes:
・render_obj_buttonでbtn_project_target_moveかつis_selected_project_highlighted時に無効化条件を追加
・既存のボタン無効表示ルール(add_enabled_uiとグレー文字)に従う表示へ統一
verification:
・cargo build (dev profile) が成功

