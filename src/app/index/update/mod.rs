use anyhow::*;
use log::{error, info};
use std::time;

mod cleaner;
mod collector;
mod inserter;
mod traverser;

use super::*;
use cleaner::Cleaner;
use collector::Collector;
use inserter::Inserter;
use traverser::Traverser;

impl Index {
	pub async fn update(&self) -> Result<()> {
		let start = time::Instant::now();
		info!("Beginning library index update");

		let album_art_pattern = self.config_manager.get_index_album_art_pattern().await?;

		let cleaner = Cleaner::new(self.db.clone(), self.vfs_manager.clone());
		cleaner.clean().await?;

		let (insert_sender, insert_receiver) = tokio::sync::mpsc::unbounded_channel();
		let inserter_db = self.db.clone();
		let insertion_thread = tokio::task::spawn(async move {
			let mut inserter = Inserter::new(inserter_db, insert_receiver);
			inserter.insert().await;
		});

		let (collect_sender, collect_receiver) = tokio::sync::mpsc::unbounded_channel();
		let collector_thread = tokio::task::spawn(async move {
			let mut collector = Collector::new(collect_receiver, insert_sender, album_art_pattern);
			collector.collect().await;
		});

		let vfs = self.vfs_manager.get_vfs().await?;
		let traverser_thread = tokio::task::spawn_blocking(move || {
			let mount_points = vfs.get_mount_points();
			let traverser = Traverser::new(collect_sender);
			traverser.traverse(mount_points.values().map(|p| p.clone()).collect());
		});

		if let Err(e) = traverser_thread.await {
			error!("Error joining on traverser thread: {:?}", e);
		}

		if let Err(e) = collector_thread.await {
			error!("Error joining on collector thread: {:?}", e);
		}

		if let Err(e) = insertion_thread.await {
			error!("Error joining on inserter thread: {:?}", e);
		}

		info!(
			"Library index update took {} seconds",
			start.elapsed().as_millis() as f32 / 1000.0
		);

		Ok(())
	}
}
