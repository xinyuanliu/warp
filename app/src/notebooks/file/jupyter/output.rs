//! Read-only rendering rules for a code cell's saved `outputs`.
//!
//! This module is the display-only projection of a cell's outputs: it never
//! mutates the underlying notebook model, so what is shown here can be
//! truncated, omitted, or reformatted without changing the bytes that are
//! written back on save (product_v1.md invariants 5, 17, 18).
//!
//! Supported output types (everything else is intentionally not rendered, but
//! is still preserved verbatim by the model):
//! - `stream` (stdout/stderr) and `text/plain` results -> preformatted text.
//! - `error` tracebacks -> preformatted text with ANSI escape codes stripped.
//! - `image/png` / `image/jpeg` -> decoded raster bytes (rendered inline by the
//!   view). Invalid base64 yields a short placeholder message.

use base64::prelude::BASE64_STANDARD;
use base64::Engine as _;
use serde_json::Value;

/// Maximum number of bytes of a single text output we will display. Larger
/// outputs are visually truncated; the saved bytes are never affected
/// (invariant 17).
const MAX_TEXT_OUTPUT_BYTES: usize = 100_000;

/// Maximum number of decoded image bytes we will display inline. Larger images
/// are omitted with a visible placeholder; the saved bytes are never affected
/// (invariant 17).
const MAX_IMAGE_BYTES: usize = 8 * 1024 * 1024;

/// A single displayable output, derived from one entry of a cell's saved
/// `outputs`. This is a display-only model: it is rebuilt from the notebook
/// model and is never written back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputItem {
    /// Preformatted text (stream / `text/plain` / error traceback). ANSI escape
    /// codes have already been stripped where applicable.
    Text(String),
    /// A decoded raster image (`image/png` or `image/jpeg`) ready to display.
    Image(Vec<u8>),
    /// A short message shown in place of an image we cannot or will not display
    /// (invalid base64, or omitted because it is too large).
    Placeholder(String),
}

/// Classify a code cell's saved `outputs` into the read-only items the view can
/// render. Output entries that this version does not render (e.g. `text/html`,
/// LaTeX, widgets) are skipped here but remain untouched in the model so they
/// round-trip on save (invariant 18).
pub fn classify_outputs(outputs: &[Value]) -> Vec<OutputItem> {
    let mut items = Vec::new();
    for output in outputs {
        let Some(obj) = output.as_object() else {
            continue;
        };
        match obj.get("output_type").and_then(Value::as_str) {
            Some("stream") => {
                if let Some(text) = obj.get("text").and_then(string_or_array) {
                    items.push(OutputItem::Text(truncate_for_display(text)));
                }
            }
            Some("error") => {
                let traceback = obj
                    .get("traceback")
                    .and_then(string_or_array)
                    .unwrap_or_else(|| error_summary(obj));
                items.push(OutputItem::Text(truncate_for_display(strip_ansi(
                    &traceback,
                ))));
            }
            Some("execute_result") | Some("display_data") => {
                if let Some(data) = obj.get("data").and_then(Value::as_object) {
                    if let Some(item) = image_item(data) {
                        items.push(item);
                    } else if let Some(text) = data.get("text/plain").and_then(string_or_array) {
                        items.push(OutputItem::Text(truncate_for_display(text)));
                    }
                    // Other MIME types (text/html, latex, widgets, ...) are not
                    // rendered in v1, but remain preserved by the model.
                }
            }
            // Unknown or absent output types are not rendered, but the model
            // keeps them verbatim for save.
            Some(_) | None => {}
        }
    }
    items
}

/// Build an [`OutputItem`] for the first supported image MIME type present in a
/// `data` map, or `None` if there is no image to render.
fn image_item(data: &serde_json::Map<String, Value>) -> Option<OutputItem> {
    for mime in ["image/png", "image/jpeg"] {
        let Some(encoded) = data.get(mime).and_then(string_or_array) else {
            continue;
        };
        // nbformat stores image payloads as base64, often split across lines.
        let cleaned: String = encoded.split_whitespace().collect();
        return Some(match BASE64_STANDARD.decode(cleaned.as_bytes()) {
            Ok(bytes) if bytes.len() > MAX_IMAGE_BYTES => OutputItem::Placeholder(format!(
                "[image output omitted for display: {} KB]",
                bytes.len() / 1024
            )),
            Ok(bytes) => OutputItem::Image(bytes),
            Err(_) => OutputItem::Placeholder("invalid image data".to_string()),
        });
    }
    None
}

/// Build a fallback one-line summary for an `error` output that has no
/// `traceback` (`ename: evalue`).
fn error_summary(obj: &serde_json::Map<String, Value>) -> String {
    let ename = obj.get("ename").and_then(Value::as_str).unwrap_or("");
    let evalue = obj.get("evalue").and_then(Value::as_str).unwrap_or("");
    match (ename.is_empty(), evalue.is_empty()) {
        (true, true) => String::new(),
        (false, true) => ename.to_string(),
        (true, false) => evalue.to_string(),
        (false, false) => format!("{ename}: {evalue}"),
    }
}

/// Collapse an nbformat string-or-list field into a single `String`. nbformat
/// allows these fields to be either a single string or a list of line strings.
fn string_or_array(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Array(items) => {
            let mut out = String::new();
            for item in items {
                if let Value::String(s) = item {
                    out.push_str(s);
                }
            }
            Some(out)
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::Object(_) => None,
    }
}

/// Visually cap a text output at [`MAX_TEXT_OUTPUT_BYTES`], appending a marker so
/// the user knows display was truncated. The original bytes in the model are
/// never affected (invariant 17).
fn truncate_for_display(text: String) -> String {
    if text.len() <= MAX_TEXT_OUTPUT_BYTES {
        return text;
    }
    let mut end = MAX_TEXT_OUTPUT_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let mut truncated = text[..end].to_string();
    truncated.push_str("\n… [output truncated for display]");
    truncated
}

/// Remove ANSI escape sequences (used by tracebacks for color) from `input`,
/// leaving only the visible text (invariant 5).
pub fn strip_ansi(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        match chars.peek().copied() {
            // CSI sequence: ESC '[' params ... final byte in 0x40..=0x7e.
            Some('[') => {
                chars.next();
                while let Some(&next) = chars.peek() {
                    chars.next();
                    if ('\u{40}'..='\u{7e}').contains(&next) {
                        break;
                    }
                }
            }
            // OSC sequence: ESC ']' ... terminated by BEL or ST (ESC '\').
            Some(']') => {
                chars.next();
                while let Some(&next) = chars.peek() {
                    if next == '\u{07}' {
                        chars.next();
                        break;
                    }
                    if next == '\u{1b}' {
                        chars.next();
                        if chars.peek().copied() == Some('\\') {
                            chars.next();
                        }
                        break;
                    }
                    chars.next();
                }
            }
            // Any other escape: drop the single following byte.
            Some(_) => {
                chars.next();
            }
            None => {}
        }
    }
    out
}

#[cfg(test)]
#[path = "output_tests.rs"]
mod tests;
