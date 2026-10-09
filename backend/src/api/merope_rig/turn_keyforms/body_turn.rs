//! 全身转身关键形：照转头的做法，以立绘为参考生成整个人向画面右、左转约 20°
//! 的两张图（脚不动），各拆一次层，在 Space 上把全身拆层的每个身体部件
//! （头整体、脖子、上衣、下装、左右大腿小腿、左右鞋、左右手臂）拟合到两张图
//! 上（`/body_keys`），只留下左右方向相反的那部分位移，放到全身的图上。
//!
//! 两张图与两份拆层按全身的图存在缓存里；拟合每次重做。拿不到身体关键形时，
//! 全身照常只带头部关键形：这一步的失败不让整个任务失败。

use std::io::Cursor;

use serde_json::Value;

use super::{ApiResult, Stage, TurnCache, draw_turn, internal_error, see_through_error, update};
use crate::services::{image_generation, see_through};

/// Bump when the body turn's prompt or framing changes: cached drawings go with it.
const BODY_CACHE: &str = "body-v1";
/// Image generators draw a standing figure on a 2:3 canvas; a figure padded to
/// it keeps its size and place instead of being stretched to fill.
const BODY_ASPECT: (u32, u32) = (2, 3);

/// Toward the viewer's right and left: the Space's order, Myriad's torsoTurn ±1.
const BODY_SIDES: [&str; 2] = ["right", "left"];

fn body_turn_prompt(side: &str) -> String {
    format!(
        "Edit this exact illustration. Keep the canvas size, framing, the character's size and the \
         spot she stands on, the outfit, colors, line style and shading exactly the same, on the \
         same background. Change only the pose: she turns her whole body about 20 degrees toward the \
         viewer's {side}, a natural three-quarter standing view, feet staying where they are; her \
         head turns with her body and her eyes keep looking at the viewer. Every garment, accessory, \
         hair lock and ornament keeps its exact design, colors and length, seen from the new angle; \
         her arms keep their pose relative to her body and her hands keep their shape. Clean \
         finished art in the original's style."
    )
}

/// The figure's body turn keys, `{canvas, keyforms, drift, knees}` on `picture`
/// (the picture `figure` was decomposed from), or None when they could not be made.
#[allow(clippy::too_many_arguments)]
pub(super) async fn body_turn_keys(
    id: &str,
    user_id: i32,
    generation: &image_generation::ImageGenerationConfig,
    master: &image_generation::ImageReference,
    picture: &image_generation::ImageReference,
    figure: &[u8],
    client: &see_through::SeeThroughClient,
    cache: &TurnCache,
) -> Option<Value> {
    match make(
        id, user_id, generation, master, picture, figure, client, cache,
    )
    .await
    {
        Ok(keys) => Some(keys),
        Err((_, axum::Json(error))) => {
            tracing::warn!(job = %id, ?error, "body turn keys could not be made; the figure keeps its head keys only");
            None
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn make(
    id: &str,
    user_id: i32,
    generation: &image_generation::ImageGenerationConfig,
    master: &image_generation::ImageReference,
    picture: &image_generation::ImageReference,
    figure: &[u8],
    client: &see_through::SeeThroughClient,
    cache: &TurnCache,
) -> ApiResult<Value> {
    update(id, |job| {
        job.stage = Stage::Generating;
        job.done = 0;
        job.total = 2;
    });
    let (padded, size, place) = padded(&master.bytes)?;
    let padded = image_generation::ImageReference {
        bytes: axum::body::Bytes::from(padded),
        media_type: "image/png".to_string(),
    };
    let mut drawn = Vec::new();
    for side in BODY_SIDES {
        let file = format!("{BODY_CACHE}-{side}.png");
        let png = match cache.read(&file).await.filter(|png| !png.is_empty()) {
            Some(png) => png,
            None => {
                let png = draw_turn(
                    user_id,
                    generation,
                    &body_turn_prompt(side),
                    &padded,
                    size.0,
                    size.1,
                )
                .await?;
                let png = cropped(&png, place)?;
                cache.write(&file, &png).await;
                png
            }
        };
        drawn.push(png);
        update(id, |job| job.done += 1);
    }
    update(id, |job| {
        job.stage = Stage::Decomposing;
        job.done = 0;
        job.total = 2;
    });
    let mut turned = Vec::new();
    for (side, png) in BODY_SIDES.iter().zip(&drawn) {
        let file = format!("{BODY_CACHE}-{side}.psd");
        let psd = match cache.read(&file).await.filter(|psd| !psd.is_empty()) {
            Some(psd) => psd,
            None => {
                let output = client
                    .decompose(
                        axum::body::Bytes::from(png.clone()),
                        "image/png",
                        see_through::DecomposeOptions::default(),
                    )
                    .await
                    .map_err(see_through_error)?;
                cache.write(&file, &output.psd).await;
                output.psd
            }
        };
        turned.push(psd);
        update(id, |job| job.done += 1);
    }
    update(id, |job| {
        job.stage = Stage::Fitting;
        job.done = 0;
        job.total = 1;
    });
    let [right_psd, left_psd]: [Vec<u8>; 2] = turned.try_into().expect("two sides");
    let [right_png, left_png]: [Vec<u8>; 2] = drawn.try_into().expect("two sides");
    let keys = client
        .body_keys(
            (picture.bytes.to_vec(), picture.media_type.clone()),
            figure.to_vec(),
            [right_psd, left_psd],
            [right_png, left_png],
        )
        .await
        .map_err(see_through_error)?;
    update(id, |job| job.done += 1);
    serde_json::from_slice(&keys).map_err(internal_error)
}

/// The master on a white BODY_ASPECT canvas, the canvas' size, and where the master sits on it.
fn padded(bytes: &[u8]) -> ApiResult<(Vec<u8>, (u32, u32), [u32; 4])> {
    let master = image::load_from_memory(bytes)
        .map_err(internal_error)?
        .to_rgba8();
    let (w, h) = master.dimensions();
    let (aw, ah) = BODY_ASPECT;
    let (width, height) = if w * ah < h * aw {
        ((h * aw).div_ceil(ah), h)
    } else {
        (w, (w * ah).div_ceil(aw))
    };
    let (x, y) = ((width - w) / 2, (height - h) / 2);
    let mut canvas = image::RgbaImage::from_pixel(width, height, image::Rgba([255, 255, 255, 255]));
    image::imageops::overlay(&mut canvas, &master, x.into(), y.into());
    Ok((
        png(&image::DynamicImage::ImageRgba8(canvas))?,
        (width, height),
        [x, y, w, h],
    ))
}

/// The master's place cut back out of a drawing on the padded canvas.
fn cropped(bytes: &[u8], [x, y, w, h]: [u32; 4]) -> ApiResult<Vec<u8>> {
    let drawn = image::load_from_memory(bytes).map_err(internal_error)?;
    png(&drawn.crop_imm(x, y, w, h))
}

fn png(image: &image::DynamicImage) -> ApiResult<Vec<u8>> {
    let mut out = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png)
        .map_err(internal_error)?;
    Ok(out)
}

/// The figure's keys JSON with its body turn keys beside the head's
/// (`bodyKeyforms`, and what the fit found in `bodyFit`), when both are on
/// the same picture.
pub(super) fn with_body_keys(keyforms: Vec<u8>, body: Option<&Value>) -> Vec<u8> {
    let Some(body) = body else { return keyforms };
    let Ok(mut keys) = serde_json::from_slice::<Value>(&keyforms) else {
        return keyforms;
    };
    if keys.get("canvas") != body.get("canvas") {
        tracing::warn!(head = ?keys.get("canvas"), body = ?body.get("canvas"), "body turn keys are on another canvas; left out");
        return keyforms;
    }
    let Some(object) = keys.as_object_mut() else {
        return keyforms;
    };
    object.insert("bodyKeyforms".to_string(), body["keyforms"].clone());
    object.insert(
        "bodyFit".to_string(),
        serde_json::json!({ "drift": body["drift"], "knees": body["knees"] }),
    );
    serde_json::to_vec(&keys).unwrap_or(keyforms)
}

/// The body part of a figure's keys JSON (`bodyKeyforms`, `bodyFit`), to keep
/// across a refit of the head's keys.
pub(super) fn body_of(keyforms: &[u8]) -> Option<Value> {
    let keys: Value = serde_json::from_slice(keyforms).ok()?;
    let body = keys.get("bodyKeyforms")?.clone();
    let fit = keys.get("bodyFit").cloned().unwrap_or(Value::Null);
    Some(serde_json::json!({
        "canvas": keys.get("canvas"),
        "keyforms": body,
        "drift": fit.get("drift"),
        "knees": fit.get("knees"),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture(w: u32, h: u32) -> Vec<u8> {
        let image = image::RgbaImage::from_pixel(w, h, image::Rgba([10, 20, 30, 255]));
        png(&image::DynamicImage::ImageRgba8(image)).unwrap()
    }

    #[test]
    fn a_tall_figure_is_padded_sideways_to_two_by_three_and_cut_back() {
        let (bytes, size, place) = padded(&picture(617, 1443)).unwrap();
        assert_eq!(size, (962, 1443));
        assert_eq!(place, [172, 0, 617, 1443]);
        let back = image::load_from_memory(&cropped(&bytes, place).unwrap()).unwrap();
        assert_eq!((back.width(), back.height()), (617, 1443));
        assert_eq!(back.to_rgba8().get_pixel(0, 0).0, [10, 20, 30, 255]);
        let wide = padded(&picture(900, 900)).unwrap();
        assert_eq!((wide.1, wide.2), ((900, 1350), [0, 225, 900, 900]));
    }

    #[test]
    fn body_keys_ride_beside_the_heads_and_survive_a_refit() {
        let head = br#"{"canvas":[1751,4096],"keyforms":{"face":{}}}"#.to_vec();
        let body = serde_json::json!({
            "canvas": [1751, 4096],
            "keyforms": {"topwear": {"plus": 1, "minus": 2}},
            "drift": {"topwear": [3, 4]},
            "knees": {"L": 2600},
        });
        let merged = with_body_keys(head.clone(), Some(&body));
        let keys: Value = serde_json::from_slice(&merged).unwrap();
        assert_eq!(keys["keyforms"]["face"], serde_json::json!({}));
        assert_eq!(keys["bodyKeyforms"]["topwear"]["plus"], 1);
        assert_eq!(keys["bodyFit"]["knees"]["L"], 2600);
        assert_eq!(body_of(&merged).unwrap(), body);
        assert!(body_of(&head).is_none());
        let elsewhere = serde_json::json!({ "canvas": [1088, 1664], "keyforms": {} });
        assert_eq!(with_body_keys(head.clone(), Some(&elsewhere)), head);
    }
}
