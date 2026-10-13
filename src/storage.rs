use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension, Transaction};
use url::Url;

use crate::settings::{SearchEngine, Settings};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SavedTab {
    pub url: String,
    pub title: String,
    pub pinned: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    pub tabs: Vec<SavedTab>,
    pub active: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub id: i64,
    pub url: String,
    pub title: String,
    pub visited_at: i64,
}

pub struct Store {
    connection: Connection,
}

impl Store {
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        let connection = Connection::open(path)?;
        connection.busy_timeout(Duration::from_secs(3))?;
        connection.execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS sessions (
                 position INTEGER PRIMARY KEY,
                 url TEXT NOT NULL,
                 title TEXT NOT NULL,
                 pinned INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS meta (
                 key TEXT PRIMARY KEY,
                 value INTEGER NOT NULL
             );
             INSERT OR IGNORE INTO meta (key, value) VALUES ('active_index', 0);
             CREATE TABLE IF NOT EXISTS preferences (
                 key TEXT PRIMARY KEY,
                 value TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS history (
                 id INTEGER PRIMARY KEY,
                 url TEXT NOT NULL,
                 title TEXT NOT NULL,
                 visited_at INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS history_latest ON history(visited_at DESC, id DESC);
             CREATE TABLE IF NOT EXISTS bookmarks (
                 id INTEGER PRIMARY KEY,
                 url TEXT NOT NULL UNIQUE,
                 title TEXT NOT NULL,
                 saved_at INTEGER NOT NULL
             );",
        )?;
        Ok(Self { connection })
    }

    pub fn load_session(&self) -> rusqlite::Result<Session> {
        let active: i64 = self.connection.query_row(
            "SELECT value FROM meta WHERE key = 'active_index'",
            [],
            |row| row.get(0),
        )?;

        let mut statement = self
            .connection
            .prepare("SELECT position, url, title, pinned FROM sessions ORDER BY position")?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                SavedTab {
                    url: row.get(1)?,
                    title: row.get(2)?,
                    pinned: row.get::<_, i64>(3)? != 0,
                },
            ))
        })?;

        let mut tabs = Vec::new();
        let mut active_after_filter = 0;
        let mut active_was_valid = false;
        let active_position = active.max(0);
        for row in rows {
            let (position, tab) = row?;
            if !is_allowed_url(&tab.url) {
                continue;
            }
            if position < active_position {
                active_after_filter += 1;
            } else if position == active_position {
                active_was_valid = true;
            }
            tabs.push(tab);
        }

        let active = if tabs.is_empty() {
            0
        } else if active_was_valid {
            active_after_filter
        } else {
            active_after_filter.min(tabs.len() - 1)
        };
        Ok(Session { tabs, active })
    }

    pub fn save_session(&mut self, session: &Session) -> rusqlite::Result<()> {
        if session.tabs.iter().any(|tab| !is_allowed_url(&tab.url)) {
            return Err(invalid_url_error());
        }

        let active = if session.tabs.is_empty() {
            0
        } else {
            session.active.min(session.tabs.len() - 1)
        };
        let transaction = self.connection.transaction()?;
        replace_session(&transaction, session, active)?;
        transaction.commit()
    }

    pub fn settings(&self) -> rusqlite::Result<Settings> {
        let mut settings = Settings::default();
        let mut statement = self
            .connection
            .prepare("SELECT key, value FROM preferences")?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let values = rows.collect::<rusqlite::Result<Vec<_>>>()?;

        for (key, value) in values {
            match key.as_str() {
                "restore_session" => settings.restore_session = parse_bool_pref(&value)?,
                "lazy_restore" => settings.lazy_restore = parse_bool_pref(&value)?,
                "search_engine" => {
                    settings.search_engine =
                        SearchEngine::from_key(&value).ok_or_else(invalid_settings_error)?;
                }
                "confirm_close_multiple" => {
                    settings.confirm_close_multiple = parse_bool_pref(&value)?;
                }
                "home_url" => settings.home_url = value,
                "page_zoom" => {
                    settings.page_zoom =
                        value.parse::<u16>().map_err(|_| invalid_settings_error())?;
                }
                "remember_history" => settings.remember_history = parse_bool_pref(&value)?,
                _ => {}
            }
        }
        settings.validate().map_err(|_| invalid_settings_error())?;
        Ok(settings)
    }

    pub fn save_settings(&self, settings: &Settings) -> rusqlite::Result<()> {
        settings.validate().map_err(|_| invalid_settings_error())?;
        self.connection.execute(
            "INSERT INTO preferences (key, value) VALUES
                 ('restore_session', ?1),
                 ('lazy_restore', ?2),
                 ('search_engine', ?3),
                 ('confirm_close_multiple', ?4),
                 ('home_url', ?5),
                 ('page_zoom', ?6),
                 ('remember_history', ?7)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![
                settings.restore_session.to_string(),
                settings.lazy_restore.to_string(),
                settings.search_engine.key(),
                settings.confirm_close_multiple.to_string(),
                settings.home_url,
                settings.page_zoom.to_string(),
                settings.remember_history.to_string(),
            ],
        )?;
        Ok(())
    }

    pub fn add_visit(&self, url: &str, title: &str) -> rusqlite::Result<()> {
        ensure_history_url(url)?;
        self.connection.execute(
            "INSERT INTO history (url, title, visited_at) VALUES (?1, ?2, ?3)",
            params![url, title, now_unix_seconds()],
        )?;
        Ok(())
    }

    pub fn history(&self) -> rusqlite::Result<Vec<Entry>> {
        let mut statement = self.connection.prepare(
            "SELECT id, url, title, visited_at FROM history
             ORDER BY visited_at DESC, id DESC LIMIT 200",
        )?;
        let entries = statement
            .query_map([], row_to_entry)?
            .collect::<rusqlite::Result<Vec<_>>>();
        entries
    }

    pub fn clear_history(&self) -> rusqlite::Result<()> {
        self.connection.execute("DELETE FROM history", [])?;
        Ok(())
    }

    pub fn toggle_bookmark(&self, url: &str, title: &str) -> rusqlite::Result<bool> {
        ensure_external_url(url)?;
        let existing: Option<i64> = self
            .connection
            .query_row("SELECT id FROM bookmarks WHERE url = ?1", [url], |row| {
                row.get(0)
            })
            .optional()?;
        if let Some(id) = existing {
            self.connection
                .execute("DELETE FROM bookmarks WHERE id = ?1", [id])?;
            Ok(false)
        } else {
            self.connection.execute(
                "INSERT INTO bookmarks (url, title, saved_at) VALUES (?1, ?2, ?3)",
                params![url, title, now_unix_seconds()],
            )?;
            Ok(true)
        }
    }

    pub fn is_bookmarked(&self, url: &str) -> rusqlite::Result<bool> {
        self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM bookmarks WHERE url = ?1)",
            [url],
            |row| row.get(0),
        )
    }

    pub fn bookmarks(&self) -> rusqlite::Result<Vec<Entry>> {
        let mut statement = self.connection.prepare(
            "SELECT id, url, title, saved_at FROM bookmarks ORDER BY saved_at DESC, id DESC",
        )?;
        let entries = statement
            .query_map([], row_to_entry)?
            .collect::<rusqlite::Result<Vec<_>>>();
        entries
    }

    pub fn bookmark(&self, id: i64) -> rusqlite::Result<Option<Entry>> {
        self.connection
            .query_row(
                "SELECT id, url, title, saved_at FROM bookmarks WHERE id = ?1",
                [id],
                row_to_entry,
            )
            .optional()
    }

    pub fn update_bookmark(&self, id: i64, url: &str, title: &str) -> rusqlite::Result<()> {
        ensure_external_url(url)?;
        let updated = self.connection.execute(
            "UPDATE bookmarks SET url = ?1, title = ?2 WHERE id = ?3",
            params![url, title, id],
        )?;
        if updated == 0 {
            return Err(rusqlite::Error::QueryReturnedNoRows);
        }
        Ok(())
    }

    pub fn remove_bookmark(&self, id: i64) -> rusqlite::Result<()> {
        let removed = self
            .connection
            .execute("DELETE FROM bookmarks WHERE id = ?1", [id])?;
        if removed == 0 {
            return Err(rusqlite::Error::QueryReturnedNoRows);
        }
        Ok(())
    }
}

fn replace_session(
    transaction: &Transaction<'_>,
    session: &Session,
    active: usize,
) -> rusqlite::Result<()> {
    transaction.execute("DELETE FROM sessions", [])?;
    {
        let mut insert = transaction.prepare(
            "INSERT INTO sessions (position, url, title, pinned) VALUES (?1, ?2, ?3, ?4)",
        )?;
        for (position, tab) in session.tabs.iter().enumerate() {
            insert.execute(params![position, tab.url, tab.title, tab.pinned as i64])?;
        }
    }
    transaction.execute(
        "UPDATE meta SET value = ?1 WHERE key = 'active_index'",
        [active as i64],
    )?;
    Ok(())
}

fn row_to_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<Entry> {
    Ok(Entry {
        id: row.get(0)?,
        url: row.get(1)?,
        title: row.get(2)?,
        visited_at: row.get(3)?,
    })
}

fn is_allowed_url(value: &str) -> bool {
    if value == "about:blank" {
        return true;
    }
    Url::parse(value)
        .is_ok_and(|url| matches!(url.scheme(), "http" | "https") && url.host_str().is_some())
}

fn ensure_external_url(value: &str) -> rusqlite::Result<()> {
    if is_external(value) {
        Ok(())
    } else {
        Err(invalid_url_error())
    }
}

fn ensure_history_url(value: &str) -> rusqlite::Result<()> {
    if is_external(value) {
        Ok(())
    } else {
        Err(invalid_url_error())
    }
}

pub fn is_external(value: &str) -> bool {
    Url::parse(value).is_ok_and(|url| {
        matches!(url.scheme(), "http" | "https")
            && url
                .host_str()
                .is_some_and(|host| !host.eq_ignore_ascii_case("axiom.localhost"))
    })
}

fn invalid_url_error() -> rusqlite::Error {
    rusqlite::Error::InvalidParameterName("url".to_string())
}

fn parse_bool_pref(value: &str) -> rusqlite::Result<bool> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(invalid_settings_error()),
    }
}

fn invalid_settings_error() -> rusqlite::Error {
    rusqlite::Error::InvalidParameterName("settings".to_string())
}

fn now_unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::{SavedTab, Session, Store};
    use crate::settings::{SearchEngine, Settings};
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn database_path() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "axiom-storage-{}-{nonce}.sqlite",
            std::process::id()
        ))
    }

    #[test]
    fn session_history_and_bookmarks_survive_reopen() {
        let path = database_path();
        {
            let mut store = Store::open(&path).unwrap();
            let session = Session {
                tabs: vec![
                    SavedTab {
                        url: "https://example.com/".into(),
                        title: "Example".into(),
                        pinned: true,
                    },
                    SavedTab {
                        url: "http://localhost:3000/".into(),
                        title: "Local app".into(),
                        pinned: false,
                    },
                ],
                active: 1,
            };
            store.save_session(&session).unwrap();
            store.add_visit("https://example.com/", "Example").unwrap();
            assert!(store
                .toggle_bookmark("https://example.com/", "Example")
                .unwrap());
        }

        {
            let store = Store::open(&path).unwrap();
            let session = store.load_session().unwrap();
            assert_eq!(session.active, 1);
            assert!(session.tabs[0].pinned);
            assert_eq!(session.tabs[1].title, "Local app");
            assert_eq!(store.history().unwrap().len(), 1);
            assert!(store.is_bookmarked("https://example.com/").unwrap());
            assert_eq!(store.bookmarks().unwrap().len(), 1);

            store.clear_history().unwrap();
            assert!(store.history().unwrap().is_empty());
            assert!(store.is_bookmarked("https://example.com/").unwrap());
            assert_eq!(store.load_session().unwrap(), session);
        }
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn invalid_restored_rows_are_filtered_without_losing_active_selection() {
        let path = database_path();
        let mut store = Store::open(&path).unwrap();
        store
            .save_session(&Session {
                tabs: vec![
                    SavedTab {
                        url: "https://one.example/".into(),
                        title: "One".into(),
                        pinned: false,
                    },
                    SavedTab {
                        url: "https://two.example/".into(),
                        title: "Two".into(),
                        pinned: false,
                    },
                ],
                active: 1,
            })
            .unwrap();
        store
            .connection
            .execute(
                "UPDATE sessions SET url = 'javascript:alert(1)' WHERE position = 0",
                [],
            )
            .unwrap();

        let loaded = store.load_session().unwrap();
        assert_eq!(loaded.tabs.len(), 1);
        assert_eq!(loaded.tabs[0].title, "Two");
        assert_eq!(loaded.active, 0);
        drop(store);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn failed_session_save_keeps_previous_session() {
        let path = database_path();
        let mut store = Store::open(&path).unwrap();
        let previous = Session {
            tabs: vec![SavedTab {
                url: "https://safe.example/".into(),
                title: "Safe".into(),
                pinned: false,
            }],
            active: 0,
        };
        store.save_session(&previous).unwrap();
        let invalid = Session {
            tabs: vec![SavedTab {
                url: "javascript:alert(1)".into(),
                title: "Bad".into(),
                pinned: false,
            }],
            active: 0,
        };
        assert!(store.save_session(&invalid).is_err());
        assert_eq!(store.load_session().unwrap(), previous);
        drop(store);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn history_skips_blank_and_internal_pages_but_session_keeps_blank() {
        let path = database_path();
        let mut store = Store::open(&path).unwrap();
        assert!(store.add_visit("about:blank", "Blank").is_err());
        assert!(store
            .add_visit("http://axiom.localhost/settings", "Settings")
            .is_err());
        store
            .save_session(&Session {
                tabs: vec![SavedTab {
                    url: "about:blank".into(),
                    title: "New tab".into(),
                    pinned: false,
                }],
                active: 0,
            })
            .unwrap();
        assert_eq!(store.load_session().unwrap().tabs[0].url, "about:blank");
        assert!(store.history().unwrap().is_empty());
        drop(store);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn settings_persist_when_database_is_reopened() {
        let path = database_path();
        let settings = Settings {
            restore_session: false,
            lazy_restore: true,
            search_engine: SearchEngine::DuckDuckGo,
            confirm_close_multiple: true,
            home_url: "https://example.com/home".into(),
            page_zoom: 135,
            remember_history: false,
        };
        {
            let store = Store::open(&path).unwrap();
            assert_eq!(store.settings().unwrap(), Settings::default());
            store.save_settings(&settings).unwrap();
        }
        {
            let store = Store::open(&path).unwrap();
            assert_eq!(store.settings().unwrap(), settings);
            store
                .connection
                .execute(
                    "DELETE FROM preferences WHERE key IN ('page_zoom', 'remember_history')",
                    [],
                )
                .unwrap();
            let migrated = store.settings().unwrap();
            assert_eq!(migrated.page_zoom, 100);
            assert!(migrated.remember_history);
        }
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn bookmarks_can_be_read_updated_and_removed() {
        let path = database_path();
        let store = Store::open(&path).unwrap();
        assert!(store
            .toggle_bookmark("https://example.com/", "Example")
            .unwrap());
        let entry = store.bookmarks().unwrap().pop().unwrap();
        assert_eq!(store.bookmark(entry.id).unwrap(), Some(entry.clone()));

        store
            .update_bookmark(entry.id, "https://example.org/guide", "Guide")
            .unwrap();
        let updated = store.bookmark(entry.id).unwrap().unwrap();
        assert_eq!(updated.url, "https://example.org/guide");
        assert_eq!(updated.title, "Guide");
        assert!(store
            .update_bookmark(entry.id, "about:blank", "Blank")
            .is_err());

        store.remove_bookmark(entry.id).unwrap();
        assert!(store.bookmark(entry.id).unwrap().is_none());
        assert!(store.bookmarks().unwrap().is_empty());
        drop(store);
        let _ = std::fs::remove_file(path);
    }
}
