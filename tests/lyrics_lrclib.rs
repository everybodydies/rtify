//! Lyrics end to end: a fake LRCLIB answers the lookups rtify makes, and the documents that come
//! back are walked with the same cursor the lyrics view uses.
//!
//! Every case LRCLIB really produces is covered: synced, plain only, instrumental, nothing,
//! broken answers, server errors and a server that never answers.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use rtify::lyrics::TimingMode;
use rtify::lyrics::lrclib::{self, Found};
use rtify::lyrics::provider::TrackQuery;
use rtify::lyrics::synchronization::cursor;

/// How long a lookup may take before the view should give up on "searching lyrics…".
const LOOKUP_BUDGET: Duration = Duration::from_secs(15);

const SYNCED: &str = "[ti:Neon Rain]\n[ar:Neon Harbor]\n[offset:+250]\n\
[00:01.00] Neonlicht im Regen\n[00:05.50] die Stadt schläft nie\n[00:09.00]\n\
[00:14.00] wir fahren durch die Nacht\n[00:18.25] Radio rauscht leise\n";

#[derive(Debug, Clone)]
struct Request {
    path: String,
    query: HashMap<String, String>,
    client_header: Option<String>,
}

struct Server {
    base: String,
    log: Mutex<Vec<Request>>,
}

fn server() -> &'static Server {
    static SERVER: OnceLock<&'static Server> = OnceLock::new();
    SERVER.get_or_init(|| {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        // SAFETY: set once, before the first lookup reads it; nothing else writes the environment.
        unsafe { std::env::set_var("RTIFY_LRCLIB_BASE", &base) };
        let server: &'static Server = Box::leak(Box::new(Server {
            base,
            log: Mutex::default(),
        }));
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                std::thread::spawn(move || handle(server, stream));
            }
        });
        server
    })
}

fn decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                out.push(u8::from_str_radix(&s[i + 1..i + 3], 16).unwrap());
                i += 2;
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8(out).unwrap()
}

fn handle(server: &Server, stream: TcpStream) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() || line.is_empty() {
        return;
    }
    let target = line.split_whitespace().nth(1).unwrap_or("/").to_owned();
    let mut client_header = None;
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h).is_err() || h.trim().is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':')
            && k.eq_ignore_ascii_case("lrclib-client")
        {
            client_header = Some(v.trim().to_owned());
        }
    }
    let (path, qs) = target.split_once('?').unwrap_or((&target, ""));
    let query: HashMap<String, String> = qs
        .split('&')
        .filter_map(|kv| kv.split_once('='))
        .map(|(k, v)| (decode(k), decode(v)))
        .collect();
    let track = query.get("track_name").cloned().unwrap_or_default();
    server.log.lock().unwrap().push(Request {
        path: path.to_owned(),
        query,
        client_header,
    });

    let json = |v: serde_json::Value| (200, v.to_string());
    let (status, body) = match (track.as_str(), path) {
        (t, "/api/get") if t.starts_with("Synced Song") => json(serde_json::json!({"id": 7, "syncedLyrics": SYNCED, "plainLyrics": "x"})),
        ("Plain Only", "/api/get") => (404, r#"{"code":404}"#.into()),
        ("Plain Only", "/api/search") => json(serde_json::json!([{"id": 8, "plainLyrics": "line one\nline two"}])),
        ("Instrumental Piece", "/api/get") => json(serde_json::json!({"id": 9, "instrumental": true})),
        ("Prefers Synced", "/api/get") => (404, "{}".into()),
        ("Prefers Synced", "/api/search") => json(serde_json::json!([
            {"id": 10, "plainLyrics": "plain first"},
            {"id": 11, "syncedLyrics": "[00:02.00] synced second"}
        ])),
        ("Empty Lyrics", "/api/get") => json(serde_json::json!({"id": 12, "syncedLyrics": "  ", "plainLyrics": ""})),
        ("Empty Lyrics", "/api/search") => json(serde_json::json!([])),
        ("Broken", "/api/get") => (200, "{not json".into()),
        ("Broken", "/api/search") => (500, "Internal Server Error".into()),
        ("Stall", _) => {
            // Accept the connection, never answer.
            std::thread::sleep(Duration::from_secs(120));
            return;
        }
        (_, "/api/get") => (404, "{}".into()),
        _ => json(serde_json::json!([])),
    };
    let mut stream = stream;
    let _ = write!(
        stream,
        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.flush();
    let _ = reader.read_to_end(&mut Vec::new());
}

fn query(title: &str) -> TrackQuery {
    TrackQuery {
        id: format!("id-{}", title.replace(' ', "-")),
        title: title.into(),
        artists: vec!["Neon Harbor".into(), "Mira Vale".into()],
        album: "Midnight Circuits".into(),
        duration_ms: 215_600,
    }
}

/// The client exactly as the app builds it (see `App::startup`).
fn http() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(concat!("rtify/", env!("CARGO_PKG_VERSION")))
        .build()
        .unwrap()
}

async fn lookup(title: &str) -> Result<Found, String> {
    server();
    match tokio::time::timeout(LOOKUP_BUDGET + Duration::from_secs(5), lrclib::fetch(&http(), &query(title))).await {
        Ok(r) => r.map_err(|e| e.to_string()),
        Err(_) => Err(format!(
            "lookup for {title:?} still running after {:?}",
            LOOKUP_BUDGET + Duration::from_secs(5)
        )),
    }
}

fn requests_for(title: &str) -> Vec<Request> {
    server()
        .log
        .lock()
        .unwrap()
        .iter()
        .filter(|r| r.query.get("track_name").map(String::as_str) == Some(title))
        .cloned()
        .collect()
}

#[tokio::test]
async fn synced_lyrics_from_exact_lookup() {
    let Ok(Found::Lyrics(doc)) = lookup("Synced Song").await else {
        panic!("expected lyrics")
    };
    assert_eq!(doc.timing, TimingMode::Line);
    assert_eq!(doc.attribution.source, "LRCLIB");
    assert_eq!(doc.attribution.source_id, "lrclib:7");
    assert_eq!(doc.track_key, "id-Synced-Song");
    let texts: Vec<&str> = doc.lines.iter().map(|l| l.original.as_str()).collect();
    assert!(
        texts.contains(&"Neonlicht im Regen") && texts.contains(&"Radio rauscht leise"),
        "{texts:?}"
    );

    let reqs = requests_for("Synced Song");
    assert_eq!(reqs.len(), 1, "an exact hit needs no search: {reqs:?}");
    let q = &reqs[0].query;
    assert_eq!(reqs[0].path, "/api/get");
    assert_eq!(q["artist_name"], "Neon Harbor", "LRCLIB matches the primary artist");
    assert_eq!(q["album_name"], "Midnight Circuits");
    assert_eq!(q["duration"], "215", "duration goes out in whole seconds");
    assert!(
        reqs[0].client_header.as_deref().is_some_and(|h| h.starts_with("rtify/")),
        "LRCLIB asks clients to identify"
    );
}

#[tokio::test]
async fn plain_lyrics_from_search_fallback() {
    let Ok(Found::Lyrics(doc)) = lookup("Plain Only").await else {
        panic!("expected lyrics")
    };
    assert_eq!(doc.timing, TimingMode::Static);
    assert_eq!(doc.lines.len(), 2);
    assert_eq!(cursor(&doc, 10_000).line, None, "static lyrics never highlight a line");
    let paths: Vec<String> = requests_for("Plain Only").into_iter().map(|r| r.path).collect();
    assert_eq!(paths, ["/api/get", "/api/search"]);
}

#[tokio::test]
async fn search_prefers_synced_hits() {
    let Ok(Found::Lyrics(doc)) = lookup("Prefers Synced").await else {
        panic!("expected lyrics")
    };
    assert_eq!(doc.attribution.source_id, "lrclib:11");
    assert_eq!(doc.timing, TimingMode::Line);
}

#[tokio::test]
async fn instrumental_is_reported_as_such() {
    assert!(matches!(lookup("Instrumental Piece").await, Ok(Found::Instrumental)));
}

#[tokio::test]
async fn blank_lyrics_and_no_hits_are_missing() {
    assert!(matches!(lookup("Empty Lyrics").await, Ok(Found::Missing)));
    assert!(matches!(lookup("Unknown Track").await, Ok(Found::Missing)));
}

#[tokio::test]
async fn broken_answers_and_server_errors_are_missing_not_errors() {
    assert!(matches!(lookup("Broken").await, Ok(Found::Missing)));
}

#[tokio::test]
async fn titles_with_unicode_and_reserved_characters_arrive_intact() {
    let title = "夜の海 & Co. #1 (feat. Ä/Ö) 100%";
    let _ = lookup(title).await;
    let reqs = requests_for(title);
    assert!(!reqs.is_empty(), "the server saw no request with the exact title");
    assert_eq!(reqs[0].query["track_name"], title);
}

/// A stalled LRCLIB must not leave the view on "searching lyrics…" forever.
#[tokio::test]
async fn a_stalled_server_does_not_hang_the_lookup() {
    let started = std::time::Instant::now();
    let result = lookup("Stall").await;
    let took = started.elapsed();
    assert!(
        took <= LOOKUP_BUDGET,
        "lookup against a server that never answers took {took:?} (budget {LOOKUP_BUDGET:?}): {result:?}"
    );
}

/// Walk the document like the view does: the right line lights up at its timestamp (with the
/// LRC offset applied), gaps point at the next line, and seeking back works.
#[tokio::test]
async fn highlighted_line_follows_playback() {
    let Ok(Found::Lyrics(doc)) = lookup("Synced Song (walk)").await else {
        panic!("expected lyrics")
    };
    let line_at = |text: &str| doc.lines.iter().position(|l| l.original == text).unwrap();
    // [offset:+250] shifts every line 250 ms earlier.
    let first = doc.lines[line_at("Neonlicht im Regen")].start_ms.unwrap();
    assert_eq!(first, 750);

    let intro = cursor(&doc, 0);
    assert_eq!(
        (intro.line, intro.active),
        (Some(line_at("Neonlicht im Regen")), false),
        "intro waits for line 1"
    );

    for (pos, text) in [
        (750, "Neonlicht im Regen"),
        (5_249, "Neonlicht im Regen"),
        (5_250, "die Stadt schläft nie"),
        (13_750, "wir fahren durch die Nacht"),
        (18_000, "Radio rauscht leise"),
    ] {
        let c = cursor(&doc, pos);
        assert_eq!(c.line, Some(line_at(text)), "at {pos} ms");
        assert!(c.active, "at {pos} ms {text:?} is being sung");
        assert!((0.0..=1.0).contains(&c.line_progress));
    }

    // The empty [00:09.00] line is an instrumental gap: nothing is sung, the next line is up.
    let gap = cursor(&doc, 10_000);
    assert!(
        !gap.active || doc.lines[gap.line.unwrap()].original.is_empty(),
        "gap at 10 s: {gap:?}"
    );

    // Seeking backwards lands on the earlier line again.
    assert_eq!(cursor(&doc, 20_000).line, Some(line_at("Radio rauscht leise")));
    assert_eq!(cursor(&doc, 2_000).line, Some(line_at("Neonlicht im Regen")));

    // Positions far past the end stay on the last line instead of panicking or wrapping.
    let end = cursor(&doc, 10 * 60 * 1000);
    assert_eq!(end.line, Some(doc.lines.len() - 1));
}

#[test]
fn server_is_reachable() {
    let base = &server().base;
    assert!(base.starts_with("http://127.0.0.1:"));
}
