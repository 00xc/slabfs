use crate::{ioerr, Inode};
use std::io;

#[repr(u32)]
#[derive(Clone, Copy, Debug)]
#[allow(dead_code, clippy::upper_case_acronyms)]
pub enum FsType {
	REG = libc::S_IFREG,
	DIR = libc::S_IFDIR,
	CHR = libc::S_IFCHR,
	BLK = libc::S_IFBLK,
	FIFO = libc::S_IFIFO,
	LNK = libc::S_IFLNK,
	SOCK = libc::S_IFSOCK,
}

impl FsType {
	pub fn bits(&self) -> u32 {
		*self as u32
	}
}

impl TryFrom<u32> for FsType {
	type Error = io::Error;
	fn try_from(val: u32) -> Result<Self, Self::Error> {
		match val & libc::S_IFMT {
			m if m == Self::REG as u32 => Ok(Self::REG),
			m if m == Self::DIR as u32 => Ok(Self::DIR),
			_ => {
				log::error!("Unsupported file mode: {:o}", val & libc::S_IFMT);
				Err(ioerr!(Unsupported))
			}
		}
	}
}

#[derive(Clone, Debug)]
pub(crate) enum FsEntry {
	File(Vec<u8>),
	Dir(Vec<(Inode, Vec<u8>)>),
}

impl FsEntry {
	pub fn dir() -> Self {
		Self::Dir(Vec::new())
	}

	pub fn file() -> Self {
		Self::File(Vec::new())
	}
}

impl TryFrom<FsType> for FsEntry {
	type Error = io::Error;
	fn try_from(mode: FsType) -> Result<Self, Self::Error> {
		match mode {
			FsType::REG => Ok(Self::file()),
			FsType::DIR => Ok(Self::dir()),
			_ => Err(ioerr!(Unsupported)),
		}
	}
}
