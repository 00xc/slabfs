# slabfs #

A small in-memory filesystem using FUSE.

## Running ##

Simply run:

`RUST_LOG="slabfs=trace" cargo r -r -- <mountpoint>`

To suppress most log messages:

`RUST_LOG="slabfs=info" cargo r -r -- <mountpoint>`

To completely disable logging:

`RUST_LOG="slabfs=off" cargo r -r -- <mountpoint>`

## Performance ##

This is a toy filesystem. It will likely outperform your regular filesystem in terms of I/O throughput because everything is stored in RAM, but it will also be slower than a ramfs in that aspect due to all the kernel-userspace communication. In fact, accessing a lot of small files underperforms when compared to a regular filesystem due to the amount of context switches. As always, your mileage may vary.

## TODO ##

* Symlinks.
* Improve multithreaded performance.
* Consider async.
