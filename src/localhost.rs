//! A tiny HTTP/1.1 server for the two pages the app serves to itself on 127.0.0.1: the
//! Apple Music sign-in page and the hidden MusicKit player. One request per connection.

use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use anyhow::{Result, anyhow};

/// Largest request body accepted (tokens and player state are a few KB).
const MAX_BODY: usize = 64 * 1024;

pub struct Request {
    pub method: String,
    pub path: String,
    pub query: String,
    pub body: Vec<u8>,
}

pub fn read_request(stream: &TcpStream) -> Result<Request> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 || line == "\r\n" {
            break;
        }
        if let Some((name, value)) = line.split_once(':')
            && name.eq_ignore_ascii_case("content-length")
        {
            content_length = value.trim().parse().unwrap_or(0);
        }
    }
    if content_length > MAX_BODY {
        return Err(anyhow!("request body too large"));
    }
    let mut parts = request_line.split_whitespace();
    let (method, target) = (parts.next().unwrap_or("").to_string(), parts.next().unwrap_or(""));
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let mut body = vec![0; content_length];
    reader.read_exact(&mut body)?;
    Ok(Request { method, path: path.to_string(), query: query.to_string(), body })
}

pub fn respond(mut stream: &TcpStream, status: &str, content_type: &str, body: &str) -> Result<()> {
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )?;
    Ok(())
}

pub fn random_hex(bytes: usize) -> Result<String> {
    let mut buf = vec![0u8; bytes];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut buf))
        .map_err(|e| anyhow!("reading /dev/urandom: {e}"))?;
    Ok(buf.iter().map(|b| format!("{b:02x}")).collect())
}

/// Accepts connections until `stop` is set or `handle` returns `Some`. A connection that
/// fails (browsers open speculative sockets that never send anything) is dropped without
/// ending the server.
pub fn serve<T>(
    listener: &TcpListener,
    stop: &AtomicBool,
    mut handle: impl FnMut(&TcpStream, Request) -> Result<Option<T>>,
) -> Option<T> {
    listener.set_nonblocking(true).ok()?;
    while !stop.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _)) => match read_request(&stream).and_then(|req| handle(&stream, req)) {
                Ok(Some(done)) => return Some(done),
                Ok(None) => {}
                Err(e) => eprintln!("localhost: dropped a connection: {e:#}"),
            },
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(40)),
            Err(e) => {
                eprintln!("localhost: accept failed: {e}");
                std::thread::sleep(Duration::from_millis(200));
            }
        }
    }
    None
}

/// A flag to stop a `serve` loop from another thread.
pub type StopFlag = Arc<AtomicBool>;

pub fn stop_flag() -> StopFlag {
    Arc::new(AtomicBool::new(false))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serve_survives_bad_connections_and_stops() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let stop = stop_flag();
        let server_stop = stop.clone();
        let server = std::thread::spawn(move || {
            serve(&listener, &server_stop, |s, req| {
                respond(s, "200 OK", "text/plain", "ok")?;
                Ok((req.path == "/done").then_some(req.query))
            })
        });
        // A socket that connects and hangs up without a request must not end the server.
        drop(TcpStream::connect(("127.0.0.1", port)).unwrap());
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.write_all(b"GET /done?x=1 HTTP/1.1\r\n\r\n").unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        assert!(out.starts_with("HTTP/1.1 200"));
        assert_eq!(server.join().unwrap().as_deref(), Some("x=1"));

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let stop2 = stop.clone();
        stop.store(true, Ordering::Relaxed);
        assert!(serve::<()>(&listener, &stop2, |_, _| Ok(None)).is_none());
    }
}
