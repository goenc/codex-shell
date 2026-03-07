日時: 2026-03-07 21:14:08 JST
対象: codex-shell
summary: 既存のプロジェクト選択結果をCodex単発実行の作業フォルダとして利用するように変更
code_changes:
・送信処理で target_project_dir_path を参照し未選択時は実行を中止して状態表示と履歴に理由を記録
・Codex実行処理の引数に作業フォルダを追加し Command::new("codex").current_dir(selected_path) を常時設定
・実行開始ステータスと履歴に実行フォルダを表示して確認可能に変更
verification:
・cargo build が dev プロファイルで成功
日時: 2026-03-07 21:34:53 JST
対象: codex-shell
summary: 未使用機能削除とモジュール整理を実施しruntimeのinit/live分離を明確化
code_changes:
・src/main.rs の include! 依存を撤廃し app::run() 呼び出しの通常モジュール構成へ変更
・未使用コマンドIDと互換専用設定キー、旧ランタイム状態分岐、未使用設定 build_command をコードと参照元から削除
・runtime/ui は init/ui.json をGit雛形、live/ui.json を起動時生成に変更し .gitignore で生成物を除外
・未使用依存クレートを Cargo.toml から削除し windows feature を使用分へ縮小
verification:
・cargo build が dev プロファイルで成功
・cargo test が 5件成功
・rg で削除対象の定数名と設定キー名の残存参照が無いことを確認

