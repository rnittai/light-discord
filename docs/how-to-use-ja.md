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

`LD_*` で始まる環境変数は sudo が dynamic linker 系の危険な変数として削除することがあります。そのため、このスクリプトは root 再実行の直前に `LD_PG_*` を `LIGHT_DISCORD_PG_*` に写して渡します。利用者側は上記の `LD_PG_*` のままで構いません。

PostgreSQL が `5432` 以外で起動している場合、Debian / Ubuntu では `pg_lsclusters` から起動中 cluster の port を検出して `LD_DATABASE_URL` の例に反映します。明示したい場合は `LD_PG_PORT` を指定してください。

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

## 8. ボイスの入力/出力デバイス選択と通話

左サイドバーの `Voice` セクションで `Input` と `Output` を選択できます。デバイスを接続し直した場合は `Refresh` を押します。`Join` を押すと音声ワーカーが起動し、次の処理が走ります。

- マイクからの入力をモノラル i16 へダウンミックスし、48 kHz へリサンプリング
- ハイパスフィルタ (約 100 Hz) と RMS ベースのノイズゲートを通す。ゲートが閉じているフレームは送信しない (Opus DTX は使用していない)
- 受信側の出力が大きいときはマイクを軽く減衰させる簡易エコー抑制 (本格的な AEC ではありません)
- 20 ms フレーム (960 サンプル) 単位で Opus 音声へエンコード (32 kbps、in-band FEC 有効、想定パケットロス 10%)
- `VoicePacket { codec: "opus", frame_samples: 960, ... }` として既存の UDP リレーへ送信

受信側はユーザーごとに jitter buffer (目標 ~60 ms = 3 パケット) を持ち、Opus PLC と FEC でパケットロスを補間しながら 48 kHz モノラルでデコードして、出力デバイスのサンプルレート/チャンネル数へ自動で適合させます。

`Mute mic` を押すとマイクからの音声送信は止まりますが、相手にはハートビートが送られ続けるため voice room には残ります。`Deafen` を押すと受信した音声の再生を完全に止めます (自動的にマイクもミュートされます)。voice user list では、いま音声を出しているユーザー (自分を含む) は緑色の `*` マーカー付きで強調表示されます。

Linux でビルドする場合、`cpal` のために ALSA 開発パッケージと、libopus を静的にビルドするために CMake と C/C++ ツールチェインが必要です。
セットアップスクリプトを使うと自動でインストールできます。

```bash
scripts/setup-linux-dev-deps.sh
```

スクリプトはディストロを検出し、必要に応じて sudo でパッケージをインストールして結果を確認します。
手動でインストールする場合のパッケージ名は次のとおりです。

| ディストロ | コマンド |
|-----------|---------|
| Debian / Ubuntu | `sudo apt-get update && sudo apt-get install -y pkg-config libasound2-dev cmake build-essential` |
| Fedora / RHEL / CentOS / Rocky / Alma | `sudo dnf install -y pkgconf-pkg-config alsa-lib-devel cmake gcc gcc-c++ make` |
| Arch / Manjaro | `sudo pacman -Sy --needed --noconfirm pkgconf alsa-lib cmake base-devel` |
| openSUSE / SLES | `sudo zypper --non-interactive install pkgconf-pkg-config alsa-devel cmake gcc gcc-c++ make` |

現時点の制約として、UDP の中身は依然として `VoicePacket` を JSON でくるんだ形式 (`codec` フィールドで Opus / 旧 PCM を区別) です。SRTP/暗号化、可変ビットレート、Opus DTX (無音抑制は RMS ノイズゲートで行い codec では行わない)、本格的な AEC、サーバ側の codec 認識は実装していないため、本番グレードの voice ではありませんが、自己ホストでの友達通話には十分使える品質を狙っています。

## 日本語が文字化けする場合

クライアントは起動時に日本語表示可能な system font を探して `egui` に登録します。Linux で日本語が文字化けする場合は、日本語フォントが入っていない可能性があります。

```bash
# Debian / Ubuntu
sudo apt-get install -y fonts-noto-cjk

# Fedora
sudo dnf install -y google-noto-sans-cjk-fonts

# Arch
sudo pacman -S noto-fonts-cjk
```

特定のフォントファイルを明示することもできます。

```bash
export LIGHT_DISCORD_FONT_PATH=/path/to/NotoSansCJK-Regular.ttc
cargo run -p light-discord-client
```

Windows では `C:\Windows\Fonts` 配下の Meiryo / Yu Gothic / MS Gothic 系フォントを探します。

## 9. 監査ログを見る

管理者で接続して、左サイドバーの `Admin` セクションから `Audit` を押します。中央に `Audit log` が表示されます。

監査ログには主に以下が含まれます。

- action
- 削除実行者 user id
- 対象 message id
- channel id
- 削除された本文 snapshot
- 時刻

## 10. Docker について

この作業環境は Docker コンテナ内です。ただし、このコンテナ内には `docker` CLI が入っていないため、ここから `docker compose up` は実行できません。

ホスト側に Docker がある場合は、ホスト側のターミナルで実行してください。

```bash
cd deploy
export LD_BOOTSTRAP_ADMIN_PASSWORD='change-this-password'
export LD_BOOTSTRAP_INVITE_CODE='share-this-once'
docker compose up --build
```

この場合、server と PostgreSQL は Compose で起動します。クライアントはホスト側で `cargo run -p light-discord-client` するか、別途パッケージ化した実行ファイルから接続します。

## 11. テスト

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
