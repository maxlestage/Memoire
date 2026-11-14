//! Lower-case hexadecimal text of binary values (JSON has no bytes): two
//! digits per byte, written and read without the formatting machinery.

const DIGITS: &[u8; 16] = b"0123456789abcdef";

/// `bytes` in lower-case hexadecimal.
pub(crate) fn encode(bytes: &[u8]) -> String {
    let mut out = Vec::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(DIGITS[usize::from(b >> 4)]);
        out.push(DIGITS[usize::from(b & 0x0f)]);
    }
    // Only ASCII digits were written.
    String::from_utf8(out).unwrap_or_default()
}

fn digit(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// The bytes of hexadecimal text (either case); `None` for an odd length or
/// anything else than two digits per byte. A pair may also be `+` and one
/// digit, which `u8::from_str_radix` (read before) accepted.
pub(crate) fn decode(s: &str) -> Option<Vec<u8>> {
    let s = s.as_bytes();
    if !s.len().is_multiple_of(2) {
        return None;
    }
    let (pairs, _) = s.as_chunks::<2>();
    pairs
        .iter()
        .map(|&[high, low]| if high == b'+' { digit(low) } else { Some(digit(high)? << 4 | digit(low)?) })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_same_rules_as_from_str_radix() {
        let all: Vec<u8> = (0..=255).collect();
        let text = encode(&all);
        assert_eq!(text, all.iter().map(|b| format!("{b:02x}")).collect::<String>());
        assert_eq!(decode(&text).unwrap(), all);
        assert_eq!(decode(&text.to_uppercase()).unwrap(), all);
        assert_eq!(decode("").unwrap(), Vec::<u8>::new());
        // Every pair of ASCII characters is read as `u8::from_str_radix` reads it.
        for a in 0u8..128 {
            for b in 0u8..128 {
                let pair = String::from_utf8(vec![a, b]).unwrap();
                assert_eq!(decode(&pair), u8::from_str_radix(&pair, 16).ok().map(|v| vec![v]), "{pair:?}");
            }
        }
        assert_eq!(decode("abc"), None);
        assert_eq!(decode("zz"), None);
        // Not ASCII: an error, never a panic.
        assert_eq!(decode("aé1"), None);
        assert_eq!(decode("éé"), None);
    }
}
