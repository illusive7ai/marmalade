use std::fs::{self, File};
use std::io::BufReader;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::meta::{AnimMeta, MAX_DIM, MAX_FRAME_FILE_BYTES, MAX_META_BYTES};

/// One decoded frame: premultiplied BGRA, ready to copy into a Win32 DIB.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
}

impl Frame {
    pub fn byte_len(&self) -> usize {
        self.bgra.len()
    }

    /// Straight-alpha RGBA -> premultiplied BGRA.
    pub fn from_rgba(width: u32, height: u32, rgba: &[u8]) -> Frame {
        let mut bgra = Vec::with_capacity(rgba.len());
        for px in rgba.as_chunks::<4>().0 {
            let a = px[3] as u32;
            let pm = |c: u8| ((c as u32 * a + 127) / 255) as u8;
            bgra.extend_from_slice(&[pm(px[2]), pm(px[1]), pm(px[0]), px[3]]);
        }
        Frame {
            width,
            height,
            bgra,
        }
    }
}

/// A fully decoded animation (all frames resident). The player holds at most one of these.
#[derive(Debug, Clone)]
pub struct Animation {
    pub meta: AnimMeta,
    pub frames: Vec<Frame>,
}

impl Animation {
    /// Load `<dir>/anim.toml` and `<dir>/0001.{webp,png}` .. `<dir>/NNNN.*`.
    pub fn load(dir: &Path) -> Result<Animation> {
        let meta_path = dir.join("anim.toml");
        if fs::metadata(&meta_path)?.len() > MAX_META_BYTES {
            return Err(Error::Meta("anim.toml is too large".into()));
        }
        let meta = AnimMeta::parse(&fs::read_to_string(&meta_path)?)?;

        let mut frames: Vec<Frame> = Vec::with_capacity(meta.frames as usize);
        for i in 1..=meta.frames {
            let frame = decode_file(&frame_path(dir, i)?)?;
            if let Some(first) = frames.first() {
                if (first.width, first.height) != (frame.width, frame.height) {
                    return Err(Error::Image(format!(
                        "frame {i} is {}x{}, expected {}x{}",
                        frame.width, frame.height, first.width, first.height
                    )));
                }
            }
            frames.push(frame);
        }
        Ok(Animation { meta, frames })
    }

    pub fn memory_bytes(&self) -> usize {
        self.frames.iter().map(Frame::byte_len).sum()
    }
}

fn frame_path(dir: &Path, index: u32) -> Result<PathBuf> {
    for ext in ["webp", "png"] {
        let p = dir.join(format!("{index:04}.{ext}"));
        if p.is_file() {
            return Ok(p);
        }
    }
    Err(Error::Image(format!(
        "missing frame {index:04}.webp/.png in {}",
        dir.display()
    )))
}

fn check_dims(w: u32, h: u32) -> Result<()> {
    if w == 0 || h == 0 || w > MAX_DIM || h > MAX_DIM {
        return Err(Error::Image(format!("{w}x{h} is outside 1..={MAX_DIM}")));
    }
    Ok(())
}

/// Decode a single frame, checking size limits before any large allocation.
pub fn decode_file(path: &Path) -> Result<Frame> {
    if fs::metadata(path)?.len() > MAX_FRAME_FILE_BYTES {
        return Err(Error::Image(format!(
            "{} is over the file size limit",
            path.display()
        )));
    }
    match path.extension().and_then(|e| e.to_str()) {
        Some("png") => decode_png(path),
        Some("webp") => decode_webp(path),
        _ => Err(Error::Image(format!(
            "unsupported file type: {}",
            path.display()
        ))),
    }
}

fn decode_png(path: &Path) -> Result<Frame> {
    let mut decoder = png::Decoder::new(BufReader::new(File::open(path)?));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder
        .read_info()
        .map_err(|e| Error::Image(e.to_string()))?;
    let (w, h) = {
        let info = reader.info();
        (info.width, info.height)
    };
    check_dims(w, h)?;

    let size = reader
        .output_buffer_size()
        .ok_or_else(|| Error::Image("image too large".into()))?;
    let mut buf = vec![0u8; size];
    let info = reader
        .next_frame(&mut buf)
        .map_err(|e| Error::Image(e.to_string()))?;
    let data = &buf[..info.buffer_size()];

    let rgba: Vec<u8> = match info.color_type {
        png::ColorType::Rgba => data.to_vec(),
        png::ColorType::Rgb => data
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => data
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        png::ColorType::Grayscale => data.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return Err(Error::Image("indexed PNG was not expanded".into())),
    };
    Ok(Frame::from_rgba(w, h, &rgba))
}

fn decode_webp(path: &Path) -> Result<Frame> {
    let mut dec = image_webp::WebPDecoder::new(BufReader::new(File::open(path)?))
        .map_err(|e| Error::Image(e.to_string()))?;
    let (w, h) = dec.dimensions();
    check_dims(w, h)?;
    let has_alpha = dec.has_alpha();

    let size = dec
        .output_buffer_size()
        .ok_or_else(|| Error::Image("image too large".into()))?;
    let mut buf = vec![0u8; size];
    dec.read_image(&mut buf)
        .map_err(|e| Error::Image(e.to_string()))?;

    let rgba: Vec<u8> = if has_alpha {
        buf
    } else {
        buf.as_chunks::<3>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect()
    };
    Ok(Frame::from_rgba(w, h, &rgba))
}
