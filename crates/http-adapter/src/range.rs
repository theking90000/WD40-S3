use std::ops::Range;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Selection {
    Full,
    Partial(Range<u64>),
    Unsatisfiable,
}

pub(crate) fn select(value: Option<&str>, length: u64) -> Result<Selection, ()> {
    let Some(value) = value else {
        return Ok(Selection::Full);
    };
    let (unit, ranges) = value.trim().split_once('=').ok_or(())?;
    if unit != "bytes" || ranges.contains(',') {
        return Ok(Selection::Full);
    }
    let (start, end) = ranges.trim().split_once('-').ok_or(())?;
    let parse = |value: &str| {
        if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(());
        }
        value.parse::<u64>().map_err(|_| ())
    };
    if start.is_empty() {
        let suffix = parse(end)?;
        return if suffix == 0 || length == 0 {
            Ok(Selection::Unsatisfiable)
        } else {
            Ok(Selection::Partial(length.saturating_sub(suffix)..length))
        };
    }
    let start = parse(start)?;
    let end = if end.is_empty() {
        length
    } else {
        let inclusive = parse(end)?;
        if inclusive < start {
            return Err(());
        }
        inclusive
            .min(length.saturating_sub(1))
            .checked_add(1)
            .ok_or(())?
    };
    if start >= length {
        return Ok(Selection::Unsatisfiable);
    }
    Ok(Selection::Partial(start..end))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_ranges() {
        for (input, length, expected) in [
            (None, 10, Selection::Full),
            (Some("bytes=2-4"), 10, Selection::Partial(2..5)),
            (Some("bytes=2-"), 10, Selection::Partial(2..10)),
            (Some("bytes=-3"), 10, Selection::Partial(7..10)),
            (Some("bytes=-20"), 10, Selection::Partial(0..10)),
            (Some("bytes=0-99"), 10, Selection::Partial(0..10)),
            (Some("bytes=10-"), 10, Selection::Unsatisfiable),
            (Some("bytes=-0"), 10, Selection::Unsatisfiable),
            (Some("bytes=0-0"), 0, Selection::Unsatisfiable),
            (Some("bytes=0-1,4-5"), 10, Selection::Full),
            (Some("items=0-1"), 10, Selection::Full),
            (
                Some("bytes=0-18446744073709551615"),
                u64::MAX,
                Selection::Partial(0..u64::MAX),
            ),
        ] {
            assert_eq!(select(input, length), Ok(expected));
        }
        for value in [
            "bytes=",
            "bytes=-",
            "bytes=4-2",
            "bytes=+1-2",
            "bytes=a-b",
            "bytes=0-18446744073709551616",
        ] {
            assert_eq!(select(Some(value), 10), Err(()));
        }
    }
}
