//! What an image costs an agent, and the four answers to it
//! (plan v2, Phase 11, task 8).
//!
//! Phase 11 is about never paying for content you cannot use, and an image is
//! the most expensive thing a document hands over: a single screenshot in a
//! report outweighs every word around it once it is base64. Until now the MCP
//! server had one answer — all of them, inline — which made
//! `convert_to_markdown` unusable on exactly the documents an agent most wants
//! to read.
//!
//! So the payload is a **ladder**, and the client picks the rung:
//!
//! | `include_images` | The client gets | Typical cost |
//! |---|---|---|
//! | `none` | nothing but the count and the byte total | 0 |
//! | `refs` (**default**) | name, MIME type and size of each image | a few tokens each |
//! | `downscaled` | the above plus a downscaled PNG it can actually look at | ~1 kB each |
//! | `full` | the original bytes, as before | the whole document's media |
//!
//! The third rung was called `thumbnails` until Phase 16-G, and the word was
//! wrong twice over. It is a *downscaled copy of a picture the document
//! embeds*, extracted from the package and reproducible from it — while a
//! **thumbnail**, everywhere else in this project and in every consumer an
//! agent has met, is a raster of a *whole page or slide* drawn by a renderer.
//! When 16-G added the second thing (`docsai_convert::render`), one word named
//! both, and a tool description is exactly where an agent cannot afford to
//! guess which. So the rung says what it does — `downscaled` — and the word
//! «thumbnail» is left to mean the picture of a slide. `thumbnails` is no
//! longer accepted: it is refused with a sentence naming both answers, because
//! a caller who meant the other one would otherwise be handed the wrong
//! pictures and never learn it. See `kb/81-phase-16-thumbnails.md`.
//!
//! Two rules hold at every rung. The **markdown never changes**: the body
//! always carries `![](assets/img-….png)`, so no rung is a lossy conversion and
//! a client that started at `refs` can ask for `full` later and get the same
//! document. And **an image is always accounted for**: `none` still reports how
//! many there were and what they weigh, because "no images in the response" and
//! "no images in the document" are different facts and an agent that cannot
//! tell them apart will conclude the wrong one.

use base64::Engine;
use docsai_convert::{AssetBytes, ConvertError};
use serde_json::{json, Value};

/// Longest side of a downscaled image, in pixels.
///
/// Small enough that a page of them costs less than one original, large enough
/// that a chart is still readable.
const DOWNSCALED_MAX_SIDE: u32 = 256;

/// Ceiling on a decoded image, in bytes.
///
/// A decompression bomb — a few kilobytes of PNG declaring 60 000 × 60 000
/// pixels — is a denial of service against a server that decodes whatever a
/// client points it at. The limit is what makes `downscaled` safe on untrusted
/// input; over it, the image degrades to a ref with the reason attached.
const DOWNSCALED_MAX_ALLOC: u64 = 64 * 1024 * 1024;

/// How much of an image the response carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ImagePolicy {
    /// Count and total size only.
    None,
    /// Name, type and size — the default since plan v2 Phase 11.
    #[default]
    Refs,
    /// A downscaled PNG of an embedded picture the client can look at. **Not**
    /// a picture of a page: `render_slides` is that.
    Downscaled,
    /// The original bytes, base64.
    Full,
}

impl ImagePolicy {
    /// Parses the `include_images` argument.
    pub fn parse(value: &str) -> Result<Self, ConvertError> {
        match value.trim() {
            "none" => Ok(ImagePolicy::None),
            "refs" => Ok(ImagePolicy::Refs),
            "downscaled" => Ok(ImagePolicy::Downscaled),
            // Refused rather than aliased. A caller who wrote `thumbnails`
            // after Phase 16-G may have meant either rung, and handing them one
            // of the two in silence is the failure this rename exists to
            // prevent — so both are named and the caller chooses.
            "thumbnails" => Err(ConvertError::Invalid(
                "`include_images=thumbnails` was renamed in Phase 16-G because it named two \
                 different things. For downscaled copies of the pictures this document embeds, \
                 pass `downscaled`. For a raster of a whole slide, call the `render_slides` tool"
                    .into(),
            )),
            "full" => Ok(ImagePolicy::Full),
            other => Err(ConvertError::Invalid(format!(
                "unknown include_images `{other}`; expected none, refs, downscaled or full"
            ))),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            ImagePolicy::None => "none",
            ImagePolicy::Refs => "refs",
            ImagePolicy::Downscaled => "downscaled",
            ImagePolicy::Full => "full",
        }
    }
}

/// The `assets` rows of a tool response, under `policy`.
pub fn image_payloads(assets: &[AssetBytes], policy: ImagePolicy) -> Vec<Value> {
    if policy == ImagePolicy::None {
        return Vec::new();
    }
    assets
        .iter()
        .map(|asset| {
            let mut row = json!({
                "file_name": asset.file_name,
                "content_type": asset.content_type,
                "byte_len": asset.data.len(),
            });
            let object = row.as_object_mut().expect("object");
            match policy {
                ImagePolicy::Full => {
                    object.insert("content_base64".into(), Value::String(encode(&asset.data)));
                }
                ImagePolicy::Downscaled => match downscale(&asset.data, &asset.content_type) {
                    Ok(small) => {
                        object.insert(
                            "downscaled_base64".into(),
                            Value::String(encode(&small.bytes)),
                        );
                        object.insert("downscaled_content_type".into(), json!(small.content_type));
                        object.insert("downscaled_width".into(), json!(small.width));
                        object.insert("downscaled_height".into(), json!(small.height));
                    }
                    // Never silently: a client that asked to see the image is
                    // told which one it cannot see and why, and still has the
                    // name to ask for it at `full`.
                    Err(reason) => {
                        object.insert("downscaled_base64".into(), Value::Null);
                        object.insert("downscaled_note".into(), Value::String(reason));
                    }
                },
                ImagePolicy::None | ImagePolicy::Refs => {}
            }
            row
        })
        .collect()
}

/// What the client gets to look at, and what it turned out to be.
struct Downscaled {
    bytes: Vec<u8>,
    content_type: String,
    width: u32,
    height: u32,
}

/// Decodes `data`, fits it inside [`DOWNSCALED_MAX_SIDE`] and re-encodes as PNG
/// — unless doing so would cost more than sending the image itself.
///
/// A document is full of icons and logos already smaller than the box, and
/// re-encoding one of those produces a *larger* PNG than the original: the rung
/// would then charge the client more for less. So the invariant is the one the
/// name promises — **a downscaled image never costs more than the image it
/// stands for** — and when downscaling does not pay, the original is what gets sent,
/// with its own type and dimensions declared.
///
/// The error is a sentence for the client, not a type: every failure here has
/// the same handling — say so on the row and carry on — and the formats that
/// reach it (EMF, WMF, TIFF, SVG) are the ones no pure-Rust decoder in the
/// dependency budget reads.
fn downscale(data: &[u8], content_type: &str) -> Result<Downscaled, String> {
    let mut reader = image::ImageReader::new(std::io::Cursor::new(data))
        .with_guessed_format()
        .map_err(|e| format!("could not read the image header: {e}"))?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(DOWNSCALED_MAX_ALLOC);
    reader.limits(limits);
    let format = reader
        .format()
        .ok_or_else(|| "not a raster format this build can decode".to_string())?;
    let image = reader
        .decode()
        .map_err(|e| format!("{format:?} could not be decoded: {e}"))?;

    let small = image.thumbnail(DOWNSCALED_MAX_SIDE, DOWNSCALED_MAX_SIDE);
    let mut png = Vec::new();
    small
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|e| format!("the downscaled copy could not be encoded: {e}"))?;

    if png.len() >= data.len() {
        return Ok(Downscaled {
            bytes: data.to_vec(),
            content_type: content_type.to_string(),
            width: image.width(),
            height: image.height(),
        });
    }
    Ok(Downscaled {
        width: small.width(),
        height: small.height(),
        bytes: png,
        content_type: "image/png".into(),
    })
}

fn encode(data: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(width: u32, height: u32) -> Vec<u8> {
        let image = image::RgbImage::from_fn(width, height, |x, y| {
            image::Rgb([(x % 256) as u8, (y % 256) as u8, 0])
        });
        let mut out = Vec::new();
        image::DynamicImage::ImageRgb8(image)
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .expect("encode");
        out
    }

    fn asset(data: Vec<u8>) -> AssetBytes {
        AssetBytes {
            file_name: "img-1.png".into(),
            content_type: "image/png".into(),
            data,
        }
    }

    #[test]
    fn refs_is_the_default_and_carries_no_bytes() {
        assert_eq!(ImagePolicy::default(), ImagePolicy::Refs);
        let rows = image_payloads(&[asset(png(8, 8))], ImagePolicy::Refs);
        assert_eq!(rows.len(), 1);
        assert!(rows[0]["byte_len"].as_u64().unwrap() > 0);
        assert!(rows[0].get("content_base64").is_none());
        assert!(rows[0].get("downscaled_base64").is_none());
    }

    #[test]
    fn none_lists_nothing_but_the_caller_still_counts_them() {
        // The count lives in the response, not in the rows: see
        // `tool_convert_to_markdown`. What `none` guarantees is empty rows.
        assert!(image_payloads(&[asset(png(8, 8))], ImagePolicy::None).is_empty());
    }

    #[test]
    fn a_downscaled_image_is_smaller_than_the_image_it_stands_for() {
        let original = png(1024, 768);
        let rows = image_payloads(&[asset(original.clone())], ImagePolicy::Downscaled);
        let small = rows[0]["downscaled_base64"]
            .as_str()
            .expect("a downscaled copy");
        assert_eq!(rows[0]["downscaled_width"], 256);
        assert_eq!(rows[0]["downscaled_height"], 192);
        assert_eq!(rows[0]["downscaled_content_type"], "image/png");
        assert!(
            small.len() < original.len(),
            "downscaled {} bytes, original {} bytes",
            small.len(),
            original.len()
        );
        // The rung is a payload choice, not a conversion: the original is still
        // there to ask for by name.
        assert_eq!(rows[0]["byte_len"], original.len());
    }

    #[test]
    fn an_image_that_cannot_be_decoded_says_so_instead_of_disappearing() {
        let rows = image_payloads(
            &[AssetBytes {
                file_name: "drawing.emf".into(),
                content_type: "image/x-emf".into(),
                data: b"\x01\x00\x00\x00 not an image".to_vec(),
            }],
            ImagePolicy::Downscaled,
        );
        assert!(rows[0]["downscaled_base64"].is_null());
        assert!(!rows[0]["downscaled_note"].as_str().unwrap().is_empty());
        assert_eq!(rows[0]["file_name"], "drawing.emf");
    }

    #[test]
    fn an_image_already_smaller_than_the_box_is_sent_as_it_is() {
        // Icons and logos: re-encoding them costs more than sending them, and
        // the rung exists to save the client bytes, not to spend them.
        let original = png(12, 9);
        let rows = image_payloads(&[asset(original.clone())], ImagePolicy::Downscaled);
        let small = rows[0]["downscaled_base64"]
            .as_str()
            .expect("a downscaled copy");
        assert!(small.len() <= original.len() * 2, "base64 of the original");
        assert_eq!(rows[0]["downscaled_width"], 12);
        assert_eq!(rows[0]["downscaled_height"], 9);
        assert_eq!(rows[0]["downscaled_content_type"], "image/png");
    }

    #[test]
    fn full_is_the_old_behaviour_and_still_available() {
        let rows = image_payloads(&[asset(png(8, 8))], ImagePolicy::Full);
        assert!(!rows[0]["content_base64"].as_str().unwrap().is_empty());
    }

    #[test]
    fn an_unknown_rung_is_refused_with_the_list() {
        let err = ImagePolicy::parse("some").unwrap_err().to_string();
        assert!(err.contains("none, refs, downscaled or full"), "{err}");
    }

    #[test]
    fn the_old_word_is_refused_with_both_answers_rather_than_aliased() {
        // The whole point of the rename: a caller who wrote `thumbnails` may
        // have meant either thing, so neither is guessed for them.
        let err = ImagePolicy::parse("thumbnails").unwrap_err().to_string();
        assert!(err.contains("downscaled"), "{err}");
        assert!(err.contains("render_slides"), "{err}");
    }
}
