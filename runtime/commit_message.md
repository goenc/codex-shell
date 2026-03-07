不要ファイル削除と常駐PowerShell前提UI設定整理

・ルートの不要一時ファイル二件を参照確認後に削除した
・_tmp_inject.cs を削除した
・_tmp_vk.cs を削除した
・git grep と rg で対象名の参照が存在しないことを確認した
・cargo build が dev プロファイルで成功した
・常駐PowerShell前提の起動停止UIと設定項目とイベント分岐を削除し単発起動化の前段整理を行った
・AppConfig から codex_command 系と input_prefix と起動時ウィンドウ設定を削除した
・設定画面定義から起動AコマンドA/B 入力先頭付加 ウィンドウ自動起動 PowerShell再起動を削除した
・メイン画面の実装起動 停止 実装ボタンを旧UI定義読み込み時に除去する正規化処理を追加した
・コマンド分岐から mode.codex_start mode.stop mode.build config.restart_listener を削除した
・ConPTYリスナーモードの入口を削除し conpty_listener.rs とモジュール公開を削除した
・ビルド確認ダイアログと関連状態を削除した
・cargo build が dev プロファイルで成功した
・削除対象の設定項目参照が src/app/main_parts から除去されていることを rg で確認した
・git status で対象外ファイルの変更が含まれていないことを確認した