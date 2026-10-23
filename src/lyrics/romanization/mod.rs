//! Language-specific romanization. Romanization is not translation: it only changes the script.

pub mod japanese;
pub mod korean;

use pinyin::ToPinyin;

use super::model::LyricDocument;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Script {
    Latin,
    Japanese,
    Korean,
    Chinese,
    Cyrillic,
    Other,
}

fn is_cyrillic(c: char) -> bool {
    matches!(c, '\u{0400}'..='\u{04FF}')
}

/// Dominant script of a text. Any kana means Japanese even when kanji dominate.
pub fn detect(text: &str) -> Script {
    let (mut kana, mut han, mut hangul, mut cyr, mut other) = (0, 0, 0, 0, 0);
    for c in text.chars() {
        if japanese::is_kana(c) {
            kana += 1;
        } else if japanese::is_kanji(c) {
            han += 1;
        } else if korean::is_hangul(c) {
            hangul += 1;
        } else if is_cyrillic(c) {
            cyr += 1;
        } else if c.is_alphabetic() && !c.is_ascii() && !matches!(c, '\u{00C0}'..='\u{024F}') {
            other += 1;
        }
    }
    if kana > 0 {
        Script::Japanese
    } else if hangul > 0 && hangul >= han {
        Script::Korean
    } else if han > 0 {
        Script::Chinese
    } else if cyr > 0 {
        Script::Cyrillic
    } else if other > 0 {
        Script::Other
    } else {
        Script::Latin
    }
}

/// Phrase-blind Pinyin with tone marks; polyphonic characters take their most common reading.
pub fn pinyin(text: &str) -> String {
    let mut out = String::new();
    let mut prev_han = false;
    for c in text.chars() {
        match c.to_pinyin() {
            Some(p) => {
                if !out.is_empty() && !out.ends_with(' ') {
                    out.push(' ');
                }
                out.push_str(p.with_tone());
                prev_han = true;
            }
            None => {
                if prev_han && c.is_alphanumeric() {
                    out.push(' ');
                }
                out.push(c);
                prev_han = false;
            }
        }
    }
    out
}

pub fn cyrillic(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        let lower = c.to_lowercase().next().unwrap_or(c);
        let r = match lower {
            'а' => "a",
            'б' => "b",
            'в' => "v",
            'г' => "g",
            'ґ' => "g",
            'д' => "d",
            'е' => "e",
            'ё' => "yo",
            'є' => "ye",
            'ж' => "zh",
            'з' => "z",
            'и' => "i",
            'і' => "i",
            'ї' => "yi",
            'й' => "y",
            'к' => "k",
            'л' => "l",
            'м' => "m",
            'н' => "n",
            'о' => "o",
            'п' => "p",
            'р' => "r",
            'с' => "s",
            'т' => "t",
            'у' => "u",
            'ў' => "w",
            'ф' => "f",
            'х' => "kh",
            'ц' => "ts",
            'ч' => "ch",
            'ш' => "sh",
            'щ' => "shch",
            'ъ' | 'ь' => "",
            'ы' => "y",
            'э' => "e",
            'ю' => "yu",
            'я' => "ya",
            _ => {
                out.push(c);
                continue;
            }
        };
        if c.is_uppercase() {
            let mut chars = r.chars();
            if let Some(f) = chars.next() {
                out.extend(f.to_uppercase());
                out.push_str(chars.as_str());
            }
        } else {
            out.push_str(r);
        }
    }
    out
}

/// Romanized form of `text`, or `None` when it is already Latin or the script is unsupported.
pub fn romanize(text: &str) -> Option<String> {
    let r = match detect(text) {
        Script::Latin | Script::Other => return None,
        Script::Japanese => match japanese::romanize(text) {
            (r, true) => r,
            // Kanji without readings (no `japanese-kanji` build): a half-romanized line helps nobody.
            (_, false) => return None,
        },
        Script::Korean => korean::romanize(text),
        Script::Chinese => pinyin(text),
        Script::Cyrillic => cyrillic(text),
    };
    (r != text).then_some(r)
}

/// Fill `romanized` for lines that do not already carry one from the source document.
pub fn apply(doc: &mut LyricDocument) {
    for line in &mut doc.lines {
        if line.romanized.is_none() {
            line.romanized = romanize(&line.original);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_scripts() {
        assert_eq!(detect("hello"), Script::Latin);
        assert_eq!(detect("Grüße"), Script::Latin);
        assert_eq!(detect("君のことが好き"), Script::Japanese);
        assert_eq!(detect("사랑해"), Script::Korean);
        assert_eq!(detect("我爱你"), Script::Chinese);
        assert_eq!(detect("Привет"), Script::Cyrillic);
        assert_eq!(detect("שלום"), Script::Other);
    }

    #[test]
    fn chinese_pinyin_with_tones() {
        assert_eq!(pinyin("我爱你"), "wǒ ài nǐ");
        assert_eq!(pinyin("你好, world"), "nǐ hǎo, world");
    }

    #[test]
    fn cyrillic_keeps_case() {
        assert_eq!(cyrillic("Привет, мир"), "Privet, mir");
        assert_eq!(cyrillic("Щука"), "Shchuka");
    }

    #[test]
    fn latin_is_left_alone() {
        assert_eq!(romanize("just english"), None);
        assert_eq!(romanize("ありがとう").as_deref(), Some("arigatou"));
        #[cfg(not(feature = "japanese-kanji"))]
        assert_eq!(romanize("君のこと"), None);
    }
}
