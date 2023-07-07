use crate::ioerr;
use std::io;
use fuse_backend_rs::common::file_traits::FileReadWriteVolatile;
use fuse_backend_rs::common::file_buf::FileVolatileSlice;

pub struct FileWriter<'a> {
	pos: usize,
	data: &'a mut Vec<u8>,
}

impl<'a> FileWriter<'a> {
	pub fn new(data: &'a mut Vec<u8>) -> Self {
		Self { pos: 0, data }
	}
}

impl FileReadWriteVolatile for FileWriter<'_> {
	fn write_volatile(&mut self, slice: FileVolatileSlice<'_>) -> io::Result<usize> {
		let res = self.write_at_volatile(slice, self.pos as u64);
		if let Ok(n) = res {
			self.pos += n;
		}
		res
	}

	fn write_at_volatile(&mut self, slice: FileVolatileSlice<'_>, off: u64) -> io::Result<usize> {
		let start = usize::try_from(off).ok();
		let end = start.and_then(|e| e.checked_add(slice.len()));
		let Some((start, end)) = start.zip(end) else {
			return Ok(0)
		};

		if end > self.data.capacity() {
			self.data
				.try_reserve(end)
				.map_err(|_| ioerr!(OutOfMemory))?;
		}

		unsafe {
			self.data
				.as_mut_ptr()
				.add(start)
				.copy_from_nonoverlapping(
					slice.as_ptr(),
					slice.len()
				);
			self.data.set_len(end);
		};
		
		Ok(slice.len())
	}

	fn read_volatile(&mut self, _slice: FileVolatileSlice<'_>) -> io::Result<usize> {
		todo!()
	}
	fn read_at_volatile(&mut self, _slice: FileVolatileSlice<'_>, _off: u64) -> io::Result<usize> {
		todo!()
	}

}

pub struct FileReader<'a> {
	pos: usize,
	data: &'a [u8],
}

impl<'a> FileReader<'a> {
	pub fn new(data: &'a [u8]) -> Self {
		Self { pos: 0, data }
	}
}

impl FileReadWriteVolatile for FileReader<'_> {
	fn read_volatile(&mut self, slice: FileVolatileSlice<'_>) -> io::Result<usize> {
		let res = self.read_at_volatile(slice, self.pos as u64);
		if let Ok(n) = res {
			self.pos += n;
		}
		res
	}

	fn read_at_volatile(&mut self, slice: FileVolatileSlice<'_>, off: u64) -> io::Result<usize> {
		let start = usize::try_from(off).ok();
		let end  = start
			.and_then(|s| s.checked_add(slice.len()))
			.map(|end| end.min(self.data.len()));
		let Some(data) = start.zip(end)
			.and_then(|(start, end)| self.data.get(start..end)) else
		{
			return Ok(0);
		};

		slice.as_volatile_slice().copy_from(data);
		Ok(data.len())
	}

	fn write_volatile(&mut self, _slice: FileVolatileSlice<'_>) -> io::Result<usize> {
		todo!()
	}

	fn write_at_volatile(&mut self, _slice: FileVolatileSlice<'_>, _off: u64) -> io::Result<usize> {
		todo!();
	}
}