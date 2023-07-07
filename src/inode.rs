use crate::{ioerr, FsEntry, FsOwner, FsPerm, FsType, ST_DEV, TIMEOUT_SECS};
use fuse_backend_rs::api::filesystem::{Context, DirEntry, Entry};
use fuse_backend_rs::abi::fuse_abi::{CreateIn, stat64};
use std::ffi::{CStr, CString};
use std::io;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, PartialOrd, Ord)]
pub struct Inode(u64);

impl From<u64> for Inode {
	fn from(v: u64) -> Self {
		Self(v)
	}
}

impl From<usize> for Inode {
	fn from(v: usize) -> Self {
		Self(v as u64)
	}
}

impl From<Inode> for usize {
	fn from(v: Inode) -> Self {
		v.0 as usize
	}
}

impl From<Inode> for u64 {
	fn from(ino: Inode) -> Self {
		ino.0
	}
}

#[derive(Debug)]
pub struct InodeInfo {
	// Use an atomic so refcount updates do not need the
	// write lock
	refs: AtomicU64,
	// Store the name as a Vec instead of CString so that
	// we can create a truly empty structure without allocating.
	name: Vec<u8>,
	pub perm: FsPerm,
	pub owner: FsOwner,
	entry: FsEntry,
}

impl InodeInfo {
	pub fn create(name: &CStr, ctx: &Context, args: CreateIn) -> io::Result<Self> {
		let perm = FsPerm::try_from(args.mode)?;
		let mode = FsType::try_from(args.mode)?;
		let owner = FsOwner::new(ctx.uid, ctx.gid);
		let entry = FsEntry::try_from(mode)?;
		Ok(Self {
			refs: 1.into(),
			name: name.to_bytes().to_vec(),
			perm,
			owner,
			entry,
		})
	}

	#[allow(dead_code)]
	pub fn file(name: &str) -> io::Result<Self> {
		let name = CString::new(name)?.into_bytes();
		Ok(Self {
			refs: 1.into(),
			name,
			perm: FsPerm::file(),
			owner: FsOwner::default(),
			entry: FsEntry::file(),
		})
	}

	pub fn dir(name: &str) -> io::Result<Self> {
		let name = CString::new(name)?.into_bytes();
		Ok(Self {
			refs: 1.into(),
			name,
			perm: FsPerm::dir(),
			owner: FsOwner::default(),
			entry: FsEntry::dir(),
		})
	}

	pub fn empty() -> Self {
		Self {
			refs: 0.into(),
			name: Vec::new(),
			perm: FsPerm::file(),
			owner: FsOwner::default(),
			entry: FsEntry::file(),
		}
	}

	pub fn refinc(&self) -> io::Result<()> {
		let r = self.refs.fetch_add(1, Ordering::Release);
		if r == u64::MAX {
			return Err(ioerr!(libc::ENFILE));
		}
		Ok(())
	}

	pub fn refsub(&self, count: u64) -> io::Result<bool> {
		let r = self.refs.fetch_sub(count, Ordering::Release);
		Ok(r <= count)
	}

	fn file_type(&self) -> FsType {
		match self.entry {
			FsEntry::File(..) => FsType::REG,
			FsEntry::Dir(..) => FsType::DIR,
		}
	}

	pub fn st_mode(&self) -> u32 {
		self.file_type().bits() | self.perm.bits()
	}

	fn st_size(&self) -> i64 {
		match &self.entry {
			FsEntry::File(d) => d.len() as i64,
			FsEntry::Dir(..) => 0i64,
		}
	}

	fn st_blocks(&self) -> i64 {
		self.st_size() / 512
	}

	fn st_rdev(&self) -> u64 {
		0
	}

	#[inline(always)]
	pub fn stat64(&self, ino: Inode) -> stat64 {
		let mut stat: stat64 = unsafe { std::mem::zeroed() };
		stat.st_dev = ST_DEV;
		stat.st_ino = ino.into();
		stat.st_mode = self.st_mode();
		stat.st_nlink = 0;
		stat.st_uid = self.owner.uid;
		stat.st_gid = self.owner.gid;
		stat.st_rdev = self.st_rdev();
		stat.st_size = self.st_size();
		stat.st_blksize = 16384;
		stat.st_blocks = self.st_blocks();
		stat.st_atime = 0;
		stat.st_atime_nsec = 0;
		stat.st_mtime = 0;
		stat.st_mtime_nsec = 0;
		stat.st_ctime = 0;
		stat.st_ctime_nsec = 0;
		stat
	}

	#[inline(always)]
	pub fn get_entry(&self, ino: Inode) -> Entry {
		Entry {
			inode: ino.into(),
			generation: 0,
			attr: self.stat64(ino),
			attr_flags: 0,
			attr_timeout: TIMEOUT_SECS,
			entry_timeout: TIMEOUT_SECS,
		}
	}

	pub fn get_direntry(&self, ino: Inode, off: u64) -> DirEntry {
		DirEntry {
			ino: ino.into(),
			offset: off,
			type_: 0,
			name: &self.name,
		}
	}

	pub fn add_child(&mut self, ino: Inode, name: &CStr) -> io::Result<()> {
		match &mut self.entry {
			FsEntry::Dir(ref mut ch) => {
				ch.push((ino, name.to_bytes().to_vec()));
				Ok(())
			},
			_ => Err(ioerr!(NotFound)),
		}
	}

	pub fn children(&self) -> io::Result<&[(Inode, Vec<u8>)]> {
		match &self.entry {
			FsEntry::Dir(ch) => Ok(ch),
			_ => Err(ioerr!(NotFound)),
		}
	}

	pub fn children_mut(&mut self) -> io::Result<&mut Vec<(Inode, Vec<u8>)>> {
		match &mut self.entry {
			FsEntry::Dir(ref mut ch) => Ok(ch),
			_ => Err(ioerr!(NotFound)),
		}
	}

	pub fn file_data(&mut self) -> io::Result<&mut Vec<u8>> {
		match &mut self.entry {
			FsEntry::File(ref mut d) => Ok(d),
			_ => Err(ioerr!(NotFound)),
		}
	}
}
