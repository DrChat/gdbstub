use core::ffi::c_void;
use core::task::{Context, Poll};
use std::io;
use std::os::unix::io::AsRawFd;
use std::os::unix::net::UnixStream;

use crate::Connection;

// TODO: Remove PeekExt once `gdbstub`'s MSRV >1.48 (rust-lang/rust#73761)
trait PeekExt {
    fn peek(&self, buf: &mut [u8]) -> io::Result<usize>;
}

impl PeekExt for UnixStream {
    #[allow(non_camel_case_types)]
    fn peek(&self, buf: &mut [u8]) -> io::Result<usize> {
        // Define some libc types inline (to avoid bringing in entire libc dep)

        // every platform supported by the libc crate uses c_int = i32
        type c_int = i32;
        type size_t = usize;
        type ssize_t = isize;
        const MSG_PEEK: c_int = 2;
        extern "C" {
            fn recv(socket: c_int, buf: *mut c_void, len: size_t, flags: c_int) -> ssize_t;
        }

        // from std/sys/unix/mod.rs
        pub fn cvt(t: isize) -> io::Result<isize> {
            if t == -1 {
                Err(io::Error::last_os_error())
            } else {
                Ok(t)
            }
        }

        // from std/sys/unix/net.rs
        let ret = cvt(unsafe {
            recv(
                self.as_raw_fd(),
                buf.as_mut_ptr() as *mut c_void,
                buf.len(),
                MSG_PEEK,
            )
        })?;
        Ok(ret as usize)
    }
}

impl Connection for UnixStream {
    type Error = std::io::Error;

    fn read(&mut self) -> Result<u8, Self::Error> {
        use std::io::Read;

        let mut buf = [0u8];
        match Read::read_exact(self, &mut buf) {
            Ok(_) => Ok(buf[0]),
            Err(e) => Err(e),
        }
    }

    fn read_exact(&mut self, buf: &mut [u8]) -> Result<(), Self::Error> {
        use std::io::Read;

        Read::read_exact(self, buf)
    }

    fn peek(&mut self) -> Result<u8, Self::Error> {
        let mut buf = [0u8];
        match PeekExt::peek(self, &mut buf) {
            Ok(_) => Ok(buf[0]),
            Err(e) => Err(e),
        }
    }

    fn write(&mut self, byte: u8) -> Result<(), Self::Error> {
        use std::io::Write;

        Write::write_all(self, &[byte])
    }

    fn write_all(&mut self, buf: &[u8]) -> Result<(), Self::Error> {
        use std::io::Write;

        Write::write_all(self, buf)
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        use std::io::Write;

        Write::flush(self)
    }

    fn on_session_start(&mut self) -> Result<(), Self::Error> {
        self.set_nonblocking(false)?;
        Ok(())
    }

    fn poll_readable(&self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.set_nonblocking(true)?;

        // busy-wait polling
        cx.waker().wake_by_ref();

        let mut buf = [0u8];
        let res = match PeekExt::peek(self, &mut buf) {
            Ok(_) => Poll::Ready(Ok(())),
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => Poll::Pending,
            Err(e) => Poll::Ready(Err(e)),
        };

        self.set_nonblocking(false)?;

        res
    }
}
