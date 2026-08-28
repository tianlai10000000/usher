pub(super) fn map_key_to_bytes(
    text: &str,
    ctrl: bool,
    alt: bool,
    _shift: bool,
    meta: bool,
) -> Option<Vec<u8>> {
    if text.is_empty() && !ctrl {
        return None;
    }

    let bytes = match text {
        "__IGNORE__" => return None,
        "__CTRL_C__" => vec![0x03],
        "__CTRL_D__" => vec![0x04],
        "__CTRL_L__" => vec![0x0c],
        "__CTRL_Z__" => vec![0x1a],
        "__RETURN__" => b"\r".to_vec(),
        "__BACKSPACE__" => vec![0x7f],
        "__DELETE__" => b"\x1b[3~".to_vec(),
        "__TAB__" => b"\t".to_vec(),
        "__ESCAPE__" => b"\x1b".to_vec(),
        "__LEFT__" => b"\x1b[D".to_vec(),
        "__RIGHT__" => b"\x1b[C".to_vec(),
        "__UP__" => b"\x1b[A".to_vec(),
        "__DOWN__" => b"\x1b[B".to_vec(),
        "__HOME__" => b"\x1b[H".to_vec(),
        "__END__" => b"\x1b[F".to_vec(),
        _ => {
            if ctrl && text.is_empty() {
                return None;
            }
            if ctrl && text.chars().count() == 1 {
                let ch = text.chars().next().unwrap_or_default();
                if ch.is_ascii_control() {
                    vec![ch as u8]
                } else {
                    let upper = ch.to_ascii_uppercase();
                    if upper.is_ascii_alphabetic() {
                        vec![(upper as u8) - b'@']
                    } else {
                        text.as_bytes().to_vec()
                    }
                }
            } else if alt || meta {
                let mut seq = vec![0x1b];
                seq.extend_from_slice(text.as_bytes());
                seq
            } else {
                text.as_bytes().to_vec()
            }
        }
    };

    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_navigation_and_control_keys_to_pty_bytes() {
        assert_eq!(
            map_key_to_bytes("__UP__", false, false, false, false),
            Some(b"\x1b[A".to_vec())
        );
        assert_eq!(
            map_key_to_bytes("c", true, false, false, false),
            Some(vec![0x03])
        );
        assert_eq!(
            map_key_to_bytes("__CTRL_D__", false, false, false, false),
            Some(vec![0x04])
        );
    }

    #[test]
    fn prefixes_alt_text_and_ignores_empty_or_sentinel_input() {
        assert_eq!(
            map_key_to_bytes("x", false, true, false, false),
            Some(vec![0x1b, b'x'])
        );
        assert_eq!(map_key_to_bytes("", false, false, false, false), None);
        assert_eq!(
            map_key_to_bytes("__IGNORE__", false, false, false, false),
            None
        );
    }
}
