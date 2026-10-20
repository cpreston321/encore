//! Apple Music sign-in. Apple only issues Music User Tokens through MusicKit JS's
//! `authorize()`, so the app serves a one-page sign-in site on localhost, opens it in the
//! browser, and waits for the page to post the token back.

use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    time::{Duration, Instant},
};

use anyhow::{Context as _, Result, anyhow, bail};

const KEYCHAIN_SERVICE: &str = "GPUI Music";
const KEYCHAIN_ACCOUNT: &str = "music-user-token";
const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(10 * 60);

pub fn stored_user_token() -> Option<String> {
    keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT)
        .ok()?
        .get_password()
        .ok()
        .filter(|t| !t.is_empty())
}

pub fn store_user_token(token: &str) -> Result<()> {
    keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT)?
        .set_password(token)
        .context("saving to Keychain")
}

pub fn clear_user_token() {
    if let Ok(entry) = keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT) {
        let _ = entry.delete_credential();
    }
}

pub struct SignInServer {
    listener: TcpListener,
    page: String,
    state: String,
    pub url: String,
}

impl SignInServer {
    pub fn start(developer_token: &str) -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").context("starting sign-in server")?;
        listener.set_nonblocking(true)?;
        let port = listener.local_addr()?.port();
        let state = random_hex(16)?;
        let page = PAGE
            .replace("__DEVELOPER_TOKEN__", &serde_json::to_string(developer_token)?)
            .replace("__STATE__", &state);
        Ok(Self {
            listener,
            page,
            state,
            url: format!("http://localhost:{port}/"),
        })
    }

    /// Serves the sign-in page until it posts back a token, or the timeout passes.
    pub fn wait_for_token(self) -> Result<String> {
        let deadline = Instant::now() + SIGN_IN_TIMEOUT;
        while Instant::now() < deadline {
            match self.listener.accept() {
                Ok((stream, _)) => {
                    if let Some(token) = self.handle(stream)? {
                        return Ok(token);
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(100));
                }
                Err(e) => return Err(e.into()),
            }
        }
        bail!("sign-in timed out")
    }

    fn handle(&self, mut stream: TcpStream) -> Result<Option<String>> {
        stream.set_nonblocking(false)?;
        stream.set_read_timeout(Some(Duration::from_secs(10)))?;
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
        let mut parts = request_line.split_whitespace();
        let (method, path) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));

        match (method, path) {
            ("GET", "/") => {
                respond(&mut stream, "200 OK", "text/html; charset=utf-8", &self.page)?;
                Ok(None)
            }
            ("POST", p) if p == format!("/token?state={}", self.state) => {
                if content_length == 0 || content_length > 16 * 1024 {
                    respond(&mut stream, "400 Bad Request", "text/plain", "bad token")?;
                    return Ok(None);
                }
                let mut body = vec![0; content_length];
                reader.read_exact(&mut body)?;
                let token = String::from_utf8(body)?.trim().to_string();
                respond(&mut stream, "200 OK", "text/plain", "ok")?;
                Ok(Some(token))
            }
            _ => {
                respond(&mut stream, "404 Not Found", "text/plain", "not found")?;
                Ok(None)
            }
        }
    }
}

fn respond(stream: &mut TcpStream, status: &str, content_type: &str, body: &str) -> Result<()> {
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )?;
    Ok(())
}

fn random_hex(bytes: usize) -> Result<String> {
    let mut buf = vec![0u8; bytes];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut buf))
        .map_err(|e| anyhow!("reading /dev/urandom: {e}"))?;
    Ok(buf.iter().map(|b| format!("{b:02x}")).collect())
}

const PAGE: &str = r#"<!doctype html>
<html>
<head>
<meta charset="utf-8">
<title>Sign in to Apple Music · Encore</title>
<script src="https://js-cdn.music.apple.com/musickit/v3/musickit.js" async></script>
<style>
  :root { color-scheme: dark light; }
  body { margin: 0; height: 100vh; display: flex; align-items: center; justify-content: center;
         font: 15px -apple-system, BlinkMacSystemFont, sans-serif; background: #1e1e1e; color: #f5f5f7; }
  @media (prefers-color-scheme: light) { body { background: #fff; color: #1d1d1f; } }
  .card { text-align: center; max-width: 420px; padding: 24px; }
  h1 { font-size: 26px; margin: 0 0 8px; }
  p { color: #8e8e93; line-height: 1.45; }
  button { margin-top: 12px; background: #fa2d48; color: #fff; border: 0; border-radius: 999px;
           padding: 12px 30px; font: 600 15px -apple-system, sans-serif; cursor: pointer; }
  button:disabled { opacity: .5; cursor: default; }
</style>
</head>
<body>
<div class="card">
  <h1>Sign in to Apple Music</h1>
  <p id="msg">Encore will be able to read your library and add music to it. Your Apple ID password goes only to Apple.</p>
  <button id="go" disabled>Sign In with Apple Music</button>
</div>
<script>
  const msg = document.getElementById('msg');
  const go = document.getElementById('go');
  document.addEventListener('musickitloaded', async () => {
    try {
      const music = await MusicKit.configure({
        developerToken: __DEVELOPER_TOKEN__,
        app: { name: 'Encore', build: '1.0.0' },
      });
      go.disabled = false;
      go.onclick = async () => {
        go.disabled = true;
        try {
          const token = await music.authorize();
          const res = await fetch('/token?state=__STATE__', { method: 'POST', body: token });
          if (!res.ok) throw new Error('the app did not accept the token');
          msg.textContent = "You're signed in. You can close this tab and go back to Encore.";
          go.remove();
        } catch (e) {
          msg.textContent = 'Sign-in failed: ' + (e && e.message || e);
          go.disabled = false;
        }
      };
    } catch (e) {
      msg.textContent = 'MusicKit could not start. Check the developer token: ' + (e && e.message || e);
    }
  });
</script>
</body>
</html>
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn http(port: u16, raw: &str) -> String {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream.write_all(raw.as_bytes()).unwrap();
        let mut out = String::new();
        stream.read_to_string(&mut out).unwrap();
        out
    }

    #[test]
    fn serves_page_and_accepts_token_only_with_state() {
        let server = SignInServer::start("dev.token.value").unwrap();
        let port: u16 = server.url.trim_end_matches('/').rsplit(':').next().unwrap().parse().unwrap();
        let state = server.state.clone();
        let waiter = std::thread::spawn(move || server.wait_for_token());

        let page = http(port, "GET / HTTP/1.1\r\nHost: localhost\r\n\r\n");
        assert!(page.starts_with("HTTP/1.1 200"));
        assert!(page.contains("\"dev.token.value\""));
        assert!(page.contains(&state));

        let forged = http(port, "POST /token?state=wrong HTTP/1.1\r\nContent-Length: 4\r\n\r\nevil");
        assert!(forged.starts_with("HTTP/1.1 404"));

        let ok = http(
            port,
            &format!("POST /token?state={state} HTTP/1.1\r\nContent-Length: 10\r\n\r\nuser-token"),
        );
        assert!(ok.starts_with("HTTP/1.1 200"));
        assert_eq!(waiter.join().unwrap().unwrap(), "user-token");
    }
}
