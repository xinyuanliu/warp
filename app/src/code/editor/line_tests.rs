use super::*;

fn current(line_number: usize) -> EditorLineLocation {
    let line_number = LineCount::from(line_number);
    EditorLineLocation::Current {
        line_number,
        line_range: line_number..line_number + LineCount::from(1),
    }
}

fn removed(line_number: usize, index: usize) -> EditorLineLocation {
    let line_number = LineCount::from(line_number);
    EditorLineLocation::Removed {
        line_number,
        line_range: line_number..line_number + LineCount::from(1),
        index,
    }
}

/// VAL-EDGE-002: a `Removed { line_number, index }` location maps to the removed-line render slot
/// `Temporary { at_line: line_number, index_from_at_line: index }` — NOT the current-line slot — so
/// a comment on a removed line is positioned by its hunk index.
#[test]
fn removed_line_maps_to_temporary_render_slot_by_index() {
    assert_eq!(
        removed(12, 2).into_render_line_location(),
        RenderLineLocation::Temporary {
            at_line: LineCount::from(12),
            index_from_at_line: 2,
        }
    );
    // The index travels through, so distinct removed-line slots map to distinct render slots.
    assert_eq!(
        removed(12, 0).into_render_line_location(),
        RenderLineLocation::Temporary {
            at_line: LineCount::from(12),
            index_from_at_line: 0,
        }
    );
}

/// A `Current` location maps to the current-line render slot (regression guard for the mapping).
#[test]
fn current_line_maps_to_current_render_slot() {
    assert_eq!(
        current(7).into_render_line_location(),
        RenderLineLocation::Current(LineCount::from(7))
    );
}

/// `is_same_line` treats two removed lines as the same only when both `line_number` and hunk
/// `index` match, so distinct removed-line slots on the same `line_number` are NOT the same line
/// (they stack at different slots rather than collapsing).
#[test]
fn is_same_line_distinguishes_removed_line_slots() {
    assert!(removed(12, 1).is_same_line(&removed(12, 1)));
    assert!(!removed(12, 0).is_same_line(&removed(12, 1)));
    assert!(!removed(12, 1).is_same_line(&removed(13, 1)));
    // A removed line and a current line on the same number are never the same line.
    assert!(!removed(12, 0).is_same_line(&current(12)));
    // Two current lines match purely on line number.
    assert!(current(7).is_same_line(&current(7)));
    assert!(!current(7).is_same_line(&current(8)));
}
