/// Forward-only migrations. Each entry moves `PRAGMA user_version` forward by
/// one and must be idempotent-safe: never edit a shipped entry, append a new one.
pub const MIGRATIONS: &[&str] = &[
    // v1 - core chat store
    r#"
    CREATE TABLE projects (
        id              TEXT PRIMARY KEY,
        name            TEXT NOT NULL,
        roots           TEXT NOT NULL DEFAULT '[]',
        instructions    TEXT NOT NULL DEFAULT '',
        permission_mode TEXT NOT NULL DEFAULT 'ask',
        accent          TEXT,
        archived        INTEGER NOT NULL DEFAULT 0,
        created_at      INTEGER NOT NULL,
        updated_at      INTEGER NOT NULL
    );

    CREATE TABLE sessions (
        id               TEXT PRIMARY KEY,
        project_id       TEXT REFERENCES projects(id) ON DELETE SET NULL,
        title            TEXT NOT NULL DEFAULT 'New chat',
        mode             TEXT NOT NULL DEFAULT 'build',
        state            TEXT NOT NULL DEFAULT 'idle',
        provider         TEXT NOT NULL DEFAULT '',
        model            TEXT NOT NULL DEFAULT '',
        reasoning_effort TEXT,
        temperature      REAL,
        max_tokens       INTEGER,
        permission_mode  TEXT NOT NULL DEFAULT 'ask',
        goal             TEXT,
        forked_from      TEXT,
        worktree         TEXT,
        favorite         INTEGER NOT NULL DEFAULT 0,
        tokens_in        INTEGER NOT NULL DEFAULT 0,
        tokens_out       INTEGER NOT NULL DEFAULT 0,
        cost_usd         REAL NOT NULL DEFAULT 0,
        created_at       INTEGER NOT NULL,
        updated_at       INTEGER NOT NULL
    );
    CREATE INDEX idx_sessions_project ON sessions(project_id, updated_at DESC);
    CREATE INDEX idx_sessions_updated ON sessions(updated_at DESC);

    CREATE TABLE messages (
        id          TEXT PRIMARY KEY,
        session_id  TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
        role        TEXT NOT NULL,
        content     TEXT NOT NULL DEFAULT '',
        reasoning   TEXT,
        state       TEXT NOT NULL DEFAULT 'done',
        model       TEXT,
        error       TEXT,
        tokens_in   INTEGER NOT NULL DEFAULT 0,
        tokens_out  INTEGER NOT NULL DEFAULT 0,
        supersedes  TEXT,
        attachments TEXT NOT NULL DEFAULT '[]',
        created_at  INTEGER NOT NULL
    );
    CREATE INDEX idx_messages_session ON messages(session_id, created_at);

    CREATE TABLE tool_calls (
        id            TEXT PRIMARY KEY,
        session_id    TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
        message_id    TEXT NOT NULL,
        name          TEXT NOT NULL,
        input         TEXT NOT NULL DEFAULT '{}',
        output        TEXT,
        state         TEXT NOT NULL DEFAULT 'pending',
        denied_reason TEXT,
        diff          TEXT,
        started_at    INTEGER NOT NULL,
        ended_at      INTEGER
    );
    CREATE INDEX idx_tool_calls_message ON tool_calls(message_id);

    CREATE TABLE checkpoints (
        id         TEXT PRIMARY KEY,
        session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
        message_id TEXT,
        label      TEXT NOT NULL DEFAULT '',
        git_head   TEXT,
        created_at INTEGER NOT NULL
    );
    CREATE INDEX idx_checkpoints_session ON checkpoints(session_id, created_at DESC);

    CREATE TABLE todos (
        id         TEXT PRIMARY KEY,
        session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
        content    TEXT NOT NULL,
        status     TEXT NOT NULL DEFAULT 'pending',
        position   INTEGER NOT NULL DEFAULT 0
    );
    CREATE INDEX idx_todos_session ON todos(session_id, position);

    CREATE TABLE kv (
        key   TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );
    "#,
    // v2 - user-defined providers and models
    r#"
    CREATE TABLE custom_providers (
        id       TEXT PRIMARY KEY,
        label    TEXT NOT NULL,
        dialect  TEXT NOT NULL DEFAULT 'openai',
        base_url TEXT NOT NULL,
        local    INTEGER NOT NULL DEFAULT 0,
        api_key  TEXT,
        headers  TEXT NOT NULL DEFAULT '[]',
        position INTEGER NOT NULL DEFAULT 0
    );

    CREATE TABLE custom_models (
        id            TEXT PRIMARY KEY,
        provider_id   TEXT NOT NULL REFERENCES custom_providers(id) ON DELETE CASCADE,
        label         TEXT NOT NULL,
        dialect       TEXT NOT NULL DEFAULT 'openai',
        context_window INTEGER,
        max_output    INTEGER,
        supports_tools  INTEGER NOT NULL DEFAULT 1,
        supports_vision INTEGER NOT NULL DEFAULT 0,
        reasoning       INTEGER NOT NULL DEFAULT 0,
        input_cost  REAL,
        output_cost REAL,
        position    INTEGER NOT NULL DEFAULT 0
    );
    CREATE INDEX idx_custom_models_provider ON custom_models(provider_id, position);
    "#,
    // v3 - repair project roots saved in the Windows extended-length form.
    //
    // The repair itself lives in `db::repair_project_roots` rather than in SQL.
    // `roots` is a JSON array, so a textual `replace()` cannot be used: JSON
    // escapes every backslash, which means the four characters `\\?\` occur at
    // a misaligned offset inside the encoded text and the substitution silently
    // corrupts the path instead of cleaning it. Parsing and re-encoding in Rust
    // is the only correct way to edit inside a JSON column.
    r#"
SELECT count(*) FROM projects;
    "#,
    // v4 - let a model be added to any provider, not only a custom one.
    //
    // `custom_models.provider_id` referenced `custom_providers(id)`, but the
    // built-in providers (Ollama, an OpenAI-compatible endpoint, ...) are
    // catalog entries with no row in `custom_providers`. Adding a model to one of
    // them therefore failed with `FOREIGN KEY constraint failed`, which is why
    // "Add model" only ever worked for providers the user had created themselves.
    //
    // SQLite cannot drop a foreign key without rebuilding the table, so the
    // table is recreated. The primary key becomes `(provider_id, id)`, because
    // the same model id under two different providers is legitimate and the old
    // key on `id` alone would have made the second insert overwrite the first.
    r#"
    PRAGMA foreign_keys = OFF;

    CREATE TABLE custom_models_new (
        id            TEXT NOT NULL,
        provider_id   TEXT NOT NULL,
        label         TEXT NOT NULL,
        dialect       TEXT NOT NULL DEFAULT 'openai',
        context_window INTEGER,
        max_output    INTEGER,
        supports_tools  INTEGER NOT NULL DEFAULT 1,
        supports_vision INTEGER NOT NULL DEFAULT 0,
        reasoning       INTEGER NOT NULL DEFAULT 0,
        input_cost  REAL,
        output_cost REAL,
        position    INTEGER NOT NULL DEFAULT 0,
        PRIMARY KEY (provider_id, id)
    );

    INSERT INTO custom_models_new
        (id,provider_id,label,dialect,context_window,max_output,supports_tools,
         supports_vision,reasoning,input_cost,output_cost,position)
    SELECT id,provider_id,label,dialect,context_window,max_output,supports_tools,
           supports_vision,reasoning,input_cost,output_cost,position
      FROM custom_models;

    DROP TABLE custom_models;
    ALTER TABLE custom_models_new RENAME TO custom_models;

    CREATE INDEX idx_custom_models_provider ON custom_models(provider_id, position);

    PRAGMA foreign_keys = ON;
    "#,
];

