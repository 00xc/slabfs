use fuse_backend_rs::transport;
use std::fmt;
use std::io;

#[derive(Debug)]
pub enum FsErr {
	Transport(transport::Error),
	Io(io::Error),
}

impl From<transport::Error> for FsErr {
	fn from(e: transport::Error) -> Self {
		Self::Transport(e)
	}
}

impl From<io::Error> for FsErr {
	fn from(e: io::Error) -> Self {
		Self::Io(e)
	}
}

impl fmt::Display for FsErr {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			Self::Transport(e) => write!(f, "Transport error: {}", e),
			Self::Io(e) => write!(f, "I/O error: {}", e),
		}
	}
}
