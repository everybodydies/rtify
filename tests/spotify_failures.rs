//! The Spotify client when things go wrong: rate limits, expired and revoked logins, a server
//! that is slow, gone or broken. A fake Web API and accounts service pick their answer from the
//! access (or refresh) token, so every case runs against the same server.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

use rtify::spotify::auth::Token;
use rtify::spotify::{ApiError, SpotifyClient};

struct Server {
    /// `METHOD path bearer` of every request, in order.
    log: Mutex<Vec<String>>,
}

fn server() -> &'static Server {
    static SERVER: OnceLock<&'static Server> = OnceLock::new();
    SERVER.get_or_init(|| {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        // SAFETY: set once, before the first client reads them; nothing else writes the environment.
        unsafe {
            std::env::set_var("RTIFY_API_BASE", format!("{base}/v1"));
            std::env::set_var("RTIFY_ACCOUNTS_BASE", &base);
        }
        let server: &'static Server = Box::leak(Box::new(Server { log: Mutex::default() }));
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                std::thread::spawn(move || handle(server, stream));
            }
        });
        server
    })
}

fn handle(server: &Server, mut stream: TcpStream) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() || line.is_empty() {
        return;
    }
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("").to_owned();
    let path = parts.next().unwrap_or("/").split('?').next().unwrap().to_owned();
    let (mut bearer, mut length) = (String::new(), 0usize);
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h).is_err() || h.trim().is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            if k.eq_ignore_ascii_case("authorization") {
                bearer = v.trim().trim_start_matches("Bearer ").to_owned();
            } else if k.eq_ignore_ascii_case("content-length") {
                length = v.trim().parse().unwrap_or(0);
            }
        }
    }
    let mut body = vec![0; length];
    let _ = reader.read_exact(&mut body);
    let body = String::from_utf8_lossy(&body).into_owned();
    server.log.lock().unwrap().push(format!("{method} {path} {bearer}"));

    let token = |access: &str, refresh: Option<&str>| {
        let mut t = serde_json::json!({"access_token": access, "token_type": "Bearer", "expires_in": 3600, "scope": ""});
        if let Some(r) = refresh {
            t["refresh_token"] = r.into();
        }
        (200, vec![], t.to_string())
    };
    let error = |status: u16, msg: &str| {
        (
            status,
            vec![],
            serde_json::json!({"error": {"status": status, "message": msg}}).to_string(),
        )
    };
    let (status, headers, text): (u16, Vec<(&str, &str)>, String) = if path == "/api/token" {
        let refresh = body.split('&').find_map(|kv| kv.strip_prefix("refresh_token=")).unwrap_or("");
        match refresh {
            "good" => token("fresh", Some("rotated")),
            "good-norotate" => token("fresh", None),
            "good-but-rejected" => token("rejected", None),
            "revoked" => (
                400,
                vec![],
                r#"{"error":"invalid_grant","error_description":"Refresh token revoked"}"#.into(),
            ),
            _ => (500, vec![], "{}".into()),
        }
    } else {
        match bearer.as_str() {
            "fresh" | "ok" => (
                200,
                vec![],
                r#"{"is_playing":true,"progress_ms":1000,"item":null,"device":null}"#.into(),
            ),
            "idle" => (204, vec![], String::new()),
            "slow-down" => (429, vec![("Retry-After", "7")], String::new()),
            "slow-down-bare" => (429, vec![], String::new()),
            b if b.starts_with("stale") => error(401, "The access token expired"),
            "broken" => error(500, "Server error"),
            "rejected" => error(401, "Invalid access token"),
            "garbage" => (200, vec![], "<html>not json".into()),
            "no-device" => error(404, "Player command failed: No active device found"),
            "premium" => error(403, "Player command failed: Premium required"),
            "scope" => error(401, "Insufficient client scope"),
            "hang-up" => return, // drop the connection without an answer
            "asleep" => {
                std::thread::sleep(Duration::from_secs(20));
                (200, vec![], "{}".into())
            }
            _ => error(400, "unexpected token"),
        }
    };
    let reason = match status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        429 => "Too Many Requests",
        _ => "Internal Server Error",
    };
    let mut head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",
        text.len()
    );
    for (k, v) in headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str("\r\n");
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(text.as_bytes());
}

fn runtime() -> &'static tokio::runtime::Runtime {
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RT.get_or_init(|| tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap())
}

/// Built like `App::startup` builds it.
fn http() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap()
}

/// A client signed in with `access`, and the refresh tokens its hook was handed.
fn client(access: &str, refresh: Option<&str>, expires_in: Duration) -> (SpotifyClient, Arc<Mutex<Vec<String>>>) {
    server();
    let rotated = Arc::new(Mutex::new(Vec::new()));
    let seen = rotated.clone();
    let c = SpotifyClient::new(http(), "test-client".into()).with_refresh_hook(Arc::new(move |r| seen.lock().unwrap().push(r)));
    let token = Token {
        access_token: access.into(),
        refresh_token: refresh.map(String::from),
        expires_at: SystemTime::now() + expires_in,
        scope: String::new(),
    };
    runtime().block_on(c.set_token(Some(token)));
    (c, rotated)
}

const HOUR: Duration = Duration::from_secs(3600);

fn poll(c: &SpotifyClient) -> Result<bool, ApiError> {
    runtime().block_on(c.player()).map(|p| p.is_some())
}

fn requests_with(bearer: &str) -> usize {
    server()
        .log
        .lock()
        .unwrap()
        .iter()
        .filter(|l| l.ends_with(&format!(" {bearer}")))
        .count()
}

#[test]
fn a_working_login_polls() {
    let (c, _) = client("ok", None, HOUR);
    assert!(poll(&c).unwrap());
    let (c, _) = client("idle", None, HOUR);
    assert!(!poll(&c).unwrap(), "204 means nothing is playing");
}

#[test]
fn rate_limits_report_how_long_to_wait() {
    let (c, _) = client("slow-down", None, HOUR);
    let before = requests_with("slow-down");
    assert!(matches!(poll(&c), Err(ApiError::RateLimited(7))));
    assert_eq!(requests_with("slow-down") - before, 1, "a 429 must not be retried on the spot");
    let (c, _) = client("slow-down-bare", None, HOUR);
    assert!(matches!(poll(&c), Err(ApiError::RateLimited(5))), "no Retry-After: wait 5 s");
}

#[test]
fn an_expired_access_token_is_refreshed_and_the_request_repeated() {
    let (c, rotated) = client("stale-once", Some("good"), HOUR);
    assert!(poll(&c).unwrap());
    assert_eq!(requests_with("stale-once"), 1);
    assert_eq!(
        *rotated.lock().unwrap(),
        ["rotated"],
        "the rotated refresh token is handed on for storage"
    );
    assert_eq!(runtime().block_on(c.refresh_token()).as_deref(), Some("rotated"));
    // From now on the fresh token is used directly.
    assert!(poll(&c).unwrap());
    assert_eq!(requests_with("stale-once"), 1);
}

#[test]
fn a_token_about_to_expire_is_refreshed_before_the_request() {
    let (c, rotated) = client("stale-soon", Some("good-norotate"), Duration::from_secs(30));
    assert!(poll(&c).unwrap());
    assert_eq!(requests_with("stale-soon"), 0, "no request with the expiring token");
    assert!(
        rotated.lock().unwrap().is_empty(),
        "nothing to store when Spotify keeps the refresh token"
    );
    assert_eq!(runtime().block_on(c.refresh_token()).as_deref(), Some("good-norotate"));
}

#[test]
fn a_revoked_login_asks_to_sign_in_again() {
    let (c, _) = client("stale", Some("revoked"), HOUR);
    let e = poll(&c).unwrap_err();
    assert!(e.is_signed_out(), "{e:?}");
    assert!(!e.is_offline(), "{e:?}");
}

#[test]
fn an_expired_token_without_refresh_token_asks_to_sign_in_again() {
    let (c, _) = client("stale", None, HOUR);
    let e = poll(&c).unwrap_err();
    assert!(e.is_signed_out(), "{e:?}");
    // Signed out entirely.
    let c = SpotifyClient::new(http(), "test-client".into());
    assert!(poll(&c).unwrap_err().is_signed_out());
}

#[test]
fn a_token_spotify_keeps_rejecting_asks_to_sign_in_again() {
    let (c, _) = client("stale-twice", Some("good-but-rejected"), HOUR);
    let e = poll(&c).unwrap_err();
    assert!(e.is_signed_out(), "{e:?}");
    assert!(e.to_string().contains("Invalid access token"), "Spotify's reason is kept: {e}");
}

#[test]
fn a_missing_permission_is_not_mistaken_for_an_expired_login() {
    let (c, rotated) = client("scope", Some("good"), HOUR);
    let e = runtime().block_on(c.pause()).unwrap_err();
    assert!(e.is_scope() && !e.is_signed_out(), "{e:?}");
    assert!(rotated.lock().unwrap().is_empty(), "no pointless refresh");
}

#[test]
fn server_errors_are_neither_offline_nor_signed_out() {
    for bearer in ["broken", "garbage"] {
        let (c, _) = client(bearer, None, HOUR);
        let e = poll(&c).unwrap_err();
        assert!(!e.is_signed_out() && !e.is_offline(), "{bearer}: {e:?}");
    }
}

#[test]
fn player_errors_are_recognised() {
    type Check = fn(&ApiError) -> bool;
    let cases: [(&str, Check); 3] = [
        ("no-device", ApiError::is_no_device),
        ("premium", ApiError::needs_premium),
        ("scope", ApiError::is_scope),
    ];
    for (bearer, check) in cases {
        let (c, _) = client(bearer, None, HOUR);
        let e = runtime().block_on(c.pause()).unwrap_err();
        assert!(check(&e), "{bearer}: {e:?}");
        assert!(!e.is_signed_out(), "{bearer}: {e:?}");
    }
}

#[test]
fn a_dropped_connection_counts_as_offline() {
    let (c, _) = client("hang-up", None, HOUR);
    let e = poll(&c).unwrap_err();
    assert!(e.is_offline(), "{e:?}");
    assert!(!e.is_signed_out());
}

#[test]
fn an_unreachable_server_counts_as_offline() {
    // A port nobody listens on.
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let url = format!("http://127.0.0.1:{port}/v1/me/player");
    let e = runtime().block_on(async { http().get(url).send().await }).unwrap_err();
    assert!(ApiError::Http(e).is_offline());
}

#[test]
fn a_server_that_never_answers_gives_up_within_15_seconds() {
    let (c, _) = client("asleep", None, HOUR);
    let start = Instant::now();
    let e = poll(&c).unwrap_err();
    assert!(start.elapsed() < Duration::from_secs(17), "took {:?}", start.elapsed());
    assert!(e.is_offline(), "{e:?}");
}

#[test]
fn concurrent_requests_with_an_expired_token_refresh_once() {
    let (c, rotated) = client("stale", Some("good"), Duration::from_secs(10));
    runtime().block_on(async {
        let polls: Vec<_> = (0..8)
            .map(|_| {
                let c = c.clone();
                tokio::spawn(async move { c.player().await })
            })
            .collect();
        for p in polls {
            assert!(p.await.unwrap().is_ok());
        }
    });
    assert_eq!(rotated.lock().unwrap().len(), 1, "one refresh for eight requests");
}
