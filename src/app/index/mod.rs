use diesel;
use log::error;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use crate::app::{config, vfs};
use crate::db::DB;

mod metadata;
mod query;
#[cfg(test)]
mod test;
mod types;
mod update;

pub use self::query::*;
pub use self::types::*;
pub use self::update::*;

#[derive(Clone)]
pub struct Index {
	db: DB,
	vfs_manager: vfs::Manager,
	config_manager: config::Manager,
	pending_reindex: Arc<(Mutex<bool>, Condvar)>,
}

impl Index {
	pub fn new(db: DB, vfs_manager: vfs::Manager, config_manager: config::Manager) -> Self {
		let index = Self {
			db,
			vfs_manager,
			config_manager,
			pending_reindex: Arc::new((Mutex::new(false), Condvar::new())),
		};

		let commands_index = index.clone();
		std::thread::spawn(move || {
			commands_index.process_commands();
		});

		index
	}

	pub fn trigger_reindex(&self) {
		let (lock, cvar) = &*self.pending_reindex;
		let mut pending_reindex = lock.lock().unwrap();
		*pending_reindex = true;
		cvar.notify_one();
	}

	pub fn begin_periodic_updates(&self) {
		let auto_index = self.clone();
		std::thread::spawn(move || {
			auto_index.automatic_reindex();
		});
	}

	fn process_commands(&self) {
		loop {
			{
				let (lock, cvar) = &*self.pending_reindex;
				let mut pending = lock.lock().unwrap();
				while !*pending {
					pending = cvar.wait(pending).unwrap();
				}
				*pending = false;
			}
			if let Err(e) = self.update() {
				error!("Error while updating index: {}", e);
			}
		}
	}

	fn automatic_reindex(&self) {
		loop {
			self.trigger_reindex();

			let (tx, rx) = crossbeam_channel::unbounded();
			let self_clone = self.clone();
			tokio::spawn(async move {
				let duration = self_clone
					.config_manager
					.get_index_sleep_duration()
					.await
					.unwrap_or_else(|e| {
						error!("Could not retrieve index sleep duration: {}", e);
						Duration::from_secs(1800)
					});
				tx.send(duration).unwrap();
			});
			let sleep_duration = rx.recv().unwrap();
			std::thread::sleep(sleep_duration);
		}
	}
}
