use super::model::{FeedStatus, FeedSummary};
use rusqlite::Connection;
use std::path::Path;
use thiserror::Error;

fn collect_results<T, E>(items: impl IntoIterator<Item = Result<T, E>>) -> Result<Vec<T>, E> {
    let iter = items.into_iter();
    let mut result = Vec::with_capacity(iter.size_hint().0);
    for item in iter {
        result.push(item?);
    }
    Ok(result)
}

#[derive(Debug, Error)]
pub enum ConnectionError {
    #[error("Database query failed")]
    SqliteError(#[from] rusqlite::Error),
    #[error("Database was updated in a newer version of hedgehog (db version: {version}, current: {version})")]
    VersionUnknown { version: u32, current: u32 },
}

#[derive(Debug)]
struct SqliteDataProvider {
    connection: Connection,
}

impl SqliteDataProvider {
    const CURRENT_VERSION: u32 = 1;

    pub fn connect<P: AsRef<Path>>(path: P) -> Result<Self, ConnectionError> {
        let connection = Connection::open(path)?;
        let version = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version > Self::CURRENT_VERSION {
            return Err(ConnectionError::VersionUnknown {
                version,
                current: Self::CURRENT_VERSION,
            });
        }

        if version < 1 {
            connection.execute_batch(include_str!("schema/init.sql"))?;
        }

        connection.pragma_update(None, "user_version", Self::CURRENT_VERSION)?;
        Ok(SqliteDataProvider { connection })
    }

    pub fn query_feeds(&self) -> Result<Vec<FeedSummary>, rusqlite::Error> {
        let mut select = self
            .connection
            .prepare("SELECT rowid, title, source, status, error_code FROM feeds")?;
        let rows = select.query_map([], |row| {
            Ok(FeedSummary {
                id: row.get(0)?,
                title: row.get(1)?,
                source: row.get(2)?,
                status: FeedStatus::from_db(row.get(3)?, row.get(4)?),
            })
        })?;
        collect_results(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::{ConnectionError, SqliteDataProvider};

    #[test]
    fn initializes_if_new() {
        let dir = tempfile::tempdir().unwrap();
        let mut path = dir.path().to_path_buf();
        path.push("db.sqlite");

        SqliteDataProvider::connect(&path).unwrap();

        let connection = rusqlite::Connection::open(path).unwrap();
        let user_version: u32 = connection
            .pragma_query_value(None, "user_version", |value| value.get(0))
            .unwrap();
        assert_eq!(user_version, SqliteDataProvider::CURRENT_VERSION);
    }

    #[test]
    fn fails_if_newer() {
        let dir = tempfile::tempdir().unwrap();
        let mut path = dir.path().to_path_buf();
        path.push("db.sqlite");

        let connection = rusqlite::Connection::open(&path).unwrap();
        connection
            .pragma_update(None, "user_version", 20u32)
            .unwrap();
        drop(connection);

        let error = SqliteDataProvider::connect(path).unwrap_err();
        assert!(matches!(
            error,
            ConnectionError::VersionUnknown {
                version: 20,
                current: 1
            }
        ));
    }
}
