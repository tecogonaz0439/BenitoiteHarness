//! TCP を TLS の下で包み、打ち切り用の複製を保持する（設計書 02-01「HTTP の通信」）。

use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::time::{Duration, Instant};

use ureq::unversioned::transport::{
    Buffers, ConnectionDetails, Connector, LazyBuffers, NextTimeout, Transport,
};

use super::stream::Control;

#[derive(Debug)]
pub(super) struct TcpConnector {
    pub control: Option<Arc<Control>>,
    pub idle: Option<Duration>,
}

impl Connector<()> for TcpConnector {
    type Out = TcpTransport;

    fn connect(
        &self,
        details: &ConnectionDetails,
        _: Option<()>,
    ) -> Result<Option<Self::Out>, ureq::Error> {
        let started = Instant::now();
        let budget = details.timeout.not_zero().map(|d| *d);
        let mut last = ureq::Error::ConnectionFailed;
        for address in &details.addrs {
            if let Some(control) = &self.control {
                control.check_io()?;
            }
            let remaining = budget.map(|b| b.saturating_sub(started.elapsed()));
            if remaining.is_some_and(|r| r.is_zero()) {
                return Err(ureq::Error::Timeout(details.timeout.reason));
            }
            // 一つの届かないアドレスに接続の予算を使い切らせない。
            let per_address = remaining.map(|r| {
                r.checked_div(u32::try_from(details.addrs.len()).unwrap_or(1))
                    .unwrap_or(r)
                    .max(Duration::from_millis(1))
            });
            let result = match per_address {
                Some(timeout) => TcpStream::connect_timeout(address, timeout),
                None => TcpStream::connect(address),
            };
            match result {
                Ok(socket) => {
                    socket.set_nodelay(details.config.no_delay())?;
                    if let Some(control) = &self.control {
                        control.attach(&socket)?;
                    }
                    return Ok(Some(TcpTransport {
                        socket,
                        buffers: LazyBuffers::new(
                            details.config.input_buffer_size(),
                            details.config.output_buffer_size(),
                        ),
                        idle: self.idle,
                    }));
                }
                Err(error) => last = io_error(error, details.timeout.reason),
            }
        }
        Err(last)
    }
}

pub(super) struct TcpTransport {
    socket: TcpStream,
    buffers: LazyBuffers,
    idle: Option<Duration>,
}

impl std::fmt::Debug for TcpTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TcpTransport").finish_non_exhaustive()
    }
}

impl Transport for TcpTransport {
    fn buffers(&mut self) -> &mut dyn Buffers {
        &mut self.buffers
    }

    fn transmit_output(&mut self, amount: usize, timeout: NextTimeout) -> Result<(), ureq::Error> {
        self.socket
            .set_write_timeout(timeout.not_zero().map(|d| *d))?;
        let output = self
            .buffers
            .output()
            .get(..amount)
            .ok_or(ureq::Error::ConnectionFailed)?;
        self.socket
            .write_all(output)
            .map_err(|e| io_error(e, timeout.reason))
    }

    fn await_input(&mut self, timeout: NextTimeout) -> Result<bool, ureq::Error> {
        let mut duration = timeout.not_zero().map(|d| *d);
        let mut reason = timeout.reason;
        if let Some(idle) = self.idle
            && duration.is_none_or(|d| idle < d)
        {
            duration = Some(idle.max(Duration::from_millis(1)));
            reason = ureq::Timeout::RecvBody;
        }
        self.socket.set_read_timeout(duration)?;
        let amount = self
            .socket
            .read(self.buffers.input_append_buf())
            .map_err(|e| io_error(e, reason))?;
        self.buffers.input_appended(amount);
        Ok(amount > 0)
    }

    fn is_open(&mut self) -> bool {
        // プールへ返した接続は、EOF と未消費の余分なデータがないときだけ再利用する。
        if self.socket.set_nonblocking(true).is_err() {
            return false;
        }
        let result = self.socket.peek(&mut [0]);
        let reset = self.socket.set_nonblocking(false);
        reset.is_ok() && matches!(result, Err(e) if e.kind() == io::ErrorKind::WouldBlock)
    }
}

fn io_error(error: io::Error, reason: ureq::Timeout) -> ureq::Error {
    if matches!(
        error.kind(),
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
    ) {
        ureq::Error::Timeout(reason)
    } else {
        error.into()
    }
}
