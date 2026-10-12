//! The one rule for server text that is about to be shown, saved or logged while a request
//! carried a credential (a bearer token, an API key): a store, provider or proxy may echo the
//! credential back, whole or split, inside an error. Control characters go first (so a split
//! cannot be rejoined by dropping them afterwards), then text holding any piece of a
//! credential is withheld whole, and only then is the rest clipped.

/// Consecutive characters of a credential that count as a piece of it.
const PIECE: usize = 8;

/// What is shown instead of server text that held part of a credential.
pub(crate) const WITHHELD: &str = "[withheld: it contained part of the credential sent]";

/// `text` from a server, safe to show: no control characters, no piece of `secrets`, at most
/// `max_chars` characters.
pub(crate) fn server_text(text: &str, secrets: &[&str], max_chars: usize) -> String {
    let text: String = text.chars().filter(|c| !c.is_control()).collect();
    if secrets.iter().any(|secret| holds_piece(&text, secret)) {
        return WITHHELD.to_owned();
    }
    text.chars().take(max_chars).collect()
}

/// `text` contains `PIECE` consecutive characters of `secret` (all of a shorter one).
fn holds_piece(text: &str, secret: &str) -> bool {
    let secret: Vec<char> = secret.chars().filter(|c| !c.is_control()).collect();
    if secret.is_empty() {
        return false;
    }
    let size = PIECE.min(secret.len());
    secret
        .windows(size)
        .any(|piece| text.contains(&piece.iter().collect::<String>()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "azt_0123456789abcdef0123456789abcdef";

    #[test]
    fn text_holding_any_piece_of_a_credential_is_withheld() {
        let (head, tail) = TOKEN.split_at(12);
        for text in [
            format!("Bad request; Authorization: Bearer {TOKEN}"),
            // Split by a control character, which dropping control characters would rejoin.
            format!("Authorization: Bearer {head}\n{tail}"),
            format!("{head}\u{1b}[0m{tail}"),
            // At the clipping boundary: the piece before the clip is enough.
            format!("{}{TOKEN}", "x".repeat(295)),
            format!("{}{}", "x".repeat(299), &TOKEN[..PIECE]),
            // Only a piece of it.
            format!("unknown field `{}`", &TOKEN[20..30]),
        ] {
            assert_eq!(server_text(&text, &[TOKEN], 300), WITHHELD, "{text:?}");
        }
    }

    #[test]
    fn other_text_loses_control_characters_and_is_clipped() {
        assert_eq!(
            server_text("Unknown\u{7} schema.\n", &[TOKEN], 300),
            "Unknown schema."
        );
        assert_eq!(server_text(&"y".repeat(500), &[TOKEN], 300).len(), 300);
        // Seven characters of it are not a piece.
        assert_eq!(server_text(&TOKEN[..7], &[TOKEN], 300), &TOKEN[..7]);
        assert_eq!(server_text("plain", &[], 300), "plain");
        assert_eq!(server_text("ab", &["ab"], 300), WITHHELD);
    }
}
