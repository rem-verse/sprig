//! A "WAL" style log for all sata requests, where before processing them we
//! will write the operation that occured, along with the return code.
//!
//! The goal of this is to make it easy to get a full log of what the PCFS Sata
//! server is doing, and where it might differ, without having to fully break
//! into a scientists like API where we're doing full diffing between
//! everything that's happening.
//!
//! THIS DOES HAVE A PERFORMANCE OVERHEAD, specifically we will be parsing
//! every sata request twice (or attempting too). So if you're not in a place
//! where you can spare that much CPU be aware.

pub mod layer;
mod mapper;

use crate::{
	errors::CatBridgeError,
	fsemul::pcfs::sata::server::connection_flags::SataConnectionFlags,
	net::models::{FromRequest, FromRequestParts, Request},
};
use bytes::Bytes;
use fnv::FnvHashMap;
use std::{
	collections::VecDeque,
	path::PathBuf,
	time::{Duration, Instant},
};
use tokio::{
	fs::File,
	io::{AsyncWriteExt, BufWriter},
	sync::mpsc::{
		Receiver as BoundedReceiver, Sender as BoundedSender, channel as bounded_channel,
	},
	task::Builder as TaskBuilder,
	time::sleep,
};
use tracing::error;

/// A reference to a single unique 'write-ahead log' for SATA.
///
/// This keeps track of requests, and responses coming in and out of a sata
/// server, to determine what is actually happening.
///
/// This WAL is expected to be from the 'servers' point of view, NOT the clients.
#[derive(Clone, Debug)]
pub struct WriteAheadLog {
	logger: BoundedSender<WriteAheadLogMessage>,
}

impl WriteAheadLog {
	/// Create a new WAL for sata requests/responses.
	///
	/// This is the 'entrypoint', and as a result will spawn the task
	/// to start receiving messages.
	///
	/// ## Errors
	///
	/// If we cannot spawn the background task.
	pub fn new(wal: PathBuf) -> Result<Self, CatBridgeError> {
		let (sender, receiver) = bounded_channel(8192);

		TaskBuilder::new()
			.name("cat_dev::fsemul::pcfs::sata::server::wal::write_to_log")
			.spawn(async move {
				process_wal(receiver, wal).await;
				error!("WAL SHUTTING DOWN...");
			})
			.map_err(CatBridgeError::SpawnFailure)?;

		Ok(Self { logger: sender })
	}

	/// Communicate that a stream has opened up.
	pub async fn record_open_stream(&self, stream_id: u64) {
		_ = self
			.logger
			.send(WriteAheadLogMessage::OpenStream(stream_id))
			.await;
	}

	/// Communicate that a stream has closed.
	pub async fn record_close_stream(&self, stream_id: u64) {
		_ = self
			.logger
			.send(WriteAheadLogMessage::CloseStream(stream_id))
			.await;
	}

	/// Communicate that an out of band (e.g. nor equest/response flow) read for write file.
	pub async fn record_oob_file_write_read(&self, stream_id: u64, fd: i32, length: usize) {
		_ = self
			.logger
			.send(WriteAheadLogMessage::WriteFileRead(stream_id, fd, length))
			.await;
	}

	/// Communicate that a request has come into the server.
	pub async fn record_request(&self, stream_id: u64, request: Bytes) {
		_ = self
			.logger
			.send(WriteAheadLogMessage::StreamEvent(stream_id, false, request))
			.await;
	}

	/// Communicate that a response has come into the server.
	pub async fn record_response(&self, stream_id: u64, request: Bytes) {
		_ = self
			.logger
			.send(WriteAheadLogMessage::StreamEvent(stream_id, true, request))
			.await;
	}
}

impl<State: Clone + Send + Sync + 'static> FromRequestParts<State> for Option<WriteAheadLog> {
	async fn from_request_parts(parts: &mut Request<State>) -> Result<Self, CatBridgeError> {
		Ok(parts.extensions().get::<WriteAheadLog>().cloned())
	}
}

impl<State: Clone + Send + Sync + 'static> FromRequest<State> for Option<WriteAheadLog> {
	async fn from_request(req: Request<State>) -> Result<Self, CatBridgeError> {
		Ok(req.extensions_owned().remove::<WriteAheadLog>())
	}
}

/// A message sent over the logging channel to be written to the WAL.
#[derive(Clone, Debug)]
enum WriteAheadLogMessage {
	/// A new TCP stream has been opened (stream id).
	OpenStream(u64),
	/// A TCP stream has closed (stream id).
	CloseStream(u64),
	/// An out of band Write File Response has been read in (stream id, fd, length read).
	WriteFileRead(u64, i32, usize),
	/// A regular stream request/response has happened (stream id, is response, data).
	StreamEvent(u64, bool, Bytes),
}

#[allow(
  // TODO(mythra): clean this up.
  clippy::too_many_lines,
)]
async fn process_wal(mut stream: BoundedReceiver<WriteAheadLogMessage>, path: PathBuf) {
	let mut fd = BufWriter::new(match File::create_new(path).await {
		Ok(fd) => fd,
		Err(cause) => {
			error!(?cause, "Failed to open WAL file, will not generate WAL!");
			return;
		}
	});
	// A list of open fd's so we just write operations on which files they're happening on
	// rather than an explicit fd.
	let mut fd_map: FnvHashMap<u64, FnvHashMap<i32, String>> = FnvHashMap::default();
	let mut folder_map: FnvHashMap<u64, FnvHashMap<i32, String>> = FnvHashMap::default();
	let mut connection_flags: FnvHashMap<u64, SataConnectionFlags> = FnvHashMap::default();
	let mut waiting_requests: FnvHashMap<u64, VecDeque<mapper::WaitingRequest>> =
		FnvHashMap::default();

	// When the last 'WriteAheadLog' is dropped, and thus the producer is closed,
	// we will automatically save. This keeps us from running forever.
	let mut last_flush = Instant::now();
	let mut needs_flush = false;

	loop {
		let msg_opt;
		tokio::select! {
			opt = stream.recv() => {
				msg_opt = opt;
			}
			() = sleep(Duration::from_secs(5)) => {
				if needs_flush {
					if let Err(cause) = fd.flush().await {
						error!(?cause, "failed to flush WAL log for SATA!");
					}
					needs_flush = false;
				}
				continue;
			}
		}

		let Some(msg) = msg_opt else {
			break;
		};
		let current_time = Instant::now();
		if current_time.duration_since(last_flush) > Duration::from_secs(3) {
			if let Err(cause) = fd.flush().await {
				error!(?cause, "failed to flush WAL log for SATA!");
			}

			last_flush = current_time;
			needs_flush = false;
		} else {
			needs_flush = true;
		}

		let (stream_id, is_response, data) = match msg {
			WriteAheadLogMessage::OpenStream(stream_id) => {
				waiting_requests.insert(stream_id, VecDeque::with_capacity(1));
				fd_map.insert(stream_id, FnvHashMap::default());
				folder_map.insert(stream_id, FnvHashMap::default());
				connection_flags.insert(stream_id, SataConnectionFlags::new());
				continue;
			}
			WriteAheadLogMessage::CloseStream(stream_id) => {
				waiting_requests.remove(&stream_id);
				fd_map.remove(&stream_id);
				folder_map.remove(&stream_id);
				connection_flags.remove(&stream_id);
				continue;
			}
			WriteAheadLogMessage::WriteFileRead(stream_id, file_desc, size) => {
				if let Some(req_waiting) = waiting_requests.get_mut(&stream_id) {
					// TODO(mythra): god this is messy, clean this shit up.
					if let Some(front) = req_waiting.front() {
						match &front {
							&mapper::WaitingRequest::WriteFile(path, _) => {
								let final_path = if let Some(p) = fd_map
									.get(&stream_id)
									.expect("impossible: fd_map / waiting_request out of sync?")
									.get(&file_desc)
								{
									p.to_owned()
								} else {
									"<Unknown; {file_desc}>".to_owned()
								};

								if path == &final_path {
									if let Err(cause) =
										fd.write_all(format!("  ->{size}\n").as_bytes()).await
									{
										error!(
											?cause,
											"Failed to write request to WAL log! MAY BE INCOMPLETE!"
										);
									}
								} else {
									error!(
										stream_id,
										fd = file_desc,
										size,
										"Mismatched WriteFileRead????"
									);
								}
							}
							_ => {
								error!(
									stream_id,
									fd = file_desc,
									size,
									"Got WriteFileRead for non write-file request???"
								);
							}
						}
					} else {
						error!(
							stream_id,
							fd = file_desc,
							size,
							"Got WriteFileRead when not waiting a request???"
						);
					}
				} else {
					error!(
						stream_id,
						"Got WriteFileRead for stream that doesn't exist???"
					);
				}

				continue;
			}
			WriteAheadLogMessage::StreamEvent(sid, isresp, data) => (sid, isresp, data),
		};

		if is_response {
			let Some(list_mut) = waiting_requests.get_mut(&stream_id) else {
				error!(
					stream_id,
					"got WAL message for stream that doesn't exist???"
				);
				return;
			};
			let Some(req) = list_mut.pop_front() else {
				error!(
					stream_id,
					response = format!("{data:02x?}"),
					"got WAL response for stream that is not waiting on request???"
				);
				return;
			};
			let fd_map_mut = fd_map
				.get_mut(&stream_id)
				.expect("impossible fd_map/waiting_requests out of sync!");
			let folder_map_mut = folder_map
				.get_mut(&stream_id)
				.expect("impossible folder_map/waiting_requests out of sync!");
			let conn_flags = connection_flags
				.get(&stream_id)
				.expect("impossible connection_flags/waiting_requests out of sync!");

			let resp = mapper::WaitingResponse::parse(conn_flags, &req, data);
			match resp {
				mapper::WaitingResponse::OpenFile(fdres) => {
					if let Ok(fd) = fdres.result() {
						if let mapper::WaitingRequest::OpenFile(path, _) = req {
							fd_map_mut.insert(fd, path);
						}
					}
				}
				mapper::WaitingResponse::OpenFolder(fdres) => {
					if let Ok(fd) = fdres.result() {
						if let mapper::WaitingRequest::OpenFolder(path) = req {
							folder_map_mut.insert(fd, path);
						}
					}
				}
				mapper::WaitingResponse::CloseFile(rc) => {
					if rc.0 == 0 {
						if let mapper::WaitingRequest::CloseFile(fd, _) = req {
							fd_map_mut.remove(&fd);
						}
					}
				}
				mapper::WaitingResponse::CloseFolder(rc) => {
					if rc.0 == 0 {
						if let mapper::WaitingRequest::CloseFolder(fd, _) = req {
							folder_map_mut.remove(&fd);
						}
					}
				}
				mapper::WaitingResponse::Pong(ffio, csr) => {
					conn_flags.set_ffio_enabled(ffio);
					conn_flags.set_csr_enabled(csr);
				}
				_ => {}
			}
			if let Err(cause) = fd.write_all(format!("<-{resp}\n").as_bytes()).await {
				error!(
					?cause,
					"Failed to write response to WAL log! MAY BE INCOMPLETE!"
				);
			}
		} else {
			let Some(list_mut) = waiting_requests.get_mut(&stream_id) else {
				error!(
					stream_id,
					"got WAL message for stream that doesn't exist???"
				);
				return;
			};

			let req = mapper::WaitingRequest::parse(
				fd_map
					.get(&stream_id)
					.expect("impossible fd_map/folder_map/waiting_requests out of sync!"),
				folder_map
					.get(&stream_id)
					.expect("impossible fd_map/folder_map/waiting_requests out of sync!"),
				data,
			);
			if let Err(cause) = fd.write_all(format!("->{req}\n").as_bytes()).await {
				error!(
					?cause,
					"Failed to write request to WAL log! MAY BE INCOMPLETE!"
				);
			}
			list_mut.push_back(req);
		}
	}

	if let Err(cause) = fd.flush().await {
		error!(?cause, "failed to flush WAL log for SATA!");
	}
}
