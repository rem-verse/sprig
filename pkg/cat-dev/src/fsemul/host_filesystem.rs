//! A representation of the filesystem folder we end up serving a cat-dev
//! client.

use crate::{
	TitleID,
	errors::{CatBridgeError, FSError},
	fsemul::{
		bsf::BootSystemFile,
		dlf::DiskLayoutFile,
		errors::{FSEmulAPIError, FSEmulFSError},
		pcfs::errors::PCFSApiError,
	},
};
use bytes::{Bytes, BytesMut};
use scc::{
	HashMap as ConcurrentMap, HashSet as ConcurrentSet, hash_map::OccupiedEntry as CMOccupiedEntry,
};
use std::{
	collections::HashMap,
	ffi::OsString,
	fs::{DirEntry, read_dir},
	hash::RandomState,
	io::{Error as IOError, SeekFrom},
	path::{Path, PathBuf},
	sync::{
		Arc,
		atomic::{AtomicBool, AtomicI32, Ordering as AtomicOrdering},
	},
};
use tokio::{
	fs::{
		File, OpenOptions, copy as copy_file, create_dir_all, read_dir as async_read_dir,
		read_link, remove_dir_all, remove_file, rename, write as fs_write,
	},
	io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt},
	sync::Mutex,
};
use valuable::{Fields, NamedField, NamedValues, StructDef, Structable, Valuable, Value, Visit};
use walkdir::WalkDir;
use whoami::username;

/// A way to create truly unique file fd's. Just a counter going up.
static UNIQUE_FILE_FD: AtomicI32 = AtomicI32::new(1);
/// Current "FD" for directories. Just a counter going up.
static FOLDER_FD: AtomicI32 = AtomicI32::new(1);

/// A wrapper around interacting with the 'host' or PC filesystem for the
/// various times a cat-dev will reach out to the host.
///
/// This is little more than a wrapper around a [`PathBuf`], and targeted
/// methods to make getting files/generating default files/etc. easy. Most of
/// the actual logic for turning a request from `SDIO`, `ATAPI`, etc. all come
/// from those client/server implementations rather than the logic living here.
#[allow(
	// Clippy the type is _not_ that complex.
	clippy::type_complexity,
)]
#[derive(Clone, Debug)]
pub struct HostFilesystem {
	/// The path to the base data directory to serve a filesystem out of.
	cafe_sdk_path: PathBuf,
	/// The actively mounted "disc".
	///
	/// This is a tuple of (isSLC, isSystem, [`TitleID`]).
	///
	/// When a disc is mounted we will copy the title from SLC/MLC
	/// directory, into `disc/` recursively.
	disc_mounted: Arc<Mutex<Option<(bool, bool, TitleID)>>>,
	/// List of open file handles.
	///
	/// This contains a value of (file, file size, path, stream owner).
	open_file_handles: Arc<ConcurrentMap<i32, (File, u64, PathBuf, Option<u64>)>>,
	/// List of open folder "handles".
	///
	/// This contains a value of (directory items, index, is end, path, stream owner)
	open_folder_handles:
		Arc<ConcurrentMap<i32, (Vec<DirEntry>, usize, bool, PathBuf, Option<u64>)>>,
	/// A set of folders that we've "marked" as read-only.
	///
	/// We don't actually synchronize this to the filesystem because the original
	/// cafe-sdk was written for Windows 7 which silently ignores "Read-Only"
	/// attributes on directories. Still allowing you to create files within
	/// directories, modify them, etc.
	///
	/// This is not the case on older windows distributions, unix based distros,
	/// or similar.
	folders_marked_read_only: Arc<ConcurrentSet<PathBuf>>,
	/// If we are forcing unique file fd's. This should only be changed
	/// if we have not opened a file yet.
	is_using_unique_fds: bool,
	/// If we've opened a file, used to safely ensure we don't switch from
	/// unique fdf's to not.
	has_opened_file: Arc<AtomicBool>,
}

impl HostFilesystem {
	/// Create a filesystem from a root cafe dir.
	///
	/// If no cafe dir is provided, we will attempt to locate the default
	/// installation path for cafe sdk which is:
	///
	/// - `C:\cafe_sdk` on windows.
	/// - `/opt/cafe_sdk` on any unix/bsd like OS.
	///
	/// NOTE: This will validate that all title id paths are lowercase, as
	/// files are always expected to be lowercase when dealing with CAFE. Other
	/// files are usually kept in the correct naming format. HOWEVER, users may
	/// notice spurious errors with case-insensitivity on linux specifically. If
	/// transferring an SDK from a Windows/Mac Case Insensitive to a Mac/Linux
	/// case sensitive file system. It is recommended users
	/// create their own folder using our recovery tools, rather than
	/// rsync'ing a path over from case-insensitive, to case-sensitive.
	///
	/// ## Errors
	///
	/// If the Cafe SDK folder is corrupt, or can't be found. A Cafe SDK
	/// folder is considered corrupt if it is missing core files that we
	/// _need_ to be able to serve a Cafe-OS distribution. These file
	/// requirements may change from version to version of this crate, but should
	/// always be compatible with a clean cafe sdk folder.
	pub async fn from_cafe_dir(cafe_dir: Option<PathBuf>) -> Result<Self, FSError> {
		let Some(cafe_sdk_path) = cafe_dir.or_else(Self::default_cafe_folder) else {
			return Err(FSEmulFSError::CantFindCafeSdkPath.into());
		};

		Self::patch_case_sensitive_title_ids(&cafe_sdk_path).await?;

		for path in [
			&[
				"data", "mlc", "sys", "title", "00050030", "1001000a", "code", "app.xml",
			] as &[&str],
			&[
				"data", "mlc", "sys", "title", "00050030", "1001010a", "code", "app.xml",
			],
			&[
				"data", "mlc", "sys", "title", "00050030", "1001020a", "code", "app.xml",
			],
			&[
				"data", "mlc", "sys", "title", "00050010", "1f700500", "code",
			],
			&[
				"data", "mlc", "sys", "title", "00050010", "1f700500", "content",
			],
			&[
				"data", "mlc", "sys", "title", "00050010", "1f700500", "meta",
			],
			// Can't generate a `fw.img` for now.... :(
			&[
				"data", "slc", "sys", "title", "00050010", "1000400a", "code", "fw.img",
			],
		] {
			if !Self::join_many(&cafe_sdk_path, path).exists() {
				return Err(FSEmulFSError::CafeSdkPathCorrupt.into());
			}
		}

		Self::prepare_for_serving(&cafe_sdk_path).await?;
		let ro_folders = Self::get_default_read_only_folders(&cafe_sdk_path);

		Ok(Self {
			cafe_sdk_path,
			disc_mounted: Arc::new(Mutex::new(None)),
			folders_marked_read_only: Arc::new(ro_folders),
			open_file_handles: Arc::new(ConcurrentMap::new()),
			open_folder_handles: Arc::new(ConcurrentMap::new()),
			is_using_unique_fds: false,
			has_opened_file: Arc::new(AtomicBool::new(false)),
		})
	}

	/// The root path to the Cafe SDK.
	///
	/// *note: although we do expose this for logging, and other info... we do
	/// not recommend manually interacting with the SDK path. There are much
	/// better alternatives.*
	#[must_use]
	pub const fn cafe_sdk_path(&self) -> &PathBuf {
		&self.cafe_sdk_path
	}

	/// The root path to the Cafe SDK.
	///
	/// *note: although we do expose this for logging, and other info... we do
	/// not recommend manually interacting with the SDK path. There are much
	/// better alternatives.*
	#[must_use]
	pub fn disc_emu_path(&self) -> PathBuf {
		Self::join_many(&self.cafe_sdk_path, ["data", "disc"])
	}

	/// Force unique file descriptors for open files.
	///
	/// Certain OS's _can_ return duplicate fd's especially when opening,
	/// and closing files. This can make deciphering logs harder because the
	/// same FD may appear multiple times, when you're trying to just find
	/// the logs related to one file descriptor.
	///
	/// When unique fd's is turned on, similar to folders we just use a global
	/// wrapping counter so that way every file descriptor is guaranteed to be
	/// unique.
	///
	/// ## Errors
	///
	/// This will error if any file has ever been opened. This is because once
	/// a client has already connected, and done some stuff with file stuff it
	/// expects one set of behaviors, we cannot change another one.
	pub fn force_unique_fds(&mut self) -> Result<(), FSEmulAPIError> {
		if self.has_opened_file.load(AtomicOrdering::Relaxed) {
			Err(FSEmulAPIError::CannotSwapFdStrategy)
		} else {
			self.is_using_unique_fds = true;
			Ok(())
		}
	}

	/// Open a file, and return it's file descriptor number.
	///
	/// ## Errors
	///
	/// If we cannot open our file with the open options provided.
	pub async fn open_file(
		&self,
		open_options: OpenOptions,
		path: &PathBuf,
		stream_owner: Option<u64>,
	) -> Result<i32, FSError> {
		self.has_opened_file.store(true, AtomicOrdering::Relaxed);
		let fd = open_options.open(path).await?;
		let raw_fd;
		#[cfg(unix)]
		{
			use std::os::fd::AsRawFd;
			raw_fd = fd.as_raw_fd();
		}
		#[cfg(target_os = "windows")]
		{
			use std::os::windows::io::AsRawHandle;
			raw_fd = fd.as_raw_handle() as i32;
		}

		let md = fd.metadata().await?;
		let final_fd = if self.is_using_unique_fds {
			UNIQUE_FILE_FD.fetch_add(1, AtomicOrdering::SeqCst)
		} else {
			raw_fd
		};

		self.open_file_handles
			.insert(final_fd, (fd, md.len(), path.clone(), stream_owner))
			.map_err(|_| IOError::other("somehow got duplicate fd?"))?;
		Ok(final_fd)
	}

	/// Get a file from a file descriptor number.
	///
	/// This file must already be opened (in order to get the file descriptor).
	pub async fn get_file(
		&self,
		fd: i32,
		for_stream: Option<u64>,
	) -> Option<CMOccupiedEntry<i32, (File, u64, PathBuf, Option<u64>), RandomState>> {
		self.open_file_handles
			.get_async(&fd)
			.await
			.and_then(|entry| {
				if Self::allow_file_access(&entry, for_stream) {
					Some(entry)
				} else {
					None
				}
			})
	}

	/// Get the file length from a file descriptor number.
	///
	/// This file must already be opened (in order to get the file descriptor).
	pub async fn file_length(&self, fd: i32, for_stream: Option<u64>) -> Option<u64> {
		self.open_file_handles
			.get_async(&fd)
			.await
			.and_then(|entry| {
				if Self::allow_file_access(&entry, for_stream) {
					Some(entry.1)
				} else {
					None
				}
			})
	}

	/// Read from a file descriptor that is actively open.
	///
	/// This will read from a currently open file descriptor, in it's current
	/// location. You might want to set your file location for this FD before
	/// if you aren't already in the same location.
	///
	/// ## Errors
	///
	/// If the file descriptor is open, but we could not read from the open file
	/// descriptor.
	pub async fn read_file(
		&self,
		fd: i32,
		mut total_data_to_read: usize,
		for_stream: Option<u64>,
	) -> Result<Option<Bytes>, FSError> {
		let Some(mut real_entry) = self.open_file_handles.get_async(&fd).await else {
			return Ok(None);
		};
		if !Self::allow_file_access(&real_entry, for_stream) {
			return Ok(None);
		}
		let file_reader = &mut real_entry.0;

		let mut file_buff = BytesMut::zeroed(total_data_to_read);
		let mut total_bytes_read = 0_usize;
		while total_data_to_read > 0 {
			let bytes_read = file_reader.read(&mut file_buff[total_bytes_read..]).await?;
			if bytes_read == 0 {
				break;
			}
			total_data_to_read -= bytes_read;
			total_bytes_read += bytes_read;
		}
		if file_buff.len() > total_bytes_read {
			file_buff.truncate(total_bytes_read);
		}

		Ok(Some(file_buff.freeze()))
	}

	/// Write to a file descriptor that is actively open.
	///
	/// This will write from a currently open file descriptor, in it's current
	/// location. You might want to set your file location for this FD before
	/// if you aren't already in the same location.
	///
	/// ## Errors
	///
	/// If the file descriptor is open, but we could not write to the open file
	/// descriptor.
	pub async fn write_file(
		&self,
		fd: i32,
		data_to_write: Bytes,
		for_stream: Option<u64>,
	) -> Result<(), FSError> {
		let Some(mut real_entry) = self.open_file_handles.get_async(&fd).await else {
			return Err(FSError::IO(IOError::other("file not open")));
		};
		if !Self::allow_file_access(&real_entry, for_stream) {
			return Err(FSError::IO(IOError::other("file not open")));
		}
		let file_writer = &mut real_entry.0;
		file_writer.write_all(&data_to_write).await?;

		Ok(())
	}

	/// Seek to the beginning or end of a file.
	///
	/// If `begin` is true then we will seek to the beginning of the file
	/// otherwise we will sync to the end of the file. Precise seeking is _not_
	/// supported at this time.
	///
	/// ## Errors
	///
	/// If we cannot seek to the beginning or end of the file.
	pub async fn seek_file(
		&self,
		fd: i32,
		begin: bool,
		for_stream: Option<u64>,
	) -> Result<(), FSError> {
		let Some(mut real_entry) = self.open_file_handles.get_async(&fd).await else {
			return Ok(());
		};
		if !Self::allow_file_access(&real_entry, for_stream) {
			return Ok(());
		}
		let file_reader = &mut real_entry.0;

		if begin {
			file_reader.seek(SeekFrom::Start(0)).await?;
		} else {
			file_reader.seek(SeekFrom::End(0)).await?;
		}

		Ok(())
	}

	/// Decrement the ref count of handles to a file.
	///
	/// If ref count reaches 0 close the underlying file handle.
	///
	/// ## Errors
	///
	/// If we cannot close our file handle when our ref count reaches 0, or if
	/// the file isn't open at all.
	pub async fn close_file(&self, fd: i32, for_stream: Option<u64>) {
		if let Some(entry) = self.open_file_handles.get_async(&fd).await {
			if !Self::allow_file_access(&entry, for_stream) {
				// Don't allow streams to close other streams files.
				return;
			}
		}

		self.open_file_handles.remove_async(&fd).await;
	}

	/// "Open" a folder, or an iterator over a directory.
	///
	/// There's no real "open file handle", or reversible directory iterator,
	/// so we just create an id from scratch.
	///
	/// ## Errors
	///
	/// If the path doesn't exist, then we can't open the folder.
	pub fn open_folder(&self, path: &PathBuf, for_stream: Option<u64>) -> Result<i32, FSError> {
		let mut dhandle = read_dir(path)?.filter_map(Result::ok).collect::<Vec<_>>();
		dhandle.sort_by_key(DirEntry::path);

		let fake_fd = FOLDER_FD.fetch_add(1, AtomicOrdering::SeqCst);

		self.open_folder_handles
			.insert(fake_fd, (dhandle, 0, false, path.clone(), for_stream))
			.map_err(|_| IOError::other("OS returned duplicate fd?"))?;
		Ok(fake_fd)
	}

	/// Mark a folder as being 'read-only' for this session.
	///
	/// ## Errors
	///
	/// If we could not actually insert the folder into the read only map.
	pub async fn mark_folder_read_only(&self, path: PathBuf) {
		_ = self.folders_marked_read_only.insert_async(path).await;
	}

	/// Mark a folder as being 'read-write' for this session.
	pub async fn ensure_folder_not_read_only(&self, path: &PathBuf) {
		self.folders_marked_read_only.remove_async(path).await;
	}

	/// Check if a folder is marked as being read only.
	pub async fn folder_is_read_only(&self, path: &PathBuf) -> bool {
		self.folders_marked_read_only.contains_async(path).await
	}

	/// Get the next filename/foldername available in a particular folder, and
	/// how many pieces to remove to get just the filename.
	///
	/// This will always return none even if it's already at the end, unlike a
	/// particular iterator.
	///
	/// ## Errors
	///
	/// If we get an IO error from the underlying filesystem.
	pub async fn next_in_folder(
		&self,
		fd: i32,
		for_stream: Option<u64>,
	) -> Result<Option<(PathBuf, usize)>, FSError> {
		let Some(mut entry) = self.open_folder_handles.get_async(&fd).await else {
			return Ok(None);
		};
		if !Self::allow_folder_access(&entry, for_stream) {
			return Ok(None);
		}

		let component_count = entry.3.components().count();
		let mut value: Option<PathBuf> = None;
		if !entry.2 {
			loop {
				if entry.1 < entry.0.len() {
					let ref_value = entry.0[entry.1].path();
					entry.1 += 1;

					if (!ref_value.is_file() && !ref_value.is_dir()) || ref_value.is_symlink() {
						continue;
					}

					value = Some(ref_value);
				}

				break;
			}
			if value.is_none() {
				entry.2 = true;
			}
		}

		Ok(value.map(|val| (val, component_count)))
	}

	/// Reverse a particular iterator over a folder by one.
	///
	/// Note: This will recreate the directory iterator, and will temporarily
	/// hold _two_ references to [`ReadDir`] at a time because the underlying
	/// iterator from read directory is not a reversible iterator.
	///
	/// ## Errors
	///
	/// If opening another read dir call does not work.
	pub async fn reverse_folder(&self, fd: i32, for_stream: Option<u64>) -> Result<(), FSError> {
		let Some(mut real_entry) = self.open_folder_handles.get_async(&fd).await else {
			return Ok(());
		};
		if !Self::allow_folder_access(&real_entry, for_stream) {
			return Ok(());
		}
		if real_entry.1 == 0 {
			return Ok(());
		}

		real_entry.1 -= 1;
		real_entry.2 = false;
		Ok(())
	}

	/// Decrement the ref count of handles to a folder.
	///
	/// If ref count reaches 0 close the underlying folder handle.
	///
	/// ## Errors
	///
	/// If we cannot close our folder handle when our ref count reaches 0, or if
	/// the folder isn't open at all.
	pub async fn close_folder(&self, fd: i32, for_stream: Option<u64>) {
		if let Some(real_entry) = self.open_folder_handles.get_async(&fd).await {
			if !Self::allow_folder_access(&real_entry, for_stream) {
				return;
			}
		}

		self.open_folder_handles.remove_async(&fd).await;
	}

	/// Get the path to the current boot1 `.bsf` file.
	///
	/// This function will create the boot1 system file, if it does not yet
	/// exist. As a result it may error, if we can't create, and place the
	/// boot system file.
	///
	/// ## Errors
	///
	/// - If the temp directory does not exist, and we can't create it.
	/// - If the boot system file does not exist, and we can't write it to disk.
	pub async fn boot1_sytstem_path(&self) -> Result<PathBuf, FSError> {
		let mut path = self.temp_path().await?;
		path.push("caferun");
		if !path.exists() {
			create_dir_all(&path).await?;
		}
		path.push("ppc.bsf");

		if !path.exists() {
			fs_write(&path, Bytes::from(BootSystemFile::default())).await?;
		}

		Ok(path)
	}

	/// Get the path to the current `diskid.bin`.
	///
	/// If the current Disk ID does not exist, we will write a blank diskid to
	/// this path.
	///
	/// ## Errors
	///
	/// - If the temporary directory does not exist, and we can't create it.
	/// - If the disk ID path does not exist, and we can't write it to disk.
	pub async fn disk_id_path(&self) -> Result<PathBuf, FSError> {
		let mut path = self.temp_path().await?;
		path.push("caferun");
		if !path.exists() {
			create_dir_all(&path).await?;
		}
		path.push("diskid.bin");

		if !path.exists() {
			fs_write(&path, BytesMut::zeroed(32).freeze()).await?;
		}

		Ok(path)
	}

	#[doc(
		// This is not yet finished and the signature may change....
		hidden,
	)]
	/// Mount a particular title as if it were a disc.
	///
	/// ## Errors
	///
	/// - If we cannot remove any existing disc that may be present.
	/// - If we cannot copy the title to the disc id path.
	pub async fn mount_disk_title(
		&mut self,
		is_slc: bool,
		is_sys: bool,
		title_id: TitleID,
	) -> Result<(), FSError> {
		let source_path = Self::join_many(
			&self.cafe_sdk_path,
			[
				"data".to_owned(),
				if is_slc { "slc" } else { "mlc" }.to_owned(),
				if is_sys { "sys" } else { "usr" }.to_owned(),
				"title".to_owned(),
				format!("{:08x}", title_id.0),
				format!("{:08x}", title_id.1),
			],
		);
		let dest_path = Self::join_many(&self.cafe_sdk_path, ["data", "disc"]);
		if dest_path.exists() {
			remove_dir_all(&dest_path).await.map_err(FSError::IO)?;
		}

		Self::copy_dir(&source_path, &dest_path).await?;
		// Mount was successful!
		{
			let mut guard = self.disc_mounted.lock().await;
			guard.replace((is_slc, is_sys, title_id));
		}
		todo!("figure out how to mount diskid.bin")
	}

	/// Get the path to the current firmware file to boot on the MION.
	///
	/// This is guaranteed to always exist, as it's part of our check for a
	/// corrupt SDK.
	#[must_use]
	pub fn firmware_file_path(&self) -> PathBuf {
		Self::join_many(
			&self.slc_path_for((0x0005_0010, 0x1000_400A)),
			["code", "fw.img"],
		)
	}

	/// Get the path to the disk layout file for the PPC booting process.
	///
	/// This function will create a disk layout file, as well as a Boot System
	/// File, and a disk id file if they do not yet exist.
	///
	/// ## Errors
	///
	/// - If the temp directory does not exist, and we can't create it.
	/// - If the boot system file does not exist, and we can't write it to disk.
	/// - If the diskid file does not exist, and we can't write it to disk.
	/// - If the firmware image file does not exist.
	/// - If the dlf file does not exist, and we can't create it.
	pub async fn ppc_boot_dlf_path(&self) -> Result<PathBuf, CatBridgeError> {
		let mut path = self.temp_path().await?;
		path.push("caferun");
		if !path.exists() {
			create_dir_all(&path).await.map_err(FSError::from)?;
		}
		path.push("ppc_boot.dlf");

		if !path.exists() {
			// This probably isn't the right set of defaults for everyone, but i'm
			// not yet smart enough to figure all this out.
			let mut root_dlf = DiskLayoutFile::new(0x00B8_8200_u128);
			root_dlf.upsert_addressed_path(0_u128, &self.disk_id_path().await?)?;
			root_dlf.upsert_addressed_path(0x80000_u128, &self.boot1_sytstem_path().await?)?;
			root_dlf.upsert_addressed_path(0x90000_u128, &self.firmware_file_path())?;
			fs_write(&path, Bytes::from(root_dlf))
				.await
				.map_err(FSError::from)?;
		}

		Ok(path)
	}

	/// Check if a path is allowed to be writable.
	#[must_use]
	pub fn path_allows_writes(&self, path: &Path) -> bool {
		// TODO(mythra): check FSEmulAttributeRules
		!path.to_string_lossy().contains("%DISC_EMU_DIR")
			&& !path.starts_with(Self::join_many(&self.cafe_sdk_path, ["data", "disc"]))
	}

	/// Given a UTF-8 string path, get a pathbuf reference.
	///
	/// This understands the current following implementations:
	///
	/// - `/%MLC_EMU_DIR`
	/// - `/%SLC_EMU_DIR`
	/// - `/%DISC_EMU_DIR`
	/// - `/%SAVE_EMU_DIR`
	/// - `/%NETWORK`
	///
	/// Most of these are just quick ways of referncing the current set of
	/// directories, within cafe sdk. `%NETWORK` is the special one which
	/// references a currently mounted network share.
	///
	/// ## Errors
	///
	/// If the path requested is not in a mounted path.
	pub fn resolve_path(
		&self,
		potentially_prefixed_path: &str,
	) -> Result<ResolvedLocation, CatBridgeError> {
		// Requests coming may optionally have `/vol/pc` prefixed if they're built
		// wrong.
		//
		// Or if a user is trying to get cat-dev style paths working with this api
		// directly. CLean it up.
		let path = potentially_prefixed_path.trim_start_matches("/vol/pc");
		if path.starts_with("/%NETWORK") {
			todo!("NETWORK shares not yet implemented :( sorry!")
		}

		let non_canonical_path = if path.starts_with("/%MLC_EMU_DIR") {
			self.replace_emu_dir(path, "mlc")
		} else if path.starts_with("/%SLC_EMU_DIR") {
			self.replace_emu_dir(path, "slc")
		} else if path.starts_with("/%DISC_EMU_DIR") {
			self.replace_emu_dir(path, "disc")
		} else if path.starts_with("/%SAVE_EMU_DIR") {
			self.replace_emu_dir(path, "save")
		} else {
			PathBuf::from(path)
		};

		// We can't actually just call `canonicalize`, as that will fail if the
		// file doesn't exist, and we could be requesting to resolve a path we want
		// to turn around and create.
		//
		// So instead we try to canonicalize to the closest possible directory, and
		// check if it is underneath our directory.
		let mut closest_canonical_directory = non_canonical_path.clone();
		let mut changed_at_all = false;
		while !closest_canonical_directory.as_os_str().is_empty() {
			if let Ok(canonicalized) = closest_canonical_directory.canonicalize() {
				closest_canonical_directory = canonicalized;
				break;
			}

			changed_at_all = true;
			closest_canonical_directory.pop();
		}
		// We failed to find any directory, which means we're nowhere close to
		// where we want to be.
		if closest_canonical_directory.as_os_str().is_empty() {
			return Err(PCFSApiError::PathNotMapped(path.to_owned()).into());
		}
		// Check for mapped directories...
		let canonicalized_cafe = self
			.cafe_sdk_path()
			.canonicalize()
			.unwrap_or_else(|_| self.cafe_sdk_path().clone());
		if !closest_canonical_directory.starts_with(canonicalized_cafe) {
			return Err(PCFSApiError::PathNotMapped(path.to_owned()).into());
		}

		Ok(ResolvedLocation::Filesystem(FilesystemLocation::new(
			non_canonical_path,
			closest_canonical_directory,
			!changed_at_all,
		)))
	}

	/// Rename a file, symlink, or directory.
	///
	/// This is implemented so we can rename directories, and files without
	/// having to worry about the logic. Especially given the fact the built in
	/// rename doesn't support directories.
	///
	/// ## Errors
	///
	/// - If we run into any filesystem error renaming a source, or directory.
	pub async fn rename(&self, from: &Path, to: &Path) -> Result<(), FSError> {
		if from.is_dir() {
			Self::rename_dir(from, to).await
		} else {
			rename(from, to).await.map_err(FSError::IO)
		}
	}

	/// Get a file from the SLC.
	///
	/// The SLC always serves "sys" files, and are relative to a title id, almost
	/// always a system title id such as (`00050010`).
	///
	/// *note: the file is not guaranteed to exist! It's just a path!*
	#[must_use]
	pub fn slc_path_for(&self, title_id: TitleID) -> PathBuf {
		Self::join_many(
			&self.cafe_sdk_path,
			[
				"data".to_owned(),
				"slc".to_owned(),
				"sys".to_owned(),
				"title".to_owned(),
				format!("{:08x}", title_id.0),
				format!("{:08x}", title_id.1),
			],
		)
	}

	/// Get the current OS's default directory path.
	///
	/// For Windows this is: `C:\cafe_sdk`.
	/// For Unix/BSD likes this is: `/opt/cafe_sdk`
	#[allow(
    // Not actually unreachable unless on unsupported OS.
    unreachable_code,
  )]
	#[must_use]
	pub fn default_cafe_folder() -> Option<PathBuf> {
		#[cfg(target_os = "windows")]
		{
			return Some(PathBuf::from(r"C:\cafe_sdk"));
		}

		#[cfg(any(
			target_os = "linux",
			target_os = "freebsd",
			target_os = "openbsd",
			target_os = "netbsd",
			target_os = "macos"
		))]
		{
			return Some(PathBuf::from("/opt/cafe_sdk"));
		}

		None
	}

	/// Get the current path to the temporary directory for this Cafe SDK
	/// install.
	///
	/// ## Errors
	///
	/// - If the temporary path does not exist and could not be created.
	async fn temp_path(&self) -> Result<PathBuf, FSError> {
		let temp_path = Self::join_many(
			&self.cafe_sdk_path,
			["temp".to_owned(), username().to_lowercase()],
		);
		if !temp_path.exists() {
			create_dir_all(&temp_path).await?;
		}
		Ok(temp_path)
	}

	/// A small utility function to join many paths into a single path effeciently.
	#[must_use]
	fn join_many<PathTy, IterTy>(base: &Path, parts: IterTy) -> PathBuf
	where
		PathTy: AsRef<Path>,
		IterTy: IntoIterator<Item = PathTy>,
	{
		let mut as_owned = PathBuf::from(base);
		for part in parts {
			as_owned = as_owned.join(part.as_ref());
		}
		as_owned
	}

	/// Replace a particular emu directory string in a path.
	fn replace_emu_dir(&self, path: &str, dir: &str) -> PathBuf {
		let path_minus = path
			.trim_start_matches(&format!("/%{}_EMU_DIR", dir.to_ascii_uppercase()))
			.trim_start_matches('/')
			.trim_start_matches('\\')
			.replace('\\', "/");

		Self::join_many(
			&Self::join_many(self.cafe_sdk_path(), ["data", dir]),
			path_minus.split('/'),
		)
	}

	async fn patch_case_sensitive_title_ids(cafe_sdk_path: &Path) -> Result<(), FSError> {
		// First we need to check if we're even on a temporary filesystem/path.
		if !cafe_sdk_path.exists() {
			return Ok(());
		}
		let capital_path = Self::join_many(cafe_sdk_path, ["InsensitiveCheck.txt"]);
		let _ = File::create(&capital_path).await?;
		let is_insensitive = File::open(Self::join_many(cafe_sdk_path, ["insensitivecheck.txt"]))
			.await
			.is_ok();
		remove_file(capital_path).await?;
		if is_insensitive {
			return Ok(());
		}

		for directory in [
			Self::join_many(cafe_sdk_path, ["data", "slc", "sys", "title"]),
			Self::join_many(cafe_sdk_path, ["data", "slc", "usr", "title"]),
			Self::join_many(cafe_sdk_path, ["data", "mlc", "sys", "title"]),
			Self::join_many(cafe_sdk_path, ["data", "mlc", "usr", "title"]),
		] {
			if !directory.exists() {
				// Don't need to patch directories that don't exist.
				continue;
			}

			// Now we need to scan, and lowercase all title ids. So those are the
			// next two sub dirs as they're split into `title/{upper}/{lower}`.
			let mut iter = async_read_dir(&directory).await?;
			let lossy_cafe_dir = cafe_sdk_path.as_os_str().to_string_lossy().to_string();
			while let Ok(Some(entry)) = iter.next_entry().await {
				let p = entry.path();
				if !p.is_dir() || !p.exists() {
					continue;
				}

				let mut inner_iter = async_read_dir(&p).await?;
				while let Ok(Some(inner_entry)) = inner_iter.next_entry().await {
					let ip = inner_entry.path();
					if !ip.is_dir() || !ip.exists() {
						continue;
					}

					// Doing a lossy conversion is safe here cause we know all title ids are valid ascii + utf-8.
					let new_path = ip
						.as_os_str()
						.to_string_lossy()
						.trim_start_matches(&lossy_cafe_dir)
						.to_ascii_lowercase();
					if ip
						.as_os_str()
						.to_string_lossy()
						.trim_start_matches(&lossy_cafe_dir)
						!= new_path
					{
						let mut final_new_path = cafe_sdk_path.as_os_str().to_owned();
						final_new_path.push(&new_path);
						let new = PathBuf::from(final_new_path);
						rename(ip, new).await?;
					}
				}

				let new_path = p
					.as_os_str()
					.to_string_lossy()
					.trim_start_matches(&lossy_cafe_dir)
					.to_ascii_lowercase();
				if p.as_os_str()
					.to_string_lossy()
					.trim_start_matches(&lossy_cafe_dir)
					!= new_path
				{
					let mut final_new_path = cafe_sdk_path.as_os_str().to_owned();
					final_new_path.push(&new_path);
					rename(p, final_new_path).await?;
				}
			}
		}

		Ok(())
	}

	fn allow_file_access(
		entry: &CMOccupiedEntry<i32, (File, u64, PathBuf, Option<u64>), RandomState>,
		requester: Option<u64>,
	) -> bool {
		let Some(requesting_stream_id) = requester else {
			return true;
		};
		let Some(owned_stream_id) = entry.3 else {
			return true;
		};

		requesting_stream_id == owned_stream_id
	}

	#[allow(
		// TODO(mythra): fix
		clippy::type_complexity
	)]
	fn allow_folder_access(
		entry: &CMOccupiedEntry<
			i32,
			(Vec<DirEntry>, usize, bool, PathBuf, Option<u64>),
			RandomState,
		>,
		requester: Option<u64>,
	) -> bool {
		let Some(requesting_stream_id) = requester else {
			return true;
		};
		let Some(owned_stream_id) = entry.4 else {
			return true;
		};

		requesting_stream_id == owned_stream_id
	}

	/// Enusre an SDK path is ready for serving this means:
	///
	/// - Create some configuration files that SDKs don't come with, but will
	///   help the OS boot up.
	/// - Mount the `DISC` directory if one is not present.
	async fn prepare_for_serving(cafe_sdk_path: &Path) -> Result<(), FSError> {
		if !Self::join_many(cafe_sdk_path, ["data", "slc", "sys", "config", "eco.xml"]).exists() {
			Self::generate_eco_xml(cafe_sdk_path).await?;
		}
		if !Self::join_many(
			cafe_sdk_path,
			["data", "slc", "sys", "proc", "prefs", "wii_acct.xml"],
		)
		.exists()
		{
			Self::generate_wii_acct_xml(cafe_sdk_path).await?;
		}
		if !Self::join_many(
			cafe_sdk_path,
			["data", "slc", "sys", "proc", "prefs", "ccr.xml"],
		)
		.exists()
		{
			Self::generate_ccr_xml(cafe_sdk_path).await?;
		}

		// If we're booting in PCFS mode we'll need TMD's for these core OS titles.
		let os_ndebug_tmd_path = Self::join_many(
			cafe_sdk_path,
			[
				"data",
				"slc",
				"sys",
				"title",
				"00050010",
				"1000400a",
				"code",
				"title.tmd",
			],
		);
		if !os_ndebug_tmd_path.exists() {
			let source_tmd = Self::join_many(
				cafe_sdk_path,
				[
					"data",
					"mlc",
					"sys",
					"update",
					"nand",
					"os_v10_ndebug",
					"title.tmd",
				],
			);
			copy_file(&source_tmd, os_ndebug_tmd_path).await?;
		}
		let os_debug_tmd_path = Self::join_many(
			cafe_sdk_path,
			[
				"data",
				"slc",
				"sys",
				"title",
				"00050010",
				"1000800a",
				"code",
				"title.tmd",
			],
		);
		if !os_debug_tmd_path.exists() {
			let source_tmd = Self::join_many(
				cafe_sdk_path,
				[
					"data",
					"mlc",
					"sys",
					"update",
					"nand",
					"os_v10_debug",
					"title.tmd",
				],
			);
			copy_file(&source_tmd, os_debug_tmd_path).await?;
		}

		// Unmount any leftover discs....
		if Self::join_many(cafe_sdk_path, ["data", "disc"]).exists() {
			remove_dir_all(Self::join_many(cafe_sdk_path, ["data", "disc"]))
				.await
				.map_err(FSError::IO)?;
		}
		// Manually mount in SysConfigTool.....
		//
		// This doesn't actually create a discid.bin, but the files do exist.
		let disc_dir = Self::join_many(cafe_sdk_path, ["data", "disc"]);
		let sctt_dir = Self::join_many(
			cafe_sdk_path,
			["data", "mlc", "sys", "title", "00050010", "1f700500"],
		);
		for subpath in ["code", "content", "meta"] {
			Self::copy_dir(
				&Self::join_many(&sctt_dir, [subpath]),
				&Self::join_many(&disc_dir, [subpath]),
			)
			.await?;
		}

		Ok(())
	}

	async fn copy_dir(source_path: &PathBuf, dest_path: &PathBuf) -> Result<(), FSError> {
		if !dest_path.exists() {
			create_dir_all(dest_path).await?;
		}
		let new_path_as_str_bytes = dest_path.as_os_str().as_encoded_bytes();
		let old_path_bytes = source_path.as_os_str().as_encoded_bytes();

		for result in WalkDir::new(source_path)
			.follow_links(false)
			.follow_root_links(false)
		{
			let rpb = result?.into_path();
			let os_str_for_entry = rpb.as_os_str().as_encoded_bytes();
			let mut new_bytes = Vec::with_capacity(os_str_for_entry.len() + 3);
			new_bytes.extend_from_slice(new_path_as_str_bytes);
			new_bytes.extend_from_slice(&os_str_for_entry[old_path_bytes.len()..]);
			let as_new_path =
				PathBuf::from(unsafe { OsString::from_encoded_bytes_unchecked(new_bytes) });

			if rpb.is_symlink() {
				let mut resolved_path = read_link(&rpb).await?;
				{
					// If this symlink is a symlink to another path within the same
					// directory, then rewrite it as well to start under our new directory.
					let os_str_for_resolved = resolved_path.as_os_str().as_encoded_bytes();
					if os_str_for_resolved.starts_with(old_path_bytes) {
						let mut new_bytes = Vec::with_capacity(os_str_for_resolved.len() + 3);
						new_bytes.extend_from_slice(new_path_as_str_bytes);
						new_bytes.extend_from_slice(&os_str_for_entry[old_path_bytes.len()..]);
						resolved_path = PathBuf::from(unsafe {
							OsString::from_encoded_bytes_unchecked(new_bytes)
						});
					}
				}

				#[cfg(unix)]
				{
					use std::os::unix::fs::symlink;
					symlink(resolved_path, &as_new_path)?;
				}

				#[cfg(target_os = "windows")]
				{
					use std::os::windows::fs::{symlink_dir, symlink_file};

					if resolved_path.is_dir() {
						symlink_dir(resolved_path, &as_new_path)?;
					} else {
						symlink_file(resolved_path, &as_new_path)?;
					}
				}
			} else if rpb.is_file() {
				copy_file(&rpb, &as_new_path).await?;
			} else if rpb.is_dir() {
				create_dir_all(&as_new_path).await?;
			}
		}

		Ok(())
	}

	/// Rename an entire directory.
	///
	/// We have to implement this ourselves, because [`tokio::fs::rename`], and
	/// [`std::fs::rename`] don't support renaming a directory at all on windows,
	/// which is one of the critical OS's that we need to support.
	///
	/// This 'rename' works by actually creating a new directory. Then
	/// moving all the files over with rename. This is slow, but
	/// works.
	async fn rename_dir(source_path: &Path, dest_path: &Path) -> Result<(), FSError> {
		if !dest_path.exists() {
			create_dir_all(dest_path).await?;
		}
		let new_path_as_str_bytes = dest_path.as_os_str().as_encoded_bytes();
		let old_path_bytes = source_path.as_os_str().as_encoded_bytes();

		for result in WalkDir::new(source_path)
			.follow_links(false)
			.follow_root_links(false)
		{
			let rpb = result?.into_path();
			let os_str_for_entry = rpb.as_os_str().as_encoded_bytes();
			let mut new_bytes = Vec::with_capacity(os_str_for_entry.len() + 3);
			new_bytes.extend_from_slice(new_path_as_str_bytes);
			new_bytes.extend_from_slice(&os_str_for_entry[old_path_bytes.len()..]);
			let as_new_path =
				PathBuf::from(unsafe { OsString::from_encoded_bytes_unchecked(new_bytes) });

			if rpb.is_symlink() {
				let mut resolved_path = read_link(&rpb).await?;
				{
					// If this symlink is a symlink to another path within the same
					// directory, then rewrite it as well to start under our new directory.
					let os_str_for_resolved = resolved_path.as_os_str().as_encoded_bytes();
					if os_str_for_resolved.starts_with(old_path_bytes) {
						let mut new_bytes = Vec::with_capacity(os_str_for_resolved.len() + 3);
						new_bytes.extend_from_slice(new_path_as_str_bytes);
						new_bytes.extend_from_slice(&os_str_for_entry[old_path_bytes.len()..]);
						resolved_path = PathBuf::from(unsafe {
							OsString::from_encoded_bytes_unchecked(new_bytes)
						});
					}
				}

				#[cfg(unix)]
				{
					use std::os::unix::fs::symlink;
					symlink(resolved_path, &as_new_path)?;
				}

				#[cfg(target_os = "windows")]
				{
					use std::os::windows::fs::{symlink_dir, symlink_file};

					if resolved_path.is_dir() {
						symlink_dir(resolved_path, &as_new_path)?;
					} else {
						symlink_file(resolved_path, &as_new_path)?;
					}
				}
				// Remove the original link, we renamed this....
				remove_file(&rpb).await?;
			} else if rpb.is_file() {
				rename(&rpb, &as_new_path).await?;
			} else if rpb.is_dir() {
				create_dir_all(&as_new_path).await?;
			}
		}
		// Clean up after ourselves...
		remove_dir_all(source_path).await?;

		Ok(())
	}

	/// Generate an `eco.xml` if one is not present.
	///
	/// This is _required_ in order to provide an actual functional PCFS install,
	/// and not actually normally created on the host filesystem with the
	/// official tools. It just generates it in memory.
	///
	/// ## Errors
	///
	/// If we cannot create the config directory, or write the eco config
	/// file to disk.
	async fn generate_eco_xml(cafe_os_path: &Path) -> Result<(), FSError> {
		let mut eco_path = Self::join_many(cafe_os_path, ["data", "slc", "sys", "config"]);
		if !eco_path.exists() {
			create_dir_all(&eco_path).await.map_err(FSError::IO)?;
		}
		eco_path.push("eco.xml");

		let mut eco_file = File::create(eco_path).await.map_err(FSError::IO)?;
		eco_file
			.write_all(
				br#"<?xml version="1.0" encoding="utf-8"?>
<eco type="complex" access="777">
  <enable type="unsignedInt" length="4">0</enable>
  <max_on_time type="unsignedInt" length="4">3601</max_on_time>
  <default_off_time type="unsignedInt" length="4">15</default_off_time>
  <wd_disable type="unsignedInt" length="4">1</wd_disable>
</eco>"#,
			)
			.await
			.map_err(FSError::IO)?;

		#[cfg(unix)]
		{
			use std::{fs::Permissions, os::unix::prelude::*};
			eco_file
				.set_permissions(Permissions::from_mode(0o770))
				.await?;
		}

		Ok(())
	}

	/// Generate a `wii_acct.xml` if one is not present.
	///
	/// This is _required_ in order to provide an actual functional PCFS install,
	/// and not actually normally created on the host filesystem with the
	/// official tools. It just generates it in memory.
	///
	/// ## Errors
	///
	/// If we cannot create the config directory, or write the wii acct config
	/// file to disk.
	async fn generate_wii_acct_xml(cafe_os_path: &Path) -> Result<(), FSError> {
		let mut wii_path = Self::join_many(cafe_os_path, ["data", "slc", "sys", "proc", "prefs"]);
		if !wii_path.exists() {
			create_dir_all(&wii_path).await.map_err(FSError::IO)?;
		}
		wii_path.push("wii_acct.xml");

		let mut wii_file = File::create(wii_path).await.map_err(FSError::IO)?;
		wii_file
			.write_all(
				br#"<?xml version="1.0" encoding="utf-8"?> 
<wii_acct type="complex"> 
  <profile type="complex"> 
    <nickname type="hexBinary" length="22">00570069006900000000000000000000000000000000</nickname>

    <language type="unsignedInt" length="4">0</language> 
    <country type="unsignedInt" length="4">1</country> 
  </profile> 
  <pc type="complex"> 
    <rating type="unsignedInt" length="4">18</rating> 
    <organization type="unsignedInt" length="4">0</organization> 
    <rst_internet_ch type="unsignedByte" length="1">0</rst_internet_ch> 
    <rst_nw_access type="unsignedByte" length="1">0</rst_nw_access> 
    <rst_pt_order type="unsignedByte" length="1">0</rst_pt_order> 
  </pc> 
</wii_acct>"#,
			)
			.await
			.map_err(FSError::IO)?;

		#[cfg(unix)]
		{
			use std::{fs::Permissions, os::unix::prelude::*};
			wii_file
				.set_permissions(Permissions::from_mode(0o770))
				.await?;
		}

		Ok(())
	}

	/// Generate a `ccr.xml` if one is not present.
	///
	/// This isn't actually required by anything but removes an error message
	/// from being printed which may be misinterpreted, and helps provide cleaner
	/// errors in the case of weird boot fialures.
	///
	/// ## Errors
	///
	/// If we cannot create the config directory, or write the ccr config
	/// file to disk.
	async fn generate_ccr_xml(cafe_os_path: &Path) -> Result<(), FSError> {
		let mut ccr_path = Self::join_many(cafe_os_path, ["data", "slc", "sys", "proc", "prefs"]);
		if !ccr_path.exists() {
			create_dir_all(&ccr_path).await.map_err(FSError::IO)?;
		}
		ccr_path.push("ccr.xml");

		#[cfg(unix)]
		let ccr_file = File::create(ccr_path).await.map_err(FSError::IO)?;
		#[cfg(not(unix))]
		let _ccr_file = File::create(ccr_path).await.map_err(FSError::IO)?;

		// We don't actually need to write anything, as it'll just be removed...

		#[cfg(unix)]
		{
			use std::{fs::Permissions, os::unix::prelude::*};
			ccr_file
				.set_permissions(Permissions::from_mode(0o770))
				.await?;
		}

		Ok(())
	}

	fn get_default_read_only_folders(cafe_dir: &Path) -> ConcurrentSet<PathBuf> {
		let set = ConcurrentSet::new();

		for cafe_sub_paths in [
			&["data", "slc", "sys", "config"] as &[&str],
			&["data", "slc", "sys", "proc"],
			&["data", "slc", "sys", "logs"],
			&["data", "mlc", "usr"],
			&["data", "mlc", "usr", "import"],
			&["data", "mlc", "usr", "title"],
		] {
			_ = set.insert(Self::join_many(cafe_dir, cafe_sub_paths));
		}

		set
	}
}

const HOST_FILESYSTEM_FIELDS: &[NamedField<'static>] = &[
	NamedField::new("cafe_sdk_path"),
	NamedField::new("open_file_handles"),
	NamedField::new("open_folder_handles"),
];

impl Structable for HostFilesystem {
	fn definition(&self) -> StructDef<'_> {
		StructDef::new_static("HostFilesystem", Fields::Named(HOST_FILESYSTEM_FIELDS))
	}
}

impl Valuable for HostFilesystem {
	fn as_value(&self) -> Value<'_> {
		Value::Structable(self)
	}

	fn visit(&self, visitor: &mut dyn Visit) {
		let mut values = HashMap::with_capacity(self.open_file_handles.len());
		self.open_file_handles.scan(|k, v| {
			values.insert(*k, format!("{}", v.2.display()));
		});
		let mut folder_values = HashMap::with_capacity(self.open_folder_handles.len());
		self.open_folder_handles.scan(|k, v| {
			folder_values.insert(*k, format!("{}", v.3.display()));
		});

		visitor.visit_named_fields(&NamedValues::new(
			HOST_FILESYSTEM_FIELDS,
			&[
				Valuable::as_value(&self.cafe_sdk_path),
				Valuable::as_value(&values),
				Valuable::as_value(&folder_values),
			],
		));
	}
}

/// A resolved location given an arbitrary path.
#[derive(Clone, Debug, PartialEq, Eq, Valuable)]
pub enum ResolvedLocation {
	/// A location on a particular filesystem.
	///
	/// This location _may not exist_. There is a boolean in this struct that
	/// tells you the final resolved location, the closest resolved location for
	/// permission checks, and if the path exists and is the same between
	/// resolution, and what actually exists.
	Filesystem(FilesystemLocation),
	/// A network location to fetch.
	///
	/// TODO(mythra): figure out type.
	Network(()),
}

/// A location that's been resolved, and is guaranteed to be in one of our
/// mounted paths.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FilesystemLocation {
	/// The final resolved path (may not exist).
	resolved_path: PathBuf,
	/// The resolved path that may not be the same as the final path, but is
	/// enough to confirm we're in the same directory.
	closest_resolved_path: PathBuf,
	/// If the canonicalized path is the same as the resolved path.
	canonicalized_is_exact: bool,
}
impl FilesystemLocation {
	#[must_use]
	pub const fn new(
		resolved_path: PathBuf,
		closest_resolved_path: PathBuf,
		canonicalized_is_exact: bool,
	) -> Self {
		Self {
			resolved_path,
			closest_resolved_path,
			canonicalized_is_exact,
		}
	}

	#[must_use]
	pub const fn resolved_path(&self) -> &PathBuf {
		&self.resolved_path
	}
	#[must_use]
	pub const fn closest_resolved_path(&self) -> &PathBuf {
		&self.closest_resolved_path
	}
	#[must_use]
	pub const fn canonicalized_is_exact(&self) -> bool {
		self.canonicalized_is_exact
	}
}

const FILESYSTEM_LOCATION_FIELDS: &[NamedField<'static>] = &[
	NamedField::new("resolved_path"),
	NamedField::new("closest_resolved_path"),
	NamedField::new("canonicalized_is_exact"),
];

impl Structable for FilesystemLocation {
	fn definition(&self) -> StructDef<'_> {
		StructDef::new_static(
			"FilesystemLocation",
			Fields::Named(FILESYSTEM_LOCATION_FIELDS),
		)
	}
}

impl Valuable for FilesystemLocation {
	fn as_value(&self) -> Value<'_> {
		Value::Structable(self)
	}

	fn visit(&self, visitor: &mut dyn Visit) {
		visitor.visit_named_fields(&NamedValues::new(
			FILESYSTEM_LOCATION_FIELDS,
			&[
				Valuable::as_value(&self.resolved_path),
				Valuable::as_value(&self.closest_resolved_path),
				Valuable::as_value(&self.canonicalized_is_exact),
			],
		));
	}
}

#[cfg_attr(docsrs, doc(cfg(test)))]
#[cfg(test)]
pub mod test_helpers {
	use super::*;
	use std::fs::{File, create_dir_all};
	use tempfile::{TempDir, tempdir};

	/// Test helper that creates a simple host filesystem.
	#[allow(
		// Allow anyone to write a test for this internally on any feature set.
		dead_code,
	)]
	pub async fn create_temporary_host_filesystem() -> (TempDir, HostFilesystem) {
		let dir = tempdir().expect("Failed to create temporary directory!");

		for directory_to_create in vec![
			// Create data directories
			vec!["data", "slc"],
			vec!["data", "mlc"],
			vec!["data", "disc"],
			vec!["data", "save"],
			// Create necessary to pass checks.
			vec![
				"data", "mlc", "sys", "title", "00050030", "1001000a", "code",
			],
			vec![
				"data", "mlc", "sys", "title", "00050010", "1f700500", "code",
			],
			vec![
				"data", "mlc", "sys", "title", "00050010", "1f700500", "content",
			],
			vec![
				"data", "mlc", "sys", "title", "00050010", "1f700500", "meta",
			],
			// Purposefully create capital so we can validate renaming works!
			vec![
				"data", "mlc", "sys", "title", "00050030", "1001010A", "code",
			],
			vec![
				"data", "mlc", "sys", "title", "00050030", "1001020a", "code",
			],
			vec![
				"data", "slc", "sys", "title", "00050010", "1000400a", "code",
			],
			vec![
				"data", "mlc", "sys", "update", "nand", "os_v10_ndebug",
			],
			vec![
				"data", "mlc", "sys", "update", "nand", "os_v10_debug",
			],
			vec![
				"data", "slc", "sys", "proc", "prefs",
			],
			vec![
				"data", "slc", "sys", "title", "00050010", "1000800a", "code",
			],
			vec![
				"data", "slc", "sys", "title", "00050010", "1000400a", "code",
			],
		] {
			create_dir_all(HostFilesystem::join_many(dir.path(), directory_to_create))
				.expect("Failed to create directories necessary for host filesystem to work.");
		}

		File::create(HostFilesystem::join_many(dir.path(), [
			"data",
			"mlc",
			"sys",
			"update",
			"nand",
			"os_v10_ndebug",
			"title.tmd",
		])).expect("Failed to create needed fake title.tmd");
		File::create(HostFilesystem::join_many(dir.path(), [
			"data",
			"mlc",
			"sys",
			"update",
			"nand",
			"os_v10_debug",
			"title.tmd",
		])).expect("Failed to create needed fake title.tmd");

		// Place files that need to exist, they are not real, but enough to "fool"
		// our basic check.
		File::create(HostFilesystem::join_many(
			dir.path(),
			[
				"data", "mlc", "sys", "title", "00050030", "1001000a", "code", "app.xml",
			],
		))
		.expect("Failed to create needed app.xml!");
		File::create(HostFilesystem::join_many(
			dir.path(),
			[
				"data", "mlc", "sys", "title", "00050030", "1001010A", "code", "app.xml",
			],
		))
		.expect("Failed to create needed app.xml!");
		File::create(HostFilesystem::join_many(
			dir.path(),
			[
				"data", "mlc", "sys", "title", "00050030", "1001020a", "code", "app.xml",
			],
		))
		.expect("Failed to create needed app.xml!");

		File::create(HostFilesystem::join_many(
			dir.path(),
			[
				"data", "slc", "sys", "title", "00050010", "1000400a", "code", "fw.img",
			],
		))
		.expect("Failed to create needed fw.img!");

		let fs = HostFilesystem::from_cafe_dir(Some(PathBuf::from(dir.path())))
			.await
			.expect("Failed to load empty host filesystem!");

		(dir, fs)
	}

	/// Re-export host file system join many for tests.
	#[allow(
		// Allow anyone to write a test for this internally on any feature set.
		dead_code,
	)]
	#[must_use]
	pub fn join_many<PathTy, IterTy>(base: &Path, parts: IterTy) -> PathBuf
	where
		PathTy: AsRef<Path>,
		IterTy: IntoIterator<Item = PathTy>,
	{
		HostFilesystem::join_many(base, parts)
	}
}

#[cfg(test)]
mod unit_tests {
	use super::test_helpers::*;
	use super::*;
	use std::fs::read;

	fn only_accepts_send_sync<T: Send + Sync>(_opt: Option<T>) {}

	#[test]
	pub fn is_send_sync() {
		only_accepts_send_sync::<HostFilesystem>(None);
	}

	#[test]
	pub fn can_find_default_cafe_directory() {
		assert!(
			HostFilesystem::default_cafe_folder().is_some(),
			"Failed to find default cafe directory for your OS",
		);
	}

	#[tokio::test]
	pub async fn creatable_files() {
		// Validate that our functions that create files can actually, well, create
		// those files.
		let (tempdir, fs) = create_temporary_host_filesystem().await;

		let expected_bsf_path = HostFilesystem::join_many(
			tempdir.path(),
			[
				"temp".to_owned(),
				username(),
				"caferun".to_owned(),
				"ppc.bsf".to_owned(),
			],
		);
		assert!(
			!expected_bsf_path.exists(),
			"ppc.bsf existed before we asked for it?"
		);
		let bsf_path = fs
			.boot1_sytstem_path()
			.await
			.expect("Failed to create bsf!");
		assert_eq!(expected_bsf_path, bsf_path);
		assert!(
			BootSystemFile::try_from(Bytes::from(
				read(bsf_path).expect("Failed to read written boot system file!")
			))
			.is_ok(),
			"Failed to read generated boot system file!"
		);

		let expected_diskid_path = HostFilesystem::join_many(
			tempdir.path(),
			[
				"temp".to_owned(),
				username(),
				"caferun".to_owned(),
				"diskid.bin".to_owned(),
			],
		);
		assert!(
			!expected_diskid_path.exists(),
			"diskid.bin existed before we asked for it?"
		);
		let diskid_path = fs
			.disk_id_path()
			.await
			.expect("Failed to create diskid.bin!");
		assert_eq!(expected_diskid_path, diskid_path);
		assert_eq!(
			read(diskid_path).expect("Failed to read written diskid.bin!"),
			vec![0; 32],
			"Failed to read generated diskid.bin!"
		);

		// Can't generate firmware files for now.
		assert_eq!(
			fs.firmware_file_path(),
			HostFilesystem::join_many(
				tempdir.path(),
				[
					"data", "slc", "sys", "title", "00050010", "1000400a", "code", "fw.img"
				],
			),
		);

		let expected_ppc_boot_dlf_path = HostFilesystem::join_many(
			tempdir.path(),
			[
				"temp".to_owned(),
				username(),
				"caferun".to_owned(),
				"ppc_boot.dlf".to_owned(),
			],
		);
		assert!(
			!expected_ppc_boot_dlf_path.exists(),
			"ppc_boot.dlf existed before we asked for it?"
		);
		let ppc_boot_dlf_path = fs
			.ppc_boot_dlf_path()
			.await
			.expect("Failed to create ppc_boot.dlf!");
		assert_eq!(expected_ppc_boot_dlf_path, ppc_boot_dlf_path);
		assert!(
			DiskLayoutFile::try_from(Bytes::from(
				read(ppc_boot_dlf_path).expect("Failed to read written ppc_boot.dlf!")
			))
			.is_ok(),
			"Failed to read generated ppc_boot.dlf!"
		);
	}

	#[tokio::test]
	pub async fn path_allows_writes() {
		let (_tempdir, fs) = create_temporary_host_filesystem().await;

		// DIRECTORIES BESIDES DISC should allow writes.
		// unless excluded by fsemul attrs.
		assert!(fs.path_allows_writes(&PathBuf::from("/vol/pc/%MLC_EMU_DIR/")));
		assert!(fs.path_allows_writes(&PathBuf::from("/vol/pc/%SLC_EMU_DIR/")));
		assert!(fs.path_allows_writes(&PathBuf::from("/vol/pc/%SAVE_EMU_DIR/")));
		assert!(!fs.path_allows_writes(&PathBuf::from("/vol/pc/%DISC_EMU_DIR/")));
		assert!(!fs.path_allows_writes(&PathBuf::from("/vol/pc/%DISC_EMU_DIR/")));
	}

	#[tokio::test]
	pub async fn resolve_path() {
		// Validate that our functions that create files can actually, well, create
		// those files.
		let (tempdir, fs) = create_temporary_host_filesystem().await;

		// Validate each of the regular directories work.
		for (dir, name) in [
			("/%MLC_EMU_DIR", "mlc"),
			("/%SLC_EMU_DIR", "slc"),
			("/%DISC_EMU_DIR", "disc"),
			("/%SAVE_EMU_DIR", "save"),
		] {
			assert!(
				fs.resolve_path(&format!("{dir}")).is_ok(),
				"Failed to resolve: `{}`: {:?}",
				dir,
				fs.resolve_path(&format!("{dir}"))
			);
			assert!(
				fs.resolve_path(&format!("{dir}/")).is_ok(),
				"Failed to resolve: `{}/`",
				dir,
			);
			assert!(
				fs.resolve_path(&format!("{dir}/./")).is_ok(),
				"Failed to resolve: `{}/./`",
				dir,
			);
			assert!(
				fs.resolve_path(&format!("{dir}/../{name}")).is_ok(),
				"Failed to resolve: `{}/../{}`",
				dir,
				name,
			);
		}

		// Validate that paths outside of our root directory don't work.
		let mut out_of_path = PathBuf::from(tempdir.path());
		// We now left tempdir, and this path isn't mounted, so we should error out
		// on this.
		out_of_path.pop();

		// We shouldn't be able to resolve paths outside of our directory.
		assert!(
			fs.resolve_path(
				&out_of_path
					.clone()
					.into_os_string()
					.into_string()
					.expect("Failed to convert pathbuf to string!")
			)
			.is_err()
		);
		assert!(fs.resolve_path("/%MLC_EMU_DIR/../../../").is_err());

		#[cfg(unix)]
		{
			use std::os::unix::fs::symlink;

			let mut tempdir_symlink = PathBuf::from(tempdir.path());
			tempdir_symlink.push("symlink");
			symlink(out_of_path, tempdir_symlink.clone()).expect("Failed to do symlink!");
			assert!(
				fs.resolve_path(&format!(
					"{}/symlink",
					tempdir_symlink
						.into_os_string()
						.into_string()
						.expect("tempdir symlink wasn't utf8?"),
				))
				.is_err()
			);
		}

		#[cfg(target_os = "windows")]
		{
			use std::os::windows::fs::symlink_dir;

			let mut tempdir_symlink = PathBuf::from(tempdir.path());
			tempdir_symlink.push("symlink");
			symlink_dir(out_of_path, tempdir_symlink.clone()).expect("Failed to do symlink!");
			assert!(
				fs.resolve_path(&format!(
					"{}/symlink",
					tempdir_symlink
						.into_os_string()
						.into_string()
						.expect("tempdir symlink wasn't utf8?"),
				))
				.is_err()
			);
		}
	}

	#[tokio::test]
	pub async fn opening_files() {
		let (tempdir, fs) = create_temporary_host_filesystem().await;
		let path = HostFilesystem::join_many(tempdir.path(), ["file.txt"]);
		tokio::fs::write(path.clone(), vec![0; 1307])
			.await
			.expect("Failed to write test file!");
		let create_path = HostFilesystem::join_many(tempdir.path(), ["new-file.txt"]);

		let mut oo = OpenOptions::new();
		oo.create(false).write(true).read(true);
		assert!(
			fs.open_file(oo, &create_path, None).await.is_err(),
			"Somehow succeeding opening a file that doesn't exist with no create flag?",
		);
		oo = OpenOptions::new();
		oo.create(true).write(true).truncate(true);
		let fd = fs
			.open_file(oo, &create_path, None)
			.await
			.expect("Failed opening a file that doesn't exist with a create flag?");
		assert!(
			fs.open_file_handles.len() == 1 && fs.open_file_handles.get(&fd).is_some(),
			"Open file wasn't in open files list!",
		);
		fs.close_file(fd, None).await;
		assert!(
			fs.open_file_handles.is_empty(),
			"Somehow after opening/closing, open file handles was not empty?",
		);
	}

	#[tokio::test]
	pub async fn seek_and_read() {
		let (tempdir, fs) = create_temporary_host_filesystem().await;
		let path = HostFilesystem::join_many(tempdir.path(), ["file.txt"]);
		tokio::fs::write(path.clone(), vec![0; 1307])
			.await
			.expect("Failed to write test file!");

		let mut oo = OpenOptions::new();
		oo.read(true).create(false).write(false);
		let fd = fs
			.open_file(oo, &path, None)
			.await
			.expect("Failed to open existing file!");

		// Should be possible to read all bytes.
		assert_eq!(
			Some(BytesMut::zeroed(1307).freeze()),
			fs.read_file(fd, 1307, None)
				.await
				.expect("Failed to read from FD!"),
		);
		fs.seek_file(fd, true, None)
			.await
			.expect("Failed to sync to beginning of file!");
		// Can read all bytes again!
		assert_eq!(
			Some(BytesMut::zeroed(1307).freeze()),
			fs.read_file(fd, 1307, None)
				.await
				.expect("Failed to read from FD!"),
		);
		fs.close_file(fd, None).await;
		assert!(
			fs.open_file_handles.is_empty(),
			"Somehow after opening/closing, open file handles was not empty?",
		);
	}

	#[tokio::test]
	pub async fn open_and_close_folder() {
		let (tempdir, fs) = create_temporary_host_filesystem().await;
		let path = HostFilesystem::join_many(tempdir.path(), ["a", "b"]);
		tokio::fs::create_dir_all(path.clone())
			.await
			.expect("Failed to create test directory!");

		let fd = fs
			.open_folder(&path, None)
			.expect("Failed to open existing folder!");
		assert!(
			fs.open_folder_handles.len() == 1,
			"Expected one open folder handle",
		);
		fs.close_folder(fd, None).await;

		assert!(
			fs.open_folder_handles.is_empty(),
			"Somehow after opening/closing, open folder handles was not empty?",
		);
	}

	#[tokio::test]
	pub async fn seek_within_folder() {
		let (tempdir, fs) = create_temporary_host_filesystem().await;
		let path = HostFilesystem::join_many(tempdir.path(), ["a", "b"]);
		tokio::fs::create_dir_all(path.clone())
			.await
			.expect("Failed to create test directory!");

		// Only `c`, `d`, and `f` should be returned.
		//
		// `e` is a symlink   (ignored)
		// `d/a` is an item in a subdirectory (ignored)
		_ = tokio::fs::File::create(HostFilesystem::join_many(&path, ["c"]))
			.await
			.expect("Failed to create file to use!");
		tokio::fs::create_dir(HostFilesystem::join_many(&path, ["d"]))
			.await
			.expect("Failed to create directory to use!");
		#[cfg(unix)]
		{
			use std::os::unix::fs::symlink;

			let mut tempdir_symlink = path.clone();
			tempdir_symlink.push("e");
			symlink(tempdir.path(), tempdir_symlink).expect("Failed to do symlink!");
		}
		#[cfg(target_os = "windows")]
		{
			use std::os::windows::fs::symlink_dir;

			let mut tempdir_symlink = path.clone();
			tempdir_symlink.push("e");
			symlink_dir(tempdir.path(), tempdir_symlink).expect("Failed to do symlink!");
		}
		_ = tokio::fs::File::create(HostFilesystem::join_many(&path, ["f"]))
			.await
			.expect("Failed to create file to use!");
		_ = tokio::fs::File::create(HostFilesystem::join_many(&path, ["d", "a"]))
			.await
			.expect("Failed to create file to use!");

		let dfd = fs.open_folder(&path, None).expect("Failed to open folder!");
		assert!(
			fs.next_in_folder(dfd, None)
				.await
				.expect("Failed to query for next in folder! 1.1!")
				.is_some()
		);
		assert!(
			fs.next_in_folder(dfd, None)
				.await
				.expect("Failed to query for next in folder! 1.2!")
				.is_some()
		);
		assert!(
			fs.next_in_folder(dfd, None)
				.await
				.expect("Failed to query for next in folder! 1.3!")
				.is_some()
		);
		// We should have hit the end...
		assert!(
			fs.next_in_folder(dfd, None)
				.await
				.expect("Failed to query for next in folder! 1.4!")
				.is_none()
		);
		// We can call as many times as we want.
		assert!(
			fs.next_in_folder(dfd, None)
				.await
				.expect("Failed to query for next in folder! 1.5!")
				.is_none()
		);
		// Rewind to get to reads again!
		fs.reverse_folder(dfd, None)
			.await
			.expect("Failed to reverse directory search!");
		assert!(
			fs.next_in_folder(dfd, None)
				.await
				.expect("Failed to query for next in folder! 2.1!")
				.is_some()
		);
		assert!(
			fs.next_in_folder(dfd, None)
				.await
				.expect("Failed to query for next in folder! 2.2!")
				.is_none()
		);
	}
}
