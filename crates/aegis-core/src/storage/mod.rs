mod traits;
#[cfg(feature = "sqlite")]
pub mod sqlite;
#[cfg(feature = "postgres")]
pub mod postgres;
#[cfg(feature = "rocksdb")]
pub mod rocksdb;
#[cfg(feature = "mysql")]
pub mod mysql;
pub mod memory;
pub mod async_traits;
#[cfg(target_arch = "wasm32")]
pub mod indexeddb;

#[cfg(feature = "sqlite")]
pub use sqlite::SqliteStorage;
#[cfg(feature = "postgres")]
pub use postgres::PostgresStorage;
#[cfg(feature = "rocksdb")]
pub use rocksdb::RocksDbStorage;
#[cfg(feature = "mysql")]
pub use mysql::MysqlStorage;
pub use memory::InMemoryStorage;
pub use traits::*;
pub use traits::compute_event_hash;
