//! Robustness of everything that parses outside data: LRC and TTML from LRCLIB, and Spotify JSON.
//!
//! A seeded generator mutates real-looking inputs thousands of times; the parsers may reject
//! input, but never panic, never hang, and never hand the lyrics view a document it cannot walk.
//! A failure prints the seed and the input so it can be turned into a unit test.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::{Duration, Instant};

use rtify::lyrics::model::LyricDocument;
use rtify::lyrics::synchronization::{cursor, line_progress, word_progress};
use rtify::lyrics::{TimingMode, lrc, provider, romanization, ttml};
use rtify::spotify::model::{Devices, Page, PlayerState, Playlist, PlaylistItem, Queue, SavedTrack, SearchResult, Track, pick_image};

const ITERATIONS: usize = 3000;
const SEED: u64 = 0x5eed_2026_1023;

/// xorshift64*: deterministic, so a failure reproduces with the printed seed.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

/// Fragments that tend to break hand-written parsers.
const NASTY: &[&str] = &[
    "[",
    "]",
    ":",
    ".",
    "<",
    ">",
    "/",
    "\"",
    "'",
    "&",
    ";",
    "\0",
    "\u{feff}",
    "\r",
    "\n",
    "\r\n",
    "\t",
    " ",
    "[00:00.00]",
    "[99:59.999]",
    "[999999999999:00.00]",
    "[-1:00.00]",
    "[00:-5.00]",
    "[1:2:3]",
    "[00:00.00.00]",
    "<00:01.00>",
    "<99:99.99>",
    "[offset:+999999999999]",
    "[offset:-99999999999]",
    "[offset:abc]",
    "[ti:",
    "[ar:]",
    "<tt>",
    "</tt>",
    "<p begin=\"\" end=\"\">",
    "<p begin=\"99:99:99.999\" end=\"0\">",
    "<span begin=\"1s\">",
    "</span>",
    "</p>",
    "<?xml version=\"1.0\"?>",
    "<![CDATA[",
    "]]>",
    "&amp;",
    "&#0;",
    "&#xFFFFFF;",
    "&bogus;",
    "<!DOCTYPE x [<!ENTITY a \"aaaaaaaa\">]>",
    "<metadata>",
    "<translation xml:lang=\"de\">",
    "ttm:agent=\"v1\"",
    "夜",
    "の",
    "海",
    "한",
    "글",
    "Ж",
    "é",
    "e\u{301}",
    "🎵",
    "\u{200b}",
    "\u{202e}",
    "\u{fffd}",
    "ｶﾀｶﾅ",
    "ー",
    "ッ",
    "ん",
    "っ",
    "999999999999999999999",
    "1e400",
    "-0",
    "NaN",
];

fn mutate(rng: &mut Rng, input: &str) -> String {
    let mut chars: Vec<char> = input.chars().collect();
    for _ in 0..=rng.below(6) {
        let at = rng.below(chars.len() + 1);
        match rng.below(7) {
            0 if !chars.is_empty() => {
                // Delete a run.
                let end = (at + rng.below(12)).min(chars.len());
                chars.drain(at.min(end)..end);
            }
            1 => {
                let frag: Vec<char> = rng.pick(NASTY).chars().collect();
                chars.splice(at..at, frag);
            }
            2 if !chars.is_empty() => {
                // Duplicate a run (repeated tags, doubled timestamps).
                let start = at.min(chars.len() - 1);
                let end = (start + rng.below(40)).min(chars.len());
                let run: Vec<char> = chars[start..end].to_vec();
                chars.splice(start..start, run);
            }
            3 => chars.truncate(at),
            4 if !chars.is_empty() => {
                let i = at.min(chars.len() - 1);
                chars[i] = char::from_u32(rng.next() as u32 % 0x3000).unwrap_or('\u{fffd}');
            }
            5 if !chars.is_empty() => {
                let i = at.min(chars.len() - 1);
                chars[i] = *rng.pick(&['0', '9', ':', '.', '[', ']', '<', '>', '"', '-', '+']);
            }
            _ => {
                let frag: String = (0..rng.below(8)).map(|_| *rng.pick(NASTY)).collect();
                let frag: Vec<char> = frag.chars().collect();
                chars.splice(at..at, frag);
            }
        }
    }
    chars.into_iter().collect()
}

fn random_text(rng: &mut Rng) -> String {
    (0..rng.below(60)).map(|_| *rng.pick(NASTY)).collect()
}

fn demo(name: &str) -> String {
    std::fs::read_to_string(format!("{}/data/demo/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

/// The document invariants the lyrics view relies on.
fn check_document(doc: &LyricDocument) -> Result<(), String> {
    if doc.timing != TimingMode::Static {
        let starts: Vec<u64> = doc.lines.iter().filter_map(|l| l.start_ms).collect();
        if starts.windows(2).any(|w| w[0] > w[1]) {
            return Err(format!("timed lines out of order: {starts:?}"));
        }
    }
    for (i, line) in doc.lines.iter().enumerate() {
        if let (Some(s), Some(e)) = (line.start_ms, line.end_ms)
            && e < s
        {
            return Err(format!("line {i} ends ({e}) before it starts ({s})"));
        }
    }
    let end = doc.duration_ms().saturating_add(5_000);
    for pos in [0, 1, end / 3, end / 2, end, u64::MAX / 2, u64::MAX] {
        let c = cursor(doc, pos);
        if let Some(i) = c.line {
            if i >= doc.lines.len() {
                return Err(format!("cursor at {pos} points past the end: {i} of {}", doc.lines.len()));
            }
            let line = &doc.lines[i];
            let p = line_progress(line, pos);
            if !(0.0..=1.0).contains(&p) {
                return Err(format!("line progress {p} at {pos}"));
            }
            if let Some(w) = word_progress(line, pos).find(|w| !(0.0..=1.0).contains(w)) {
                return Err(format!("word progress {w} at {pos}"));
            }
        }
        if !(0.0..=1.0).contains(&c.line_progress) {
            return Err(format!("cursor progress {} at {pos}", c.line_progress));
        }
    }
    Ok(())
}

/// Run `f` on many inputs; collect panics, invariant failures and slow inputs.
fn fuzz(name: &str, seeds: &[String], f: impl Fn(&str) -> Result<(), String>) {
    let mut rng = Rng(SEED ^ name.len() as u64);
    let mut failures = Vec::new();
    let mut slowest = (Duration::ZERO, String::new());
    for i in 0..ITERATIONS {
        let input = match i % 4 {
            3 => random_text(&mut rng),
            _ => {
                let seed = rng.pick(seeds).clone();
                mutate(&mut rng, &seed)
            }
        };
        let started = Instant::now();
        FUZZING.with(|f| f.set(true));
        let outcome = catch_unwind(AssertUnwindSafe(|| f(&input)));
        FUZZING.with(|f| f.set(false));
        let took = started.elapsed();
        if took > slowest.0 {
            slowest = (took, input.clone());
        }
        match outcome {
            Ok(Ok(())) => {}
            Ok(Err(why)) => failures.push(format!("iteration {i}: {why}\n    input: {input:?}")),
            Err(panic) => {
                let msg = panic
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_default();
                failures.push(format!("iteration {i}: PANIC {msg}\n    input: {input:?}"));
            }
        }
        if failures.len() >= 5 {
            break;
        }
    }
    assert!(failures.is_empty(), "{name} (seed {SEED:#x}):\n  {}", failures.join("\n  "));
    assert!(
        slowest.0 < Duration::from_secs(2),
        "{name}: one input took {:?} to parse: {:?}",
        slowest.0,
        slowest.1.chars().take(300).collect::<String>()
    );
}

thread_local! {
    static FUZZING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The fuzzer catches panics itself; keep them from printing thousands of messages, while
/// panics outside the fuzz loop (a plain assertion) still print normally.
fn quiet_panics() {
    static HOOK: std::sync::Once = std::sync::Once::new();
    HOOK.call_once(|| {
        let default = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if !FUZZING.with(|f| f.get()) {
                default(info);
            }
        }));
    });
}

#[test]
fn lrc_parser_survives_garbage() {
    quiet_panics();
    let seeds = vec![
        demo("bam-bada.lrc"),
        demo("hoshizora-densha.lrc"),
        "[ti:x]\n[offset:+250]\n[00:01.00]a\n[00:02.50]<00:02.50>b <00:03.00>c\n\n[00:05.00]".into(),
        "plain\nlyrics\nonly".into(),
    ];
    fuzz("lrc::parse", &seeds, |text| check_document(&lrc::parse(text)));
}

#[test]
fn ttml_parser_survives_garbage() {
    quiet_panics();
    let seeds = vec![
        demo("neon-harbor.ttml"),
        r#"<tt xmlns="http://www.w3.org/ns/ttml"><body><div><p begin="00:01.000" end="00:02.000"><span begin="1s" end="1.5s">a</span> <span begin="1.5s" end="2s">b</span></p></div></body></tt>"#.into(),
    ];
    fuzz("ttml::parse", &seeds, |xml| match ttml::parse(xml) {
        Ok(doc) => check_document(&doc),
        Err(_) => Ok(()),
    });
}

#[test]
fn format_sniffing_survives_garbage() {
    quiet_panics();
    let seeds = vec![demo("neon-harbor.ttml"), demo("bam-bada.lrc"), "\u{feff}  <tt>".into()];
    fuzz("provider::parse_text", &seeds, |text| match provider::parse_text(text) {
        Ok(doc) => check_document(&doc),
        Err(_) => Ok(()),
    });
}

#[test]
fn timestamps_survive_garbage() {
    quiet_panics();
    let seeds: Vec<String> = ["00:00.00", "01:02.345", "99:59.99", "1:02:03.5", "12.5s", "1500ms", "00:00:01.000"]
        .map(String::from)
        .into();
    fuzz("timestamps", &seeds, |s| {
        let _ = lrc::parse_timestamp(s);
        let _ = ttml::parse_time(s);
        Ok(())
    });
}

#[test]
fn romanization_survives_garbage() {
    quiet_panics();
    let seeds: Vec<String> = [
        "星空電車",
        "ほしぞら でんしゃ",
        "カタカナー ッ",
        "밤바다 한글",
        "Жизнь",
        "你好世界",
        "mixed 夜 한 Ж é",
    ]
    .map(String::from)
    .into();
    fuzz("romanization", &seeds, |s| {
        let _ = romanization::detect(s);
        let _ = romanization::pinyin(s);
        let _ = romanization::cyrillic(s);
        let _ = romanization::korean::romanize(s);
        let _ = romanization::japanese::kana_to_romaji(s);
        Ok(())
    });
}

/// A very long synced file (a podcast transcript, a bad upload) parses in reasonable time.
#[test]
fn huge_lrc_parses_quickly() {
    let text: String = (0..50_000)
        .map(|i| {
            format!(
                "[{:02}:{:02}.{:02}]<{:02}:{:02}.00>word{i} <{:02}:{:02}.50>next\n",
                i / 6000,
                (i / 100) % 60,
                i % 100,
                i / 6000,
                (i / 100) % 60,
                i / 6000,
                (i / 100) % 60
            )
        })
        .collect();
    let started = Instant::now();
    let doc = lrc::parse(&text);
    let took = started.elapsed();
    assert_eq!(doc.lines.len(), 50_000);
    assert!(took < Duration::from_secs(5), "50 000 lines took {took:?}");
    check_document(&doc).unwrap();
    let started = Instant::now();
    for pos in (0..3_600_000).step_by(997) {
        let _ = cursor(&doc, pos);
    }
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "cursor lookups are slow on long documents"
    );
}

// ---------------------------------------------------------------------------------------------
// Spotify JSON: the shapes the real API sends, then mutations of them.

/// An ad: item is null while something plays.
const PLAYER_AD: &str = r#"{"device":{"id":"d1","is_active":true,"is_private_session":false,"is_restricted":false,"name":"Kitchen","type":"Speaker","volume_percent":null,"supports_volume":false},"shuffle_state":false,"smart_shuffle":false,"repeat_state":"off","timestamp":1,"context":null,"progress_ms":4000,"item":null,"currently_playing_type":"ad","actions":{"disallows":{"pausing":true}},"is_playing":true}"#;

/// A podcast episode (additional_types=episode).
const PLAYER_EPISODE: &str = r#"{"device":{"id":"d2","is_active":true,"is_restricted":false,"name":"Phone","type":"Smartphone","volume_percent":100},"shuffle_state":false,"repeat_state":"context","progress_ms":720000,"is_playing":false,"currently_playing_type":"episode","context":{"type":"show","uri":"spotify:show:sh0","href":"x","external_urls":{}},"item":{"type":"episode","id":"ep1","uri":"spotify:episode:ep1","name":"Folge 12","duration_ms":1800000,"images":[{"url":"https://i/1","width":640,"height":640}],"show":{"id":"sh0","uri":"spotify:show:sh0","name":"Tech im Gespräch","publisher":"Demo","images":[]},"resume_point":{"fully_played":false,"resume_position_ms":720000},"explicit":false}}"#;

/// A local file: no id, empty album, no images.
const LOCAL_TRACK: &str = r#"{"added_at":"2026-01-01T00:00:00Z","added_by":{"id":"me"},"is_local":true,"primary_color":null,"track":{"album":{"album_type":null,"artists":[],"available_markets":[],"external_urls":{},"href":null,"id":null,"images":[],"name":"","release_date":null,"release_date_precision":null,"type":"album","uri":null},"artists":[{"external_urls":{},"href":null,"id":null,"name":"Unknown Artist","type":"artist","uri":null}],"available_markets":[],"disc_number":0,"duration_ms":211000,"explicit":false,"external_ids":{},"external_urls":{},"href":null,"id":null,"is_local":true,"name":"home recording.mp3","popularity":0,"preview_url":null,"track_number":0,"type":"track","uri":"spotify:local:Unknown+Artist::home+recording.mp3:211"}}"#;

/// A playlist page with a removed (null) entry, a local file and a normal track.
const PLAYLIST_PAGE: &str = r#"{"href":"x","limit":100,"next":null,"offset":0,"previous":null,"total":3,"items":[{"added_at":"2026-01-01T00:00:00Z","track":null},{"added_at":"2026-01-02T00:00:00Z","item":{"type":"track","id":"t1","uri":"spotify:track:t1","name":"Neon Rain","duration_ms":150000,"is_playable":false,"restrictions":{"reason":"market"},"artists":[{"id":"a1","uri":"spotify:artist:a1","name":"Neon Harbor"}],"album":{"id":"al1","uri":"spotify:album:al1","name":"Midnight Circuits","images":[{"url":"https://i/s","width":64,"height":64},{"url":"https://i/l","width":640,"height":640},{"url":"https://i/n","width":null,"height":null}]}}},{"added_at":"","track":{"id":null,"uri":"spotify:local:a:b:c:1","name":"x","is_local":true,"duration_ms":1000,"artists":[],"album":{"name":"","images":[]}}}]}"#;

/// Search: Spotify really puts null entries into playlist results; an empty playlist has
/// `images: null`.
const SEARCH: &str = r#"{"playlists":{"href":"x","items":[null,{"id":"p1","uri":"spotify:playlist:p1","name":"Leer","description":null,"collaborative":false,"images":null,"owner":{"id":"u","display_name":null},"tracks":{"href":"x","total":0}},null],"limit":20,"next":null,"offset":0,"total":3},"tracks":{"items":[],"total":0},"albums":{"items":[{"id":"al1","uri":"spotify:album:al1","name":"A","images":[],"artists":[],"release_date":"2010"}]},"artists":{"items":[{"id":"ar1","uri":"spotify:artist:ar1","name":"B","images":[],"genres":[],"followers":{"total":null}}]}}"#;

const DEVICES: &str = r#"{"devices":[{"id":null,"is_active":false,"is_private_session":true,"is_restricted":true,"name":"Restricted TV","type":"TV","volume_percent":null,"supports_volume":false},{"id":"d1","is_active":true,"is_restricted":false,"name":"rtify","type":"Computer","volume_percent":64,"supports_volume":true}]}"#;

const QUEUE: &str = r#"{"currently_playing":null,"queue":[{"type":"episode","id":"ep1","uri":"spotify:episode:ep1","name":"Folge","duration_ms":1,"images":[],"show":{"name":"Show","images":[]}}]}"#;

#[test]
fn real_spotify_shapes_parse() {
    // Local files: Spotify sends `null` for the album's and artists' id, uri and href. A
    // playlist with one local file must still load.
    let ad: PlayerState = serde_json::from_str(PLAYER_AD).expect("ad playback");
    assert!(ad.item.is_none() && ad.is_playing);
    assert_eq!(ad.device.as_ref().and_then(|d| d.volume_percent), None);

    let ep: PlayerState = serde_json::from_str(PLAYER_EPISODE).expect("episode playback");
    let item = ep.item.expect("episode item");
    assert_eq!(item.album_name(), "Tech im Gespräch", "episodes show their podcast as album");
    assert_eq!(
        item.artist_names(),
        "Tech im Gespräch",
        "episodes have no artists; the show stands in"
    );
    assert_eq!(
        item.images().len(),
        1,
        "falls back to the episode's own images when the show has none"
    );

    let local: SavedTrack = serde_json::from_str(LOCAL_TRACK).expect("local file");
    assert!(local.track.is_local && local.track.id.is_none());
    assert!(pick_image(local.track.images(), 64).is_none());
    assert_eq!(local.track.artist_names(), "Unknown Artist");

    let page: Page<PlaylistItem> = serde_json::from_str(PLAYLIST_PAGE).expect("playlist page");
    assert_eq!(page.items.len(), 3);
    assert!(page.items[0].item.is_none(), "removed tracks come as null");
    let t = page.items[1].item.as_ref().unwrap();
    assert_eq!(pick_image(t.images(), 300).map(|i| i.url.as_str()), Some("https://i/l"));

    let search: SearchResult = serde_json::from_str(SEARCH).expect("search with null playlists");
    let playlists: Vec<Playlist> = search.playlists.unwrap().items.into_iter().flatten().collect();
    assert_eq!(playlists.len(), 1);
    assert_eq!(playlists[0].count(), 0);

    let devices: Devices = serde_json::from_str(DEVICES).expect("devices");
    assert_eq!(devices.devices.len(), 2);
    let queue: Queue = serde_json::from_str(QUEUE).expect("queue with an episode");
    assert_eq!(queue.queue[0].artist_names(), "Show");
}

#[test]
fn spotify_json_survives_garbage() {
    quiet_panics();
    let seeds: Vec<String> = [PLAYER_AD, PLAYER_EPISODE, LOCAL_TRACK, PLAYLIST_PAGE, SEARCH, DEVICES, QUEUE]
        .map(String::from)
        .into();
    fuzz("spotify json", &seeds, |json| {
        if let Ok(p) = serde_json::from_str::<PlayerState>(json)
            && let Some(item) = p.item
        {
            touch(&item);
        }
        if let Ok(page) = serde_json::from_str::<Page<PlaylistItem>>(json) {
            page.items.iter().filter_map(|i| i.item.as_ref()).for_each(touch);
        }
        if let Ok(s) = serde_json::from_str::<SavedTrack>(json) {
            touch(&s.track);
        }
        if let Ok(s) = serde_json::from_str::<SearchResult>(json) {
            for p in s.playlists.into_iter().flat_map(|p| p.items).flatten() {
                let _ = p.count();
                let _ = p.images.as_deref().and_then(|i| pick_image(i, 64));
            }
        }
        let _ = serde_json::from_str::<Devices>(json);
        let _ = serde_json::from_str::<Queue>(json);
        Ok(())
    });
}

fn touch(t: &Track) {
    let _ = t.artist_names();
    let _ = t.album_name();
    let _ = pick_image(t.images(), 300);
}
