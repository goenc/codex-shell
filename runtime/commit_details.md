日時: 2026-03-01 23:03:15 JST
対象: codex-shell main.rs ui_command.rs
summary: UIコマンド文字列を新規モジュールへ集約しdispatch周辺の直書き文字列を定数参照へ置換しました
変更:
・ui_command.rsを追加して既存UIコマンド文字列を値変更なしで定数化し一覧を用意しました
・default_settings_screenとdispatch_ui_commandおよび周辺の判定分岐を定数参照へ機械的に置換しました
・dispatch入口で未知UIコマンドをデバッグ時のみ履歴へ記録する事前ログを追加しました
code_changes: 文字列定義の集中管理と参照置換のみを行いロジック仕様は変更していません
確認:
・cargo build が成功しました
・デバッグ実行ファイル起動後に named pipe へ最低1件の送信が成功しました
verification: デバッグビルド成功と最小動作確認成功を確認しました
