//! Hangul → Revised Romanization with the common sound changes between syllables of a word:
//! liaison, nasalisation, ㄹ assimilation and ㅎ aspiration. Rarer rules (tensification,
//! palatalisation of ㄷ/ㅌ + 이) are not applied.

const BASE: u32 = 0xAC00;

const INITIAL: [&str; 19] = [
    "g", "kk", "n", "d", "tt", "r", "m", "b", "pp", "s", "ss", "", "j", "jj", "ch", "k", "t", "p", "h",
];
const VOWEL: [&str; 21] = [
    "a", "ae", "ya", "yae", "eo", "e", "yeo", "ye", "o", "wa", "wae", "oe", "yo", "u", "wo", "we", "wi", "yu", "eu", "ui", "i",
];

// Final consonant indices (0 = none).
const F_N: usize = 4;
const F_NH: usize = 6;
const F_L: usize = 8;
const F_LH: usize = 15;
const F_H: usize = 27;

// Initial indices.
const I_G: usize = 0;
const I_D: usize = 3;
const I_J: usize = 12;
const I_N: usize = 2;
const I_R: usize = 5;
const I_M: usize = 6;
const I_SILENT: usize = 11;
const I_H: usize = 18;

/// Representative sound of a final consonant when nothing follows it.
fn final_sound(f: usize) -> &'static str {
    match f {
        0 => "",
        1 | 2 | 3 | 9 | 24 => "k",              // ㄱ ㄲ ㄳ ㄺ ㅋ
        4..=6 => "n",                           // ㄴ ㄵ ㄶ
        7 | 19 | 20 | 22 | 23 | 25 | 27 => "t", // ㄷ ㅅ ㅆ ㅈ ㅊ ㅌ ㅎ
        8 | 11 | 12 | 13 | 15 => "l",           // ㄹ ㄼ ㄽ ㄾ ㅀ
        10 | 16 => "m",                         // ㄻ ㅁ
        14 | 17 | 18 | 26 => "p",               // ㄿ ㅂ ㅄ ㅍ
        21 => "ng",
        _ => "",
    }
}

/// When a final is followed by a silent ㅇ it moves over: (what stays, what moves).
fn liaison(f: usize) -> (&'static str, &'static str) {
    match f {
        1 => ("", "g"),
        2 => ("", "kk"),
        3 => ("k", "s"),
        4 => ("", "n"),
        5 => ("n", "j"),
        6 => ("", "n"),
        7 => ("", "d"),
        8 => ("", "r"),
        9 => ("l", "g"),
        10 => ("l", "m"),
        11 => ("l", "b"),
        12 => ("l", "s"),
        13 => ("l", "t"),
        14 => ("l", "p"),
        15 => ("", "r"),
        16 => ("", "m"),
        17 => ("", "b"),
        18 => ("p", "s"),
        19 => ("", "s"),
        20 => ("", "ss"),
        21 => ("ng", ""),
        22 => ("", "j"),
        23 => ("", "ch"),
        24 => ("", "k"),
        25 => ("", "t"),
        26 => ("", "p"),
        27 => ("", ""),
        _ => ("", ""),
    }
}

#[derive(Clone, Copy)]
struct Syl {
    i: usize,
    v: usize,
    f: usize,
}

fn decompose(c: char) -> Option<Syl> {
    let s = (c as u32).checked_sub(BASE)?;
    (s < 11172).then_some(Syl {
        i: (s / 588) as usize,
        v: ((s % 588) / 28) as usize,
        f: (s % 28) as usize,
    })
}

pub fn is_hangul(c: char) -> bool {
    decompose(c).is_some()
}

fn romanize_word(syls: &[Syl]) -> String {
    let mut out = String::new();
    let mut carried: Option<String> = None;
    for (k, s) in syls.iter().enumerate() {
        let onset = match carried.take() {
            Some(c) => c,
            None if k == 0 && s.i == I_R => "r".into(),
            None => INITIAL[s.i].into(),
        };
        out.push_str(&onset);
        out.push_str(VOWEL[s.v]);
        let Some(next) = syls.get(k + 1) else {
            out.push_str(final_sound(s.f));
            break;
        };
        let (coda, onset_next): (String, Option<String>) = match (s.f, next.i) {
            (0, _) => (String::new(), None),
            (f, I_SILENT) => {
                let (stay, moved) = liaison(f);
                (stay.into(), Some(moved.into()))
            }
            // ㅎ final before ㄱ/ㄷ/ㅈ aspirates them.
            (F_H | F_NH | F_LH, i @ (I_G | I_D | I_J)) => {
                let stay = match s.f {
                    F_NH => "n",
                    F_LH => "l",
                    _ => "",
                };
                let onset = match i {
                    I_G => "k",
                    I_D => "t",
                    _ => "ch",
                };
                (stay.into(), Some(onset.into()))
            }
            // k/t/p + ㅎ → aspirated onset.
            (f, I_H) if matches!(final_sound(f), "k" | "t" | "p") => (String::new(), Some(final_sound(f).into())),
            // ㄹ assimilation.
            (F_L | F_N, I_R) | (F_L, I_N) => ("l".into(), Some("l".into())),
            // ㄹ after other consonants is pronounced ㄴ, which may nasalise the coda.
            (f, I_R) => {
                let coda = match final_sound(f) {
                    "k" => "ng",
                    "t" => "n",
                    "p" => "m",
                    other => other,
                };
                (coda.into(), Some("n".into()))
            }
            // Nasalisation before ㄴ/ㅁ.
            (f, I_N | I_M) => (
                match final_sound(f) {
                    "k" => "ng",
                    "t" => "n",
                    "p" => "m",
                    other => other,
                }
                .into(),
                None,
            ),
            (f, _) => (final_sound(f).into(), None),
        };
        out.push_str(&coda);
        carried = onset_next;
    }
    out
}

pub fn romanize(text: &str) -> String {
    let mut out = String::new();
    let mut word: Vec<Syl> = Vec::new();
    for c in text.chars() {
        if let Some(s) = decompose(c) {
            word.push(s);
            continue;
        }
        if !word.is_empty() {
            out.push_str(&romanize_word(&word));
            word.clear();
        }
        out.push(c);
    }
    if !word.is_empty() {
        out.push_str(&romanize_word(&word));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::romanize;

    #[test]
    fn plain_syllables() {
        assert_eq!(romanize("한국"), "hanguk");
        assert_eq!(romanize("서울"), "seoul");
        assert_eq!(romanize("사랑해"), "saranghae");
    }

    #[test]
    fn liaison_and_assimilation() {
        assert_eq!(romanize("음악"), "eumak");
        assert_eq!(romanize("한국어"), "hangugeo");
        assert_eq!(romanize("신라"), "silla");
        assert_eq!(romanize("종로"), "jongno");
        assert_eq!(romanize("백마"), "baengma");
        assert_eq!(romanize("좋다"), "jota");
        assert_eq!(romanize("입학"), "ipak");
    }

    #[test]
    fn mixed_text() {
        assert_eq!(romanize("안녕, 세상!"), "annyeong, sesang!");
    }
}
