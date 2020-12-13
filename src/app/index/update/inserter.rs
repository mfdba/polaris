use anyhow::*;
use diesel;
use diesel::prelude::*;
use log::error;
use tokio::sync::mpsc::UnboundedReceiver;

use crate::db::{directories, songs, DB};

const INDEX_BUILDING_INSERT_BUFFER_SIZE: usize = 1000; // Insertions in each transaction

#[derive(Debug, Insertable)]
#[table_name = "songs"]
pub struct Song {
	pub path: String,
	pub parent: String,
	pub track_number: Option<i32>,
	pub disc_number: Option<i32>,
	pub title: Option<String>,
	pub artist: Option<String>,
	pub album_artist: Option<String>,
	pub year: Option<i32>,
	pub album: Option<String>,
	pub artwork: Option<String>,
	pub duration: Option<i32>,
}

#[derive(Debug, Insertable)]
#[table_name = "directories"]
pub struct Directory {
	pub path: String,
	pub parent: Option<String>,
	pub artist: Option<String>,
	pub year: Option<i32>,
	pub album: Option<String>,
	pub artwork: Option<String>,
	pub date_added: i32,
}

pub enum Item {
	Directory(Directory),
	Song(Song),
}

pub struct Inserter {
	receiver: UnboundedReceiver<Item>,
	new_directories: Vec<Directory>,
	new_songs: Vec<Song>,
	db: DB,
}

impl Inserter {
	pub fn new(db: DB, receiver: UnboundedReceiver<Item>) -> Self {
		let new_directories = Vec::with_capacity(INDEX_BUILDING_INSERT_BUFFER_SIZE);
		let new_songs = Vec::with_capacity(INDEX_BUILDING_INSERT_BUFFER_SIZE);
		Self {
			db,
			receiver,
			new_directories,
			new_songs,
		}
	}

	pub async fn insert(&mut self) {
		loop {
			match self.receiver.recv().await {
				Some(item) => self.insert_item(item).await,
				None => break,
			}
		}

		if self.new_directories.len() > 0 {
			self.flush_directories().await;
		}

		if self.new_songs.len() > 0 {
			self.flush_songs().await;
		}
	}

	async fn insert_item(&mut self, insert: Item) {
		match insert {
			Item::Directory(d) => {
				self.new_directories.push(d);
				if self.new_directories.len() >= INDEX_BUILDING_INSERT_BUFFER_SIZE {
					self.flush_directories().await;
				}
			}
			Item::Song(s) => {
				self.new_songs.push(s);
				if self.new_songs.len() >= INDEX_BUILDING_INSERT_BUFFER_SIZE {
					self.flush_songs().await;
				}
			}
		};
	}

	async fn flush_directories(&mut self) {
		if self
			.db
			.connect()
			.await
			.and_then(|connection| {
				diesel::insert_into(directories::table)
					.values(&self.new_directories)
					.execute(&**connection) // TODO https://github.com/diesel-rs/diesel/issues/1822
					.map_err(Error::new)
			})
			.is_err()
		{
			error!("Could not insert new directories in database");
		}
		self.new_directories.clear();
	}

	async fn flush_songs(&mut self) {
		if self
			.db
			.connect()
			.await
			.and_then(|connection| {
				diesel::insert_into(songs::table)
					.values(&self.new_songs)
					.execute(&**connection) // TODO https://github.com/diesel-rs/diesel/issues/1822
					.map_err(Error::new)
			})
			.is_err()
		{
			error!("Could not insert new songs in database");
		}
		self.new_songs.clear();
	}
}
