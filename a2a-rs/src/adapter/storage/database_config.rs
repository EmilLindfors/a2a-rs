//! Database configuration for SQLx storage

#[cfg(feature = "sqlx-storage")]
use bon::Builder;
#[cfg(feature = "sqlx-storage")]
use serde::{Deserialize, Serialize};
#[cfg(feature = "sqlx-storage")]
use std::collections::HashMap;

/// Supported database types, detected from the connection URL scheme.
#[cfg(feature = "sqlx-storage")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatabaseType {
    /// SQLite database (URLs starting with `sqlite:`)
    Sqlite,
    /// PostgreSQL database (URLs starting with `postgres:` or `postgresql:`)
    Postgres,
    /// MySQL database (URLs starting with `mysql:`)
    Mysql,
}

#[cfg(feature = "sqlx-storage")]
impl DatabaseType {
    /// Detect the database type from a connection URL.
    ///
    /// Returns `None` if the URL scheme is not recognized.
    pub fn from_url(url: &str) -> Option<Self> {
        if url.starts_with("sqlite:") {
            Some(Self::Sqlite)
        } else if url.starts_with("postgres:") || url.starts_with("postgresql:") {
            Some(Self::Postgres)
        } else if url.starts_with("mysql:") || url.starts_with("mariadb:") {
            Some(Self::Mysql)
        } else {
            None
        }
    }

    /// Check whether this database type is supported by the currently compiled features.
    pub fn is_feature_enabled(self) -> bool {
        match self {
            Self::Sqlite => cfg!(feature = "sqlite"),
            Self::Postgres => cfg!(feature = "postgres"),
            Self::Mysql => cfg!(feature = "mysql"),
        }
    }

    /// Returns the feature flag name needed to enable this database type.
    pub fn feature_name(self) -> &'static str {
        match self {
            Self::Sqlite => "sqlite",
            Self::Postgres => "postgres",
            Self::Mysql => "mysql",
        }
    }
}

#[cfg(feature = "sqlx-storage")]
impl std::fmt::Display for DatabaseType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sqlite => write!(f, "SQLite"),
            Self::Postgres => write!(f, "PostgreSQL"),
            Self::Mysql => write!(f, "MySQL"),
        }
    }
}

#[cfg(feature = "sqlx-storage")]
/// Database configuration with connection examples
#[derive(Debug, Clone, Builder, Serialize, Deserialize)]
pub struct DatabaseConfig {
    /// Database connection URL
    pub url: String,
    /// Maximum number of connections in the pool
    #[builder(default = 10)]
    pub max_connections: u32,
    /// Connection timeout in seconds
    #[builder(default = 30)]
    pub timeout_seconds: u64,
    /// Whether to enable SQL query logging
    #[builder(default = false)]
    pub enable_logging: bool,
}

#[cfg(feature = "sqlx-storage")]
impl DatabaseConfig {
    /// Example configurations for different environments and databases
    pub fn examples() -> HashMap<&'static str, Self> {
        [
            (
                "sqlite_memory",
                Self::builder()
                    .url("sqlite::memory:".to_string())
                    .max_connections(1)
                    .enable_logging(true)
                    .build(),
            ),
            (
                "sqlite_file",
                Self::builder()
                    .url("sqlite:a2a_tasks.db".to_string())
                    .max_connections(5)
                    .build(),
            ),
            (
                "postgres_dev",
                Self::builder()
                    .url("postgres://user:password@localhost/a2a_dev".to_string())
                    .max_connections(10)
                    .timeout_seconds(10)
                    .build(),
            ),
            (
                "postgres_prod",
                Self::builder()
                    .url("postgres://user:password@prod-db/a2a_prod".to_string())
                    .max_connections(50)
                    .timeout_seconds(5)
                    .enable_logging(false)
                    .build(),
            ),
            (
                "mysql_dev",
                Self::builder()
                    .url("mysql://user:password@localhost/a2a_dev".to_string())
                    .max_connections(10)
                    .timeout_seconds(10)
                    .build(),
            ),
        ]
        .into_iter()
        .collect()
    }

    /// Create a new configuration from environment variables
    ///
    /// Expected environment variables:
    /// - `DATABASE_URL`: Required - the database connection URL
    /// - `DATABASE_MAX_CONNECTIONS`: Optional - defaults to 10
    /// - `DATABASE_TIMEOUT_SECONDS`: Optional - defaults to 30  
    /// - `DATABASE_ENABLE_LOGGING`: Optional - defaults to false
    pub fn from_env() -> Result<Self, std::env::VarError> {
        let url = std::env::var("DATABASE_URL")?;

        let max_connections = std::env::var("DATABASE_MAX_CONNECTIONS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(10);

        let timeout_seconds = std::env::var("DATABASE_TIMEOUT_SECONDS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(30);

        let enable_logging = std::env::var("DATABASE_ENABLE_LOGGING")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(false);

        Ok(Self::builder()
            .url(url)
            .max_connections(max_connections)
            .timeout_seconds(timeout_seconds)
            .enable_logging(enable_logging)
            .build())
    }

    /// Validate the configuration
    pub fn validate(&self) -> Result<(), String> {
        if self.url.is_empty() {
            return Err("Database URL cannot be empty".to_string());
        }

        if self.max_connections == 0 {
            return Err("Max connections must be greater than 0".to_string());
        }

        if self.timeout_seconds == 0 {
            return Err("Timeout must be greater than 0".to_string());
        }

        // Basic URL validation
        if !self.url.contains("://") && !self.url.starts_with("sqlite:") {
            return Err(
                "Database URL must contain a protocol (e.g., sqlite://, postgres://, mysql://)"
                    .to_string(),
            );
        }

        Ok(())
    }

    /// Get the database type from the URL.
    ///
    /// Returns `None` if the URL scheme is not recognized.
    pub fn database_type(&self) -> Option<DatabaseType> {
        DatabaseType::from_url(&self.url)
    }

    /// Validate that the database URL scheme matches a compiled feature.
    ///
    /// Returns an error if the URL scheme is unrecognized or if the corresponding
    /// feature flag is not enabled.
    pub fn validate_database_support(&self) -> Result<DatabaseType, String> {
        let db_type = self.database_type().ok_or_else(|| {
            format!(
                "Unrecognized database URL scheme in '{}'. Expected sqlite:, postgres:, or mysql:",
                self.url
            )
        })?;

        if !db_type.is_feature_enabled() {
            return Err(format!(
                "{} database detected from URL but the '{}' feature is not enabled. \
                 Add `features = [\"{}\"]` to your a2a-rs dependency.",
                db_type,
                db_type.feature_name(),
                db_type.feature_name(),
            ));
        }

        Ok(db_type)
    }
}

#[cfg(feature = "sqlx-storage")]
impl Default for DatabaseConfig {
    fn default() -> Self {
        Self::builder().url("sqlite::memory:".to_string()).build()
    }
}

#[cfg(test)]
#[cfg(feature = "sqlx-storage")]
mod tests {
    use super::*;

    #[test]
    fn test_database_config_validation() {
        // Valid config
        let config = DatabaseConfig::builder()
            .url("sqlite:test.db".to_string())
            .build();
        assert!(config.validate().is_ok());

        // Empty URL
        let config = DatabaseConfig::builder().url("".to_string()).build();
        assert!(config.validate().is_err());

        // Invalid max connections
        let config = DatabaseConfig::builder()
            .url("sqlite:test.db".to_string())
            .max_connections(0)
            .build();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_database_type_detection() {
        let sqlite_config = DatabaseConfig::builder()
            .url("sqlite:test.db".to_string())
            .build();
        assert_eq!(sqlite_config.database_type(), Some(DatabaseType::Sqlite));

        let postgres_config = DatabaseConfig::builder()
            .url("postgres://localhost/test".to_string())
            .build();
        assert_eq!(postgres_config.database_type(), Some(DatabaseType::Postgres));

        let postgresql_config = DatabaseConfig::builder()
            .url("postgresql://localhost/test".to_string())
            .build();
        assert_eq!(
            postgresql_config.database_type(),
            Some(DatabaseType::Postgres)
        );

        let mysql_config = DatabaseConfig::builder()
            .url("mysql://localhost/test".to_string())
            .build();
        assert_eq!(mysql_config.database_type(), Some(DatabaseType::Mysql));

        let unknown_config = DatabaseConfig::builder()
            .url("http://localhost".to_string())
            .build();
        assert_eq!(unknown_config.database_type(), None);
    }

    #[test]
    fn test_database_type_from_url() {
        assert_eq!(
            DatabaseType::from_url("sqlite::memory:"),
            Some(DatabaseType::Sqlite)
        );
        assert_eq!(
            DatabaseType::from_url("sqlite:data.db"),
            Some(DatabaseType::Sqlite)
        );
        assert_eq!(
            DatabaseType::from_url("postgres://user:pass@host/db"),
            Some(DatabaseType::Postgres)
        );
        assert_eq!(
            DatabaseType::from_url("postgresql://user:pass@host/db"),
            Some(DatabaseType::Postgres)
        );
        assert_eq!(
            DatabaseType::from_url("mysql://user:pass@host/db"),
            Some(DatabaseType::Mysql)
        );
        assert_eq!(
            DatabaseType::from_url("mariadb://user:pass@host/db"),
            Some(DatabaseType::Mysql)
        );
        assert_eq!(DatabaseType::from_url("ftp://something"), None);
    }

    #[test]
    fn test_examples() {
        let examples = DatabaseConfig::examples();
        assert!(examples.contains_key("sqlite_memory"));
        assert!(examples.contains_key("postgres_dev"));

        // Validate all examples
        for (name, config) in examples {
            assert!(
                config.validate().is_ok(),
                "Example '{}' failed validation",
                name
            );
        }
    }
}
