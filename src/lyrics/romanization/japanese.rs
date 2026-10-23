//! Kana → modified Hepburn. Kanji need a reading from morphological analysis: with the
//! `japanese-kanji` feature Lindera supplies it, otherwise kanji are kept as written and the
//! result is flagged as partial.

fn kana_base(c: char) -> Option<&'static str> {
    // Katakana are folded to hiragana by the caller.
    Some(match c {
        'あ' => "a",
        'い' => "i",
        'う' => "u",
        'え' => "e",
        'お' => "o",
        'か' => "ka",
        'き' => "ki",
        'く' => "ku",
        'け' => "ke",
        'こ' => "ko",
        'が' => "ga",
        'ぎ' => "gi",
        'ぐ' => "gu",
        'げ' => "ge",
        'ご' => "go",
        'さ' => "sa",
        'し' => "shi",
        'す' => "su",
        'せ' => "se",
        'そ' => "so",
        'ざ' => "za",
        'じ' => "ji",
        'ず' => "zu",
        'ぜ' => "ze",
        'ぞ' => "zo",
        'た' => "ta",
        'ち' => "chi",
        'つ' => "tsu",
        'て' => "te",
        'と' => "to",
        'だ' => "da",
        'ぢ' => "ji",
        'づ' => "zu",
        'で' => "de",
        'ど' => "do",
        'な' => "na",
        'に' => "ni",
        'ぬ' => "nu",
        'ね' => "ne",
        'の' => "no",
        'は' => "ha",
        'ひ' => "hi",
        'ふ' => "fu",
        'へ' => "he",
        'ほ' => "ho",
        'ば' => "ba",
        'び' => "bi",
        'ぶ' => "bu",
        'べ' => "be",
        'ぼ' => "bo",
        'ぱ' => "pa",
        'ぴ' => "pi",
        'ぷ' => "pu",
        'ぺ' => "pe",
        'ぽ' => "po",
        'ま' => "ma",
        'み' => "mi",
        'む' => "mu",
        'め' => "me",
        'も' => "mo",
        'や' => "ya",
        'ゆ' => "yu",
        'よ' => "yo",
        'ら' => "ra",
        'り' => "ri",
        'る' => "ru",
        'れ' => "re",
        'ろ' => "ro",
        'わ' => "wa",
        'ゐ' => "i",
        'ゑ' => "e",
        'を' => "o",
        'ん' => "n",
        'ゔ' => "vu",
        'ぁ' => "a",
        'ぃ' => "i",
        'ぅ' => "u",
        'ぇ' => "e",
        'ぉ' => "o",
        'ゃ' => "ya",
        'ゅ' => "yu",
        'ょ' => "yo",
        'ゎ' => "wa",
        _ => return None,
    })
}

fn to_hiragana(c: char) -> char {
    match c {
        'ァ'..='ヶ' => char::from_u32(c as u32 - 0x60).unwrap_or(c),
        _ => c,
    }
}

pub fn is_kana(c: char) -> bool {
    matches!(c, '\u{3041}'..='\u{309F}' | '\u{30A0}'..='\u{30FF}')
}

pub fn is_kanji(c: char) -> bool {
    matches!(c, '\u{4E00}'..='\u{9FFF}' | '\u{3400}'..='\u{4DBF}' | '々')
}

/// Small-kana digraphs: きゃ → kya, しゃ → sha, ちゃ → cha, じゃ → ja, ふぁ → fa …
fn digraph(base: &str, small: char) -> Option<String> {
    let stem = base.strip_suffix('i')?;
    let vowel = match small {
        'ゃ' => "a",
        'ゅ' => "u",
        'ょ' => "o",
        'ぇ' => "e",
        _ => return None,
    };
    Some(match stem {
        "sh" | "ch" | "j" => format!("{stem}{vowel}"),
        _ => format!("{stem}y{vowel}"),
    })
}

fn f_v_digraph(base: &str, small: char) -> Option<String> {
    let stem = match base {
        "fu" => "f",
        "vu" => "v",
        "te" => "t",
        "de" => "d",
        "u" => "w",
        "tsu" => "ts",
        "shi" => "sh",
        "ji" => "j",
        "chi" => "ch",
        _ => return None,
    };
    let vowel = match small {
        'ぁ' => "a",
        'ぃ' => "i",
        'ぅ' => "u",
        'ぇ' => "e",
        'ぉ' => "o",
        _ => return None,
    };
    Some(format!("{stem}{vowel}"))
}

/// Romanize kana; anything else is copied through. Returns `(text, fully_converted)`.
pub fn kana_to_romaji(input: &str) -> (String, bool) {
    let chars: Vec<char> = input.chars().map(to_hiragana).collect();
    let mut out = String::new();
    let mut complete = true;
    let mut geminate = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == 'っ' {
            geminate = true;
            i += 1;
            continue;
        }
        if c == 'ー' {
            if let Some(v) = out.chars().last().filter(|v| "aeiou".contains(*v)) {
                out.push(v);
            }
            i += 1;
            continue;
        }
        let Some(base) = kana_base(c) else {
            if is_kanji(c) {
                complete = false;
            }
            geminate = false;
            out.push(c);
            i += 1;
            continue;
        };
        let mut syl = base.to_owned();
        if let Some(&next) = chars.get(i + 1)
            && let Some(d) = digraph(base, next).or_else(|| f_v_digraph(base, next))
        {
            syl = d;
            i += 1;
        }
        if c == 'ん' {
            // n' before a vowel or y keeps "kan'i" distinct from "kani".
            if chars
                .get(i + 1)
                .and_then(|&n| kana_base(n))
                .is_some_and(|s| s.starts_with(['a', 'i', 'u', 'e', 'o', 'y']))
            {
                syl.push('\'');
            }
        }
        if geminate {
            if syl.starts_with("ch") {
                out.push('t');
            } else if let Some(first) = syl.chars().next().filter(|f| !"aeioun".contains(*f)) {
                out.push(first);
            }
            geminate = false;
        }
        out.push_str(&syl);
        i += 1;
    }
    (out, complete)
}

#[cfg(feature = "japanese-kanji")]
mod analyzer {
    use std::sync::OnceLock;

    use std::borrow::Cow;

    use lindera::dictionary::load_dictionary;
    use lindera::mode::Mode;
    use lindera::segmenter::Segmenter;

    fn segmenter() -> Option<&'static Segmenter> {
        static S: OnceLock<Option<Segmenter>> = OnceLock::new();
        S.get_or_init(|| {
            let dict = load_dictionary("embedded://ipadic").ok()?;
            Some(Segmenter::new(Mode::Normal, dict, None))
        })
        .as_ref()
    }

    /// `(surface, katakana reading, is particle)` per token.
    pub fn readings(text: &str) -> Option<Vec<(String, String, bool)>> {
        let mut tokens = segmenter()?.segment(Cow::Borrowed(text)).ok()?;
        Some(
            tokens
                .iter_mut()
                .map(|t| {
                    let surface = t.surface.to_string();
                    let details = t.details();
                    let particle = details.first().is_some_and(|p| *p == "助詞");
                    let reading = details
                        .get(7)
                        .filter(|r| **r != "*")
                        .map(|r| r.to_string())
                        .unwrap_or_else(|| surface.clone());
                    (surface, reading, particle)
                })
                .collect(),
        )
    }
}

pub fn romanize(text: &str) -> (String, bool) {
    #[cfg(feature = "japanese-kanji")]
    if let Some(tokens) = analyzer::readings(text) {
        let mut words = Vec::new();
        for (surface, reading, particle) in tokens {
            if surface.trim().is_empty() {
                continue;
            }
            let r = match (particle, surface.as_str()) {
                (true, "は") => "wa".to_owned(),
                (true, "へ") => "e".to_owned(),
                (true, "を") => "o".to_owned(),
                _ => kana_to_romaji(&reading).0,
            };
            words.push(r);
        }
        let joined = words.join(" ");
        let complete = !joined.chars().any(is_kanji);
        return (joined, complete);
    }
    kana_to_romaji(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hepburn_basics() {
        assert_eq!(kana_to_romaji("ありがとう").0, "arigatou");
        assert_eq!(kana_to_romaji("しゃしん").0, "shashin");
        assert_eq!(kana_to_romaji("ちょっと").0, "chotto");
        assert_eq!(kana_to_romaji("まっちゃ").0, "matcha");
        assert_eq!(kana_to_romaji("きって").0, "kitte");
        assert_eq!(kana_to_romaji("じゃあね").0, "jaane");
        assert_eq!(kana_to_romaji("きゃく").0, "kyaku");
    }

    #[test]
    fn katakana_and_long_vowels() {
        assert_eq!(kana_to_romaji("コーヒー").0, "koohii");
        assert_eq!(kana_to_romaji("ファン").0, "fan");
        assert_eq!(kana_to_romaji("パーティー").0, "paatii");
    }

    #[test]
    fn syllabic_n() {
        assert_eq!(kana_to_romaji("きんようび").0, "kin'youbi");
        assert_eq!(kana_to_romaji("さんぽ").0, "sanpo");
    }

    #[cfg(feature = "japanese-kanji")]
    #[test]
    fn kanji_readings_with_analyzer() {
        let (r, complete) = romanize("星空の電車に乗って");
        assert!(complete, "{r}");
        assert!(r.starts_with("hoshizora no densha ni"), "{r}");
        assert_eq!(romanize("君は").0, "kimi wa");
    }

    #[test]
    fn kanji_is_flagged_without_analyzer() {
        let (_, complete) = kana_to_romaji("君のこと");
        assert!(!complete);
    }
}
