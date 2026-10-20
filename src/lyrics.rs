//! Time-synced lyrics from LRCLIB (lrclib.net), a free, open lyrics database. Apple doesn't
//! give third-party apps its own lyrics.

use anyhow::Result;
use serde_json::Value;

#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    /// Seconds into the song; `None` for unsynced lyrics.
    pub at: Option<f32>,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Lyrics {
    pub lines: Vec<Line>,
    pub synced: bool,
}

impl Lyrics {
    /// The line being sung at `t` seconds (synced lyrics only).
    pub fn current(&self, t: f32) -> Option<usize> {
        if !self.synced {
            return None;
        }
        self.lines.iter().rposition(|l| l.at.is_some_and(|at| at <= t + 0.15))
    }
}

/// Parses LRC: `[mm:ss.xx] text`, possibly several timestamps per line.
pub fn parse_lrc(lrc: &str) -> Vec<Line> {
    let mut lines = Vec::new();
    for raw in lrc.lines() {
        let mut rest = raw.trim();
        let mut stamps = Vec::new();
        while let Some(stripped) = rest.strip_prefix('[') {
            let Some(end) = stripped.find(']') else { break };
            let tag = &stripped[..end];
            if let Some((m, s)) = tag.split_once(':')
                && let (Ok(m), Ok(s)) = (m.parse::<f32>(), s.parse::<f32>())
            {
                stamps.push(m * 60. + s);
            }
            rest = stripped[end + 1..].trim_start();
        }
        for at in stamps {
            lines.push(Line { at: Some(at), text: rest.to_string() });
        }
    }
    lines.sort_by(|a, b| a.at.partial_cmp(&b.at).unwrap_or(std::cmp::Ordering::Equal));
    lines
}

fn from_json(v: &Value) -> Option<Lyrics> {
    if let Some(synced) = v["syncedLyrics"].as_str().filter(|s| !s.trim().is_empty()) {
        let lines = parse_lrc(synced);
        if !lines.is_empty() {
            return Some(Lyrics { lines, synced: true });
        }
    }
    let plain = v["plainLyrics"].as_str().filter(|s| !s.trim().is_empty())?;
    Some(Lyrics {
        lines: plain.lines().map(|l| Line { at: None, text: l.trim().to_string() }).collect(),
        synced: false,
    })
}

fn get(url: &str) -> Result<Option<Value>> {
    // LRCLIB answers 404 for "no lyrics", which shouldn't be an error.
    static AGENT: std::sync::LazyLock<ureq::Agent> = std::sync::LazyLock::new(|| {
        ureq::Agent::config_builder()
            .timeout_global(Some(std::time::Duration::from_secs(10)))
            .http_status_as_error(false)
            .build()
            .into()
    });
    let mut resp = AGENT.get(url).header("User-Agent", USER_AGENT).call()?;
    if resp.status() == 404 {
        return Ok(None);
    }
    Ok(Some(resp.body_mut().read_json()?))
}

/// Strips "(feat. …)" and " - Single"-style suffixes that LRCLIB entries usually lack.
fn clean_title(title: &str) -> &str {
    let mut t = title;
    for marker in [" (feat.", " (with ", " [feat.", " - "] {
        if let Some(i) = t.find(marker) {
            t = &t[..i];
        }
    }
    t.trim()
}

/// LRCLIB asks clients to identify themselves.
const USER_AGENT: &str = concat!("Encore/", env!("CARGO_PKG_VERSION"), " (https://github.com/cpreston321/encore)");

pub fn fetch(title: &str, artist: &str, album: &str, duration_secs: f32) -> Result<Option<Lyrics>> {
    let primary_artist = artist.split([',', '&']).next().unwrap_or(artist).trim();
    let exact = format!(
        "https://lrclib.net/api/get?track_name={}&artist_name={}&album_name={}&duration={}",
        crate::api::query_escape(title),
        crate::api::query_escape(primary_artist),
        crate::api::query_escape(album),
        duration_secs.round() as u32
    );
    if let Some(v) = get(&exact)?
        && let Some(l) = from_json(&v)
    {
        return Ok(Some(l));
    }
    // Fuzzy fallback: search, prefer synced results with a close duration.
    let search = format!(
        "https://lrclib.net/api/search?track_name={}&artist_name={}",
        crate::api::query_escape(clean_title(title)),
        crate::api::query_escape(primary_artist)
    );
    let Some(Value::Array(results)) = get(&search)? else {
        return Ok(None);
    };
    let best = results
        .iter()
        .filter(|r| (r["duration"].as_f64().unwrap_or(0.) as f32 - duration_secs).abs() < 8. || duration_secs <= 0.)
        .max_by_key(|r| r["syncedLyrics"].as_str().is_some_and(|s| !s.is_empty()) as u8);
    Ok(best.and_then(from_json))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lrc_lines() {
        let lines = parse_lrc("[00:32.88] Hello there\n[01:02.50][02:10.00] Chorus\n[ar:Someone]\n");
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], Line { at: Some(32.88), text: "Hello there".into() });
        assert_eq!(lines[1].at, Some(62.5));
        assert_eq!(lines[2].text, "Chorus");
        let l = Lyrics { lines, synced: true };
        assert_eq!(l.current(10.), None);
        assert_eq!(l.current(70.), Some(1));
    }

    #[test]
    fn cleans_titles() {
        assert_eq!(clean_title("Lies (feat. Lana Del Rey)"), "Lies");
        assert_eq!(clean_title("pop it - Single"), "pop it");
    }

    #[test]
    #[ignore]
    fn live_lrclib() {
        let l = fetch("Something in the Orange", "Zach Bryan", "American Heartbreak", 228.).unwrap().unwrap();
        assert!(l.synced && l.lines.len() > 10);
        let f = fetch("Lies (feat. Lana Del Rey)", "Zach Bryan & Lana Del Rey", "x", 220.).unwrap();
        println!("lies: {:?}", f.map(|l| l.lines.len()));
    }
}
