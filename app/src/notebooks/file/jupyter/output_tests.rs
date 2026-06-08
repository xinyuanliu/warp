use base64::prelude::BASE64_STANDARD;
use base64::Engine as _;
use serde_json::json;

use super::{classify_outputs, strip_ansi, OutputItem};

#[test]
fn strip_ansi_removes_color_codes() {
    // A typical colored traceback fragment (invariant 5).
    let input = "\u{1b}[0;31mTraceback\u{1b}[0m: \u{1b}[1mboom\u{1b}[22m";
    assert_eq!(strip_ansi(input), "Traceback: boom");
}

#[test]
fn strip_ansi_leaves_plain_text_untouched() {
    assert_eq!(strip_ansi("just text\nwith lines"), "just text\nwith lines");
}

#[test]
fn classify_stream_output_as_text() {
    let outputs = vec![json!({
        "output_type": "stream",
        "name": "stdout",
        "text": ["hello\n", "world\n"],
    })];
    assert_eq!(
        classify_outputs(&outputs),
        vec![OutputItem::Text("hello\nworld\n".to_string())]
    );
}

#[test]
fn classify_text_plain_result_as_text() {
    let outputs = vec![json!({
        "output_type": "execute_result",
        "data": {"text/plain": "42"},
        "metadata": {},
        "execution_count": 1,
    })];
    assert_eq!(
        classify_outputs(&outputs),
        vec![OutputItem::Text("42".to_string())]
    );
}

#[test]
fn classify_error_strips_ansi_from_traceback() {
    let outputs = vec![json!({
        "output_type": "error",
        "ename": "ValueError",
        "evalue": "bad",
        "traceback": ["\u{1b}[0;31mValueError\u{1b}[0m", "bad"],
    })];
    assert_eq!(
        classify_outputs(&outputs),
        vec![OutputItem::Text("ValueErrorbad".to_string())]
    );
}

#[test]
fn classify_valid_png_as_image() {
    // A 1x1 transparent PNG.
    let png_bytes: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f,
        0x15, 0xc4, 0x89,
    ];
    let encoded = BASE64_STANDARD.encode(png_bytes);
    let outputs = vec![json!({
        "output_type": "display_data",
        "data": {"image/png": encoded},
        "metadata": {},
    })];
    assert_eq!(
        classify_outputs(&outputs),
        vec![OutputItem::Image(png_bytes.to_vec())]
    );
}

#[test]
fn classify_invalid_base64_image_as_placeholder() {
    // Invalid base64 image data shows a short message instead (invariant 5).
    let outputs = vec![json!({
        "output_type": "display_data",
        "data": {"image/png": "not%%%valid%%%base64"},
        "metadata": {},
    })];
    assert_eq!(
        classify_outputs(&outputs),
        vec![OutputItem::Placeholder("invalid image data".to_string())]
    );
}

#[test]
fn classify_oversized_image_as_placeholder_without_decoding() {
    // A base64 payload whose estimated decoded size exceeds the display cap is
    // rejected with a placeholder before it is cleaned or decoded, so a crafted
    // notebook cannot force large allocations.
    let over_cap_b64 = "A".repeat(9 * 1024 * 1024 / 3 * 4);
    let outputs = vec![json!({
        "output_type": "display_data",
        "data": {"image/png": over_cap_b64},
        "metadata": {},
    })];
    let classified = classify_outputs(&outputs);
    assert!(matches!(
        classified.as_slice(),
        [OutputItem::Placeholder(msg)] if msg.starts_with("[image output omitted for display:")
    ));
}

#[test]
fn classify_prefers_image_over_text_plain() {
    // matplotlib outputs carry both image/png and a text/plain repr; the image wins.
    let png = BASE64_STANDARD.encode([0x89, 0x50, 0x4e, 0x47]);
    let outputs = vec![json!({
        "output_type": "execute_result",
        "data": {"image/png": png, "text/plain": "<Figure>"},
        "metadata": {},
        "execution_count": 2,
    })];
    assert_eq!(
        classify_outputs(&outputs),
        vec![OutputItem::Image(vec![0x89, 0x50, 0x4e, 0x47])]
    );
}

#[test]
fn classify_skips_unsupported_mime_types() {
    // text/html (and other unrendered types) are not displayed; preserved by the model (inv 18).
    let outputs = vec![json!({
        "output_type": "display_data",
        "data": {"text/html": "<b>x</b>"},
        "metadata": {},
    })];
    assert!(classify_outputs(&outputs).is_empty());
}

#[test]
fn classify_skips_unknown_output_type() {
    let outputs = vec![json!({"output_type": "future_type", "payload": 7})];
    assert!(classify_outputs(&outputs).is_empty());
}
