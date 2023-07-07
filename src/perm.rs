use crate::ioerr;
use std::io;

bitflags::bitflags! {
	#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
	pub struct FsPerm: u32 {
		const USER_READ  = libc::S_IRUSR;
		const USER_WRITE = libc::S_IWUSR;
		const USER_EXEC  = libc::S_IXUSR;
		const USER_RWX   = libc::S_IRWXU;
		const GROUP_READ  = libc::S_IRGRP;
		const GROUP_WRITE = libc::S_IWGRP;
		const GROUP_EXEC  = libc::S_IXGRP;
		const GROUP_RWX   = libc::S_IRWXG;
		const OTHER_READ  = libc::S_IROTH;
		const OTHER_WRITE = libc::S_IWOTH;
		const OTHER_EXEC  = libc::S_IXOTH;
		const OTHER_RWX   = libc::S_IRWXO;
	}
}

impl TryFrom<u32> for FsPerm {
	type Error = io::Error;
	fn try_from(val: u32) -> Result<Self, Self::Error> {
		Self::from_bits(val & !(libc::S_IFMT))
			.ok_or(ioerr!(InvalidInput))
	}
}

impl FsPerm {
	pub fn dir() -> Self {
		Self::USER_RWX
			| Self::GROUP_READ | Self::GROUP_EXEC
			| Self::OTHER_READ | Self::OTHER_EXEC
	}

	pub fn file() -> Self {
		Self::USER_READ
			| FsPerm::USER_WRITE
			| Self::GROUP_READ
			| Self::OTHER_READ
	}
}

#[derive(Clone, Copy, Debug)]
pub struct FsOwner {
	pub uid: u32,
	pub gid: u32,
}

impl FsOwner {
	pub const fn new(uid: u32, gid: u32) -> Self {
		Self { uid, gid }
	}
}

impl Default for FsOwner {
	fn default() -> Self {
		Self {
			uid: 1000,
			gid: 100,
		}
	}
}
