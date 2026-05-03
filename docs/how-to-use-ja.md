# How To Use

このドキュメントは、現時点の実装での使い方です。まだ MVP の土台なので、UI やアカウント管理は最小限です。

## 1. ローカル開発モード

PostgreSQL なしで動かす場合、サーバーはメモリ保存になります。このときだけ、表示名だけで入る `Dev` ログインが既定で有効です。

```bash
cargo run -p light-discord-server
```

別ターミナルでクライアントを起動します。

```bash
cargo run -p light-discord-client
```

クライアントでは次のように操作します。

- `Server`: `127.0.0.1:41610`
- auth mode: `Dev`
- `Name`: 任意の表示名
- `Connect`

このモードは開発用です。公開サーバーでは使わないでください。

## 2. PostgreSQL ありの self-hosted モード

PostgreSQL を使う場合、ログイン/セッションが必須になります。`light-discord-server` から接続できる PostgreSQL が起動している必要があります。

PostgreSQL は次のどれでも構いません。

- 同じサーバー上で動く PostgreSQL
- 別サーバー上で動く PostgreSQL
- Docker Compose の PostgreSQL
- managed PostgreSQL

まず同じ Linux サーバー上に PostgreSQL を入れる場合は、セットアップスクリプトを使えます。

```bash
export LD_PG_DB=light_discord
export LD_PG_USER=light_discord
export LD_PG_PASSWORD='replace-with-a-long-random-password'
scripts/setup-postgres-linux.sh
```

このスクリプトは以下を行います。

- PostgreSQL server/client package のインストール
- PostgreSQL service の起動
- `LD_PG_DB` の database 作成
- `LD_PG_USER` の database user 作成
- password 設定
- `LD_DATABASE_URL` の例を表示

外部公開用の `listen_addresses` や `pg_hba.conf` は自動変更しません。まずは同一ホストの `localhost` 接続として使う想定です。

手動でパッケージを入れる場合:

```bash
# Debian / Ubuntu
sudo apt-get update
sudo apt-get install -y postgresql postgresql-client

# Fedora / RHEL 系
sudo dnf install -y postgresql-server postgresql-contrib
sudo postgresql-setup --initdb
sudo systemctl enable --now postgresql

# openSUSE
sudo zypper --non-interactive install postgresql-server postgresql-contrib
sudo systemctl enable --now postgresql
```

DB 接続確認:

```bash
export LD_DATABASE_URL=postgres://light_discord:your-password@localhost:5432/light_discord
scripts/check-postgres.sh
```

接続できることを確認したら、初期管理者を指定してサーバーを起動します。

```bash
export LD_DATABASE_URL=postgres://light_discord:your-password@localhost:5432/light_discord
export LD_BOOTSTRAP_ADMIN_NAME=admin
export LD_BOOTSTRAP_ADMIN_PASSWORD='change-this-password'
export LD_BOOTSTRAP_INVITE_CODE='first-friend-invite'
export LD_DEV_AUTH=0
cargo run -p light-discord-server
```

クライアントを起動します。

```bash
cargo run -p light-discord-client
```

管理者でログインします。

- auth mode: `Login`
- `Name`: `admin`
- `Password`: `LD_BOOTSTRAP_ADMIN_PASSWORD` に設定した値
- `Connect`

ログインに成功すると、サーバーは session token を返します。現在のクライアントはその token をメモリ上に保持します。まだファイル保存はしていません。

## 3. 友達を招待する

管理者で接続すると、左サイドバーに `Admin` セクションが出ます。

1. 必要なら invite note を入力します。
2. `Invite` を押します。
3. 生成された invite code を友達に共有します。

`LD_BOOTSTRAP_INVITE_CODE` を設定して起動した場合、そのコードも初回登録用として使えます。

## 4. 招待されたユーザーの登録

友達側のクライアントで次のように操作します。

- auth mode: `Register`
- `Name`: 使いたい表示名
- `Password`: 8文字以上
- `Invite`: 共有された invite code
- `Connect`

登録に成功すると、そのユーザー用の session token が返ります。次回以降は `Login` または `Session` を使えます。

## 5. 通常ログイン

登録済みユーザーは次のようにログインします。

- auth mode: `Login`
- `Name`: 登録した表示名
- `Password`: 登録したパスワード
- `Connect`

## 6. セッション再開

セッション token がある場合は次のように接続できます。

- auth mode: `Session`
- `Session token`: 以前返された token
- `Connect`

現時点ではクライアントが session token をディスクへ保存しないため、アプリを終了すると token は失われます。永続保存は今後の実装対象です。

## 7. チャットと削除

- 中央下部の入力欄にメッセージを書き、`Send` を押します。
- 自分が投稿したメッセージには `Delete` ボタンが出ます。
- 削除したメッセージは通常のチャンネル履歴から消えます。
- 削除内容は管理者専用の監査ログに本文 snapshot 付きで保存されます。

## 8. 監査ログを見る

管理者で接続して、左サイドバーの `Admin` セクションから `Audit` を押します。中央に `Audit log` が表示されます。

監査ログには主に以下が含まれます。

- action
- 削除実行者 user id
- 対象 message id
- channel id
- 削除された本文 snapshot
- 時刻

## 9. Docker について

この作業環境は Docker コンテナ内です。ただし、このコンテナ内には `docker` CLI が入っていないため、ここから `docker compose up` は実行できません。

ホスト側に Docker がある場合は、ホスト側のターミナルで実行してください。

```bash
cd deploy
export LD_BOOTSTRAP_ADMIN_PASSWORD='change-this-password'
export LD_BOOTSTRAP_INVITE_CODE='share-this-once'
docker compose up --build
```

この場合、server と PostgreSQL は Compose で起動します。クライアントはホスト側で `cargo run -p light-discord-client` するか、別途パッケージ化した実行ファイルから接続します。

## 10. テスト

通常の Rust テスト:

```bash
cargo test --workspace
```

PostgreSQL 統合テスト:

```bash
export LD_TEST_DATABASE_URL=postgres://light_discord:light_discord_dev_password@localhost:5432/light_discord
cargo test -p light-discord-storage --test postgres
```

`LD_TEST_DATABASE_URL` が未設定の場合、PostgreSQL 統合テストは DB に触らず成功扱いで終了します。Docker-in-Docker や DB がない環境でも通常テストを通せるようにするためです。
