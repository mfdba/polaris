use anyhow::*;
use diesel;
use diesel::prelude::*;
use std::path::Path;

use crate::app::vfs;
use crate::db::{directories, songs, DB};

const INDEX_BUILDING_CLEAN_BUFFER_SIZE: usize = 500; // Deletions in each transaction

pub struct Cleaner {
	db: DB,
	vfs_manager: vfs::Manager,
}

impl Cleaner {
	pub fn new(db: DB, vfs_manager: vfs::Manager) -> Self {
		Self { db, vfs_manager }
	}

	pub async fn clean(&self) -> Result<()> {
		let vfs = self.vfs_manager.get_vfs()?;

		let connection = self.db.connect().await?;

		let all_directories: Vec<String> = {
			directories::table
				.select(directories::path)
				.load(&*connection)?
		};

		let all_songs: Vec<String> = {
			let connection = self.db.connect().await?;
			songs::table.select(songs::path).load(&*connection)?
		};

		// TODO consider re-introducing rayon for the filtering below
		// TODO consider using spawn_blocking to run the filtering below as it could take a while

		let list_missing_directories = || {
			all_directories
				.iter()
				.filter(|ref directory_path| {
					let path = Path::new(&directory_path);
					!path.exists() || vfs.real_to_virtual(path).is_err()
				})
				.collect::<Vec<_>>()
		};

		let list_missing_songs = || {
			all_songs
				.iter()
				.filter(|ref song_path| {
					let path = Path::new(&song_path);
					!path.exists() || vfs.real_to_virtual(path).is_err()
				})
				.collect::<Vec<_>>()
		};

		let thread_pool = rayon::ThreadPoolBuilder::new().build()?;
		let (missing_songs, missing_directories) =
			thread_pool.join(list_missing_directories, list_missing_songs);

		{
			let connection = self.db.connect().await?;
			for chunk in missing_directories[..].chunks(INDEX_BUILDING_CLEAN_BUFFER_SIZE) {
				diesel::delete(directories::table.filter(directories::path.eq_any(chunk)))
					.execute(&*connection)?;
			}
			for chunk in missing_songs[..].chunks(INDEX_BUILDING_CLEAN_BUFFER_SIZE) {
				diesel::delete(songs::table.filter(songs::path.eq_any(chunk)))
					.execute(&*connection)?;
			}
		}

		Ok(())
	}
}
