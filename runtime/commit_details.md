日時: 2026-03-06 22:57 JST
summary: メイン画面で相談系実装系プロジェクト選択系UIの表示可否を相談画面実装画面の起動状態に連動させた
code_changes:
・src/app/main_parts/part06_shell_commands.rs の is_object_runtime_visible に consult_visible implement_visible project_select_visible 判定を追加した
・相談画面未起動時は 相談起動 停止 相談 を非表示にし実装画面未起動時は 実装起動 停止 実装 デバッグ を非表示にした
・両画面未起動時は cmb_project_selector と btn_project_target_move を非表示にした
verification:
・cargo build が成功した

