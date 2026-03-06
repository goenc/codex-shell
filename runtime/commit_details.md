日時: 2026-03-07 01:25:54 JST
summary: プロジェクト未選択時に起動系ボタンを共通で無効化し緑背景判定を単一化
code_changes:
・is_project_launch_ready を追加して緑背景選択状態の真偽値を共通化
・MODE_CODEX_START と MODE_CODEX_START_B と MODE_PROJECT_DEBUG_RUN を未選択時に共通無効化
・デバッグEXE起動関連の緑背景判定呼び出しを共通ヘルパーへ統一
verification:
・cargo build で dev プロファイルのビルド成功を確認
・UI有効/無効切替と押下不能は判定ロジックをコード上で確認