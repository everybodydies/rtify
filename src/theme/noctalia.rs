//! Reads the stylesheet Noctalia's user-template system renders for us (see
//! `data/noctalia/`). We only need the declared colors, so the file is parsed rather than
//! loaded as CSS, which also keeps it working on GTK releases without CSS custom properties.

use std::collections::HashMap;
use std::path::Path;

use super::contrast::Rgb;
use super::system24::BaseColors;

/// `--s24-name: #hex;` declarations anywhere in the file.
pub fn parse_vars(css: &str) -> HashMap<String, Rgb> {
    let mut vars = HashMap::new();
    for decl in css.split([';', '{', '}']) {
        let Some((name, value)) = decl.split_once(':') else { continue };
        let Some(name) = name.trim().strip_prefix("--s24-") else { continue };
        if let Some(color) = Rgb::parse(value.trim()) {
            vars.insert(name.to_owned(), color);
        }
    }
    vars
}

pub fn colors_from_css(css: &str) -> Option<BaseColors> {
    let v = parse_vars(css);
    Some(BaseColors {
        background: *v.get("bg")?,
        foreground: *v.get("fg")?,
        accent: *v.get("accent")?,
        border: v.get("border").copied(),
        error: v.get("error").copied(),
    })
}

pub fn load(path: &Path) -> Result<BaseColors, String> {
    let css = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    colors_from_css(&css).ok_or_else(|| format!("{}: needs --s24-bg, --s24-fg and --s24-accent", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_template_output() {
        let css = ":root {\n  --s24-bg: #1e1e2e;\n  --s24-fg: #cdd6f4;\n  --s24-accent: #cba6f7;\n  --s24-border: #45475a;\n}\n";
        let c = colors_from_css(css).unwrap();
        assert_eq!(c.background.to_hex(), "#1e1e2e");
        assert_eq!(c.accent.to_hex(), "#cba6f7");
        assert_eq!(c.border.unwrap().to_hex(), "#45475a");
        assert!(c.error.is_none());
    }

    #[test]
    fn incomplete_or_unrendered_template_is_rejected() {
        assert!(colors_from_css(":root { --s24-bg: #000; }").is_none());
        assert!(colors_from_css(":root { --s24-bg: {{ colors.surface.default.hex }}; --s24-fg: #fff; --s24-accent: #f0f; }").is_none());
    }
}
