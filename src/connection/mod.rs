use core::task::{Context, Poll};

mod impls;

/// A trait to perform in-order, serial, byte-wise I/O.
///
/// When the `std` feature is enabled, this trait is automatically implemented
/// for [`TcpStream`](std::net::TcpStream) and
/// [`UnixStream`](std::os::unix::net::UnixStream) (on unix systems).
pub trait Connection {
    /// Transport-specific error type.
    type Error;

    /// Read a single byte.
    fn read(&mut self) -> Result<u8, Self::Error>;

    /// Read the exact number of bytes required to fill the buffer.
    ///
    /// This method's default implementation calls `self.read()` for each byte
    /// in the buffer. This can be quite inefficient, so if a more efficient
    /// implementation exists (such as calling `read_exact()` on an underlying
    /// `std::io::Read` object), this method should be overwritten.
    fn read_exact(&mut self, buf: &mut [u8]) -> Result<(), Self::Error> {
        for b in buf {
            *b = self.read()?;
        }
        Ok(())
    }

    /// Write a single byte.
    fn write(&mut self, byte: u8) -> Result<(), Self::Error>;

    /// Write the entire buffer, blocking until complete.
    ///
    /// This method's default implementation calls `self.write()` on each byte
    /// in the buffer. This can be quite inefficient, so if a more efficient
    /// implementation exists (such as calling `write_all()` on an underlying
    /// `std::io::Write` object), this method should be overwritten.
    fn write_all(&mut self, buf: &[u8]) -> Result<(), Self::Error> {
        for b in buf {
            self.write(*b)?;
        }
        Ok(())
    }

    /// Peek a single byte.
    fn peek(&mut self) -> Result<u8, Self::Error>;

    /// Flush this Connection, ensuring that all intermediately buffered
    /// contents reach their destination.
    ///
    /// _Note:_ Not all `Connection`s have internal buffering (e.g: writing data
    /// to a UART TX register with FIFOs disabled). In these cases, it's fine to
    /// simply return `Ok(())`.
    fn flush(&mut self) -> Result<(), Self::Error>;

    /// Called at the start of a debugging session _before_ any GDB packets have
    /// been sent/received.
    ///
    /// This method's default implementation is a no-op.
    ///
    /// # Example
    ///
    /// The `on_session_start` implementation for `TcpStream` ensures that
    /// [`set_nodelay(true)`](std::net::TcpStream::set_nodelay)
    /// is called. The GDB remote serial protocol requires sending/receiving
    /// many small packets, so forgetting to enable `TCP_NODELAY` can result in
    /// a massively degraded debugging experience.
    fn on_session_start(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    /// Polls the connection for readability.
    ///
    /// Used as part of the
    /// [`GdbInterrupt`](crate::target::ext::base::GdbInterrupt) future.
    ///
    /// When possible, the implementation should simply forward to
    /// an existing `poll_readable`-like method on the underlying connection.
    /// For example, `tokio::net::TcpStream` includes a [`poll_read_ready`](https://docs.rs/tokio/0.3.5/tokio/net/struct.TcpStream.html#method.poll_read_ready)
    /// method.
    ///
    /// If the underlying connection doesn't expose an async interface, a
    /// "busy-polling" implementation can be used instead:
    ///
    /// ```rust,ignore
    /// fn poll_readable(&self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
    ///     cx.waker().wake_by_ref();
    ///     // check if readable, and return Poll::Ready(()) or Poll::Pending...
    /// }
    /// ```
    ///
    /// _Warning:_ accidentally passing a busy-polling `GdbInterrupt` future to
    /// an async executor will typically result in very high CPU usage! Instead,
    /// the future should be  _manually_ polled at certain intervals (e.g: after
    /// X clock cycles).
    fn poll_readable(&self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>>;
}
