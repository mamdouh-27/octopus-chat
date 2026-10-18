use std::sync::Mutex;

use octopus_core::db::Store;
use octopus_core::error::{Error, Result};
use octopus_core::paths;

/// Opens an independent connection to the app database.
///
/// The main window holds one connection for request/response work, but spawned
/// agent runs and awaited provider calls need to outlive a single command, and
/// a Tauri `State` reference cannot cross an await point safely. SQLite runs in
/// WAL mode, so a second connection sees the same data and concurrent readers
/// never block the writer.
pub fn open_store() -> Result<Store> {
    let conn = octopus_core::db::open(&paths::db_path()?)?;
    Ok(Store::new(conn))
}

/// Tauri-managed application state for short, synchronous command bodies.
pub struct AppState {
    store: Mutex<Store>,
    http: reqwest::Client,
}

impl AppState {
    pub fn new(store: Store) -> Self {
        // Built once and shared: connection pooling matters because every
        // provider request goes through this client.
        let http = octopus_core::providers::build_http(false);
        Self { store: Mutex::new(store), http }
    }

    /// Runs `f` with the store locked. SQLite is configured with a busy timeout
    /// so contention resolves rather than failing.
    ///
    /// The closure must be short and must not await: it holds a std mutex.
    pub fn with<T>(&self, f: impl FnOnce(&Store) -> Result<T>) -> Result<T> {
        let guard = self.store.lock().map_err(|_| Error::msg("store lock poisoned"))?;
        f(&guard)
    }

    pub fn http(&self) -> reqwest::Client {
        self.http.clone()
    }
}