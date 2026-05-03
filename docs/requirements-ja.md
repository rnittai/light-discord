# Light Discord Requirements

## Product Direction

最初は友達同士で使う self-hosted MVP として作る。ただし、問題なければ本格的な hosted service に移行する前提で、初期実装から境界を分けておく。

## Goals

- Windows と Linux で動くネイティブデスクトップアプリにする。
- Rust を主言語にする。
- Chromium / Electron は使わない。
- 友達用の小規模運用では 1 台の VPS と Docker Compose で動かせるようにする。
- 本格サービス化するときに、DB、認証、音声リレー、API/Realtime gateway を分離しやすくする。

## Initial MVP Scope

- 招待制アカウントを前提にできる認証基盤を用意する。
- PostgreSQL 運用ではログイン/セッションを必須にする。
- DB なしのローカル開発では表示名だけの開発ログインを残す。
- 初期管理者は `LD_BOOTSTRAP_ADMIN_PASSWORD` で作成する。
- 管理者は招待コードを作成できる。
- 管理者は監査ログを取得できる。
- チャット履歴は永続化する。
- 表示されるチャット履歴は最大 30 日保持する。
- ユーザーが削除したメッセージは監査ログに残す。
- 監査ログは管理者専用データとして扱う。
- 音声はまず UDP relay + Opus 導入を目指す。現時点では音声パケットリレーの土台を維持する。

## Retention Policy

- 通常メッセージ履歴: 最大 30 日。
- 削除済みメッセージの監査ログ: MVP では最大 30 日。
- 将来の hosted service では、監査ログ保持期間を利用規約と管理画面設定で明示する。
- バックアップにも保持期間の考え方を適用する。

## Audit Policy

ユーザーがメッセージを削除した場合、通常のチャンネル履歴からは見えなくする。ただし、以下を監査ログに保存する。

- 削除実行者 user id
- 元メッセージ投稿者 user id
- channel id
- message id
- message body snapshot
- 削除時刻
- 追加 metadata

監査ログは一般ユーザーのクライアント API からは返さない。将来的に管理者 API と管理画面だけから閲覧できるようにする。

現時点では管理者権限のあるセッションだけが `AdminListAuditLog` を使える。招待コード発行も管理者セッションに限定する。

## Auth Policy

- 友達用 MVP は invite + password login を採用する。
- パスワードは Argon2id で hash 化する。
- セッション token は client に一度だけ返し、server 側では hash を保存する。
- PostgreSQL が有効なときは dev auth を無効にする。
- `LD_DEV_AUTH=1` はローカル検証専用とし、公開サーバーでは使わない。

## Recommended Friend-Use Deployment

```text
1 VPS
  Caddy or Nginx
  light-discord-server
  PostgreSQL
  UDP voice port
```

## Future Hosted-Service Deployment

```text
Load Balancer
  API / Realtime Gateway x N
  Voice Relay x N
  PostgreSQL managed service
  Redis for presence/rate limit/pubsub
  Object storage for attachments
  Metrics and log aggregation
```

## Design Constraints

- Internal user ids must be stable UUID-like ids and must not depend on usernames or external providers.
- Storage, auth, voice, platform-specific code, and protocol types must stay in separate crates.
- Protocol changes should be backward-compatible where practical.
- Auth should support password + invite first, then OIDC / Passkey later.
- Message deletion must be soft-delete from the user-facing history and must write an audit event.
