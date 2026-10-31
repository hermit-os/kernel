use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicU64, Ordering};

use hadris_fat::r#async::read::FileReader;
use hadris_fat::r#async::write::FileWriter;
use hadris_fat::r#async::{
	Error as IoError, FatDir, FatVolume, FatVolumeReadExt, FatVolumeWriteExt, FileEntry, Read,
	Seek, SeekFrom, Write,
};
use hadris_fat::time::{FatDateTime, TimeProvider};
use hermit_sync::OnceCell;
use time::OffsetDateTime;

#[cfg(not(feature = "pci"))]
use crate::arch::kernel::mmio::get_block_driver;
use crate::drivers::blk::{SECTOR_SIZE, driver, with_driver};
#[cfg(feature = "pci")]
use crate::drivers::pci::get_block_driver;
use crate::errno::Errno;
use crate::executor::block_on;
use crate::fd::{AccessPermission, Fd, ObjectInterface, OpenOption};
use crate::fs::{self, DirectoryEntry, FileAttr, FileType, NodeKind, SeekWhence, VfsNode};
use crate::io;
use crate::syscalls::DirentFormat;
use crate::time::SystemTime;

/// Number of sectors `FatStream` keeps in memory.
///
/// Several times [`RUN_SECTORS`], so that a read-ahead does not evict the
/// FAT and directory sectors in use.
const CACHE_SECTORS: usize = 256;

/// Sectors fetched per cache miss and maximum length of a merged write.
const RUN_SECTORS: usize = 64;

/// One cached device sector.
struct CachedSector {
	/// Sector index on the device.
	sector: usize,
	/// Sector content.
	data: [u8; SECTOR_SIZE],
	/// Modified, but not yet written back.
	dirty: bool,
	/// Last access time for LRU eviction.
	used: u64,
}

/// Sector cache providing byte-wise `Read`/`Write`/`Seek` on the block
/// device for `hadris-fat`. Writes reach the device on eviction or flush.
struct FatStream {
	cache: Vec<CachedSector>,
	/// Access counter for LRU eviction.
	clock: u64,
	/// Byte position on the device.
	pos: usize,
	/// Staging buffer for read-ahead.
	read_run: Vec<u8>,
	/// Staging buffer for write-back. Separate from `read_run`, because an
	/// eviction may flush during a read-ahead.
	write_run: Vec<u8>,
}

impl FatStream {
	pub fn new() -> Self {
		Self {
			cache: Vec::with_capacity(CACHE_SECTORS),
			clock: 0,
			pos: 0,
			read_run: Vec::new(),
			write_run: Vec::new(),
		}
	}

	/// The number of sectors the device holds.
	fn capacity(&self) -> Result<usize, Errno> {
		with_driver(|drv| drv.capacity()).ok_or(Errno::Nodev)
	}

	/// Returns the slot holding `sector` and loads it on a miss.
	///
	/// With `fill == false` the sector is not read, because the caller
	/// overwrites it completely.
	async fn slot(&mut self, sector: usize, fill: bool) -> Result<usize, Errno> {
		self.clock += 1;

		if let Some(slot) = self.find(sector) {
			self.cache[slot].used = self.clock;
			return Ok(slot);
		}

		if !fill {
			return self.install(sector, &[0u8; SECTOR_SIZE]).await;
		}

		// Read ahead, bounded by the device.
		let sectors = RUN_SECTORS.min(self.capacity()? - sector);

		// Take the buffer out to avoid borrowing `self` twice.
		let mut run = core::mem::take(&mut self.read_run);
		run.resize(RUN_SECTORS * SECTOR_SIZE, 0);

		let result = self
			.fill_run(sector, &mut run[..sectors * SECTOR_SIZE])
			.await;

		self.read_run = run;
		result
	}

	/// Reads a run of sectors into the cache and returns the slot holding
	/// `sector`.
	async fn fill_run(&mut self, sector: usize, run: &mut [u8]) -> Result<usize, Errno> {
		driver().ok_or(Errno::Nodev)?.read(sector, run).await?;

		let mut requested = None;
		for (index, data) in run.as_chunks::<SECTOR_SIZE>().0.iter().enumerate() {
			// Keep cached copies, they may be dirty.
			if self
				.cache
				.iter()
				.any(|entry| entry.sector == sector + index)
			{
				continue;
			}

			let slot = self.install(sector + index, data).await?;
			if index == 0 {
				requested = Some(slot);
			}
		}

		match requested {
			Some(slot) => Ok(slot),
			None => self.find(sector).ok_or(Errno::Io),
		}
	}

	/// Returns the slot holding `sector`, if it is cached.
	fn find(&self, sector: usize) -> Option<usize> {
		self.cache.iter().position(|entry| entry.sector == sector)
	}

	/// Puts `data` into a free or freshly evicted slot and returns it.
	async fn install(&mut self, sector: usize, data: &[u8]) -> Result<usize, Errno> {
		let entry = CachedSector {
			sector,
			data: data.try_into().unwrap(),
			dirty: false,
			used: self.clock,
		};

		if self.cache.len() < CACHE_SECTORS {
			self.cache.push(entry);
			return Ok(self.cache.len() - 1);
		}

		// LRU eviction; sectors of the current run are never chosen.
		let victim = self
			.cache
			.iter()
			.enumerate()
			.min_by_key(|(_, entry)| entry.used)
			.map(|(slot, _)| slot)
			.unwrap();

		// Flush all dirty sectors at once, so that adjacent ones are merged.
		if self.cache[victim].dirty {
			self.flush_all().await?;
		}

		self.cache[victim] = entry;

		Ok(victim)
	}

	/// Writes all dirty sectors back and merges adjacent ones into one request.
	async fn flush_all(&mut self) -> Result<(), Errno> {
		let mut dirty: Vec<usize> = (0..self.cache.len())
			.filter(|&slot| self.cache[slot].dirty)
			.collect();
		if dirty.is_empty() {
			return Ok(());
		}
		dirty.sort_unstable_by_key(|&slot| self.cache[slot].sector);

		// `Batch::write` copies the data, so one buffer serves every run.
		let mut run = core::mem::take(&mut self.write_run);
		run.resize(RUN_SECTORS * SECTOR_SIZE, 0);

		let mut batch = driver().ok_or(Errno::Nodev)?.batch().await;
		let mut submitted = Ok(());

		let mut rest = dirty.as_slice();
		while let Some((&first, _)) = rest.split_first() {
			// Longest run of consecutive sectors that fits the buffer.
			let len = rest
				.iter()
				.enumerate()
				.take(RUN_SECTORS)
				.take_while(|&(index, &slot)| {
					self.cache[slot].sector == self.cache[first].sector + index
				})
				.count();
			let (group, tail) = rest.split_at(len);
			rest = tail;

			for (index, &slot) in group.iter().enumerate() {
				run[index * SECTOR_SIZE..(index + 1) * SECTOR_SIZE]
					.copy_from_slice(&self.cache[slot].data);
			}

			submitted = batch.write(self.cache[first].sector, &run[..len * SECTOR_SIZE]);
			if submitted.is_err() {
				break;
			}
		}

		self.write_run = run;

		// Keep the sectors dirty until the device has acknowledged them.
		batch.finish().await?;
		submitted?;

		for &slot in &dirty {
			self.cache[slot].dirty = false;
		}

		Ok(())
	}
}

impl From<Errno> for IoError<Errno> {
	fn from(e: Errno) -> Self {
		IoError::from_source(e)
	}
}

impl Read for FatStream {
	type Error = Errno;

	async fn read(&mut self, buf: &mut [u8]) -> Result<usize, IoError<Errno>> {
		if buf.is_empty() {
			return Ok(0);
		}

		let sector = self.pos / SECTOR_SIZE;
		let offset = self.pos % SECTOR_SIZE;

		if sector >= self.capacity()? {
			return Ok(0);
		}

		let slot = self.slot(sector, true).await?;

		// Short reads are allowed, so stay within one sector.
		let n = usize::min(buf.len(), SECTOR_SIZE - offset);
		buf[..n].copy_from_slice(&self.cache[slot].data[offset..offset + n]);
		self.pos += n;

		Ok(n)
	}
}

impl Write for FatStream {
	type Error = Errno;

	async fn write(&mut self, buf: &[u8]) -> Result<usize, IoError<Errno>> {
		if buf.is_empty() {
			return Ok(0);
		}

		let sector = self.pos / SECTOR_SIZE;
		let offset = self.pos % SECTOR_SIZE;

		if sector >= self.capacity()? {
			return Err(IoError::from_source(Errno::Nospc));
		}

		let n = usize::min(buf.len(), SECTOR_SIZE - offset);

		// Only a partial sector has to be read first.
		let slot = self.slot(sector, n != SECTOR_SIZE).await?;

		self.cache[slot].data[offset..offset + n].copy_from_slice(&buf[..n]);
		self.cache[slot].dirty = true;
		self.pos += n;

		Ok(n)
	}

	async fn flush(&mut self) -> Result<(), IoError<Errno>> {
		self.flush_all().await?;
		driver().ok_or(Errno::Nodev)?.flush().await?;

		Ok(())
	}
}

impl Seek for FatStream {
	type Error = Errno;

	async fn seek(&mut self, pos: SeekFrom) -> Result<u64, IoError<Errno>> {
		let end = i64::try_from(self.capacity()? * SECTOR_SIZE).unwrap();
		let cur = i64::try_from(self.pos).unwrap();

		let new = match pos {
			SeekFrom::Start(n) => i64::try_from(n).ok(),
			SeekFrom::Current(offset) => cur.checked_add(offset),
			SeekFrom::End(offset) => end.checked_add(offset),
		};

		// Seeking past the end is allowed, accessing it is not.
		let new = new.filter(|new| *new >= 0).ok_or(Errno::Inval)?;

		self.pos = usize::try_from(new).unwrap();

		Ok(new as u64)
	}
}

fn format_file_size(size: u64) -> String {
	const KB: u64 = 1024;
	const MB: u64 = 1024 * KB;
	const GB: u64 = 1024 * MB;
	if size < KB {
		format!("{size}B")
	} else if size < MB {
		format!("{}KB", size / KB)
	} else if size < GB {
		format!("{}MB", size / MB)
	} else {
		format!("{}GB", size / GB)
	}
}

pub(crate) fn init() {
	debug!("Try to initialize vfat filesystem");

	if get_block_driver().is_none() {
		return;
	};

	let stream = FatStream::new();
	debug!(
		"Found block device with a capacity of {}",
		format_file_size(
			(stream.capacity().unwrap() * SECTOR_SIZE)
				.try_into()
				.unwrap()
		)
	);

	match block_on(
		async {
			FatVolume::builder(stream)
				.time_provider(&TIME_PROVIDER)
				.open()
				.await
				.map_err(map_err)
		},
		None,
	) {
		Ok(volume) => {
			info!("Mounting {} volume at {MOUNT_POINT}", volume.fat_type());

			if VFAT.set(SharedVolume(volume)).is_err() {
				error!("Unable to mount block device at {MOUNT_POINT}");
				return;
			}

			if let Err(err) = fs::FILESYSTEM
				.get()
				.unwrap()
				.mount(MOUNT_POINT, Box::new(VfatDirectory::new(String::new())))
			{
				error!("Unable to mount block device at {MOUNT_POINT}: {err:?}");
			}
		}
		Err(_) => {
			warn!("Unable to create FAT volume");
		}
	}
}

/// Stamps directory entries with the system's wall-clock time.
#[derive(Debug)]
struct HermitTimeProvider;

impl HermitTimeProvider {
	pub const fn new() -> Self {
		Self {}
	}
}

impl TimeProvider for HermitTimeProvider {
	fn now(&self) -> FatDateTime {
		let now = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH);
		let Ok(now) = OffsetDateTime::from_unix_timestamp_nanos(now.as_nanos().cast_signed())
		else {
			return FatDateTime::EPOCH;
		};

		let mut date_time = FatDateTime::new(
			u16::try_from(now.year()).unwrap_or(1980),
			now.month() as u8,
			now.day(),
			now.hour(),
			now.minute(),
			now.second(),
		);

		// `time_tenth` holds the odd second and the 10 ms units.
		date_time.time_tenth =
			u8::try_from((u16::from(now.second() % 2) * 100 + now.millisecond() / 10).min(199))
				.unwrap();

		date_time
	}
}

static TIME_PROVIDER: HermitTimeProvider = HermitTimeProvider::new();

// ---------------------------------------------------------------------------
// VFS integration
// ---------------------------------------------------------------------------

/// `FatVolume` is not `Sync`, because it keeps counters in `Cell`s.
struct SharedVolume(FatVolume<FatStream>);

// SAFETY: the volume is only accessed while `VFAT_LOCK` is held, see `volume`.
unsafe impl Sync for SharedVolume {}

static VFAT: OnceCell<SharedVolume> = OnceCell::new();

/// Serializes file system operations, see [`volume`].
static VFAT_LOCK: async_lock::Mutex<()> = async_lock::Mutex::new(());

/// Incremented on every modification, invalidates cached readers.
static GENERATION: AtomicU64 = AtomicU64::new(0);

fn modified() {
	GENERATION.fetch_add(1, Ordering::Relaxed);
}

/// Where the FAT file system is attached in the VFS.
const MOUNT_POINT: &str = "/root";

/// Translates a `hadris-fat` error into the kernel's error number.
fn map_err(err: hadris_fat::error::Error) -> Errno {
	use hadris_fat::error::Error;

	match err {
		Error::EntryNotFound => Errno::Noent,
		Error::NotADirectory => Errno::Notdir,
		Error::NotAFile => Errno::Isdir,
		Error::NoFreeSpace => Errno::Nospc,
		Error::InvalidPath | Error::InvalidShortFilename => Errno::Inval,
		_ => Errno::Io,
	}
}

/// Locks the volume for one complete file system operation.
///
/// `hadris-fat` locks only single accesses, so a lookup and the following
/// write could otherwise race on another core.
async fn volume() -> io::Result<Volume> {
	let fs = &VFAT.get().ok_or(Errno::Nodev)?.0;
	let _guard = VFAT_LOCK.lock().await;

	Ok(Volume { fs, _guard })
}

/// The mounted volume, locked for one operation.
struct Volume {
	fs: &'static FatVolume<FatStream>,
	_guard: async_lock::MutexGuard<'static, ()>,
}

impl core::ops::Deref for Volume {
	type Target = FatVolume<FatStream>;

	fn deref(&self) -> &Self::Target {
		self.fs
	}
}

/// Joins the directory's own path with a path relative to it.
fn join(prefix: &str, path: &str) -> String {
	match (prefix.trim_matches('/'), path.trim_matches('/')) {
		("", rest) => rest.to_string(),
		(base, "") => base.to_string(),
		(base, rest) => [base, rest].join("/"),
	}
}

/// Finds the entry called `name` directly below `dir`.
async fn find(dir: &FatDir<'_, FatStream>, name: &str) -> io::Result<Option<FileEntry>> {
	let mut entries = dir.entries();
	while let Some(entry) = entries.next_entry().await {
		let entry = entry.map_err(map_err)?;
		if entry.name() == name
			&& let Some(entry) = entry.as_entry()
		{
			return Ok(Some(entry.clone()));
		}
	}

	Ok(None)
}

/// Walks `path` and returns the directory holding its last component together
/// with that component's entry, if it exists.
async fn resolve<'a>(
	volume: &'a FatVolume<FatStream>,
	path: &str,
) -> io::Result<(FatDir<'a, FatStream>, Option<FileEntry>)> {
	let mut dir = volume.root_dir();

	let mut components = path.split('/').filter(|part| !part.is_empty()).peekable();

	while let Some(component) = components.next() {
		let entry = find(&dir, component).await?;

		if components.peek().is_none() {
			return Ok((dir, entry));
		}

		let entry = entry.ok_or(Errno::Noent)?;
		dir = dir.open_entry(&entry).map_err(map_err)?;
	}

	Ok((dir, None))
}

/// Resolves `path` to a directory.
async fn resolve_dir<'a>(
	volume: &'a FatVolume<FatStream>,
	path: &str,
) -> io::Result<FatDir<'a, FatStream>> {
	let (parent, entry) = resolve(volume, path).await?;

	match entry {
		Some(entry) => parent.open_entry(&entry).map_err(map_err),
		None if is_root(path) => Ok(parent),
		None => Err(Errno::Noent),
	}
}

/// Whether `path` names the root of the volume.
fn is_root(path: &str) -> bool {
	path.split('/').all(str::is_empty)
}

/// Resolves `path` to an existing file entry.
async fn resolve_file(volume: &FatVolume<FatStream>, path: &str) -> io::Result<FileEntry> {
	let (_, entry) = resolve(volume, path).await?;
	let entry = entry.ok_or(Errno::Noent)?;

	if entry.is_directory() {
		return Err(Errno::Isdir);
	}

	Ok(entry)
}

/// Builds a `FileAttr`; FAT has no ownership or link count.
fn file_attr(size: u64, kind: NodeKind) -> FileAttr {
	let mode = match kind {
		NodeKind::Directory => {
			AccessPermission::from_bits(0o755).unwrap() | AccessPermission::S_IFDIR
		}
		NodeKind::File => AccessPermission::from_bits(0o644).unwrap() | AccessPermission::S_IFREG,
	};

	FileAttr {
		st_size: i64::try_from(size).unwrap_or(i64::MAX),
		st_mode: mode,
		st_blksize: i64::try_from(SECTOR_SIZE).unwrap(),
		st_blocks: i64::try_from(size.div_ceil(SECTOR_SIZE as u64)).unwrap_or(i64::MAX),
		st_nlink: 1,
		..Default::default()
	}
}

/// Lists the entries of the directory at `path`, relative to the FAT root.
async fn readdir_at(
	volume: &FatVolume<FatStream>,
	path: String,
) -> io::Result<Vec<DirectoryEntry>> {
	let dir = resolve_dir(volume, &path).await?;

	let mut result = Vec::new();
	let mut entries = dir.entries();
	while let Some(entry) = entries.next_entry().await {
		let entry = entry.map_err(map_err)?;
		result.push(DirectoryEntry::new(entry.name().into_owned()));
	}

	Ok(result)
}

/// A directory of the FAT file system, as seen by the VFS.
#[derive(Debug)]
pub(crate) struct VfatDirectory {
	/// Path of this directory relative to the FAT root, without leading slash.
	prefix: String,
}

impl VfatDirectory {
	pub fn new(prefix: String) -> Self {
		Self { prefix }
	}
}

impl VfsNode for VfatDirectory {
	fn get_kind(&self) -> NodeKind {
		NodeKind::Directory
	}

	fn get_file_attributes(&self) -> io::Result<FileAttr> {
		Ok(file_attr(0, NodeKind::Directory))
	}

	fn get_object(&self) -> io::Result<Arc<async_lock::RwLock<Fd>>> {
		Ok(Arc::new(async_lock::RwLock::new(
			VfatDirectoryHandle::new(self.prefix.clone()).into(),
		)))
	}

	fn traverse_readdir(&self, path: &str) -> io::Result<Vec<DirectoryEntry>> {
		let path = join(&self.prefix, path);

		block_on(
			async {
				let volume = volume().await?;

				readdir_at(&volume, path).await
			},
			None,
		)
	}

	fn traverse_stat(&self, path: &str) -> io::Result<FileAttr> {
		let path = join(&self.prefix, path);

		block_on(
			async {
				let volume = volume().await?;

				let (_, entry) = resolve(&volume, &path).await?;
				let Some(entry) = entry else {
					return if is_root(&path) {
						Ok(file_attr(0, NodeKind::Directory))
					} else {
						Err(Errno::Noent)
					};
				};

				if entry.is_directory() {
					Ok(file_attr(0, NodeKind::Directory))
				} else {
					Ok(file_attr(entry.len(), NodeKind::File))
				}
			},
			None,
		)
	}

	fn traverse_lstat(&self, path: &str) -> io::Result<FileAttr> {
		// FAT has no symbolic links.
		self.traverse_stat(path)
	}

	fn traverse_mkdir(&self, path: &str, _mode: AccessPermission) -> io::Result<()> {
		let path = join(&self.prefix, path);
		let name = path.rsplit('/').next().ok_or(Errno::Inval)?;

		block_on(
			async {
				let volume = volume().await?;

				let (parent, entry) = resolve(&volume, &path).await?;
				if entry.is_some() {
					return Err(Errno::Exist);
				}

				volume.create_dir(&parent, name).await.map_err(map_err)?;

				Ok(())
			},
			None,
		)
	}

	fn traverse_rmdir(&self, path: &str) -> io::Result<()> {
		block_on(
			async {
				let volume = volume().await?;

				let entry = resolve(&volume, &join(&self.prefix, path))
					.await?
					.1
					.ok_or(Errno::Noent)?;
				if !entry.is_directory() {
					return Err(Errno::Notdir);
				}

				modified();
				volume.delete(&entry).await.map_err(map_err)
			},
			None,
		)
	}

	fn traverse_unlink(&self, path: &str) -> io::Result<()> {
		block_on(
			async {
				let volume = volume().await?;

				let entry = resolve_file(&volume, &join(&self.prefix, path)).await?;

				modified();
				volume.delete(&entry).await.map_err(map_err)
			},
			None,
		)
	}

	fn traverse_open(
		&self,
		path: &str,
		opt: OpenOption,
		_mode: AccessPermission,
	) -> io::Result<Arc<async_lock::RwLock<Fd>>> {
		let full = join(&self.prefix, path);

		block_on(
			async {
				let volume = volume().await?;

				let (parent, entry) = resolve(&volume, &full).await?;

				// Directories are always opened as directory handle.
				if entry
					.as_ref()
					.map_or(is_root(&full), FileEntry::is_directory)
				{
					return Ok(Arc::new(async_lock::RwLock::new(
						VfatDirectoryHandle::new(full.clone()).into(),
					)));
				}

				let entry = match entry {
					Some(entry) => {
						if opt.contains(OpenOption::O_CREAT | OpenOption::O_EXCL) {
							return Err(Errno::Exist);
						}
						if opt.contains(OpenOption::O_TRUNC) {
							modified();
							volume.truncate(&entry, 0).await.map_err(map_err)?;
						}
						entry
					}
					None => {
						if !opt.contains(OpenOption::O_CREAT) {
							return Err(Errno::Noent);
						}
						let name = full.rsplit('/').next().ok_or(Errno::Inval)?;
						volume.create_file(&parent, name).await.map_err(map_err)?
					}
				};

				let pos = if opt.contains(OpenOption::O_APPEND) {
					entry.len()
				} else {
					0
				};

				Ok(Arc::new(async_lock::RwLock::new(
					VfatFileHandle::new(full.clone(), pos).into(),
				)))
			},
			None,
		)
	}
}

/// An open file of the FAT file system.
///
/// Keeps the reader of the last `read`, so that sequential reads continue
/// at the current cluster instead of resolving the path and walking the
/// FAT chain again.
pub(crate) struct VfatFileHandle {
	path: String,
	state: async_lock::Mutex<FileState>,
}

struct FileState {
	pos: u64,
	/// Reader and the [`GENERATION`] it was created in.
	reader: Option<(u64, FileReader<'static, FatStream>)>,
}

// SAFETY: the reader borrows the volume, but is only used in
// `VfatFileHandle::read` while `VFAT_LOCK` is held, and has no `Drop`.
unsafe impl Send for FileState {}

impl core::fmt::Debug for VfatFileHandle {
	fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
		f.debug_struct("VfatFileHandle")
			.field("path", &self.path)
			.finish_non_exhaustive()
	}
}

impl VfatFileHandle {
	pub fn new(path: String, pos: u64) -> Self {
		Self {
			path,
			state: async_lock::Mutex::new(FileState { pos, reader: None }),
		}
	}
}

impl ObjectInterface for VfatFileHandle {
	async fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
		let volume = volume().await?;
		let mut state = self.state.lock().await;
		let generation = GENERATION.load(Ordering::Relaxed);

		let mut reader = match state.reader.take() {
			Some((created, reader)) if created == generation && reader.position() == state.pos => {
				reader
			}
			_ => {
				let entry = resolve_file(volume.fs, &self.path).await?;
				let mut reader = volume
					.fs
					.read_file(&entry)
					.map_err(map_err)?
					.with_cached_chain()
					.await
					.map_err(map_err)?;
				reader
					.seek(SeekFrom::Start(state.pos))
					.await
					.map_err(map_err)?;
				reader
			}
		};

		let read = reader.read(buf).await.map_err(map_err)?;
		state.pos += u64::try_from(read).unwrap();
		state.reader = Some((generation, reader));

		Ok(read)
	}

	async fn write(&self, buf: &[u8]) -> io::Result<usize> {
		if buf.is_empty() {
			return Ok(0);
		}

		let volume = volume().await?;
		let mut entry = resolve_file(&volume, &self.path).await?;
		let mut state = self.state.lock().await;
		state.reader = None;
		modified();

		let start = usize::try_from(state.pos).map_err(|_| Errno::Fbig)?;
		let end = start.checked_add(buf.len()).ok_or(Errno::Fbig)?;
		let old_len = usize::try_from(entry.len()).map_err(|_| Errno::Fbig)?;

		// `FileWriter` only appends, so data behind `start` is cut off and
		// written again after `buf`.
		let mut tail = Vec::new();
		let mut gap = Vec::new();
		if start < old_len {
			if end < old_len {
				let mut reader = volume.read_file(&entry).map_err(map_err)?;
				reader
					.seek(SeekFrom::Start(u64::try_from(end).unwrap()))
					.await
					.map_err(map_err)?;
				tail = reader.read_to_vec().await.map_err(map_err)?;
			}

			volume.truncate(&entry, start).await.map_err(map_err)?;
			entry = resolve_file(&volume, &self.path).await?;
		} else {
			gap.resize(start - old_len, 0);
		}

		let mut writer = FileWriter::new_append(&volume, &entry)
			.await
			.map_err(map_err)?;
		for part in [gap.as_slice(), buf, tail.as_slice()] {
			if !part.is_empty() {
				writer.write(part).await.map_err(map_err)?;
			}
		}
		writer.finish().await.map_err(map_err)?;

		state.pos = u64::try_from(end).unwrap();

		Ok(buf.len())
	}

	async fn lseek(&self, offset: isize, whence: SeekWhence) -> io::Result<isize> {
		let volume = volume().await?;
		let mut state = self.state.lock().await;
		let offset = i64::try_from(offset).unwrap();

		let new = match whence {
			SeekWhence::Set => offset,
			SeekWhence::Cur => i64::try_from(state.pos).unwrap() + offset,
			SeekWhence::End => {
				let entry = resolve_file(&volume, &self.path).await?;
				i64::try_from(entry.len()).unwrap() + offset
			}
			_ => return Err(Errno::Inval),
		};

		if new < 0 {
			return Err(Errno::Inval);
		}

		state.pos = u64::try_from(new).unwrap();

		Ok(isize::try_from(new).unwrap())
	}

	async fn fstat(&self) -> io::Result<FileAttr> {
		let volume = volume().await?;
		let entry = resolve_file(&volume, &self.path).await?;

		Ok(file_attr(entry.len(), NodeKind::File))
	}

	async fn truncate(&self, size: usize) -> io::Result<()> {
		let volume = volume().await?;
		let entry = resolve_file(&volume, &self.path).await?;

		modified();
		volume.truncate(&entry, size).await.map_err(map_err)
	}

	async fn isatty(&self) -> io::Result<bool> {
		Ok(false)
	}

	async fn fsync(&self) -> io::Result<()> {
		volume().await?.sync().await.map_err(map_err)
	}
}

/// An open directory of the FAT file system.
#[derive(Debug)]
pub(crate) struct VfatDirectoryHandle {
	prefix: String,
	/// Index of the next entry `getdents` will report.
	next: async_lock::Mutex<usize>,
}

impl VfatDirectoryHandle {
	pub fn new(prefix: String) -> Self {
		Self {
			prefix,
			next: async_lock::Mutex::new(0),
		}
	}
}

impl ObjectInterface for VfatDirectoryHandle {
	async fn getdents(
		&self,
		buf: &mut [MaybeUninit<u8>],
		format: DirentFormat,
	) -> io::Result<usize> {
		let entries = {
			let volume = volume().await?;
			readdir_at(&volume, self.prefix.clone()).await?
		};

		let mut next = self.next.lock().await;
		let mut offset = 0;

		while let Some(entry) = entries.get(*next) {
			let Some(next_offset) =
				format.write_entry(buf, offset, 0, FileType::Unknown, entry.name.as_bytes())
			else {
				if offset == 0 {
					return Err(Errno::Inval);
				}
				break;
			};

			offset = next_offset;
			*next += 1;
		}

		Ok(offset)
	}

	async fn lseek(&self, offset: isize, whence: SeekWhence) -> io::Result<isize> {
		if whence != SeekWhence::Set || offset != 0 {
			return Err(Errno::Inval);
		}
		*self.next.lock().await = 0;

		Ok(0)
	}

	async fn fstat(&self) -> io::Result<FileAttr> {
		Ok(file_attr(0, NodeKind::Directory))
	}

	async fn fsync(&self) -> io::Result<()> {
		volume().await?.sync().await.map_err(map_err)
	}
}

pub(crate) fn sync() -> io::Result<()> {
	block_on(
		async {
			match volume().await {
				Ok(v) => v.sync().await.map_err(map_err),
				Err(_) => Ok(()),
			}
		},
		None,
	)
}
