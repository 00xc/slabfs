mod error;
mod file_entry;
mod file_io;
mod inode;
mod perm;

use crate::{
	error::FsErr,
	file_entry::{FsEntry, FsType},
	file_io::{FileReader, FileWriter},
	inode::{Inode, InodeInfo},
	perm::{FsOwner, FsPerm},
};
use core::time::Duration;
use fastcmp::Compare;
use fuse_backend_rs::abi::fuse_abi::{CreateIn, FsOptions, stat64};
use fuse_backend_rs::api::filesystem::{
	Context,
	DirEntry,
	Entry,
	FileSystem,
	OpenOptions,
	SetattrValid,
	ZeroCopyReader,
	ZeroCopyWriter,
};
use fuse_backend_rs::api::server::Server;
use fuse_backend_rs::transport::{FuseChannel, FuseSession};
use slab::Slab;
use std::ffi::CStr;
use std::io;
use std::path::Path;
use std::sync::{Arc, RwLock};

const ST_DEV: u64 = 666420;
const TIMEOUT_SECS: Duration = Duration::from_secs(10000);
const NUM_THREADS: usize = 1;

#[macro_export]
macro_rules! ioerr {
	($k:ident) => {
		std::io::Error::from(std::io::ErrorKind::$k)
	};
	($k:ident, $e:expr) => {
		std::io::Error::new(std::io::ErrorKind::$k, $e)
	};
	($k:expr) => {
		std::io::Error::from_raw_os_error($k)
	};
}

#[derive(Debug)]
struct FsFiles {
	files: Slab<InodeInfo>,
}

impl FsFiles {
	fn new() -> Self {
		Self {
			files: Slab::with_capacity(256),
		}
	}

	#[inline(always)]
	fn get(&self, ino: Inode) -> io::Result<&InodeInfo> {
		let idx = usize::from(ino);
		self.files.get(idx).ok_or(ioerr!(NotFound))
	}

	#[inline(always)]
	fn get_mut(&mut self, ino: Inode) -> io::Result<&mut InodeInfo> {
		let idx = usize::from(ino);
		self.files.get_mut(idx).ok_or(ioerr!(NotFound))
	}

	#[inline(always)]
	unsafe fn get_unchecked_mut(&mut self, ino: Inode) -> &mut InodeInfo {
		let idx = usize::from(ino);
		self.files.get_unchecked_mut(idx)
	}

	#[inline(always)]
	unsafe fn get_unchecked(&self, ino: Inode) -> &InodeInfo {
		let idx = usize::from(ino);
		self.files.get_unchecked(idx)
	}

	fn remove(&mut self, ino: Inode) {
		let idx = usize::from(ino);
		self.files.remove(idx);
	}

	fn insert_and_get(&mut self, info: InodeInfo) -> (Inode, Entry) {
		let slot = self.files.vacant_entry();
		let ino = Inode::from(slot.key());
		let entry = slot.insert(info).get_entry(ino);
		(ino, entry)
	}

	fn insert(&mut self, info: InodeInfo) -> Inode {
		Inode::from(self.files.insert(info))
	}

	fn unlink_inode(&mut self, parent: Inode, name: &CStr) -> io::Result<()> {
		let pinfo = self.get_mut(parent)?;
		let children = pinfo.children_mut()?;
		let name_bytes = name.to_bytes();
		let idx = children
			.iter()
			.position(|(_, cname)| cname.feq(name_bytes))
			.ok_or(ioerr!(NotFound))?;
		children.swap_remove(idx);
		Ok(())
	}

	#[inline(always)]
	fn read_ino<F, T>(&self, ino: Inode, f: F) -> io::Result<T>
	where
		F: Fn(&InodeInfo) -> io::Result<T>,
		T: Sized,
	{
		self.get(ino).and_then(f)
	}

	#[inline(always)]
	fn write_ino<F, T>(&mut self, ino: Inode, f: F) -> io::Result<T>
	where
		F: FnMut(&mut InodeInfo) -> io::Result<T>,
		T: Sized,
	{
		self.get_mut(ino).and_then(f)
	}

	#[allow(unused)]
	fn write_name<F, T>(&mut self, parent: Inode, name: &CStr, mut f: F) -> io::Result<T>
	where
		F: FnMut((Inode, &mut InodeInfo)) -> io::Result<T>,
		T: Sized,
	{
		let ino = self.get(parent)?
			.children()?
			.iter()
			.find_map(|(ino, cname)| {
				cname.feq(name.to_bytes()).then_some(*ino)
			})
			.ok_or(ioerr!(NotFound))?;
		let info = unsafe { self.get_unchecked_mut(ino) };
		f((ino, info))
	}

	#[inline(always)]
	fn read_name<F, T>(&self, parent: Inode, name: &CStr, f: F) -> io::Result<T>
	where
		F: Fn((Inode, &InodeInfo)) -> io::Result<T>,
		T: Sized,
	{
		let name_bytes = name.to_bytes();
		for (child, cname) in self.get(parent)?.children()? {
			if cname.feq(name_bytes) {
				let info = if cfg!(debug_assertions) {
					self.get(*child).expect("Stale child")
				} else {
					unsafe { self.get_unchecked(*child) }
				};
				return f((*child, info));
			}
		}

		Err(ioerr!(NotFound))
	}
}

#[derive(Debug)]
struct SlabFs {
	files: RwLock<FsFiles>,
}

impl SlabFs {
	fn new() -> Self {
		let fs = Self {
			files: RwLock::new(FsFiles::new()),
		};
		fs.insert_entry(InodeInfo::empty());
		fs.insert_entry(InodeInfo::dir("/").unwrap());
		fs
	}

	fn insert_entry(&self, info: InodeInfo) -> Inode {
		self.files.write().unwrap().insert(info)
	}
}

impl FileSystem for SlabFs {
	type Inode = Inode;
	type Handle = u64;

	fn init(&self, _capable: FsOptions) -> io::Result<FsOptions> {
		log::trace!("init(capable={:?})", _capable);
		let mut cap = FsOptions::empty();
		cap.set(FsOptions::HAS_IOCTL_DIR, true);
		cap.set(FsOptions::ABORT_ERROR, true);
		cap.set(FsOptions::ASYNC_READ, true);
		cap.set(FsOptions::ASYNC_DIO, true);
		cap.set(FsOptions::BIG_WRITES, true);
		cap.set(FsOptions::PARALLEL_DIROPS, true);
		cap.set(FsOptions::ZERO_MESSAGE_OPEN, true);
		//cap.set(FsOptions::DO_READDIRPLUS, true);
		cap.set(FsOptions::WRITEBACK_CACHE, true);
		//cap.set(FsOptions::EXPLICIT_INVAL_DATA, true);
		cap.set(FsOptions::SPLICE_READ, true);
		cap.set(FsOptions::SPLICE_WRITE, true);
		cap.set(FsOptions::SPLICE_MOVE, true);
		Ok(cap)
	}

	fn readdir(
		&self,
		_ctx: &Context,
		inode: Self::Inode,
		_handle: Self::Handle,
		size: u32,
		offset: u64,
		add_entry: &mut dyn FnMut(DirEntry<'_>) -> io::Result<usize>,
	) -> io::Result<()> {
		log::trace!(
			"readdir(inode={:?}, off={}, size={})",
			inode, offset, size
		);

		let size = size as usize;
		let offset = offset as usize;
		if size == 0 {
			return Ok(());
		}

		let files = self.files.read().unwrap();
		for (i, (child, _)) in files.get(inode)?
			.children()?
			.iter()
			.enumerate()
			.skip(offset)
		{
			let info = files.get(*child).expect("Stale child?");
			let dir_entry = info.get_direntry(*child, (i as u64) + 1);
			if add_entry(dir_entry)? == 0 {
				break;
			}
		}

		Ok(())
	}

	fn create(
		&self,
		ctx: &Context,
		parent: Self::Inode,
		name: &CStr,
		args: CreateIn,
	) -> io::Result<(Entry, Option<Self::Handle>, OpenOptions)> {
		log::trace!("create(parent={:?}, name={:?})", parent, name);
		let info = InodeInfo::create(name, ctx, args)?;

		let mut files = self.files.write().unwrap();
		let (ino, entry) = files.insert_and_get(info);

		// Add to parent
		if let Err(e) = files.write_ino(parent, |pinfo| {
			pinfo.add_child(ino, name)
		}) {
			files.remove(ino);
			return Err(e);
		}

		Ok((entry, None, OpenOptions::empty()))
	}

	fn mkdir(
		&self,
		ctx: &Context,
		parent: Self::Inode,
		name: &CStr,
		mode: u32,
		umask: u32,
	) -> io::Result<Entry> {
		log::trace!("mkdir(parent={:?}, name={:?})", parent, name);
		let args = CreateIn {
			flags: 0,
			mode: mode | libc::S_IFDIR,
			umask,
			fuse_flags: 0,
		};
		let (entry, _, _) = self.create(ctx, parent, name, args)?;
		Ok(entry)
	}

	fn read(
		&self,
		_ctx: &Context,
		inode: Self::Inode,
		_handle: Self::Handle,
		w: &mut dyn ZeroCopyWriter,
		size: u32,
		offset: u64,
		_lock_owner: Option<u64>,
		_flags: u32,
	) -> io::Result<usize> {
		log::trace!("read(inode={:?}, sz={}, off={})", inode, size, offset);
		let mut files = self.files.write().unwrap();
		files.write_ino(inode, |info| {
			let data = info.file_data()?;
			let mut reader = FileReader::new(data);
			w.write_from(&mut reader, size as usize, offset)
		})
	}

	fn write(
		&self,
		_ctx: &Context,
		inode: Self::Inode,
		_handle: Self::Handle,
		r: &mut dyn ZeroCopyReader,
		size: u32,
		offset: u64,
		_lock_owner: Option<u64>,
		_delayed_write: bool,
		_flags: u32,
		_fuse_flags: u32,
	) -> io::Result<usize> {
		log::trace!("write(inode={:?}, sz={}, off={})", inode, size, offset);
		let mut files = self.files.write().unwrap();
		files.write_ino(inode, |info| {
			let data = info.file_data()?;
			let mut writer = FileWriter::new(data);
			r.read_to(&mut writer, size as usize, offset)
		})
	}

	fn lookup(
		&self,
		_ctx: &Context,
		parent: Self::Inode,
		name: &CStr,
	) -> io::Result<Entry> {
		log::trace!("lookup(parent={:?}, name={:?})", parent, name);
		// The lookup count is atomic, so go through the read lock
		let files = self.files.read().unwrap();
		files.read_name(parent, name, |(ino, info)| {
			info.refinc()?;
			Ok(info.get_entry(ino))
		})
	}

	fn forget(
		&self,
		_ctx: &Context,
		inode: Self::Inode,
		count: u64,
	) {
		log::trace!("forget(inode={:?}, count={})", inode, count);
		let mut files = self.files.write().unwrap();
		let deleted = files.read_ino(inode, |info| {
			info.refsub(count)
		}).unwrap();
		if deleted {
			files.remove(inode);
		}
	}

	fn batch_forget(
		&self,
		_ctx: &Context,
		requests: Vec<(Self::Inode, u64)>,
	) {
		log::trace!("batch_forget()");
		let mut files = self.files.write().unwrap();
		for (ino, count) in requests.into_iter() {
			let deleted = files.write_ino(ino, |info| {
				info.refsub(count)
			}).unwrap();
			if deleted {
				files.remove(ino);
			}
		}
	}

	fn getattr(
		&self,
		_ctx: &Context,
		inode: Self::Inode,
		_handle: Option<Self::Handle>,
	) -> io::Result<(stat64, Duration)> {
		log::trace!("getattr({:?})", inode);
		let files = self.files.read().unwrap();
		files.read_ino(inode, |info| {
			Ok((info.stat64(inode), TIMEOUT_SECS))
		})
	}

	fn setattr(
		&self,
		_ctx: &Context,
		inode: Self::Inode,
		attr: stat64,
		_handle: Option<Self::Handle>,
		valid: SetattrValid,
	) -> io::Result<(stat64, Duration)> {
		log::trace!("setattr(inode={:?}, valid={:?})", inode, valid);

		let mut files = self.files.write().unwrap();
		files.write_ino(inode, |info| {
			if valid.contains(SetattrValid::UID) {
				info.owner.uid = attr.st_uid;
			}
			if valid.contains(SetattrValid::GID) {
				info.owner.gid = attr.st_gid;
			}
			if valid.contains(SetattrValid::MODE) {
				info.perm = FsPerm::try_from(attr.st_mode)?;
				debug_assert_eq!(info.st_mode(), attr.st_mode);
			}
			if valid.contains(SetattrValid::SIZE) {
				let data = info.file_data()?;
				data.resize(attr.st_size as usize, 0);
			}

			Ok((info.stat64(inode), TIMEOUT_SECS))
		})
	}

	fn rmdir(
		&self,
		_ctx: &Context,
		parent: Self::Inode,
		name: &CStr,
	) -> io::Result<()> {
		log::trace!("rmdir(parent={:?}, name={:?})", parent, name);
		self.files.write().unwrap().unlink_inode(parent, name)
	}

	fn unlink(
		&self,
		_ctx: &Context,
		parent: Self::Inode,
		name: &CStr,
	) -> io::Result<()> {
		log::trace!("unlink(parent={:?}, name={:?})", parent, name);
		self.files.write().unwrap().unlink_inode(parent, name)
	}
}

fn svc_loop(srv: Arc<Server<SlabFs>>, mut channel: FuseChannel) {
	log::info!("Starting thread: {:?}", std::thread::current().id());
	while let Ok(rq) = channel.get_request() {
		let Some((rd, wr)) = rq else {
			continue;
		};
		if let Err(e) = srv.handle_message(rd, wr.into(), None, None) {
			log::error!("FUSE error: {:?}", e);
		}
	}
}

fn usage() -> ! {
	eprintln!("Usage: {} <mountpoint>", std::env::args().next().unwrap());
	std::process::exit(0)
}

fn main() -> Result<(), FsErr> {
	env_logger::init();

	let Some(mountpoint) = std::env::args().nth(1) else {
		usage();
	};

	let server = Arc::new(Server::new(SlabFs::new()));
	let mut sess = FuseSession::new_with_autounmount(
		Path::new(&mountpoint),
		"slabfs",
		"",
		false,
		true,
	)?;
	sess.mount()?;

	let mut thrds = Vec::with_capacity(NUM_THREADS);
	for _ in 0..NUM_THREADS {
		let srv = server.clone();
		let ch = sess.new_channel().unwrap();
		let t = std::thread::Builder::new()
			.name("fuse_server".to_string())
			.spawn(move || svc_loop(srv, ch))
			.unwrap();
		thrds.push(t);
	}

	for t in thrds {
		t.join().unwrap();
	}

	log::info!("Exiting");

	Ok(())
}
