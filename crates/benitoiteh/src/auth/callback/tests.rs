//! accept の一時的な誤りの列と、その後の本物の接続を順に処理する。

use super::*;
use rustix::io::Errno;
use std::collections::VecDeque;

#[test]
fn rereview_transient_accept_errors_keep_waiting_for_a_connection()
-> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let client = TcpStream::connect(listener.local_addr()?)?;
    let mut results = VecDeque::new();
    for kind in [
        std::io::ErrorKind::WouldBlock,
        std::io::ErrorKind::Interrupted,
        std::io::ErrorKind::ConnectionAborted,
    ] {
        results.push_back(Err(std::io::Error::from(kind)));
    }
    for errno in [
        Errno::CONNABORTED,
        Errno::PROTO,
        Errno::NETDOWN,
        Errno::NOPROTOOPT,
        Errno::HOSTDOWN,
        Errno::HOSTUNREACH,
        Errno::OPNOTSUPP,
        Errno::NETUNREACH,
        Errno::MFILE,
        Errno::NFILE,
    ] {
        results.push_back(Err(std::io::Error::from(errno)));
    }
    #[cfg(target_os = "linux")]
    results.push_back(Err(std::io::Error::from(Errno::NONET)));
    let errors = results.len();
    results.push_back(listener.accept());
    let mut retries = 0usize;
    let mut received = None;
    while let Some(result) = results.pop_front() {
        match accepted(result)? {
            None => retries = retries.saturating_add(1usize),
            Some(socket) => {
                received = Some(socket);
                break;
            }
        }
    }
    assert_eq!(retries, errors);
    assert_eq!(
        received.ok_or("connection")?.peer_addr()?,
        client.local_addr()?
    );
    assert!(matches!(
        accepted(Err(std::io::Error::from(Errno::BADF))),
        Err(AuthError::Communication)
    ));
    Ok(())
}
