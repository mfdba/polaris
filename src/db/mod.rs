use anyhow::*;
use bb8::{Pool, PooledConnection};
use bb8_diesel::DieselConnectionManager;
use diesel::{sqlite::SqliteConnection, RunQueryDsl};
use diesel_migrations;
use std::path::{Path, PathBuf};

mod schema;

pub use self::schema::*;

#[allow(dead_code)]
const DB_MIGRATIONS_PATH: &str = "migrations";
embed_migrations!("migrations");

#[derive(Clone)]
pub struct DB {
	pool: Pool<DieselConnectionManager<SqliteConnection>>,
	location: PathBuf,
}

#[derive(Debug)]
struct ConnectionCustomizer {}
impl diesel::r2d2::CustomizeConnection<SqliteConnection, diesel::r2d2::Error>
	for ConnectionCustomizer
{
	fn on_acquire(&self, connection: &mut SqliteConnection) -> Result<(), diesel::r2d2::Error> {
		let query = diesel::sql_query(
			r#"
			PRAGMA busy_timeout = 60000;
			PRAGMA journal_mode = WAL;
			PRAGMA synchronous = NORMAL;
			PRAGMA foreign_keys = ON;
		"#,
		);
		query
			.execute(connection)
			.map_err(|e| diesel::r2d2::Error::QueryError(e))?;
		Ok(())
	}
}

impl<'a> DB {
	pub async fn new(path: &Path) -> Result<DB> {
		let manager = DieselConnectionManager::<SqliteConnection>::new(path.to_string_lossy());
		let pool = Pool::builder()
			// https://github.com/djc/bb8/issues/88
			// TODO
			// .connection_customizer(Box::new(ConnectionCustomizer {}))
			.build(manager)
			.await?;
		let db = DB {
			pool: pool,
			location: path.to_owned(),
		};
		db.migrate_up().await?;
		Ok(db)
	}

	pub fn location(&self) -> &Path {
		&self.location
	}

	pub async fn connect(
		&'a self,
	) -> Result<PooledConnection<'a, DieselConnectionManager<SqliteConnection>>> {
		self.pool.get().await.map_err(Error::new)
	}

	#[allow(dead_code)]
	async fn migrate_down(&self) -> Result<()> {
		let connection = self.connect().await.unwrap();
		loop {
			match diesel_migrations::revert_latest_migration_in_directory(
				&*connection,
				Path::new(DB_MIGRATIONS_PATH),
			) {
				Ok(_) => (),
				Err(diesel_migrations::RunMigrationsError::MigrationError(
					diesel_migrations::MigrationError::NoMigrationRun,
				)) => break,
				Err(e) => bail!(e),
			}
		}
		Ok(())
	}

	async fn migrate_up(&self) -> Result<()> {
		let connection = self.connect().await.unwrap();
		embedded_migrations::run(&*connection)?;
		Ok(())
	}
}

#[cfg(test)]
pub async fn get_test_db(name: &str) -> DB {
	use crate::app::{config, user};
	let config_path = Path::new("test-data/config.toml");
	let config = config::Config::from_path(&config_path).unwrap();

	let mut db_path = std::path::PathBuf::new();
	db_path.push("test-output");
	std::fs::create_dir_all(&db_path).unwrap();

	db_path.push(name);
	if db_path.exists() {
		std::fs::remove_file(&db_path).unwrap();
	}

	let db = DB::new(&db_path).await.unwrap();
	let user_manager = user::Manager::new(db.clone());
	let config_manager = config::Manager::new(db.clone(), user_manager);

	config_manager.amend(&config).await.unwrap();
	db
}

#[tokio::test]
async fn test_migrations_up() {
	get_test_db("migrations_up.sqlite").await;
}

#[tokio::test]
async fn test_migrations_down() {
	let db = get_test_db("migrations_down.sqlite").await;
	db.migrate_down().await.unwrap();
	db.migrate_up().await.unwrap();
}
