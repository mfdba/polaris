use anyhow::anyhow;
use diesel;
use diesel::prelude::*;

use super::*;
use crate::db::DB;

const HASH_ITERATIONS: u32 = 10000;

#[derive(Clone)]
pub struct Manager {
	pub db: DB,
}

impl Manager {
	pub fn new(db: DB) -> Self {
		Self { db }
	}

	pub async fn create_user(&self, username: &str, password: &str) -> Result<(), Error> {
		if password.is_empty() {
			return Err(Error::EmptyPassword);
		}
		let password_hash = hash_password(password)?;
		let connection = self.db.connect().await?;
		let new_user = User {
			name: username.to_owned(),
			password_hash,
			admin: 0,
		};
		diesel::insert_into(users::table)
			.values(&new_user)
			.execute(&*connection)
			.map_err(|_| Error::Unspecified)?;
		Ok(())
	}

	pub async fn set_password(&self, username: &str, password: &str) -> Result<(), Error> {
		let password_hash = hash_password(password)?;
		let connection = self.db.connect().await?;
		diesel::update(users::table.filter(users::name.eq(username)))
			.set(users::password_hash.eq(password_hash))
			.execute(&*connection)
			.map_err(|_| Error::Unspecified)?;
		Ok(())
	}

	pub async fn auth(&self, username: &str, password: &str) -> anyhow::Result<bool> {
		use crate::db::users::dsl::*;
		let connection = self.db.connect().await?;
		match users
			.select(password_hash)
			.filter(name.eq(username))
			.get_result(&*connection)
		{
			Err(diesel::result::Error::NotFound) => Ok(false),
			Ok(hash) => {
				let hash: String = hash;
				Ok(verify_password(&hash, password))
			}
			Err(e) => Err(e.into()),
		}
	}

	pub async fn count(&self) -> anyhow::Result<i64> {
		use crate::db::users::dsl::*;
		let connection = self.db.connect().await?;
		let count = users.count().get_result(&*connection)?;
		Ok(count)
	}

	pub async fn exists(&self, username: &str) -> anyhow::Result<bool> {
		use crate::db::users::dsl::*;
		let connection = self.db.connect().await?;
		let results: Vec<String> = users
			.select(name)
			.filter(name.eq(username))
			.get_results(&*connection)?;
		Ok(results.len() > 0)
	}

	pub async fn is_admin(&self, username: &str) -> anyhow::Result<bool> {
		use crate::db::users::dsl::*;
		let connection = self.db.connect().await?;
		let is_admin: i32 = users
			.filter(name.eq(username))
			.select(admin)
			.get_result(&*connection)?;
		Ok(is_admin != 0)
	}

	pub async fn lastfm_link(
		&self,
		username: &str,
		lastfm_login: &str,
		session_key: &str,
	) -> anyhow::Result<()> {
		use crate::db::users::dsl::*;
		let connection = self.db.connect().await?;
		diesel::update(users.filter(name.eq(username)))
			.set((
				lastfm_username.eq(lastfm_login),
				lastfm_session_key.eq(session_key),
			))
			.execute(&*connection)?;
		Ok(())
	}

	pub async fn get_lastfm_session_key(&self, username: &str) -> anyhow::Result<String> {
		use crate::db::users::dsl::*;
		let connection = self.db.connect().await?;
		let token = users
			.filter(name.eq(username))
			.select(lastfm_session_key)
			.get_result(&*connection)?;
		match token {
			Some(t) => Ok(t),
			_ => Err(anyhow!("Missing LastFM credentials")),
		}
	}

	pub async fn is_lastfm_linked(&self, username: &str) -> bool {
		self.get_lastfm_session_key(username).await.is_ok()
	}

	pub async fn lastfm_unlink(&self, username: &str) -> anyhow::Result<()> {
		use crate::db::users::dsl::*;
		let connection = self.db.connect().await?;
		diesel::update(users.filter(name.eq(username)))
			.set((lastfm_session_key.eq(""), lastfm_username.eq("")))
			.execute(&*connection)?;
		Ok(())
	}
}

fn hash_password(password: &str) -> anyhow::Result<String> {
	match pbkdf2::pbkdf2_simple(password, HASH_ITERATIONS) {
		Ok(hash) => Ok(hash),
		Err(e) => Err(e.into()),
	}
}

fn verify_password(password_hash: &str, attempted_password: &str) -> bool {
	pbkdf2::pbkdf2_check(attempted_password, password_hash).is_ok()
}
