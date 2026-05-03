use anyhow::{Context, Result};
use light_discord_core::{now_unix_ms, AuditLogSummary, ChatMessage};
use serde_json::json;
use std::{collections::HashMap, env, sync::Arc};
use tokio::sync::RwLock;
use tokio_postgres::{Client, NoTls};
use uuid::Uuid;

const DEFAULT_GUILD_ID: &str = "default";
const DEFAULT_VISIBLE_HISTORY_DAYS: u64 = 30;
const DEFAULT_AUDIT_LOG_DAYS: u64 = 30;
const DAY_MS: u64 = 24 * 60 * 60 * 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetentionPolicy {
    pub visible_history_days: u64,
    pub audit_log_days: u64,
}

impl RetentionPolicy {
    pub fn from_env() -> Self {
        let visible_history_days = read_days("LD_VISIBLE_HISTORY_DAYS")
            .unwrap_or(DEFAULT_VISIBLE_HISTORY_DAYS)
            .min(DEFAULT_VISIBLE_HISTORY_DAYS);
        let audit_log_days = read_days("LD_AUDIT_LOG_DAYS")
            .unwrap_or(DEFAULT_AUDIT_LOG_DAYS)
            .min(DEFAULT_AUDIT_LOG_DAYS);

        Self {
            visible_history_days,
            audit_log_days,
        }
    }

    fn visible_cutoff_unix_ms(self) -> u64 {
        cutoff_unix_ms(self.visible_history_days)
    }

    fn audit_cutoff_unix_ms(self) -> u64 {
        cutoff_unix_ms(self.audit_log_days)
    }
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        Self {
            visible_history_days: DEFAULT_VISIBLE_HISTORY_DAYS,
            audit_log_days: DEFAULT_AUDIT_LOG_DAYS,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    pub user_id: String,
    pub display_name: String,
    pub password_hash: Option<String>,
    pub is_admin: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionInfo {
    pub user_id: String,
    pub display_name: String,
    pub is_admin: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreateAccountResult {
    Created(Account),
    DisplayNameTaken,
    InvalidInvite,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeletedMessage {
    pub message_id: String,
    pub channel_id: String,
    pub deleted_by_user_id: String,
    pub deleted_at_unix_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeleteMessageResult {
    Deleted(DeletedMessage),
    Forbidden,
    NotFound,
}

#[derive(Clone)]
pub enum Storage {
    Memory(MemoryStorage),
    Postgres(PostgresStorage),
}

impl Storage {
    pub async fn from_env() -> Result<Self> {
        let policy = RetentionPolicy::from_env();

        match env::var("LD_DATABASE_URL") {
            Ok(database_url) if !database_url.trim().is_empty() => {
                Self::connect_postgres(&database_url, policy).await
            }
            _ => Ok(Self::Memory(MemoryStorage::new(policy))),
        }
    }

    pub async fn connect_postgres(database_url: &str, policy: RetentionPolicy) -> Result<Self> {
        let storage = PostgresStorage::connect(database_url, policy).await?;
        storage.migrate().await?;
        Ok(Self::Postgres(storage))
    }

    pub fn backend_name(&self) -> &'static str {
        match self {
            Self::Memory(_) => "memory",
            Self::Postgres(_) => "postgres",
        }
    }

    pub fn is_persistent(&self) -> bool {
        matches!(self, Self::Postgres(_))
    }

    pub async fn ensure_bootstrap_admin(
        &self,
        display_name: &str,
        password_hash: &str,
    ) -> Result<Account> {
        match self {
            Self::Memory(storage) => {
                storage
                    .ensure_bootstrap_admin(display_name, password_hash)
                    .await
            }
            Self::Postgres(storage) => {
                storage
                    .ensure_bootstrap_admin(display_name, password_hash)
                    .await
            }
        }
    }

    pub async fn create_invite(
        &self,
        code_hash: &str,
        created_by_user_id: Option<&str>,
        note: &str,
    ) -> Result<()> {
        match self {
            Self::Memory(storage) => {
                storage
                    .create_invite(code_hash, created_by_user_id, note)
                    .await
            }
            Self::Postgres(storage) => {
                storage
                    .create_invite(code_hash, created_by_user_id, note)
                    .await
            }
        }
    }

    pub async fn create_user_with_invite(
        &self,
        display_name: &str,
        password_hash: &str,
        invite_code_hash: &str,
    ) -> Result<CreateAccountResult> {
        match self {
            Self::Memory(storage) => {
                storage
                    .create_user_with_invite(display_name, password_hash, invite_code_hash)
                    .await
            }
            Self::Postgres(storage) => {
                storage
                    .create_user_with_invite(display_name, password_hash, invite_code_hash)
                    .await
            }
        }
    }

    pub async fn create_user_with_password_hash(
        &self,
        display_name: &str,
        password_hash: &str,
        is_admin: bool,
    ) -> Result<CreateAccountResult> {
        match self {
            Self::Memory(storage) => {
                storage
                    .create_user_with_password_hash(display_name, password_hash, is_admin)
                    .await
            }
            Self::Postgres(storage) => {
                storage
                    .create_user_with_password_hash(display_name, password_hash, is_admin)
                    .await
            }
        }
    }

    pub async fn find_user_by_display_name(&self, display_name: &str) -> Result<Option<Account>> {
        match self {
            Self::Memory(storage) => storage.find_user_by_display_name(display_name).await,
            Self::Postgres(storage) => storage.find_user_by_display_name(display_name).await,
        }
    }

    pub async fn register_ephemeral_user(&self, user_id: &str, display_name: &str) -> Result<()> {
        match self {
            Self::Memory(storage) => storage.register_ephemeral_user(user_id, display_name).await,
            Self::Postgres(storage) => storage.register_ephemeral_user(user_id, display_name).await,
        }
    }

    pub async fn create_session(&self, token_hash: &str, user_id: &str) -> Result<SessionInfo> {
        match self {
            Self::Memory(storage) => storage.create_session(token_hash, user_id).await,
            Self::Postgres(storage) => storage.create_session(token_hash, user_id).await,
        }
    }

    pub async fn validate_session(&self, token_hash: &str) -> Result<Option<SessionInfo>> {
        match self {
            Self::Memory(storage) => storage.validate_session(token_hash).await,
            Self::Postgres(storage) => storage.validate_session(token_hash).await,
        }
    }

    pub async fn save_message(&self, message: &ChatMessage) -> Result<()> {
        match self {
            Self::Memory(storage) => storage.save_message(message).await,
            Self::Postgres(storage) => storage.save_message(message).await,
        }
    }

    pub async fn recent_messages(
        &self,
        channel_id: &str,
        limit: usize,
    ) -> Result<Vec<ChatMessage>> {
        match self {
            Self::Memory(storage) => storage.recent_messages(channel_id, limit).await,
            Self::Postgres(storage) => storage.recent_messages(channel_id, limit).await,
        }
    }

    pub async fn soft_delete_message(
        &self,
        message_id: &str,
        actor_user_id: &str,
        actor_is_admin: bool,
    ) -> Result<DeleteMessageResult> {
        match self {
            Self::Memory(storage) => {
                storage
                    .soft_delete_message(message_id, actor_user_id, actor_is_admin)
                    .await
            }
            Self::Postgres(storage) => {
                storage
                    .soft_delete_message(message_id, actor_user_id, actor_is_admin)
                    .await
            }
        }
    }

    pub async fn purge_expired(&self) -> Result<()> {
        match self {
            Self::Memory(storage) => storage.purge_expired().await,
            Self::Postgres(storage) => storage.purge_expired().await,
        }
    }

    pub async fn audit_log(&self, limit: usize) -> Result<Vec<AuditLogSummary>> {
        match self {
            Self::Memory(storage) => storage.audit_log(limit).await,
            Self::Postgres(storage) => storage.audit_log(limit).await,
        }
    }
}

#[derive(Clone)]
pub struct MemoryStorage {
    inner: Arc<RwLock<MemoryInner>>,
    policy: RetentionPolicy,
}

#[derive(Default)]
struct MemoryInner {
    users: HashMap<String, StoredUser>,
    display_name_index: HashMap<String, String>,
    invite_codes: HashMap<String, MemoryInvite>,
    sessions: HashMap<String, String>,
    messages: HashMap<String, MemoryMessage>,
    audit_log: Vec<AuditLogSummary>,
}

#[derive(Clone)]
struct StoredUser {
    display_name: String,
    password_hash: Option<String>,
    is_admin: bool,
}

#[derive(Clone)]
struct MemoryInvite {
    consumed_by_user_id: Option<String>,
}

#[derive(Clone)]
struct MemoryMessage {
    message: ChatMessage,
    deleted_at_unix_ms: Option<u64>,
    deleted_by_user_id: Option<String>,
}

impl MemoryStorage {
    pub fn new(policy: RetentionPolicy) -> Self {
        Self {
            inner: Arc::new(RwLock::new(MemoryInner::default())),
            policy,
        }
    }

    async fn ensure_bootstrap_admin(
        &self,
        display_name: &str,
        password_hash: &str,
    ) -> Result<Account> {
        let mut inner = self.inner.write().await;
        let key = display_name_key(display_name);

        let user_id = inner
            .display_name_index
            .get(&key)
            .cloned()
            .unwrap_or_else(|| Uuid::new_v4().to_string());

        inner.display_name_index.insert(key, user_id.clone());
        inner.users.insert(
            user_id.clone(),
            StoredUser {
                display_name: display_name.to_owned(),
                password_hash: Some(password_hash.to_owned()),
                is_admin: true,
            },
        );

        Ok(Account {
            user_id,
            display_name: display_name.to_owned(),
            password_hash: Some(password_hash.to_owned()),
            is_admin: true,
        })
    }

    async fn create_invite(
        &self,
        code_hash: &str,
        _created_by_user_id: Option<&str>,
        _note: &str,
    ) -> Result<()> {
        let mut inner = self.inner.write().await;
        inner.invite_codes.insert(
            code_hash.to_owned(),
            MemoryInvite {
                consumed_by_user_id: None,
            },
        );
        Ok(())
    }

    async fn create_user_with_invite(
        &self,
        display_name: &str,
        password_hash: &str,
        invite_code_hash: &str,
    ) -> Result<CreateAccountResult> {
        let mut inner = self.inner.write().await;
        let Some(invite) = inner.invite_codes.get(invite_code_hash) else {
            return Ok(CreateAccountResult::InvalidInvite);
        };
        if invite.consumed_by_user_id.is_some() {
            return Ok(CreateAccountResult::InvalidInvite);
        }

        let key = display_name_key(display_name);
        if inner.display_name_index.contains_key(&key) {
            return Ok(CreateAccountResult::DisplayNameTaken);
        }

        let user_id = Uuid::new_v4().to_string();
        inner.display_name_index.insert(key, user_id.clone());
        inner.users.insert(
            user_id.clone(),
            StoredUser {
                display_name: display_name.to_owned(),
                password_hash: Some(password_hash.to_owned()),
                is_admin: false,
            },
        );
        if let Some(invite) = inner.invite_codes.get_mut(invite_code_hash) {
            invite.consumed_by_user_id = Some(user_id.clone());
        }

        Ok(CreateAccountResult::Created(Account {
            user_id,
            display_name: display_name.to_owned(),
            password_hash: Some(password_hash.to_owned()),
            is_admin: false,
        }))
    }

    async fn create_user_with_password_hash(
        &self,
        display_name: &str,
        password_hash: &str,
        is_admin: bool,
    ) -> Result<CreateAccountResult> {
        let mut inner = self.inner.write().await;
        let key = display_name_key(display_name);
        if inner.display_name_index.contains_key(&key) {
            return Ok(CreateAccountResult::DisplayNameTaken);
        }

        let user_id = Uuid::new_v4().to_string();
        inner.display_name_index.insert(key, user_id.clone());
        inner.users.insert(
            user_id.clone(),
            StoredUser {
                display_name: display_name.to_owned(),
                password_hash: Some(password_hash.to_owned()),
                is_admin,
            },
        );

        Ok(CreateAccountResult::Created(Account {
            user_id,
            display_name: display_name.to_owned(),
            password_hash: Some(password_hash.to_owned()),
            is_admin,
        }))
    }

    async fn find_user_by_display_name(&self, display_name: &str) -> Result<Option<Account>> {
        let inner = self.inner.read().await;
        let Some(user_id) = inner
            .display_name_index
            .get(&display_name_key(display_name))
        else {
            return Ok(None);
        };
        let Some(user) = inner.users.get(user_id) else {
            return Ok(None);
        };

        Ok(Some(Account {
            user_id: user_id.clone(),
            display_name: user.display_name.clone(),
            password_hash: user.password_hash.clone(),
            is_admin: user.is_admin,
        }))
    }

    async fn register_ephemeral_user(&self, user_id: &str, display_name: &str) -> Result<()> {
        let mut inner = self.inner.write().await;
        inner.users.insert(
            user_id.to_owned(),
            StoredUser {
                display_name: display_name.to_owned(),
                password_hash: None,
                is_admin: false,
            },
        );
        Ok(())
    }

    async fn create_session(&self, token_hash: &str, user_id: &str) -> Result<SessionInfo> {
        let mut inner = self.inner.write().await;
        let Some(user) = inner.users.get(user_id).cloned() else {
            anyhow::bail!("cannot create session for missing user");
        };

        inner
            .sessions
            .insert(token_hash.to_owned(), user_id.to_owned());

        Ok(SessionInfo {
            user_id: user_id.to_owned(),
            display_name: user.display_name,
            is_admin: user.is_admin,
        })
    }

    async fn validate_session(&self, token_hash: &str) -> Result<Option<SessionInfo>> {
        let inner = self.inner.read().await;
        let Some(user_id) = inner.sessions.get(token_hash) else {
            return Ok(None);
        };
        let Some(user) = inner.users.get(user_id) else {
            return Ok(None);
        };

        Ok(Some(SessionInfo {
            user_id: user_id.clone(),
            display_name: user.display_name.clone(),
            is_admin: user.is_admin,
        }))
    }

    async fn save_message(&self, message: &ChatMessage) -> Result<()> {
        let mut inner = self.inner.write().await;
        inner.messages.insert(
            message.id.clone(),
            MemoryMessage {
                message: message.clone(),
                deleted_at_unix_ms: None,
                deleted_by_user_id: None,
            },
        );
        Ok(())
    }

    async fn recent_messages(&self, channel_id: &str, limit: usize) -> Result<Vec<ChatMessage>> {
        let cutoff = self.policy.visible_cutoff_unix_ms();
        let inner = self.inner.read().await;
        let mut messages = inner
            .messages
            .values()
            .filter(|stored| stored.message.channel_id == channel_id)
            .filter(|stored| stored.deleted_at_unix_ms.is_none())
            .filter(|stored| stored.message.unix_ms >= cutoff)
            .map(|stored| stored.message.clone())
            .collect::<Vec<_>>();
        messages.sort_by_key(|message| message.unix_ms);

        if messages.len() > limit {
            Ok(messages.split_off(messages.len() - limit))
        } else {
            Ok(messages)
        }
    }

    async fn soft_delete_message(
        &self,
        message_id: &str,
        actor_user_id: &str,
        actor_is_admin: bool,
    ) -> Result<DeleteMessageResult> {
        let mut inner = self.inner.write().await;
        let Some(stored) = inner.messages.get_mut(message_id) else {
            return Ok(DeleteMessageResult::NotFound);
        };

        if stored.deleted_at_unix_ms.is_some() {
            return Ok(DeleteMessageResult::NotFound);
        }

        if stored.message.user_id != actor_user_id && !actor_is_admin {
            return Ok(DeleteMessageResult::Forbidden);
        }

        let deleted_at_unix_ms = now_unix_ms();
        stored.deleted_at_unix_ms = Some(deleted_at_unix_ms);
        stored.deleted_by_user_id = Some(actor_user_id.to_owned());

        let message = stored.message.clone();
        inner.audit_log.push(AuditLogSummary {
            id: Uuid::new_v4().to_string(),
            action: "message.deleted".to_owned(),
            actor_user_id: actor_user_id.to_owned(),
            target_user_id: Some(message.user_id.clone()),
            target_message_id: Some(message.id.clone()),
            channel_id: Some(message.channel_id.clone()),
            message_body_snapshot: Some(message.body),
            unix_ms: deleted_at_unix_ms,
        });

        Ok(DeleteMessageResult::Deleted(DeletedMessage {
            message_id: message_id.to_owned(),
            channel_id: message.channel_id,
            deleted_by_user_id: actor_user_id.to_owned(),
            deleted_at_unix_ms,
        }))
    }

    async fn purge_expired(&self) -> Result<()> {
        let visible_cutoff = self.policy.visible_cutoff_unix_ms();
        let audit_cutoff = self.policy.audit_cutoff_unix_ms();
        let mut inner = self.inner.write().await;
        inner
            .messages
            .retain(|_, stored| stored.message.unix_ms >= visible_cutoff);
        inner
            .audit_log
            .retain(|entry| entry.unix_ms >= audit_cutoff);
        Ok(())
    }

    async fn audit_log(&self, limit: usize) -> Result<Vec<AuditLogSummary>> {
        let inner = self.inner.read().await;
        let mut entries = inner.audit_log.clone();
        entries.sort_by_key(|entry| entry.unix_ms);
        if entries.len() > limit {
            Ok(entries.split_off(entries.len() - limit))
        } else {
            Ok(entries)
        }
    }
}

#[derive(Clone)]
pub struct PostgresStorage {
    client: Arc<Client>,
    policy: RetentionPolicy,
}

impl PostgresStorage {
    pub async fn connect(database_url: &str, policy: RetentionPolicy) -> Result<Self> {
        let (client, connection) = tokio_postgres::connect(database_url, NoTls)
            .await
            .context("failed to connect to PostgreSQL")?;

        tokio::spawn(async move {
            if let Err(err) = connection.await {
                eprintln!("postgres connection error: {err}");
            }
        });

        Ok(Self {
            client: Arc::new(client),
            policy,
        })
    }

    async fn migrate(&self) -> Result<()> {
        self.client
            .batch_execute(include_str!("../migrations/0001_init.sql"))
            .await
            .context("failed to run storage migration")
    }

    async fn ensure_bootstrap_admin(
        &self,
        display_name: &str,
        password_hash: &str,
    ) -> Result<Account> {
        if let Some(account) = self.find_user_by_display_name(display_name).await? {
            self.client
                .execute(
                    "UPDATE users
                     SET password_hash = $2, is_admin = TRUE, disabled_at_unix_ms = NULL
                     WHERE id = $1",
                    &[&account.user_id, &password_hash],
                )
                .await
                .context("failed to update bootstrap admin")?;

            return Ok(Account {
                password_hash: Some(password_hash.to_owned()),
                is_admin: true,
                ..account
            });
        }

        match self
            .create_user_with_password_hash(display_name, password_hash, true)
            .await?
        {
            CreateAccountResult::Created(account) => Ok(account),
            CreateAccountResult::DisplayNameTaken => {
                anyhow::bail!("bootstrap admin display name is already taken")
            }
            CreateAccountResult::InvalidInvite => unreachable!("invite is not used here"),
        }
    }

    async fn create_invite(
        &self,
        code_hash: &str,
        created_by_user_id: Option<&str>,
        note: &str,
    ) -> Result<()> {
        let now = now_unix_ms() as i64;
        self.client
            .execute(
                "INSERT INTO invite_codes
                 (code_hash, created_by_user_id, note, created_at_unix_ms)
                 VALUES ($1, $2, $3, $4)
                 ON CONFLICT (code_hash) DO NOTHING",
                &[&code_hash, &created_by_user_id, &note, &now],
            )
            .await
            .context("failed to create invite")?;
        Ok(())
    }

    async fn create_user_with_invite(
        &self,
        display_name: &str,
        password_hash: &str,
        invite_code_hash: &str,
    ) -> Result<CreateAccountResult> {
        let invite = self
            .client
            .query_opt(
                "SELECT code_hash
                 FROM invite_codes
                 WHERE code_hash = $1 AND consumed_at_unix_ms IS NULL",
                &[&invite_code_hash],
            )
            .await
            .context("failed to validate invite")?;

        if invite.is_none() {
            return Ok(CreateAccountResult::InvalidInvite);
        }

        let account = match self
            .create_user_with_password_hash(display_name, password_hash, false)
            .await?
        {
            CreateAccountResult::Created(account) => account,
            CreateAccountResult::DisplayNameTaken => {
                return Ok(CreateAccountResult::DisplayNameTaken)
            }
            CreateAccountResult::InvalidInvite => unreachable!("invite is checked separately"),
        };

        let now = now_unix_ms() as i64;
        self.client
            .execute(
                "UPDATE invite_codes
                 SET consumed_by_user_id = $2, consumed_at_unix_ms = $3
                 WHERE code_hash = $1 AND consumed_at_unix_ms IS NULL",
                &[&invite_code_hash, &account.user_id, &now],
            )
            .await
            .context("failed to consume invite")?;

        Ok(CreateAccountResult::Created(account))
    }

    async fn create_user_with_password_hash(
        &self,
        display_name: &str,
        password_hash: &str,
        is_admin: bool,
    ) -> Result<CreateAccountResult> {
        if self
            .find_user_by_display_name(display_name)
            .await?
            .is_some()
        {
            return Ok(CreateAccountResult::DisplayNameTaken);
        }

        let user_id = Uuid::new_v4().to_string();
        let now = now_unix_ms() as i64;
        self.client
            .execute(
                "INSERT INTO users
                 (id, display_name, password_hash, is_admin, created_at_unix_ms)
                 VALUES ($1, $2, $3, $4, $5)",
                &[&user_id, &display_name, &password_hash, &is_admin, &now],
            )
            .await
            .context("failed to create user")?;

        let role_name = if is_admin { "admin" } else { "member" };
        self.client
            .execute(
                "INSERT INTO members (guild_id, user_id, role_name, joined_at_unix_ms)
                 VALUES ($1, $2, $3, $4)
                 ON CONFLICT (guild_id, user_id) DO UPDATE SET role_name = EXCLUDED.role_name",
                &[&DEFAULT_GUILD_ID, &user_id, &role_name, &now],
            )
            .await
            .context("failed to create membership")?;

        Ok(CreateAccountResult::Created(Account {
            user_id,
            display_name: display_name.to_owned(),
            password_hash: Some(password_hash.to_owned()),
            is_admin,
        }))
    }

    async fn find_user_by_display_name(&self, display_name: &str) -> Result<Option<Account>> {
        let row = self
            .client
            .query_opt(
                "SELECT id, display_name, password_hash, is_admin
                 FROM users
                 WHERE lower(display_name) = lower($1) AND disabled_at_unix_ms IS NULL",
                &[&display_name],
            )
            .await
            .context("failed to find user by display name")?;

        Ok(row.map(|row| Account {
            user_id: row.get("id"),
            display_name: row.get("display_name"),
            password_hash: row.get("password_hash"),
            is_admin: row.get("is_admin"),
        }))
    }

    async fn register_ephemeral_user(&self, user_id: &str, display_name: &str) -> Result<()> {
        let now = now_unix_ms() as i64;
        self.client
            .execute(
                "INSERT INTO users (id, display_name, created_at_unix_ms)
                 VALUES ($1, $2, $3)
                 ON CONFLICT (id) DO UPDATE SET display_name = EXCLUDED.display_name",
                &[&user_id, &display_name, &now],
            )
            .await
            .context("failed to register ephemeral user")?;

        self.client
            .execute(
                "INSERT INTO members (guild_id, user_id, role_name, joined_at_unix_ms)
                 VALUES ($1, $2, 'member', $3)
                 ON CONFLICT (guild_id, user_id) DO NOTHING",
                &[&DEFAULT_GUILD_ID, &user_id, &now],
            )
            .await
            .context("failed to register ephemeral membership")?;

        Ok(())
    }

    async fn create_session(&self, token_hash: &str, user_id: &str) -> Result<SessionInfo> {
        let account = self
            .find_user_by_id(user_id)
            .await?
            .context("cannot create session for missing user")?;
        let now = now_unix_ms() as i64;
        self.client
            .execute(
                "INSERT INTO sessions (token_hash, user_id, created_at_unix_ms, last_seen_at_unix_ms)
                 VALUES ($1, $2, $3, $3)",
                &[&token_hash, &user_id, &now],
            )
            .await
            .context("failed to create session")?;

        Ok(SessionInfo {
            user_id: account.user_id,
            display_name: account.display_name,
            is_admin: account.is_admin,
        })
    }

    async fn validate_session(&self, token_hash: &str) -> Result<Option<SessionInfo>> {
        let row = self
            .client
            .query_opt(
                "SELECT users.id, users.display_name, users.is_admin
                 FROM sessions
                 JOIN users ON users.id = sessions.user_id
                 WHERE sessions.token_hash = $1
                   AND sessions.revoked_at_unix_ms IS NULL
                   AND users.disabled_at_unix_ms IS NULL",
                &[&token_hash],
            )
            .await
            .context("failed to validate session")?;

        let Some(row) = row else {
            return Ok(None);
        };

        let now = now_unix_ms() as i64;
        self.client
            .execute(
                "UPDATE sessions SET last_seen_at_unix_ms = $2 WHERE token_hash = $1",
                &[&token_hash, &now],
            )
            .await
            .context("failed to update session last seen")?;

        Ok(Some(SessionInfo {
            user_id: row.get("id"),
            display_name: row.get("display_name"),
            is_admin: row.get("is_admin"),
        }))
    }

    async fn find_user_by_id(&self, user_id: &str) -> Result<Option<Account>> {
        let row = self
            .client
            .query_opt(
                "SELECT id, display_name, password_hash, is_admin
                 FROM users
                 WHERE id = $1 AND disabled_at_unix_ms IS NULL",
                &[&user_id],
            )
            .await
            .context("failed to find user by id")?;

        Ok(row.map(|row| Account {
            user_id: row.get("id"),
            display_name: row.get("display_name"),
            password_hash: row.get("password_hash"),
            is_admin: row.get("is_admin"),
        }))
    }

    async fn save_message(&self, message: &ChatMessage) -> Result<()> {
        self.client
            .execute(
                "INSERT INTO messages
                 (id, guild_id, channel_id, user_id, display_name_snapshot, body, created_at_unix_ms)
                 VALUES ($1, $2, $3, $4, $5, $6, $7)",
                &[
                    &message.id,
                    &DEFAULT_GUILD_ID,
                    &message.channel_id,
                    &message.user_id,
                    &message.display_name,
                    &message.body,
                    &(message.unix_ms as i64),
                ],
            )
            .await
            .context("failed to save message")?;

        Ok(())
    }

    async fn recent_messages(&self, channel_id: &str, limit: usize) -> Result<Vec<ChatMessage>> {
        let cutoff = self.policy.visible_cutoff_unix_ms() as i64;
        let limit = limit as i64;
        let rows = self
            .client
            .query(
                "SELECT id, channel_id, user_id, display_name_snapshot, body, created_at_unix_ms
                 FROM messages
                 WHERE channel_id = $1
                   AND deleted_at_unix_ms IS NULL
                   AND created_at_unix_ms >= $2
                 ORDER BY created_at_unix_ms ASC
                 LIMIT $3",
                &[&channel_id, &cutoff, &limit],
            )
            .await
            .context("failed to load recent messages")?;

        Ok(rows
            .into_iter()
            .map(|row| ChatMessage {
                id: row.get("id"),
                channel_id: row.get("channel_id"),
                user_id: row.get("user_id"),
                display_name: row.get("display_name_snapshot"),
                body: row.get("body"),
                unix_ms: row.get::<_, i64>("created_at_unix_ms") as u64,
            })
            .collect())
    }

    async fn soft_delete_message(
        &self,
        message_id: &str,
        actor_user_id: &str,
        actor_is_admin: bool,
    ) -> Result<DeleteMessageResult> {
        let row = self
            .client
            .query_opt(
                "SELECT guild_id, channel_id, user_id, body
                 FROM messages
                 WHERE id = $1 AND deleted_at_unix_ms IS NULL",
                &[&message_id],
            )
            .await
            .context("failed to load message for delete")?;

        let Some(row) = row else {
            return Ok(DeleteMessageResult::NotFound);
        };

        let guild_id: String = row.get("guild_id");
        let channel_id: String = row.get("channel_id");
        let target_user_id: String = row.get("user_id");
        let body: String = row.get("body");

        if target_user_id != actor_user_id && !actor_is_admin {
            return Ok(DeleteMessageResult::Forbidden);
        }

        let deleted_at_unix_ms = now_unix_ms();
        let deleted_at_i64 = deleted_at_unix_ms as i64;
        let updated = self
            .client
            .execute(
                "UPDATE messages
                 SET deleted_at_unix_ms = $2, deleted_by_user_id = $3
                 WHERE id = $1 AND deleted_at_unix_ms IS NULL",
                &[&message_id, &deleted_at_i64, &actor_user_id],
            )
            .await
            .context("failed to soft delete message")?;

        if updated == 0 {
            return Ok(DeleteMessageResult::NotFound);
        }

        let metadata = json!({ "source": "client" });
        self.client
            .execute(
                "INSERT INTO audit_log
                 (id, guild_id, action, actor_user_id, target_user_id, target_message_id,
                  channel_id, message_body_snapshot, metadata_json, created_at_unix_ms)
                 VALUES ($1, $2, 'message.deleted', $3, $4, $5, $6, $7, $8, $9)",
                &[
                    &Uuid::new_v4().to_string(),
                    &guild_id,
                    &actor_user_id,
                    &target_user_id,
                    &message_id,
                    &channel_id,
                    &body,
                    &metadata,
                    &deleted_at_i64,
                ],
            )
            .await
            .context("failed to write message delete audit log")?;

        Ok(DeleteMessageResult::Deleted(DeletedMessage {
            message_id: message_id.to_owned(),
            channel_id,
            deleted_by_user_id: actor_user_id.to_owned(),
            deleted_at_unix_ms,
        }))
    }

    async fn purge_expired(&self) -> Result<()> {
        let visible_cutoff = self.policy.visible_cutoff_unix_ms() as i64;
        let audit_cutoff = self.policy.audit_cutoff_unix_ms() as i64;

        self.client
            .execute(
                "DELETE FROM messages WHERE created_at_unix_ms < $1",
                &[&visible_cutoff],
            )
            .await
            .context("failed to purge expired messages")?;

        self.client
            .execute(
                "DELETE FROM audit_log WHERE created_at_unix_ms < $1",
                &[&audit_cutoff],
            )
            .await
            .context("failed to purge expired audit logs")?;

        Ok(())
    }

    async fn audit_log(&self, limit: usize) -> Result<Vec<AuditLogSummary>> {
        let limit = limit as i64;
        let rows = self
            .client
            .query(
                "SELECT id, action, actor_user_id, target_user_id, target_message_id,
                        channel_id, message_body_snapshot, created_at_unix_ms
                 FROM audit_log
                 ORDER BY created_at_unix_ms DESC
                 LIMIT $1",
                &[&limit],
            )
            .await
            .context("failed to load audit log")?;

        let mut entries = rows
            .into_iter()
            .map(|row| AuditLogSummary {
                id: row.get("id"),
                action: row.get("action"),
                actor_user_id: row.get("actor_user_id"),
                target_user_id: row.get("target_user_id"),
                target_message_id: row.get("target_message_id"),
                channel_id: row.get("channel_id"),
                message_body_snapshot: row.get("message_body_snapshot"),
                unix_ms: row.get::<_, i64>("created_at_unix_ms") as u64,
            })
            .collect::<Vec<_>>();
        entries.sort_by_key(|entry| entry.unix_ms);
        Ok(entries)
    }
}

fn display_name_key(display_name: &str) -> String {
    display_name.trim().to_ascii_lowercase()
}

fn read_days(name: &str) -> Option<u64> {
    env::var(name)
        .ok()?
        .parse::<u64>()
        .ok()
        .filter(|days| *days > 0)
}

fn cutoff_unix_ms(days: u64) -> u64 {
    now_unix_ms().saturating_sub(days.saturating_mul(DAY_MS))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn memory_storage_hides_deleted_message_and_writes_audit_log() {
        let storage = MemoryStorage::new(RetentionPolicy::default());
        let message = ChatMessage::new("msg-1", "general", "user-1", "alice", "secret");

        storage
            .register_ephemeral_user("user-1", "alice")
            .await
            .unwrap();
        storage.save_message(&message).await.unwrap();
        assert_eq!(
            storage.recent_messages("general", 100).await.unwrap().len(),
            1
        );

        let delete_result = storage
            .soft_delete_message("msg-1", "user-1", false)
            .await
            .unwrap();
        assert!(matches!(delete_result, DeleteMessageResult::Deleted(_)));
        assert!(storage
            .recent_messages("general", 100)
            .await
            .unwrap()
            .is_empty());

        let audit_log = storage.audit_log(10).await.unwrap();
        assert_eq!(audit_log.len(), 1);
        assert_eq!(
            audit_log[0].message_body_snapshot.as_deref(),
            Some("secret")
        );
    }

    #[tokio::test]
    async fn memory_storage_rejects_delete_by_other_user() {
        let storage = MemoryStorage::new(RetentionPolicy::default());
        let message = ChatMessage::new("msg-1", "general", "user-1", "alice", "secret");

        storage.save_message(&message).await.unwrap();
        let delete_result = storage
            .soft_delete_message("msg-1", "user-2", false)
            .await
            .unwrap();

        assert_eq!(delete_result, DeleteMessageResult::Forbidden);
        assert_eq!(
            storage.recent_messages("general", 100).await.unwrap().len(),
            1
        );
        assert!(storage.audit_log(10).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn memory_storage_supports_invites_and_sessions() {
        let storage = MemoryStorage::new(RetentionPolicy::default());
        storage
            .create_invite("invite-hash", None, "test")
            .await
            .unwrap();

        let created = storage
            .create_user_with_invite("alice", "password-hash", "invite-hash")
            .await
            .unwrap();
        let CreateAccountResult::Created(account) = created else {
            panic!("expected account");
        };

        storage
            .create_session("session-hash", &account.user_id)
            .await
            .unwrap();
        let session = storage
            .validate_session("session-hash")
            .await
            .unwrap()
            .unwrap();

        assert_eq!(session.display_name, "alice");
        assert!(!session.is_admin);
    }
}
