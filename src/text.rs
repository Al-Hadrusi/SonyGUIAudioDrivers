//! Helpers for text that comes from the headset or from Bluetooth (device
//! names, firmware strings, setting labels). Any nearby device can choose
//! these strings, so they are cleaned before they are shown or stored.

/// Is `c` a character that can reorder or hide surrounding text
/// (Unicode bidirectional controls, zero-width marks)?
fn is_format_control(c: char) -> bool {
    matches!(
        c,
        '\u{061C}'
            | '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{2069}'
            | '\u{FEFF}'
    )
}

/// Drop control and bidi-formatting characters, trim, and cap the length
/// at `max_chars`.
pub fn sanitize(text: &str, max_chars: usize) -> String {
    text.chars()
        .filter(|c| !c.is_control() && !is_format_control(*c))
        .take(max_chars)
        .collect::<String>()
        .trim()
        .to_string()
}

/// Escape text for places that interpret a subset of HTML/Pango markup
/// (freedesktop notification bodies, StatusNotifierItem tooltips).
pub fn escape_markup(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_controls_and_bidi() {
        assert_eq!(
            sanitize("  WH\u{202E}-1000\nXM5\u{200B} ", 64),
            "WH-1000XM5"
        );
        assert_eq!(sanitize("abcdef", 3), "abc");
    }

    #[test]
    fn escapes_markup() {
        assert_eq!(
            escape_markup("<a href=\"x\">A&B</a>"),
            "&lt;a href=&quot;x&quot;&gt;A&amp;B&lt;/a&gt;"
        );
    }
}
