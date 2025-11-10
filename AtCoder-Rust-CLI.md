# AtCoder-Rust-CLI

## 機能

- kp init   : 初期設定
- kp config : 設定変更
- kp new    : パッケージ作成
- kp test   : テスト実行
- kp submit : 提出
- kp open   : ブラウザで問題を開く
- kp temp   : 一問だけだけ解きたいとき

## 使い方

### 必要環境

- **Rust & Cargo**  
  プロジェクトのビルドに必要です。

- **AtCoder CLI**  
  テストのfetchや提出に使用します。

- **オンラインジャッジツール (`oj`)**  
  テストの実行に使用されます。

### init

`$ kp init`  
初期設定を行うためのコマンド。初めにこのコマンドを実行する必要がある。  
実行に必要なツールが一式揃っているか確認する。  
設定ファイル(kp-config.toml)がなければ生成し、2回目以降はテンプレートのgit pullのみ行う。

実行されるコマンド例:

```sh
acc config-dir
git pull (if <config_dir>/kp-rust exists; cwd=<config_dir>/kp-rust)
git clone https://github.com/wogikaze/kp-rust (else; cwd=<config_dir>)
acc config default-template
acc config default-template kp-rust (only if current != "kp-rust")
acc config default-task-dirname-format ./
acc config default-task-choice all
```

質問(config): default

- template-repository-url: <https://github.com/wogikaze/kp-rust>

### config

`$ kp config (list|set) {key} {value}`  
kpの設定に関するコマンド。

| config | 説明 | 備考 |
|--|--|--|
| template_repository_url | テンプレートリポジトリのURL | |
| minify_on_submit | 提出時にコードをminifyするか | true/false |

変更したら、initのコマンドを自動で実行する(cd->clone->acc config)

### new

`$ kp new {contest_id} (--open)`
新しくコンテストのためのパッケージを作成するコマンド。
--openをつけることでブラウザで自動的に開く。

実行される内容:

```sh
acc new {contest_id}
# Cargo.tomlの編集: [[bin]]セクションの追加, nameの設定
# .vscode/settings.jsonの編集: rust-analyzerの設定追加
```

### test

`$ kp test ({contest_id}) {problem_id}`
指定した問題に対してテストを実行するコマンド。
実行される内容:

```sh
(cd {contest_id} && )
oj test -c "cargo run --bin {problem_id}" -d {problem_id}/tests
```

### submit

`$ kp submit ({contest_id}) {problem_id}`
指定した問題に対して提出を行うコマンド。
実行される内容:

```sh
(cd {contest_id} && )
oj submit -c "cargo run --bin {problem_id}" -d {problem_id}/tests
```

### open

`$ kp open {problem_id}`  
ブラウザで問題のページを開く。

```sh
(cd {contest_id} && )
# ブラウザで問題ページを開く
```

### temp

`$ kp temp {contest_id} {problem_id} (--test|--submit)`
一問だけ解きたいときに使うコマンド。
--testでテスト実行、--submitで提出を行う。
ファイル構造は以下のようになる。

```
temp/
  └── Cargo.toml
  └── src/
      └── {contest_id}{problem_id}.rs
  └── {contest_id}{problem_id}_tests/
```
実行される内容:
- Cargo.tomlに[[bin]]セクションの追加, nameの設定
- テストディレクトリの作成

## 今後の対応予定

### すぐに対応させられること

- 自作テンプレートの利用
- windows, linux対応

### 対応させたいこと

- 自作ライブラリの導入への対応
- 使っていないコードを削除するminify

### やらないこと

- 入力の自動生成
- 生成時の自動ビルド
- targetディレクトリを共有することでビルドを高速化できる?
- バイナリ提出への対応
