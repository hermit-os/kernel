use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::mem::MaybeUninit;

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
use crate::syscalls::Dirent64;
use crate::time::SystemTime;

/// Number of sectors `FatStream` keeps in memory.
///
/// A FAT walk, the file data it addresses and the directory entry naming the
/// file live in three different regions of the device. With a single staging
/// sector they evict each other at every step, which roughly doubles the
/// transfers of a plain sequential read. The cache keeps the active FAT and
/// directory sectors resident while data streams past, and has to be several
/// times [`RUN_SECTORS`] so that one read-ahead cannot displace them.
const CACHE_SECTORS: usize = 256;

/// Number of sectors a cache miss fetches, and the largest write the cache
/// coalesces adjacent dirty sectors into.
const RUN_SECTORS: usize = 64;

/// One cached device sector.
struct CachedSector {
	/// Sector index on the device.
	sector: usize,
	/// internal buffer to cache sectors
	data: [u8; SECTOR_SIZE],
	/// Whether `data` holds changes that are not on the device yet.
	dirty: bool,
	/// Value of `FatStream::clock` at the last access, for LRU eviction.
	used: u64,
}

/// Byte-granular, cached view of the block device for `hadris-fat`.
///
/// The device transfers whole sectors, so this stages them and serves the
/// byte-wise `Read`/`Write`/`Seek` the file system expects. Writes are cached
/// and reach the device on eviction or on an explicit flush.
struct FatStream {
	/// Sector cache
	cache: Vec<CachedSector>,
	/// Monotonic access counter driving LRU eviction.
	clock: u64,
	/// Absolute byte position of the stream within the device.
	pos: usize,
	/// Staging buffer for read-ahead, see `FatStream::with_run`.
	read_run: Vec<u8>,
	/// Staging buffer for coalesced write-back.
	///
	/// Separate from `read_run` because a read-ahead that has to evict a dirty
	/// sector flushes from inside its own transfer, and the two would
	/// otherwise contend for one buffer.
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

	/// Returns the slot holding `sector`, reading it in if it is not cached.
	///
	/// `fill` requests the sector's current contents. A caller that overwrites
	/// the whole sector can pass `false` and save the read.
	async fn slot(&mut self, sector: usize, fill: bool) -> Result<usize, Errno> {
		self.clock += 1;

		if let Some(slot) = self.find(sector) {
			self.cache[slot].used = self.clock;
			return Ok(slot);
		}

		if !fill {
			return self.install(sector, &[0u8; SECTOR_SIZE]).await;
		}

		// Fetch the requested sector together with the ones that follow it,
		// bounded by the device. One request for the whole run costs about
		// what a single-sector one would, and a file system reads forwards.
		let sectors = RUN_SECTORS.min(self.capacity()? - sector);

		// The staging buffer is owned by the stream so that a transfer does
		// not allocate, but the work below needs `&mut self` for the cache as
		// well. Taking it out for the duration keeps the two borrows disjoint;
		// the inner call may fail, so the buffer goes back either way.
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
			// A sector already in the cache keeps its cached copy: that one
			// may be dirty, and would then be newer than what the device
			// just handed us.
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
			// Only reachable if the requested sector was cached after all,
			// which `slot` has already ruled out above.
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

		// Evict the least recently used sector. The sectors installed earlier
		// in this run carry the current clock value and are therefore never
		// the victim.
		let victim = self
			.cache
			.iter()
			.enumerate()
			.min_by_key(|(_, entry)| entry.used)
			.map(|(slot, _)| slot)
			.unwrap();

		// Writing just the victim back would issue one request per evicted
		// sector, and a sequential write dirties the whole cache — that was
		// the bulk of the traffic. Flushing everything instead lets the runs
		// merge, and leaves every later eviction free until something is
		// dirtied again.
		if self.cache[victim].dirty {
			self.flush_all().await?;
		}

		self.cache[victim] = entry;

		Ok(victim)
	}

	/// Writes every dirty sector back, merging adjacent ones into one request.
	///
	/// The dirty sectors of a single operation are rarely scattered — a FAT
	/// chain and the directory entry naming it occupy runs of neighbours — so
	/// sorting them and writing each run as a whole turns a flush of the
	/// entire cache into a handful of requests.
	async fn flush_all(&mut self) -> Result<(), Errno> {
		let mut dirty: Vec<usize> = (0..self.cache.len())
			.filter(|&slot| self.cache[slot].dirty)
			.collect();
		if dirty.is_empty() {
			return Ok(());
		}
		dirty.sort_unstable_by_key(|&slot| self.cache[slot].sector);

		// The staging buffer is lent out for the duration so that a transfer
		// does not allocate, and returned even when one fails. `Batch::write`
		// copies before it returns, so one buffer serves every run.
		let mut run = core::mem::take(&mut self.write_run);
		run.resize(RUN_SECTORS * SECTOR_SIZE, 0);

		let mut batch = driver().ok_or(Errno::Nodev)?.batch().await;
		let mut submitted = Ok(());

		let mut rest = dirty.as_slice();
		while let Some((&first, _)) = rest.split_first() {
			// Longest prefix of consecutive sectors that still fits the buffer.
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

		// The sectors stay dirty until the device has acknowledged all of
		// them; marking them earlier would drop the data on a failed flush.
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

		// Reading past the last sector is end of stream, not a failure.
		if sector >= self.capacity()? {
			return Ok(0);
		}

		let slot = self.slot(sector, true).await?;

		// A single call never crosses a sector boundary. `Read::read` is
		// permitted to return a short count, and the caller loops for the
		// remainder, so there is no reason to chain sectors here.
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

		// The device has a fixed size, so there is nothing to grow into.
		if sector >= self.capacity()? {
			return Err(IoError::from_source(Errno::Nospc));
		}

		// Like `read`, a single call stays within one sector and the caller
		// loops for the remainder.
		let n = usize::min(buf.len(), SECTOR_SIZE - offset);

		// A partial sector is read-modify-write; a full one is overwritten
		// completely, so reading it first would be wasted.
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

		// Seeking before 0 is an error. Seeking past the end is not:
		// it only fails once something is actually read or written there.
		let new = new.filter(|new| *new >= 0).ok_or(Errno::Inval)?;

		// The cache stays valid — it is keyed by sector index, independent of
		// where the stream happens to point.
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

	let volume = block_on(
		async {
			FatVolume::builder(stream)
				.time_provider(&TIME_PROVIDER)
				.open()
				.await
				.map_err(map_err)
		},
		None,
	)
	.expect("Unable to create FAT volume");

	info!("Mounting {} volume at {MOUNT_POINT}", volume.fat_type());

	if VFAT.set(async_lock::Mutex::new(volume)).is_err() {
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

		// `time_tenth` counts 10 ms units on top of the two-second resolution
		// of the `time` field, so the odd second has to be folded back in.
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

static VFAT: OnceCell<async_lock::Mutex<FatVolume<FatStream>>> = OnceCell::new();

// check if VFAT is Sync
const _: () = {
	const fn assert_sync<T: Sync>() {}
	assert_sync::<async_lock::Mutex<FatVolume<FatStream>>>();
};

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
		// Everything else — corrupt structures, cluster-chain damage, and the
		// underlying block device's own errors — surfaces as an I/O error.
		_ => Errno::Io,
	}
}

/// Locks the mounted volume for the duration of one file system operation.
///
/// `hadris-fat` locks its sector stream per access, not per operation, so a
/// lookup and the write that follows it would otherwise be separate critical
/// sections and two cores could pick the same free directory slot or cluster.
/// Holding the volume itself is also what keeps that stream lock uncontended,
/// which matters because it is a spin lock and the driver awaits the device
/// underneath it.
async fn volume() -> io::Result<async_lock::MutexGuard<'static, FatVolume<FatStream>>> {
	Ok(VFAT.get().ok_or(Errno::Nodev)?.lock().await)
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

		// An intermediate component has to exist and be a directory.
		let entry = entry.ok_or(Errno::Noent)?;
		dir = dir.open_entry(&entry).map_err(map_err)?;
	}

	// The path named the root directory itself.
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
		// No trailing component left: `parent` already is the directory.
		None => Ok(parent),
	}
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

/// Builds a `FileAttr` from what FAT can tell us.
///
/// FAT has no ownership or link count, so those stay at their defaults.
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
					// The root of the mounted volume.
					return Ok(file_attr(0, NodeKind::Directory));
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
		// FAT has no symbolic links, so `lstat` and `stat` cannot differ.
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

				// A directory is opened as a directory handle, whatever the flags say.
				if let Some(entry) = &entry
					&& entry.is_directory()
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
/// The handle keeps only the path and the offset. Readers and writers borrow
/// the volume and cannot be parked in a long lived file descriptor, so every
/// operation resolves the path again.
#[derive(Debug)]
pub(crate) struct VfatFileHandle {
	path: String,
	pos: async_lock::Mutex<u64>,
}

impl VfatFileHandle {
	pub fn new(path: String, pos: u64) -> Self {
		Self {
			path,
			pos: async_lock::Mutex::new(pos),
		}
	}
}

impl ObjectInterface for VfatFileHandle {
	async fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
		let volume = volume().await?;
		let entry = resolve_file(&volume, &self.path).await?;
		let mut pos = self.pos.lock().await;

		let mut reader = volume.read_file(&entry).map_err(map_err)?;

		// Walks the FAT chain to the position without touching file data;
		// positions at or past the end make the following read return 0.
		reader.seek(SeekFrom::Start(*pos)).await.map_err(map_err)?;

		let read = reader.read(buf).await.map_err(map_err)?;
		*pos += u64::try_from(read).unwrap();

		Ok(read)
	}

	async fn write(&self, buf: &[u8]) -> io::Result<usize> {
		let volume = volume().await?;
		let entry = resolve_file(&volume, &self.path).await?;
		let mut pos = self.pos.lock().await;

		// `FileWriter` rewrites the file from the start, so anything before the
		// offset has to be carried over.
		let prefix = if *pos > 0 {
			let mut reader = volume.read_file(&entry).map_err(map_err)?;
			let mut kept = Vec::with_capacity(usize::try_from(*pos).unwrap());
			let mut scratch = [0u8; SECTOR_SIZE];
			while u64::try_from(kept.len()).unwrap() < *pos {
				let want = usize::try_from(*pos - u64::try_from(kept.len()).unwrap())
					.unwrap()
					.min(SECTOR_SIZE);
				let read = reader.read(&mut scratch[..want]).await.map_err(map_err)?;
				if read == 0 {
					// Writing past the end pads the gap with zeros.
					kept.resize(usize::try_from(*pos).unwrap(), 0);
					break;
				}
				kept.extend_from_slice(&scratch[..read]);
			}
			kept
		} else {
			Vec::new()
		};

		let mut writer = volume.write_file(&entry).map_err(map_err)?;
		if !prefix.is_empty() {
			writer.write(&prefix).await.map_err(map_err)?;
		}
		let written = writer.write(buf).await.map_err(map_err)?;
		writer.finish().await.map_err(map_err)?;

		*pos += u64::try_from(written).unwrap();

		Ok(written)
	}

	async fn lseek(&self, offset: isize, whence: SeekWhence) -> io::Result<isize> {
		let volume = volume().await?;
		let mut pos = self.pos.lock().await;
		let offset = i64::try_from(offset).unwrap();

		let new = match whence {
			SeekWhence::Set => offset,
			SeekWhence::Cur => i64::try_from(*pos).unwrap() + offset,
			SeekWhence::End => {
				let entry = resolve_file(&volume, &self.path).await?;
				i64::try_from(entry.len()).unwrap() + offset
			}
			_ => return Err(Errno::Inval),
		};

		if new < 0 {
			return Err(Errno::Inval);
		}

		*pos = u64::try_from(new).unwrap();

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
	async fn getdents(&self, buf: &mut [MaybeUninit<u8>]) -> io::Result<usize> {
		let entries = {
			let volume = volume().await?;

			readdir_at(&volume, self.prefix.clone()).await?
		};

		let mut next = self.next.lock().await;
		let mut offset = 0usize;

		while let Some(entry) = entries.get(*next) {
			let name = entry.name.as_bytes();
			let len = core::mem::offset_of!(Dirent64, d_name) + name.len() + 1;
			let reclen = len.next_multiple_of(align_of::<Dirent64>());

			if offset + reclen > buf.len() {
				if offset == 0 {
					// Not even one entry fits.
					return Err(Errno::Inval);
				}
				break;
			}

			let target = buf[offset].as_mut_ptr().cast::<Dirent64>();
			// SAFETY: the bounds check above guarantees that the entry and its
			// trailing zero byte fit into `buf`.
			unsafe {
				target.write(Dirent64 {
					d_ino: 0,
					d_off: 0,
					d_reclen: u16::try_from(reclen).unwrap(),
					d_type: FileType::Unknown,
					d_name: core::marker::PhantomData {},
				});
				let name_ptr = core::ptr::from_mut(&mut (*target).d_name).cast::<u8>();
				name_ptr.copy_from_nonoverlapping(name.as_ptr(), name.len());
				name_ptr.add(name.len()).write(0);
			}

			offset += reclen;
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
		async { volume().await?.sync().await.map_err(map_err) },
		None,
	)
}
