use crate::{DevicePixels, EntityId, Pixels, Result, SharedString, Size, size};
use collections::FxHashMap;
use parking_lot::Mutex;
use smallvec::SmallVec;

use image::{Delay, Frame, imageops::FilterType};
use std::{
    borrow::Cow,
    fmt,
    hash::Hash,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering::SeqCst},
    },
};

/// A source of assets for this app to use.
pub trait AssetSource: 'static + Send + Sync {
    /// Load the given asset from the source path.
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>>;

    /// List the assets at the given path.
    fn list(&self, path: &str) -> Result<Vec<SharedString>>;
}

impl AssetSource for () {
    fn load(&self, _path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(None)
    }

    fn list(&self, _path: &str) -> Result<Vec<SharedString>> {
        Ok(vec![])
    }
}

/// A unique identifier for the image cache
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ImageId(pub usize);

#[derive(PartialEq, Eq, Hash, Clone)]
pub(crate) struct RenderImageParams {
    pub(crate) image_id: ImageId,
    pub(crate) frame_index: usize,
    /// 0 for the image itself, otherwise its downscaled copy at that [`level_size`].
    pub(crate) level: u32,
}

/// Each downscaled copy is this much smaller than the one before, so a copy is never sampled
/// at more than 1.5 texels per pixel, which bilinear filtering handles without aliasing.
const LEVEL_STEP: f32 = 1.5;
const MAX_LEVEL: u32 = 16;

/// The size of an image's downscaled copy at `level`.
pub(crate) fn level_size(size: Size<DevicePixels>, level: u32) -> Size<DevicePixels> {
    let scale = LEVEL_STEP.powi(level as i32);
    size.map(|side| DevicePixels(((side.0 as f32 / scale).round() as i32).max(1)))
}

/// The smallest copy of an image of `size` that still has `texels` pixels in both directions,
/// so it's only ever minified when drawn. 0 is the image itself.
pub(crate) fn level_for(size: Size<DevicePixels>, texels: Size<f32>) -> u32 {
    if !(texels.width >= 1. && texels.height >= 1.) {
        return 0;
    }
    let mut level = 0;
    while level < MAX_LEVEL {
        let next = level_size(size, level + 1);
        if next == level_size(size, level)
            || (next.width.0 as f32) < texels.width
            || (next.height.0 as f32) < texels.height
        {
            break;
        }
        level += 1;
    }
    level
}

/// A downscaled copy of an image, in BGRA format.
pub(crate) struct LevelPixels {
    pub(crate) size: Size<DevicePixels>,
    pub(crate) bytes: Vec<u8>,
}

enum ImageLevel {
    /// Being downscaled; the views to redraw once it's ready.
    Pending(SmallVec<[EntityId; 2]>),
    Ready(Arc<LevelPixels>),
}

/// A cached and processed image, in BGRA format
pub struct RenderImage {
    /// The ID associated with this image
    pub id: ImageId,
    /// The scale factor of this image on render.
    pub(crate) scale_factor: f32,
    data: SmallVec<[Frame; 1]>,
    levels: Mutex<FxHashMap<u32, ImageLevel>>,
}

impl PartialEq for RenderImage {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for RenderImage {}

impl RenderImage {
    /// Create a new image from the given data.
    pub fn new(data: impl Into<SmallVec<[Frame; 1]>>) -> Self {
        static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

        Self {
            id: ImageId(NEXT_ID.fetch_add(1, SeqCst)),
            scale_factor: 1.0,
            data: data.into(),
            levels: Mutex::default(),
        }
    }

    /// Convert this image into a byte slice.
    pub fn as_bytes(&self, frame_index: usize) -> Option<&[u8]> {
        self.data
            .get(frame_index)
            .map(|frame| frame.buffer().as_raw().as_slice())
    }

    /// Get the size of this image, in pixels.
    pub fn size(&self, frame_index: usize) -> Size<DevicePixels> {
        let (width, height) = self.data[frame_index].buffer().dimensions();
        size(width.into(), height.into())
    }

    /// Get the size of this image, in pixels for display, adjusted for the scale factor.
    pub(crate) fn render_size(&self, frame_index: usize) -> Size<Pixels> {
        self.size(frame_index)
            .map(|v| (v.0 as f32 / self.scale_factor).into())
    }

    /// Get the delay of this frame from the previous
    pub fn delay(&self, frame_index: usize) -> Delay {
        self.data[frame_index].delay()
    }

    /// Get the number of frames for this image.
    pub fn frame_count(&self) -> usize {
        self.data.len()
    }

    pub(crate) fn ready_level(&self, level: u32) -> Option<Arc<LevelPixels>> {
        match self.levels.lock().get(&level) {
            Some(ImageLevel::Ready(pixels)) => Some(pixels.clone()),
            _ => None,
        }
    }

    /// Records that `view` is waiting for `level`. True if nobody has started making it yet.
    pub(crate) fn request_level(&self, level: u32, view: EntityId) -> bool {
        let mut levels = self.levels.lock();
        match levels.get_mut(&level) {
            Some(ImageLevel::Pending(views)) => {
                if !views.contains(&view) {
                    views.push(view);
                }
                false
            }
            Some(ImageLevel::Ready(_)) => false,
            None => {
                levels.insert(level, ImageLevel::Pending(SmallVec::from_elem(view, 1)));
                true
            }
        }
    }

    /// Downscales the first frame to `level` and stores it, returning the views waiting for it.
    pub(crate) fn build_level(&self, level: u32) -> SmallVec<[EntityId; 2]> {
        let target = level_size(self.size(0), level);
        let resized = image::imageops::resize(
            self.data[0].buffer(),
            target.width.0 as u32,
            target.height.0 as u32,
            FilterType::CatmullRom,
        );
        let pixels = Arc::new(LevelPixels {
            size: target,
            bytes: resized.into_raw(),
        });
        match self.levels.lock().insert(level, ImageLevel::Ready(pixels)) {
            Some(ImageLevel::Pending(views)) => views,
            _ => SmallVec::new(),
        }
    }

    pub(crate) fn levels(&self) -> Vec<u32> {
        self.levels.lock().keys().copied().collect()
    }
}

impl fmt::Debug for RenderImage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ImageData")
            .field("id", &self.id)
            .field("size", &self.size(0))
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn px_size(width: i32, height: i32) -> Size<DevicePixels> {
        size(DevicePixels(width), DevicePixels(height))
    }

    #[test]
    fn level_sizes_shrink_by_one_and_a_half() {
        let full = px_size(2400, 1200);
        assert_eq!(level_size(full, 0), full);
        assert_eq!(level_size(full, 1), px_size(1600, 800));
        assert_eq!(level_size(full, 2), px_size(1067, 533));
        assert_eq!(level_size(px_size(3, 2), 10), px_size(1, 1));
    }

    #[test]
    fn picks_the_smallest_level_that_is_not_magnified() {
        let full = px_size(2400, 2400);
        assert_eq!(level_for(full, size(2400., 2400.)), 0);
        assert_eq!(level_for(full, size(1601., 1601.)), 0);
        assert_eq!(level_for(full, size(1600., 1600.)), 1);
        assert_eq!(level_for(full, size(340., 340.)), 4);
        assert_eq!(level_size(full, 4), px_size(474, 474));
        assert_eq!(level_for(full, size(40., 40.)), 10);
        assert_eq!(level_size(full, 10), px_size(42, 42));
        for texels in [20., 40., 75., 120., 333., 900.] {
            let side = level_size(full, level_for(full, size(texels, texels)))
                .width
                .0 as f32;
            assert!(
                side >= texels && side < texels * LEVEL_STEP + 1.,
                "{texels} -> {side}"
            );
        }
    }

    #[test]
    fn the_tighter_direction_decides() {
        let full = px_size(1200, 600);
        assert_eq!(level_for(full, size(100., 500.)), 0);
        assert_eq!(level_for(full, size(500., 100.)), 2);
    }

    #[test]
    fn empty_tiny_or_larger_bounds_use_the_image() {
        let full = px_size(600, 600);
        assert_eq!(level_for(full, size(0., 0.)), 0);
        assert_eq!(level_for(full, size(f32::NAN, 10.)), 0);
        assert_eq!(level_for(full, size(1200., 1200.)), 0);
        assert_eq!(level_for(px_size(1, 1), size(1., 1.)), 0);
        assert_eq!(
            level_size(px_size(4, 4), level_for(px_size(4, 4), size(1., 1.))),
            px_size(1, 1)
        );
    }

    #[test]
    fn builds_a_level_and_wakes_its_waiters() {
        let buffer = image::RgbaImage::from_pixel(300, 150, image::Rgba([10, 20, 30, 255]));
        let image = RenderImage::new(SmallVec::from_elem(Frame::new(buffer), 1));
        let view = EntityId::from(1u64);
        assert!(image.request_level(2, view));
        assert!(!image.request_level(2, view));
        assert!(image.ready_level(2).is_none());
        assert_eq!(image.build_level(2).as_slice(), &[view]);
        let pixels = image.ready_level(2).unwrap();
        assert_eq!(pixels.size, px_size(133, 67));
        assert_eq!(pixels.bytes.len(), 133 * 67 * 4);
        assert_eq!(&pixels.bytes[..4], &[10, 20, 30, 255]);
        assert!(!image.request_level(2, view));
        assert_eq!(image.levels(), vec![2]);
    }
}
