use super::metal_atlas::MetalAtlas;
use crate::{
    AtlasTextureId, BackdropBlur, Background, Bounds, ContentMask, DevicePixels, MonochromeSprite,
    PaintSurface,
    Path, Point, PolychromeSprite, PrimitiveBatch, Quad, ScaledPixels, Scene, Shadow, Size,
    Surface, Underline, point, size,
};
use anyhow::Result;
use block::ConcreteBlock;
use cocoa::{
    base::{NO, YES},
    foundation::{NSSize, NSUInteger},
    quartzcore::AutoresizingMask,
};

use core_foundation::base::TCFType;
use core_video::{
    metal_texture::CVMetalTextureGetTexture, metal_texture_cache::CVMetalTextureCache,
    pixel_buffer::kCVPixelFormatType_420YpCbCr8BiPlanarFullRange,
};
use foreign_types::{ForeignType, ForeignTypeRef};
use core_graphics::color_space::{CGColorSpace, kCGColorSpaceExtendedSRGB};
use metal::{
    CAMetalLayer, CommandQueue, MTLGPUFamily, MTLPixelFormat, MTLResourceOptions, NSRange,
    RenderPassColorAttachmentDescriptorRef,
};
use objc::{self, msg_send, sel, sel_impl};
use parking_lot::Mutex;

use std::{cell::Cell, ffi::c_void, mem, ptr, sync::Arc};

// Exported to metal
pub(crate) type PointF = crate::Point<f32>;

#[cfg(not(feature = "runtime_shaders"))]
const SHADERS_METALLIB: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/shaders.metallib"));
#[cfg(feature = "runtime_shaders")]
const SHADERS_SOURCE_FILE: &str = include_str!(concat!(env!("OUT_DIR"), "/stitched_shaders.metal"));
// Use 4x MSAA, all devices support it.
// https://developer.apple.com/documentation/metal/mtldevice/1433355-supportstexturesamplecount
const PATH_SAMPLE_COUNT: u32 = 4;

pub type Context = Arc<Mutex<InstanceBufferPool>>;
pub type Renderer = MetalRenderer;

pub unsafe fn new_renderer(
    context: self::Context,
    _native_window: *mut c_void,
    _native_view: *mut c_void,
    _bounds: crate::Size<f32>,
    transparent: bool,
) -> Renderer {
    MetalRenderer::new(context, transparent)
}

pub(crate) struct InstanceBufferPool {
    buffer_size: usize,
    buffers: Vec<metal::Buffer>,
}

impl Default for InstanceBufferPool {
    fn default() -> Self {
        Self {
            buffer_size: 2 * 1024 * 1024,
            buffers: Vec::new(),
        }
    }
}

pub(crate) struct InstanceBuffer {
    metal_buffer: metal::Buffer,
    size: usize,
}

impl InstanceBufferPool {
    pub(crate) fn reset(&mut self, buffer_size: usize) {
        self.buffer_size = buffer_size;
        self.buffers.clear();
    }

    pub(crate) fn acquire(&mut self, device: &metal::Device) -> InstanceBuffer {
        let buffer = self.buffers.pop().unwrap_or_else(|| {
            device.new_buffer(
                self.buffer_size as u64,
                MTLResourceOptions::StorageModeManaged,
            )
        });
        InstanceBuffer {
            metal_buffer: buffer,
            size: self.buffer_size,
        }
    }

    pub(crate) fn release(&mut self, buffer: InstanceBuffer) {
        if buffer.size == self.buffer_size {
            self.buffers.push(buffer.metal_buffer)
        }
    }
}

pub(crate) struct MetalRenderer {
    device: metal::Device,
    layer: metal::MetalLayer,
    presents_with_transaction: bool,
    command_queue: CommandQueue,
    library: metal::Library,
    paths_rasterization_pipeline_state: metal::RenderPipelineState,
    pipelines: DrawablePipelines,
    /// Scratch textures for backdrop blurs, kept between frames: the copy of the drawable,
    /// and a shrunken one for the first of the two blur passes.
    blur_textures: Option<BlurTextures>,
    /// Each blur of the last frame, blurred, for the next frame to reuse while its backdrop
    /// is unchanged.
    blur_cache: Vec<CachedBlur>,
    /// Textures of blurs that went away, to blur into again.
    blur_spares: Vec<CachedBlur>,
    unit_vertices: metal::Buffer,
    #[allow(clippy::arc_with_non_send_sync)]
    instance_buffer_pool: Arc<Mutex<InstanceBufferPool>>,
    sprite_atlas: Arc<MetalAtlas>,
    core_video_texture_cache: core_video::metal_texture_cache::CVMetalTextureCache,
    path_intermediate_texture: Option<metal::Texture>,
    path_intermediate_msaa_texture: Option<metal::Texture>,
    path_sample_count: u32,
}

struct BlurTextures {
    capture: metal::Texture,
    capture_size: (u64, u64),
    low: metal::Texture,
    low_size: (u64, u64),
}

struct CachedBlur {
    /// The blur's geometry and backdrop signature; `None` when it can't be reused.
    key: Option<u64>,
    texture: metal::Texture,
    size: (u64, u64),
}

/// Frames that reuse at most this many textures of blurs that went away.
const MAX_SPARE_BLURS: usize = 4;

/// Matches `BlurPass` in shaders.metal.
#[repr(C)]
struct BlurPass {
    source_scale: [f32; 2],
    source_max: [f32; 2],
    direction: [f32; 2],
    sigma: f32,
    _pad: f32,
}

/// Matches `BackdropComposite` in shaders.metal.
#[repr(C)]
struct BackdropComposite {
    origin: [f32; 2],
    low_max: [f32; 2],
    downsample: f32,
    _pad: f32,
}

#[repr(C)]
pub struct PathRasterizationVertex {
    pub xy_position: Point<ScaledPixels>,
    pub st_position: Point<f32>,
    pub color: Background,
    pub bounds: Bounds<ScaledPixels>,
}

impl MetalRenderer {
    pub fn new(instance_buffer_pool: Arc<Mutex<InstanceBufferPool>>, transparent: bool) -> Self {
        // Prefer low‐power integrated GPUs on Intel Mac. On Apple
        // Silicon, there is only ever one GPU, so this is equivalent to
        // `metal::Device::system_default()`.
        let mut devices = metal::Device::all();
        devices.sort_by_key(|device| (device.is_removable(), device.is_low_power()));
        let Some(device) = devices.pop() else {
            log::error!("unable to access a compatible graphics device");
            std::process::exit(1);
        };

        let layer = metal::MetalLayer::new();
        layer.set_device(&device);
        // Without a colorspace the compositor doesn't color-match the drawable, so sRGB colors
        // show as the display's own primaries, oversaturated on wide-gamut displays.
        if let Some(extended_srgb) =
            CGColorSpace::create_with_name(unsafe { kCGColorSpaceExtendedSRGB })
        {
            unsafe {
                let _: () = msg_send![&*layer, setColorspace: extended_srgb.as_ptr()];
            }
        }
        layer.set_maximum_drawable_count(3);
        // Backdrop blurs read the drawable back.
        layer.set_framebuffer_only(false);
        unsafe {
            let _: () = msg_send![&*layer, setAllowsNextDrawableTimeout: NO];
            let _: () = msg_send![&*layer, setNeedsDisplayOnBoundsChange: YES];
            let _: () = msg_send![
                &*layer,
                setAutoresizingMask: AutoresizingMask::WIDTH_SIZABLE
                    | AutoresizingMask::HEIGHT_SIZABLE
            ];
        }
        #[cfg(feature = "runtime_shaders")]
        let library = device
            .new_library_with_source(&SHADERS_SOURCE_FILE, &metal::CompileOptions::new())
            .expect("error building metal library");
        #[cfg(not(feature = "runtime_shaders"))]
        let library = device
            .new_library_with_data(SHADERS_METALLIB)
            .expect("error building metal library");

        fn to_float2_bits(point: PointF) -> u64 {
            let mut output = point.y.to_bits() as u64;
            output <<= 32;
            output |= point.x.to_bits() as u64;
            output
        }

        let unit_vertices = [
            to_float2_bits(point(0., 0.)),
            to_float2_bits(point(1., 0.)),
            to_float2_bits(point(0., 1.)),
            to_float2_bits(point(0., 1.)),
            to_float2_bits(point(1., 0.)),
            to_float2_bits(point(1., 1.)),
        ];
        let unit_vertices = device.new_buffer_with_data(
            unit_vertices.as_ptr() as *const c_void,
            mem::size_of_val(&unit_vertices) as u64,
            MTLResourceOptions::StorageModeManaged,
        );

        let paths_rasterization_pipeline_state = build_path_rasterization_pipeline_state(
            &device,
            &library,
            "paths_rasterization",
            "path_rasterization_vertex",
            "path_rasterization_fragment",
            MTLPixelFormat::BGRA8Unorm,
            PATH_SAMPLE_COUNT,
        );
        let format = drawable_format(&device, transparent);
        layer.set_pixel_format(format);
        layer.set_opaque(!has_alpha(format));
        let pipelines = DrawablePipelines::new(&device, &library, format);

        let command_queue = device.new_command_queue();
        let sprite_atlas = Arc::new(MetalAtlas::new(device.clone()));
        let core_video_texture_cache =
            CVMetalTextureCache::new(None, device.clone(), None).unwrap();

        Self {
            device,
            layer,
            presents_with_transaction: false,
            command_queue,
            library,
            paths_rasterization_pipeline_state,
            pipelines,
            blur_textures: None,
            blur_cache: Vec::new(),
            blur_spares: Vec::new(),
            unit_vertices,
            instance_buffer_pool,
            sprite_atlas,
            core_video_texture_cache,
            path_intermediate_texture: None,
            path_intermediate_msaa_texture: None,
            path_sample_count: PATH_SAMPLE_COUNT,
        }
    }

    pub fn layer(&self) -> &metal::MetalLayerRef {
        &self.layer
    }

    pub fn layer_ptr(&self) -> *mut CAMetalLayer {
        self.layer.as_ptr()
    }

    pub fn sprite_atlas(&self) -> &Arc<MetalAtlas> {
        &self.sprite_atlas
    }

    pub fn set_presents_with_transaction(&mut self, presents_with_transaction: bool) {
        self.presents_with_transaction = presents_with_transaction;
        self.layer
            .set_presents_with_transaction(presents_with_transaction);
    }

    pub fn update_drawable_size(&mut self, size: Size<DevicePixels>) {
        let size = NSSize {
            width: size.width.0 as f64,
            height: size.height.0 as f64,
        };
        unsafe {
            let _: () = msg_send![
                self.layer(),
                setDrawableSize: size
            ];
        }
        let device_pixels_size = Size {
            width: DevicePixels(size.width as i32),
            height: DevicePixels(size.height as i32),
        };
        self.update_path_intermediate_textures(device_pixels_size);
    }

    /// Renders `scene` into an offscreen texture the size of the drawable times `factor` and
    /// reads it back as 8-bit sRGB, clamping wide-gamut colors.
    pub fn snapshot(&mut self, scene: &Scene, factor: f32) -> Option<image::RgbaImage> {
        let drawable_size = self.layer.drawable_size();
        let factor = f64::from(factor);
        let (width, height) =
            ((drawable_size.width * factor).ceil() as u32, (drawable_size.height * factor).ceil() as u32);
        if width == 0 || height == 0 {
            return None;
        }
        let format = self.layer.pixel_format();
        let descriptor = metal::TextureDescriptor::new();
        descriptor.set_width(width as u64);
        descriptor.set_height(height as u64);
        descriptor.set_pixel_format(format);
        descriptor.set_storage_mode(metal::MTLStorageMode::Managed);
        descriptor.set_usage(metal::MTLTextureUsage::RenderTarget | metal::MTLTextureUsage::ShaderRead);
        let target = self.device.new_texture(&descriptor);
        let viewport = size(DevicePixels(width as i32), DevicePixels(height as i32));
        self.update_path_intermediate_textures(viewport);

        let mut instance_buffer = self.instance_buffer_pool.lock().acquire(&self.device);
        let command_buffer = self.draw_primitives(scene, &mut instance_buffer, &target, viewport).ok()?;
        let blit = command_buffer.new_blit_command_encoder();
        blit.synchronize_resource(&target);
        blit.end_encoding();
        command_buffer.commit();
        command_buffer.wait_until_completed();
        self.instance_buffer_pool.lock().release(instance_buffer);
        // Paths draw through textures sized for the viewport; put them back for the window.
        self.update_path_intermediate_textures(size(
            DevicePixels(drawable_size.width.ceil() as i32),
            DevicePixels(drawable_size.height.ceil() as i32),
        ));

        let bytes_per_pixel = match format {
            MTLPixelFormat::RGBA16Float => 8,
            _ => 4,
        };
        let mut pixels = vec![0u8; width as usize * height as usize * bytes_per_pixel];
        target.get_bytes(
            pixels.as_mut_ptr().cast(),
            (width as usize * bytes_per_pixel) as u64,
            metal::MTLRegion::new_2d(0, 0, width as u64, height as u64),
            0,
        );
        let to_byte = |v: f32| (v.clamp(0., 1.) * 255. + 0.5) as u8;
        let half = |bits: u16| {
            let sign = if bits & 0x8000 != 0 { -1. } else { 1. };
            let exponent = ((bits >> 10) & 0x1f) as i32;
            let mantissa = (bits & 0x3ff) as f32;
            sign * if exponent == 0 {
                mantissa / 1024. * 2f32.powi(-14)
            } else {
                (1. + mantissa / 1024.) * 2f32.powi(exponent - 15)
            }
        };
        let mut out = image::RgbaImage::new(width, height);
        for (i, pixel) in out.pixels_mut().enumerate() {
            let p = &pixels[i * bytes_per_pixel..(i + 1) * bytes_per_pixel];
            *pixel = match format {
                MTLPixelFormat::RGBA16Float => {
                    let c = |k: usize| half(u16::from_le_bytes([p[2 * k], p[2 * k + 1]]));
                    image::Rgba([to_byte(c(0)), to_byte(c(1)), to_byte(c(2)), 255])
                }
                MTLPixelFormat::BGRA8Unorm => image::Rgba([p[2], p[1], p[0], 255]),
                _ => {
                    // BGR10_XR: 384 is 0 and 894 is 1.
                    let packed = u32::from_le_bytes(p.try_into().unwrap());
                    let field = |shift: u32| (((packed >> shift) & 0x3ff) as f32 - 384.) / 510.;
                    image::Rgba([to_byte(field(20)), to_byte(field(10)), to_byte(field(0)), 255])
                }
            };
        }
        Some(out)
    }

    fn update_path_intermediate_textures(&mut self, size: Size<DevicePixels>) {
        // We are uncertain when this happens, but sometimes size can be 0 here. Most likely before
        // the layout pass on window creation. Zero-sized texture creation causes SIGABRT.
        // https://github.com/zed-industries/zed/issues/36229
        if size.width.0 <= 0 || size.height.0 <= 0 {
            self.path_intermediate_texture = None;
            self.path_intermediate_msaa_texture = None;
            return;
        }

        let texture_descriptor = metal::TextureDescriptor::new();
        texture_descriptor.set_width(size.width.0 as u64);
        texture_descriptor.set_height(size.height.0 as u64);
        texture_descriptor.set_pixel_format(metal::MTLPixelFormat::BGRA8Unorm);
        texture_descriptor
            .set_usage(metal::MTLTextureUsage::RenderTarget | metal::MTLTextureUsage::ShaderRead);
        self.path_intermediate_texture = Some(self.device.new_texture(&texture_descriptor));

        if self.path_sample_count > 1 {
            let mut msaa_descriptor = texture_descriptor;
            msaa_descriptor.set_texture_type(metal::MTLTextureType::D2Multisample);
            msaa_descriptor.set_storage_mode(metal::MTLStorageMode::Private);
            msaa_descriptor.set_sample_count(self.path_sample_count as _);
            self.path_intermediate_msaa_texture = Some(self.device.new_texture(&msaa_descriptor));
        } else {
            self.path_intermediate_msaa_texture = None;
        }
    }

    pub fn update_transparency(&mut self, transparent: bool) {
        let format = drawable_format(&self.device, transparent);
        if format == self.layer.pixel_format() {
            return;
        }
        self.layer.set_pixel_format(format);
        self.layer.set_opaque(!has_alpha(format));
        self.pipelines = DrawablePipelines::new(&self.device, &self.library, format);
        self.blur_textures = None;
        self.blur_cache.clear();
        self.blur_spares.clear();
    }

    pub fn destroy(&self) {
        // nothing to do
    }

    pub fn draw(&mut self, scene: &Scene) {
        let layer = self.layer.clone();
        let viewport_size = layer.drawable_size();
        let viewport_size: Size<DevicePixels> = size(
            (viewport_size.width.ceil() as i32).into(),
            (viewport_size.height.ceil() as i32).into(),
        );
        let drawable = if let Some(drawable) = layer.next_drawable() {
            drawable
        } else {
            log::error!(
                "failed to retrieve next drawable, drawable size: {:?}",
                viewport_size
            );
            return;
        };

        loop {
            let mut instance_buffer = self.instance_buffer_pool.lock().acquire(&self.device);

            let command_buffer =
                self.draw_primitives(scene, &mut instance_buffer, drawable.texture(), viewport_size);

            match command_buffer {
                Ok(command_buffer) => {
                    let instance_buffer_pool = self.instance_buffer_pool.clone();
                    let instance_buffer = Cell::new(Some(instance_buffer));
                    let block = ConcreteBlock::new(move |command_buffer: &metal::CommandBufferRef| {
                        let (start, end): (f64, f64) = unsafe {
                            (msg_send![command_buffer, GPUStartTime], msg_send![command_buffer, GPUEndTime])
                        };
                        if end > start {
                            crate::frame_timings::record_gpu_time(std::time::Duration::from_secs_f64(end - start));
                        }
                        if let Some(instance_buffer) = instance_buffer.take() {
                            instance_buffer_pool.lock().release(instance_buffer);
                        }
                    });
                    let block = block.copy();
                    command_buffer.add_completed_handler(&block);

                    if self.presents_with_transaction {
                        command_buffer.commit();
                        command_buffer.wait_until_scheduled();
                        drawable.present();
                    } else {
                        command_buffer.present_drawable(drawable);
                        command_buffer.commit();
                    }
                    return;
                }
                Err(err) => {
                    log::error!(
                        "failed to render: {}. retrying with larger instance buffer size",
                        err
                    );
                    let mut instance_buffer_pool = self.instance_buffer_pool.lock();
                    let buffer_size = instance_buffer_pool.buffer_size;
                    if buffer_size >= 256 * 1024 * 1024 {
                        log::error!("instance buffer size grew too large: {}", buffer_size);
                        break;
                    }
                    instance_buffer_pool.reset(buffer_size * 2);
                    log::info!(
                        "increased instance buffer size to {}",
                        instance_buffer_pool.buffer_size
                    );
                }
            }
        }
    }

    fn draw_primitives(
        &mut self,
        scene: &Scene,
        instance_buffer: &mut InstanceBuffer,
        target: &metal::TextureRef,
        viewport_size: Size<DevicePixels>,
    ) -> Result<metal::CommandBuffer> {
        let command_queue = self.command_queue.clone();
        let command_buffer = command_queue.new_command_buffer();
        let alpha = if self.layer.is_opaque() { 1. } else { 0. };
        let mut instance_offset = 0;
        let signatures = if scene.backdrop_blurs.is_empty() {
            Vec::new()
        } else {
            scene.backdrop_signatures()
        };
        let mut previous_blurs = mem::take(&mut self.blur_cache);
        let mut blur_index = 0;
        let mut blurs_reused = 0;

        let mut command_encoder = new_command_encoder(
            command_buffer,
            target,
            viewport_size,
            |color_attachment| {
                color_attachment.set_load_action(metal::MTLLoadAction::Clear);
                color_attachment.set_clear_color(metal::MTLClearColor::new(0., 0., 0., alpha));
            },
        );

        for batch in scene.batches() {
            let ok = match batch {
                PrimitiveBatch::Shadows(shadows) => self.draw_shadows(
                    shadows,
                    instance_buffer,
                    &mut instance_offset,
                    viewport_size,
                    command_encoder,
                ),
                PrimitiveBatch::Quads(quads) => self.draw_quads(
                    quads,
                    instance_buffer,
                    &mut instance_offset,
                    viewport_size,
                    command_encoder,
                ),
                PrimitiveBatch::Paths(paths) => {
                    command_encoder.end_encoding();

                    let did_draw = self.draw_paths_to_intermediate(
                        paths,
                        instance_buffer,
                        &mut instance_offset,
                        viewport_size,
                        command_buffer,
                    );

                    command_encoder = new_command_encoder(
                        command_buffer,
                        target,
                        viewport_size,
                        |color_attachment| {
                            color_attachment.set_load_action(metal::MTLLoadAction::Load);
                        },
                    );

                    if did_draw {
                        self.draw_paths_from_intermediate(
                            paths,
                            instance_buffer,
                            &mut instance_offset,
                            viewport_size,
                            command_encoder,
                        )
                    } else {
                        false
                    }
                }
                PrimitiveBatch::Underlines(underlines) => self.draw_underlines(
                    underlines,
                    instance_buffer,
                    &mut instance_offset,
                    viewport_size,
                    command_encoder,
                ),
                PrimitiveBatch::MonochromeSprites {
                    texture_id,
                    sprites,
                } => self.draw_monochrome_sprites(
                    texture_id,
                    sprites,
                    instance_buffer,
                    &mut instance_offset,
                    viewport_size,
                    command_encoder,
                ),
                PrimitiveBatch::PolychromeSprites {
                    texture_id,
                    sprites,
                } => self.draw_polychrome_sprites(
                    texture_id,
                    sprites,
                    instance_buffer,
                    &mut instance_offset,
                    viewport_size,
                    scene.motion_time.get(),
                    command_encoder,
                ),
                PrimitiveBatch::Surfaces(surfaces) => self.draw_surfaces(
                    surfaces,
                    instance_buffer,
                    &mut instance_offset,
                    viewport_size,
                    command_encoder,
                ),
                PrimitiveBatch::BackdropBlurs(blurs) => {
                    command_encoder.end_encoding();
                    for blur in blurs {
                        let signature = signatures.get(blur_index).copied().flatten();
                        blur_index += 1;
                        let reused = self.draw_backdrop_blur(
                            blur,
                            signature,
                            &mut previous_blurs,
                            command_buffer,
                            target,
                            viewport_size,
                        );
                        blurs_reused += reused as u32;
                    }
                    command_encoder = new_command_encoder(
                        command_buffer,
                        target,
                        viewport_size,
                        |color_attachment| {
                            color_attachment.set_load_action(metal::MTLLoadAction::Load);
                        },
                    );
                    true
                }
            };
            if !ok {
                command_encoder.end_encoding();
                // This command buffer is never committed, so the blurs it encoded never happen.
                self.blur_cache.clear();
                anyhow::bail!(
                    "scene too large: {} paths, {} shadows, {} quads, {} underlines, {} mono, {} poly, {} surfaces",
                    scene.paths.len(),
                    scene.shadows.len(),
                    scene.quads.len(),
                    scene.underlines.len(),
                    scene.monochrome_sprites.len(),
                    scene.polychrome_sprites.len(),
                    scene.surfaces.len(),
                );
            }
        }

        command_encoder.end_encoding();

        self.blur_spares.extend(previous_blurs);
        self.blur_spares
            .sort_by_key(|b| std::cmp::Reverse(b.size.0 * b.size.1));
        self.blur_spares.truncate(MAX_SPARE_BLURS);
        crate::frame_timings::record_blurs(blurs_reused, blur_index as u32 - blurs_reused);

        instance_buffer.metal_buffer.did_modify_range(NSRange {
            location: 0,
            length: instance_offset as NSUInteger,
        });
        Ok(command_buffer.to_owned())
    }

    /// Draws `blur` inside its shape. Unless last frame drew a blur with the same geometry
    /// and `signature`, whose blurred backdrop it reuses, it first copies the drawable under
    /// it and shrinks and blurs the copy. Returns whether it reused one.
    fn draw_backdrop_blur(
        &mut self,
        blur: &BackdropBlur,
        signature: Option<u64>,
        previous: &mut Vec<CachedBlur>,
        command_buffer: &metal::CommandBufferRef,
        target: &metal::TextureRef,
        viewport_size: Size<DevicePixels>,
    ) -> bool {
        let (vw, vh) = (
            i32::from(viewport_size.width).max(0) as f32,
            i32::from(viewport_size.height).max(0) as f32,
        );
        let b = &blur.bounds;
        let x0 = b.origin.x.0.floor().clamp(0., vw);
        let y0 = b.origin.y.0.floor().clamp(0., vh);
        let x1 = (b.origin.x.0 + b.size.width.0).ceil().clamp(0., vw);
        let y1 = (b.origin.y.0 + b.size.height.0).ceil().clamp(0., vh);
        if x1 - x0 < 1. || y1 - y0 < 1. {
            return false;
        }
        let (w, h) = ((x1 - x0) as u64, (y1 - y0) as u64);
        // A wide blur loses nothing at lower resolution, and costs a fraction as much.
        let radius = blur.radius.0;
        let downsample: u64 = if radius >= 16. {
            4
        } else if radius >= 4. {
            2
        } else {
            1
        };
        let (lw, lh) = (w.div_ceil(downsample), h.div_ceil(downsample));
        let d = downsample as f32;
        let sigma = radius / d;
        let low_max = [lw as f32, lh as f32];

        let key = signature.map(|signature| {
            use std::hash::{Hash, Hasher};
            let mut hasher = collections::FxHasher::default();
            (signature, [x0, y0, vw, vh, sigma].map(f32::to_bits), w, h, downsample).hash(&mut hasher);
            hasher.finish()
        });
        let reused = key.and_then(|key| previous.iter().position(|c| c.key == Some(key)));
        let cached = match reused {
            Some(i) => previous.swap_remove(i),
            None => {
                let mut cached = self.take_blur_texture(lw, lh);
                cached.key = key;
                self.blur_backdrop(&cached.texture, (w, h), (lw, lh), [x0, y0], downsample, sigma, command_buffer, target);
                cached
            }
        };

        let encoder =
            new_command_encoder(command_buffer, target, viewport_size, |color_attachment| {
                color_attachment.set_load_action(metal::MTLLoadAction::Load);
            });
        encoder.set_render_pipeline_state(&self.pipelines.backdrop_blur);
        encoder.set_vertex_buffer(0, Some(&self.unit_vertices), 0);
        encoder.set_vertex_bytes(
            1,
            mem::size_of::<BackdropBlur>() as u64,
            blur as *const BackdropBlur as *const c_void,
        );
        encoder.set_vertex_bytes(
            2,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );
        let composite = BackdropComposite { origin: [x0, y0], low_max, downsample: d, _pad: 0. };
        encoder.set_fragment_bytes(
            0,
            mem::size_of::<BackdropBlur>() as u64,
            blur as *const BackdropBlur as *const c_void,
        );
        encoder.set_fragment_bytes(
            1,
            mem::size_of::<BackdropComposite>() as u64,
            &composite as *const BackdropComposite as *const c_void,
        );
        encoder.set_fragment_texture(0, Some(&cached.texture));
        encoder.draw_primitives(metal::MTLPrimitiveType::Triangle, 0, 6);
        encoder.end_encoding();

        self.blur_cache.push(cached);
        reused.is_some()
    }

    /// Copies the `w` × `h` pixels of `source` at `origin`, shrinks them by `downsample`
    /// into `output`, and blurs them there.
    #[allow(clippy::too_many_arguments)]
    fn blur_backdrop(
        &mut self,
        output: &metal::TextureRef,
        (w, h): (u64, u64),
        (lw, lh): (u64, u64),
        origin: [f32; 2],
        downsample: u64,
        sigma: f32,
        command_buffer: &metal::CommandBufferRef,
        source: &metal::TextureRef,
    ) {
        let (x0, y0) = (origin[0], origin[1]);
        self.ensure_blur_textures(w, h, lw, lh);
        let Some(textures) = &self.blur_textures else {
            return;
        };

        let blit = command_buffer.new_blit_command_encoder();
        blit.copy_from_texture(
            source,
            0,
            0,
            metal::MTLOrigin { x: x0 as u64, y: y0 as u64, z: 0 },
            metal::MTLSize { width: w, height: h, depth: 1 },
            &textures.capture,
            0,
            0,
            metal::MTLOrigin { x: 0, y: 0, z: 0 },
        );
        blit.end_encoding();

        let pass = |pipeline: &metal::RenderPipelineStateRef,
                    source: &metal::TextureRef,
                    target: &metal::TextureRef,
                    params: BlurPass| {
            let descriptor = metal::RenderPassDescriptor::new();
            let attachment = descriptor.color_attachments().object_at(0).unwrap();
            attachment.set_texture(Some(target));
            attachment.set_load_action(metal::MTLLoadAction::DontCare);
            attachment.set_store_action(metal::MTLStoreAction::Store);
            let encoder = command_buffer.new_render_command_encoder(descriptor);
            encoder.set_viewport(metal::MTLViewport {
                originX: 0.,
                originY: 0.,
                width: lw as f64,
                height: lh as f64,
                znear: 0.,
                zfar: 1.,
            });
            encoder.set_render_pipeline_state(pipeline);
            encoder.set_fragment_texture(0, Some(source));
            encoder.set_fragment_bytes(
                0,
                mem::size_of::<BlurPass>() as u64,
                &params as *const BlurPass as *const c_void,
            );
            encoder.draw_primitives(metal::MTLPrimitiveType::Triangle, 0, 3);
            encoder.end_encoding();
        };
        let d = downsample as f32;
        let low_max = [lw as f32, lh as f32];
        pass(
            &self.pipelines.blur_downsample,
            &textures.capture,
            output,
            BlurPass {
                source_scale: [d, d],
                source_max: [w as f32, h as f32],
                direction: [0., 0.],
                sigma,
                _pad: 0.,
            },
        );
        for (source, destination, direction) in
            [(output, &*textures.low, [1., 0.]), (&*textures.low, output, [0., 1.])]
        {
            pass(
                &self.pipelines.blur_gaussian,
                source,
                destination,
                BlurPass { source_scale: [1., 1.], source_max: low_max, direction, sigma, _pad: 0. },
            );
        }
    }

    /// A texture of at least `lw` × `lh` to blur into: a spare that fits, or a new one.
    fn take_blur_texture(&mut self, lw: u64, lh: u64) -> CachedBlur {
        if let Some(i) = self.blur_spares.iter().position(|c| c.size.0 >= lw && c.size.1 >= lh) {
            return self.blur_spares.swap_remove(i);
        }
        let size = (lw.div_ceil(64) * 64, lh.div_ceil(64) * 64);
        CachedBlur { key: None, texture: self.new_blur_texture(size), size }
    }

    fn new_blur_texture(&self, (width, height): (u64, u64)) -> metal::Texture {
        let descriptor = metal::TextureDescriptor::new();
        descriptor.set_width(width);
        descriptor.set_height(height);
        descriptor.set_pixel_format(self.layer.pixel_format());
        descriptor.set_storage_mode(metal::MTLStorageMode::Private);
        descriptor.set_usage(metal::MTLTextureUsage::RenderTarget | metal::MTLTextureUsage::ShaderRead);
        self.device.new_texture(&descriptor)
    }

    /// Grows the blur scratch textures to fit, in steps so a resizing window doesn't
    /// reallocate every frame.
    fn ensure_blur_textures(&mut self, w: u64, h: u64, lw: u64, lh: u64) {
        let fits = self.blur_textures.as_ref().is_some_and(|t| {
            t.capture_size.0 >= w && t.capture_size.1 >= h && t.low_size.0 >= lw && t.low_size.1 >= lh
        });
        if fits {
            return;
        }
        let round = |n: u64| n.div_ceil(128) * 128;
        let previous = self.blur_textures.as_ref().map_or(((0, 0), (0, 0)), |t| (t.capture_size, t.low_size));
        let capture_size = (round(w).max(previous.0.0), round(h).max(previous.0.1));
        let low_size = (round(lw).max(previous.1.0), round(lh).max(previous.1.1));
        self.blur_textures = Some(BlurTextures {
            capture: self.new_blur_texture(capture_size),
            capture_size,
            low: self.new_blur_texture(low_size),
            low_size,
        });
    }

    fn draw_paths_to_intermediate(
        &self,
        paths: &[Path<ScaledPixels>],
        instance_buffer: &mut InstanceBuffer,
        instance_offset: &mut usize,
        viewport_size: Size<DevicePixels>,
        command_buffer: &metal::CommandBufferRef,
    ) -> bool {
        if paths.is_empty() {
            return true;
        }
        let Some(intermediate_texture) = &self.path_intermediate_texture else {
            return false;
        };

        let render_pass_descriptor = metal::RenderPassDescriptor::new();
        let color_attachment = render_pass_descriptor
            .color_attachments()
            .object_at(0)
            .unwrap();
        color_attachment.set_load_action(metal::MTLLoadAction::Clear);
        color_attachment.set_clear_color(metal::MTLClearColor::new(0., 0., 0., 0.));

        if let Some(msaa_texture) = &self.path_intermediate_msaa_texture {
            color_attachment.set_texture(Some(msaa_texture));
            color_attachment.set_resolve_texture(Some(intermediate_texture));
            color_attachment.set_store_action(metal::MTLStoreAction::MultisampleResolve);
        } else {
            color_attachment.set_texture(Some(intermediate_texture));
            color_attachment.set_store_action(metal::MTLStoreAction::Store);
        }

        let command_encoder = command_buffer.new_render_command_encoder(render_pass_descriptor);
        command_encoder.set_render_pipeline_state(&self.paths_rasterization_pipeline_state);

        align_offset(instance_offset);
        let mut vertices = Vec::new();
        for path in paths {
            vertices.extend(path.vertices.iter().map(|v| PathRasterizationVertex {
                xy_position: v.xy_position,
                st_position: v.st_position,
                color: path.color,
                bounds: path.bounds.intersect(&path.content_mask.bounds),
            }));
        }
        let vertices_bytes_len = mem::size_of_val(vertices.as_slice());
        let next_offset = *instance_offset + vertices_bytes_len;
        if next_offset > instance_buffer.size {
            command_encoder.end_encoding();
            return false;
        }
        command_encoder.set_vertex_buffer(
            PathRasterizationInputIndex::Vertices as u64,
            Some(&instance_buffer.metal_buffer),
            *instance_offset as u64,
        );
        command_encoder.set_vertex_bytes(
            PathRasterizationInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );
        command_encoder.set_fragment_buffer(
            PathRasterizationInputIndex::Vertices as u64,
            Some(&instance_buffer.metal_buffer),
            *instance_offset as u64,
        );
        let buffer_contents =
            unsafe { (instance_buffer.metal_buffer.contents() as *mut u8).add(*instance_offset) };
        unsafe {
            ptr::copy_nonoverlapping(
                vertices.as_ptr() as *const u8,
                buffer_contents,
                vertices_bytes_len,
            );
        }
        command_encoder.draw_primitives(
            metal::MTLPrimitiveType::Triangle,
            0,
            vertices.len() as u64,
        );
        *instance_offset = next_offset;

        command_encoder.end_encoding();
        true
    }

    fn draw_shadows(
        &self,
        shadows: &[Shadow],
        instance_buffer: &mut InstanceBuffer,
        instance_offset: &mut usize,
        viewport_size: Size<DevicePixels>,
        command_encoder: &metal::RenderCommandEncoderRef,
    ) -> bool {
        if shadows.is_empty() {
            return true;
        }
        align_offset(instance_offset);

        command_encoder.set_render_pipeline_state(&self.pipelines.shadows);
        command_encoder.set_vertex_buffer(
            ShadowInputIndex::Vertices as u64,
            Some(&self.unit_vertices),
            0,
        );
        command_encoder.set_vertex_buffer(
            ShadowInputIndex::Shadows as u64,
            Some(&instance_buffer.metal_buffer),
            *instance_offset as u64,
        );
        command_encoder.set_fragment_buffer(
            ShadowInputIndex::Shadows as u64,
            Some(&instance_buffer.metal_buffer),
            *instance_offset as u64,
        );

        command_encoder.set_vertex_bytes(
            ShadowInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );

        let shadow_bytes_len = mem::size_of_val(shadows);
        let buffer_contents =
            unsafe { (instance_buffer.metal_buffer.contents() as *mut u8).add(*instance_offset) };

        let next_offset = *instance_offset + shadow_bytes_len;
        if next_offset > instance_buffer.size {
            return false;
        }

        unsafe {
            ptr::copy_nonoverlapping(
                shadows.as_ptr() as *const u8,
                buffer_contents,
                shadow_bytes_len,
            );
        }

        command_encoder.draw_primitives_instanced(
            metal::MTLPrimitiveType::Triangle,
            0,
            6,
            shadows.len() as u64,
        );
        *instance_offset = next_offset;
        true
    }

    fn draw_quads(
        &self,
        quads: &[Quad],
        instance_buffer: &mut InstanceBuffer,
        instance_offset: &mut usize,
        viewport_size: Size<DevicePixels>,
        command_encoder: &metal::RenderCommandEncoderRef,
    ) -> bool {
        if quads.is_empty() {
            return true;
        }
        align_offset(instance_offset);

        command_encoder.set_render_pipeline_state(&self.pipelines.quads);
        command_encoder.set_vertex_buffer(
            QuadInputIndex::Vertices as u64,
            Some(&self.unit_vertices),
            0,
        );
        command_encoder.set_vertex_buffer(
            QuadInputIndex::Quads as u64,
            Some(&instance_buffer.metal_buffer),
            *instance_offset as u64,
        );
        command_encoder.set_fragment_buffer(
            QuadInputIndex::Quads as u64,
            Some(&instance_buffer.metal_buffer),
            *instance_offset as u64,
        );

        command_encoder.set_vertex_bytes(
            QuadInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );

        let quad_bytes_len = mem::size_of_val(quads);
        let buffer_contents =
            unsafe { (instance_buffer.metal_buffer.contents() as *mut u8).add(*instance_offset) };

        let next_offset = *instance_offset + quad_bytes_len;
        if next_offset > instance_buffer.size {
            return false;
        }

        unsafe {
            ptr::copy_nonoverlapping(quads.as_ptr() as *const u8, buffer_contents, quad_bytes_len);
        }

        command_encoder.draw_primitives_instanced(
            metal::MTLPrimitiveType::Triangle,
            0,
            6,
            quads.len() as u64,
        );
        *instance_offset = next_offset;
        true
    }

    fn draw_paths_from_intermediate(
        &self,
        paths: &[Path<ScaledPixels>],
        instance_buffer: &mut InstanceBuffer,
        instance_offset: &mut usize,
        viewport_size: Size<DevicePixels>,
        command_encoder: &metal::RenderCommandEncoderRef,
    ) -> bool {
        let Some(first_path) = paths.first() else {
            return true;
        };

        let Some(ref intermediate_texture) = self.path_intermediate_texture else {
            return false;
        };

        command_encoder.set_render_pipeline_state(&self.pipelines.path_sprites);
        command_encoder.set_vertex_buffer(
            SpriteInputIndex::Vertices as u64,
            Some(&self.unit_vertices),
            0,
        );
        command_encoder.set_vertex_bytes(
            SpriteInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );

        command_encoder.set_fragment_texture(
            SpriteInputIndex::AtlasTexture as u64,
            Some(intermediate_texture),
        );

        // When copying paths from the intermediate texture to the drawable,
        // each pixel must only be copied once, in case of transparent paths.
        //
        // If all paths have the same draw order, then their bounds are all
        // disjoint, so we can copy each path's bounds individually. If this
        // batch combines different draw orders, we perform a single copy
        // for a minimal spanning rect.
        let sprites;
        if paths.last().unwrap().order == first_path.order {
            sprites = paths
                .iter()
                .map(|path| PathSprite {
                    bounds: path.clipped_bounds(),
                })
                .collect();
        } else {
            let mut bounds = first_path.clipped_bounds();
            for path in paths.iter().skip(1) {
                bounds = bounds.union(&path.clipped_bounds());
            }
            sprites = vec![PathSprite { bounds }];
        }

        align_offset(instance_offset);
        let sprite_bytes_len = mem::size_of_val(sprites.as_slice());
        let next_offset = *instance_offset + sprite_bytes_len;
        if next_offset > instance_buffer.size {
            return false;
        }

        command_encoder.set_vertex_buffer(
            SpriteInputIndex::Sprites as u64,
            Some(&instance_buffer.metal_buffer),
            *instance_offset as u64,
        );

        let buffer_contents =
            unsafe { (instance_buffer.metal_buffer.contents() as *mut u8).add(*instance_offset) };
        unsafe {
            ptr::copy_nonoverlapping(
                sprites.as_ptr() as *const u8,
                buffer_contents,
                sprite_bytes_len,
            );
        }

        command_encoder.draw_primitives_instanced(
            metal::MTLPrimitiveType::Triangle,
            0,
            6,
            sprites.len() as u64,
        );
        *instance_offset = next_offset;

        true
    }

    fn draw_underlines(
        &self,
        underlines: &[Underline],
        instance_buffer: &mut InstanceBuffer,
        instance_offset: &mut usize,
        viewport_size: Size<DevicePixels>,
        command_encoder: &metal::RenderCommandEncoderRef,
    ) -> bool {
        if underlines.is_empty() {
            return true;
        }
        align_offset(instance_offset);

        command_encoder.set_render_pipeline_state(&self.pipelines.underlines);
        command_encoder.set_vertex_buffer(
            UnderlineInputIndex::Vertices as u64,
            Some(&self.unit_vertices),
            0,
        );
        command_encoder.set_vertex_buffer(
            UnderlineInputIndex::Underlines as u64,
            Some(&instance_buffer.metal_buffer),
            *instance_offset as u64,
        );
        command_encoder.set_fragment_buffer(
            UnderlineInputIndex::Underlines as u64,
            Some(&instance_buffer.metal_buffer),
            *instance_offset as u64,
        );

        command_encoder.set_vertex_bytes(
            UnderlineInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );

        let underline_bytes_len = mem::size_of_val(underlines);
        let buffer_contents =
            unsafe { (instance_buffer.metal_buffer.contents() as *mut u8).add(*instance_offset) };

        let next_offset = *instance_offset + underline_bytes_len;
        if next_offset > instance_buffer.size {
            return false;
        }

        unsafe {
            ptr::copy_nonoverlapping(
                underlines.as_ptr() as *const u8,
                buffer_contents,
                underline_bytes_len,
            );
        }

        command_encoder.draw_primitives_instanced(
            metal::MTLPrimitiveType::Triangle,
            0,
            6,
            underlines.len() as u64,
        );
        *instance_offset = next_offset;
        true
    }

    fn draw_monochrome_sprites(
        &self,
        texture_id: AtlasTextureId,
        sprites: &[MonochromeSprite],
        instance_buffer: &mut InstanceBuffer,
        instance_offset: &mut usize,
        viewport_size: Size<DevicePixels>,
        command_encoder: &metal::RenderCommandEncoderRef,
    ) -> bool {
        if sprites.is_empty() {
            return true;
        }
        align_offset(instance_offset);

        let sprite_bytes_len = mem::size_of_val(sprites);
        let buffer_contents =
            unsafe { (instance_buffer.metal_buffer.contents() as *mut u8).add(*instance_offset) };

        let next_offset = *instance_offset + sprite_bytes_len;
        if next_offset > instance_buffer.size {
            return false;
        }

        let Some(texture) = self.sprite_atlas.metal_texture(texture_id) else {
            log::error!("sprites refer to a freed atlas texture; skipping them");
            return true;
        };
        let texture_size = size(
            DevicePixels(texture.width() as i32),
            DevicePixels(texture.height() as i32),
        );
        command_encoder.set_render_pipeline_state(&self.pipelines.monochrome_sprites);
        command_encoder.set_vertex_buffer(
            SpriteInputIndex::Vertices as u64,
            Some(&self.unit_vertices),
            0,
        );
        command_encoder.set_vertex_buffer(
            SpriteInputIndex::Sprites as u64,
            Some(&instance_buffer.metal_buffer),
            *instance_offset as u64,
        );
        command_encoder.set_vertex_bytes(
            SpriteInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );
        command_encoder.set_vertex_bytes(
            SpriteInputIndex::AtlasTextureSize as u64,
            mem::size_of_val(&texture_size) as u64,
            &texture_size as *const Size<DevicePixels> as *const _,
        );
        command_encoder.set_fragment_buffer(
            SpriteInputIndex::Sprites as u64,
            Some(&instance_buffer.metal_buffer),
            *instance_offset as u64,
        );
        command_encoder.set_fragment_texture(SpriteInputIndex::AtlasTexture as u64, Some(&texture));

        unsafe {
            ptr::copy_nonoverlapping(
                sprites.as_ptr() as *const u8,
                buffer_contents,
                sprite_bytes_len,
            );
        }

        command_encoder.draw_primitives_instanced(
            metal::MTLPrimitiveType::Triangle,
            0,
            6,
            sprites.len() as u64,
        );
        *instance_offset = next_offset;
        true
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_polychrome_sprites(
        &self,
        texture_id: AtlasTextureId,
        sprites: &[PolychromeSprite],
        instance_buffer: &mut InstanceBuffer,
        instance_offset: &mut usize,
        viewport_size: Size<DevicePixels>,
        motion_time: f32,
        command_encoder: &metal::RenderCommandEncoderRef,
    ) -> bool {
        if sprites.is_empty() {
            return true;
        }
        align_offset(instance_offset);

        let Some(texture) = self.sprite_atlas.metal_texture(texture_id) else {
            log::error!("sprites refer to a freed atlas texture; skipping them");
            return true;
        };
        let texture_size = size(
            DevicePixels(texture.width() as i32),
            DevicePixels(texture.height() as i32),
        );
        command_encoder.set_render_pipeline_state(&self.pipelines.polychrome_sprites);
        command_encoder.set_vertex_bytes(
            SpriteInputIndex::MotionTime as u64,
            mem::size_of::<f32>() as u64,
            &motion_time as *const f32 as *const _,
        );
        command_encoder.set_vertex_buffer(
            SpriteInputIndex::Vertices as u64,
            Some(&self.unit_vertices),
            0,
        );
        command_encoder.set_vertex_buffer(
            SpriteInputIndex::Sprites as u64,
            Some(&instance_buffer.metal_buffer),
            *instance_offset as u64,
        );
        command_encoder.set_vertex_bytes(
            SpriteInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );
        command_encoder.set_vertex_bytes(
            SpriteInputIndex::AtlasTextureSize as u64,
            mem::size_of_val(&texture_size) as u64,
            &texture_size as *const Size<DevicePixels> as *const _,
        );
        command_encoder.set_fragment_buffer(
            SpriteInputIndex::Sprites as u64,
            Some(&instance_buffer.metal_buffer),
            *instance_offset as u64,
        );
        command_encoder.set_fragment_texture(SpriteInputIndex::AtlasTexture as u64, Some(&texture));

        let sprite_bytes_len = mem::size_of_val(sprites);
        let buffer_contents =
            unsafe { (instance_buffer.metal_buffer.contents() as *mut u8).add(*instance_offset) };

        let next_offset = *instance_offset + sprite_bytes_len;
        if next_offset > instance_buffer.size {
            return false;
        }

        unsafe {
            ptr::copy_nonoverlapping(
                sprites.as_ptr() as *const u8,
                buffer_contents,
                sprite_bytes_len,
            );
        }

        command_encoder.draw_primitives_instanced(
            metal::MTLPrimitiveType::Triangle,
            0,
            6,
            sprites.len() as u64,
        );
        *instance_offset = next_offset;
        true
    }

    fn draw_surfaces(
        &mut self,
        surfaces: &[PaintSurface],
        instance_buffer: &mut InstanceBuffer,
        instance_offset: &mut usize,
        viewport_size: Size<DevicePixels>,
        command_encoder: &metal::RenderCommandEncoderRef,
    ) -> bool {
        command_encoder.set_render_pipeline_state(&self.pipelines.surfaces);
        command_encoder.set_vertex_buffer(
            SurfaceInputIndex::Vertices as u64,
            Some(&self.unit_vertices),
            0,
        );
        command_encoder.set_vertex_bytes(
            SurfaceInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );

        for surface in surfaces {
            let texture_size = size(
                DevicePixels::from(surface.image_buffer.get_width() as i32),
                DevicePixels::from(surface.image_buffer.get_height() as i32),
            );

            assert_eq!(
                surface.image_buffer.get_pixel_format(),
                kCVPixelFormatType_420YpCbCr8BiPlanarFullRange
            );

            let y_texture = self
                .core_video_texture_cache
                .create_texture_from_image(
                    surface.image_buffer.as_concrete_TypeRef(),
                    None,
                    MTLPixelFormat::R8Unorm,
                    surface.image_buffer.get_width_of_plane(0),
                    surface.image_buffer.get_height_of_plane(0),
                    0,
                )
                .unwrap();
            let cb_cr_texture = self
                .core_video_texture_cache
                .create_texture_from_image(
                    surface.image_buffer.as_concrete_TypeRef(),
                    None,
                    MTLPixelFormat::RG8Unorm,
                    surface.image_buffer.get_width_of_plane(1),
                    surface.image_buffer.get_height_of_plane(1),
                    1,
                )
                .unwrap();

            align_offset(instance_offset);
            let next_offset = *instance_offset + mem::size_of::<Surface>();
            if next_offset > instance_buffer.size {
                return false;
            }

            command_encoder.set_vertex_buffer(
                SurfaceInputIndex::Surfaces as u64,
                Some(&instance_buffer.metal_buffer),
                *instance_offset as u64,
            );
            command_encoder.set_vertex_bytes(
                SurfaceInputIndex::TextureSize as u64,
                mem::size_of_val(&texture_size) as u64,
                &texture_size as *const Size<DevicePixels> as *const _,
            );
            // let y_texture = y_texture.get_texture().unwrap().
            command_encoder.set_fragment_texture(SurfaceInputIndex::YTexture as u64, unsafe {
                let texture = CVMetalTextureGetTexture(y_texture.as_concrete_TypeRef());
                Some(metal::TextureRef::from_ptr(texture as *mut _))
            });
            command_encoder.set_fragment_texture(SurfaceInputIndex::CbCrTexture as u64, unsafe {
                let texture = CVMetalTextureGetTexture(cb_cr_texture.as_concrete_TypeRef());
                Some(metal::TextureRef::from_ptr(texture as *mut _))
            });

            unsafe {
                let buffer_contents = (instance_buffer.metal_buffer.contents() as *mut u8)
                    .add(*instance_offset)
                    as *mut SurfaceBounds;
                ptr::write(
                    buffer_contents,
                    SurfaceBounds {
                        bounds: surface.bounds,
                        content_mask: surface.content_mask.clone(),
                    },
                );
            }

            command_encoder.draw_primitives(metal::MTLPrimitiveType::Triangle, 0, 6);
            *instance_offset = next_offset;
        }
        true
    }
}

fn new_command_encoder<'a>(
    command_buffer: &'a metal::CommandBufferRef,
    target: &'a metal::TextureRef,
    viewport_size: Size<DevicePixels>,
    configure_color_attachment: impl Fn(&RenderPassColorAttachmentDescriptorRef),
) -> &'a metal::RenderCommandEncoderRef {
    let render_pass_descriptor = metal::RenderPassDescriptor::new();
    let color_attachment = render_pass_descriptor
        .color_attachments()
        .object_at(0)
        .unwrap();
    color_attachment.set_texture(Some(target));
    color_attachment.set_store_action(metal::MTLStoreAction::Store);
    configure_color_attachment(color_attachment);

    let command_encoder = command_buffer.new_render_command_encoder(render_pass_descriptor);
    command_encoder.set_viewport(metal::MTLViewport {
        originX: 0.0,
        originY: 0.0,
        width: i32::from(viewport_size.width) as f64,
        height: i32::from(viewport_size.height) as f64,
        znear: 0.0,
        zfar: 1.0,
    });
    command_encoder
}

/// The pipelines that draw into the drawable, built for its pixel format.
struct DrawablePipelines {
    path_sprites: metal::RenderPipelineState,
    shadows: metal::RenderPipelineState,
    quads: metal::RenderPipelineState,
    underlines: metal::RenderPipelineState,
    monochrome_sprites: metal::RenderPipelineState,
    polychrome_sprites: metal::RenderPipelineState,
    surfaces: metal::RenderPipelineState,
    blur_downsample: metal::RenderPipelineState,
    blur_gaussian: metal::RenderPipelineState,
    backdrop_blur: metal::RenderPipelineState,
}

impl DrawablePipelines {
    fn new(device: &metal::DeviceRef, library: &metal::LibraryRef, format: MTLPixelFormat) -> Self {
        let path_sprites = build_path_sprite_pipeline_state(
            device,
            library,
            "path_sprites",
            "path_sprite_vertex",
            "path_sprite_fragment",
            format,
        );
        let shadows = build_pipeline_state(
            device,
            library,
            "shadows",
            "shadow_vertex",
            "shadow_fragment",
            format,
        );
        let quads = build_pipeline_state(
            device,
            library,
            "quads",
            "quad_vertex",
            "quad_fragment",
            format,
        );
        let underlines = build_pipeline_state(
            device,
            library,
            "underlines",
            "underline_vertex",
            "underline_fragment",
            format,
        );
        let monochrome_sprites = build_pipeline_state(
            device,
            library,
            "monochrome_sprites",
            "monochrome_sprite_vertex",
            "monochrome_sprite_fragment",
            format,
        );
        let polychrome_sprites = build_pipeline_state(
            device,
            library,
            "polychrome_sprites",
            "polychrome_sprite_vertex",
            "polychrome_sprite_fragment",
            format,
        );
        let surfaces = build_pipeline_state(
            device,
            library,
            "surfaces",
            "surface_vertex",
            "surface_fragment",
            format,
        );

        let blur_downsample = build_pipeline_state(
            device,
            library,
            "blur_downsample",
            "blur_pass_vertex",
            "blur_downsample_fragment",
            format,
        );
        let blur_gaussian = build_pipeline_state(
            device,
            library,
            "blur_gaussian",
            "blur_pass_vertex",
            "blur_gaussian_fragment",
            format,
        );
        let backdrop_blur = build_pipeline_state(
            device,
            library,
            "backdrop_blur",
            "backdrop_blur_vertex",
            "backdrop_blur_fragment",
            format,
        );
        Self {
            path_sprites,
            shadows,
            quads,
            underlines,
            monochrome_sprites,
            polychrome_sprites,
            surfaces,
            blur_downsample,
            blur_gaussian,
            backdrop_blur,
        }
    }
}

/// The drawable holds extended sRGB: sRGB colors keep their values, and wide-gamut images
/// reach outside 0...1. BGR10_XR is as small as BGRA8Unorm and holds every 8-bit sRGB value
/// exactly, but it has no alpha and only Apple GPUs have it, so transparent windows and
/// other GPUs use half floats, which take twice the memory.
fn drawable_format(device: &metal::DeviceRef, transparent: bool) -> MTLPixelFormat {
    if !transparent && device.supports_family(MTLGPUFamily::Apple3) {
        MTLPixelFormat::BGR10_XR
    } else {
        MTLPixelFormat::RGBA16Float
    }
}

fn has_alpha(format: MTLPixelFormat) -> bool {
    format != MTLPixelFormat::BGR10_XR
}

fn build_pipeline_state(
    device: &metal::DeviceRef,
    library: &metal::LibraryRef,
    label: &str,
    vertex_fn_name: &str,
    fragment_fn_name: &str,
    pixel_format: metal::MTLPixelFormat,
) -> metal::RenderPipelineState {
    let vertex_fn = library
        .get_function(vertex_fn_name, None)
        .expect("error locating vertex function");
    let fragment_fn = library
        .get_function(fragment_fn_name, None)
        .expect("error locating fragment function");

    let descriptor = metal::RenderPipelineDescriptor::new();
    descriptor.set_label(label);
    descriptor.set_vertex_function(Some(vertex_fn.as_ref()));
    descriptor.set_fragment_function(Some(fragment_fn.as_ref()));
    let color_attachment = descriptor.color_attachments().object_at(0).unwrap();
    color_attachment.set_pixel_format(pixel_format);
    color_attachment.set_blending_enabled(true);
    color_attachment.set_rgb_blend_operation(metal::MTLBlendOperation::Add);
    color_attachment.set_alpha_blend_operation(metal::MTLBlendOperation::Add);
    color_attachment.set_source_rgb_blend_factor(metal::MTLBlendFactor::SourceAlpha);
    color_attachment.set_source_alpha_blend_factor(metal::MTLBlendFactor::One);
    color_attachment.set_destination_rgb_blend_factor(metal::MTLBlendFactor::OneMinusSourceAlpha);
    color_attachment.set_destination_alpha_blend_factor(metal::MTLBlendFactor::One);

    device
        .new_render_pipeline_state(&descriptor)
        .expect("could not create render pipeline state")
}

fn build_path_sprite_pipeline_state(
    device: &metal::DeviceRef,
    library: &metal::LibraryRef,
    label: &str,
    vertex_fn_name: &str,
    fragment_fn_name: &str,
    pixel_format: metal::MTLPixelFormat,
) -> metal::RenderPipelineState {
    let vertex_fn = library
        .get_function(vertex_fn_name, None)
        .expect("error locating vertex function");
    let fragment_fn = library
        .get_function(fragment_fn_name, None)
        .expect("error locating fragment function");

    let descriptor = metal::RenderPipelineDescriptor::new();
    descriptor.set_label(label);
    descriptor.set_vertex_function(Some(vertex_fn.as_ref()));
    descriptor.set_fragment_function(Some(fragment_fn.as_ref()));
    let color_attachment = descriptor.color_attachments().object_at(0).unwrap();
    color_attachment.set_pixel_format(pixel_format);
    color_attachment.set_blending_enabled(true);
    color_attachment.set_rgb_blend_operation(metal::MTLBlendOperation::Add);
    color_attachment.set_alpha_blend_operation(metal::MTLBlendOperation::Add);
    color_attachment.set_source_rgb_blend_factor(metal::MTLBlendFactor::One);
    color_attachment.set_source_alpha_blend_factor(metal::MTLBlendFactor::One);
    color_attachment.set_destination_rgb_blend_factor(metal::MTLBlendFactor::OneMinusSourceAlpha);
    color_attachment.set_destination_alpha_blend_factor(metal::MTLBlendFactor::One);

    device
        .new_render_pipeline_state(&descriptor)
        .expect("could not create render pipeline state")
}

fn build_path_rasterization_pipeline_state(
    device: &metal::DeviceRef,
    library: &metal::LibraryRef,
    label: &str,
    vertex_fn_name: &str,
    fragment_fn_name: &str,
    pixel_format: metal::MTLPixelFormat,
    path_sample_count: u32,
) -> metal::RenderPipelineState {
    let vertex_fn = library
        .get_function(vertex_fn_name, None)
        .expect("error locating vertex function");
    let fragment_fn = library
        .get_function(fragment_fn_name, None)
        .expect("error locating fragment function");

    let descriptor = metal::RenderPipelineDescriptor::new();
    descriptor.set_label(label);
    descriptor.set_vertex_function(Some(vertex_fn.as_ref()));
    descriptor.set_fragment_function(Some(fragment_fn.as_ref()));
    if path_sample_count > 1 {
        descriptor.set_raster_sample_count(path_sample_count as _);
        descriptor.set_alpha_to_coverage_enabled(false);
    }
    let color_attachment = descriptor.color_attachments().object_at(0).unwrap();
    color_attachment.set_pixel_format(pixel_format);
    color_attachment.set_blending_enabled(true);
    color_attachment.set_rgb_blend_operation(metal::MTLBlendOperation::Add);
    color_attachment.set_alpha_blend_operation(metal::MTLBlendOperation::Add);
    color_attachment.set_source_rgb_blend_factor(metal::MTLBlendFactor::One);
    color_attachment.set_source_alpha_blend_factor(metal::MTLBlendFactor::One);
    color_attachment.set_destination_rgb_blend_factor(metal::MTLBlendFactor::OneMinusSourceAlpha);
    color_attachment.set_destination_alpha_blend_factor(metal::MTLBlendFactor::OneMinusSourceAlpha);

    device
        .new_render_pipeline_state(&descriptor)
        .expect("could not create render pipeline state")
}

// Align to multiples of 256 make Metal happy.
fn align_offset(offset: &mut usize) {
    *offset = (*offset).div_ceil(256) * 256;
}

#[repr(C)]
enum ShadowInputIndex {
    Vertices = 0,
    Shadows = 1,
    ViewportSize = 2,
}

#[repr(C)]
enum QuadInputIndex {
    Vertices = 0,
    Quads = 1,
    ViewportSize = 2,
}

#[repr(C)]
enum UnderlineInputIndex {
    Vertices = 0,
    Underlines = 1,
    ViewportSize = 2,
}

#[repr(C)]
enum SpriteInputIndex {
    Vertices = 0,
    Sprites = 1,
    ViewportSize = 2,
    AtlasTextureSize = 3,
    AtlasTexture = 4,
    MotionTime = 5,
}

#[repr(C)]
enum SurfaceInputIndex {
    Vertices = 0,
    Surfaces = 1,
    ViewportSize = 2,
    TextureSize = 3,
    YTexture = 4,
    CbCrTexture = 5,
}

#[repr(C)]
enum PathRasterizationInputIndex {
    Vertices = 0,
    ViewportSize = 1,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct PathSprite {
    pub bounds: Bounds<ScaledPixels>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct SurfaceBounds {
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AtlasKey, Corners, Hsla, ImageId, PlatformAtlas, RenderImageParams, SpriteMotion, rgb};
    use std::borrow::Cow;

    const SIDE: i32 = 4;

    fn rect(x: i32) -> Bounds<ScaledPixels> {
        Bounds::new(
            point(ScaledPixels(x as f32), ScaledPixels(0.)),
            size(ScaledPixels(SIDE as f32), ScaledPixels(SIDE as f32)),
        )
    }

    fn full_mask() -> ContentMask<ScaledPixels> {
        ContentMask {
            bounds: Bounds::new(point(ScaledPixels(0.), ScaledPixels(0.)), size(ScaledPixels(1e4), ScaledPixels(1e4))),
            corner_radii: Corners::default(),
        }
    }

    fn quad(x: i32, color: Hsla) -> Quad {
        Quad {
            order: 0,
            border_style: Default::default(),
            bounds: rect(x),
            content_mask: full_mask(),
            background: color.into(),
            border_color: Default::default(),
            corner_radii: Default::default(),
            border_widths: Default::default(),
        }
    }

    fn sprite(renderer: &MetalRenderer, id: usize, x: i32, bgra: [u8; 4], display_p3: bool) -> PolychromeSprite {
        let bytes: Vec<u8> = bgra.repeat((SIDE * SIDE) as usize);
        let key = AtlasKey::Image(RenderImageParams { image_id: ImageId(usize::MAX - id), frame_index: 0, level: 0 });
        let tile = renderer
            .sprite_atlas
            .get_or_insert_with(&key, &mut || {
                Ok(Some((size(DevicePixels(SIDE), DevicePixels(SIDE)), Cow::Owned(bytes.clone()))))
            })
            .unwrap()
            .unwrap();
        PolychromeSprite {
            order: 0,
            pad: 0,
            grayscale: false,
            display_p3,
            opacity: 1.,
            bounds: rect(x),
            content_mask: full_mask(),
            corner_radii: Default::default(),
            tile,
            texture_crop: PolychromeSprite::WHOLE_TEXTURE,
            motion: SpriteMotion::default(),
        }
    }

    /// Renders `scene` into an offscreen texture in the drawable's format and returns the
    /// center pixel of each `SIDE`-wide column.
    fn render(renderer: &mut MetalRenderer, scene: &Scene, columns: i32) -> Vec<[f32; 3]> {
        let format = renderer.layer.pixel_format();
        let width = SIDE * columns;
        let descriptor = metal::TextureDescriptor::new();
        descriptor.set_width(width as u64);
        descriptor.set_height(SIDE as u64);
        descriptor.set_pixel_format(format);
        descriptor.set_storage_mode(metal::MTLStorageMode::Managed);
        descriptor.set_usage(metal::MTLTextureUsage::RenderTarget | metal::MTLTextureUsage::ShaderRead);
        let target = renderer.device.new_texture(&descriptor);
        let viewport = size(DevicePixels(width), DevicePixels(SIDE));
        renderer.update_path_intermediate_textures(viewport);

        let mut instance_buffer = renderer.instance_buffer_pool.lock().acquire(&renderer.device);
        let command_buffer = renderer
            .draw_primitives(scene, &mut instance_buffer, &target, viewport)
            .unwrap();
        let blit = command_buffer.new_blit_command_encoder();
        blit.synchronize_resource(&target);
        blit.end_encoding();
        command_buffer.commit();
        command_buffer.wait_until_completed();

        let bytes_per_pixel = if format == MTLPixelFormat::RGBA16Float { 8 } else { 4 };
        let mut pixels = vec![0u8; (width * SIDE) as usize * bytes_per_pixel];
        target.get_bytes(
            pixels.as_mut_ptr().cast(),
            (width as usize * bytes_per_pixel) as u64,
            metal::MTLRegion::new_2d(0, 0, width as u64, SIDE as u64),
            0,
        );
        let row = (SIDE / 2) as usize * width as usize * bytes_per_pixel;
        (0..columns)
            .map(|column| {
                let at = row + (column * SIDE + SIDE / 2) as usize * bytes_per_pixel;
                let pixel = &pixels[at..at + bytes_per_pixel];
                if format == MTLPixelFormat::RGBA16Float {
                    let half = |i: usize| f16_to_f32(u16::from_le_bytes([pixel[2 * i], pixel[2 * i + 1]]));
                    [half(0), half(1), half(2)]
                } else {
                    // BGR10_XR: 384 is 0 and 894 is 1.
                    let packed = u32::from_le_bytes(pixel.try_into().unwrap());
                    let field = |shift: u32| (((packed >> shift) & 0x3ff) as f32 - 384.) / 510.;
                    [field(20), field(10), field(0)]
                }
            })
            .collect()
    }

    fn f16_to_f32(bits: u16) -> f32 {
        let sign = if bits & 0x8000 != 0 { -1. } else { 1. };
        let exponent = ((bits >> 10) & 0x1f) as i32;
        let mantissa = (bits & 0x3ff) as f32;
        sign * if exponent == 0 {
            mantissa / 1024. * 2f32.powi(-14)
        } else {
            (1. + mantissa / 1024.) * 2f32.powi(exponent - 15)
        }
    }

    /// What ColorSync makes of a Display P3 color in extended sRGB.
    fn reference_p3_to_extended_srgb(rgb: [f32; 3]) -> [f32; 3] {
        use core_graphics::color_space::kCGColorSpaceDisplayP3;
        unsafe extern "C" {
            fn CGColorCreate(space: core_graphics::sys::CGColorSpaceRef, components: *const f64) -> *mut c_void;
            fn CGColorCreateCopyByMatchingToColorSpace(
                space: core_graphics::sys::CGColorSpaceRef,
                intent: i32,
                color: *mut c_void,
                options: *const c_void,
            ) -> *mut c_void;
            fn CGColorGetComponents(color: *mut c_void) -> *const f64;
            fn CFRelease(object: *mut c_void);
        }
        let p3 = CGColorSpace::create_with_name(unsafe { kCGColorSpaceDisplayP3 }).unwrap();
        let extended = CGColorSpace::create_with_name(unsafe { kCGColorSpaceExtendedSRGB }).unwrap();
        unsafe {
            let components = [rgb[0] as f64, rgb[1] as f64, rgb[2] as f64, 1.];
            let color = CGColorCreate(p3.as_ptr(), components.as_ptr());
            let matched = CGColorCreateCopyByMatchingToColorSpace(extended.as_ptr(), 0, color, ptr::null());
            let out = std::slice::from_raw_parts(CGColorGetComponents(matched), 3);
            let result = [out[0] as f32, out[1] as f32, out[2] as f32];
            CFRelease(matched);
            CFRelease(color);
            result
        }
    }

    #[test]
    fn srgb_colors_keep_their_values_and_p3_images_leave_srgb() {
        let ui_colors = [0x3478f6, 0xff0000, 0x1c1c1e, 0xfa2d48, 0x808080, 0xffffff];
        let p3_pixels: [[u8; 3]; 4] = [[255, 0, 0], [0, 255, 0], [204, 77, 51], [128, 128, 128]];
        for transparent in [false, true] {
            let mut renderer =
                MetalRenderer::new(Arc::new(Mutex::new(InstanceBufferPool::default())), transparent);
            let format = renderer.layer.pixel_format();
            let mut scene = Scene::default();
            let mut x = 0;
            for color in ui_colors {
                scene.insert_primitive(quad(x, rgb(color).into()));
                x += SIDE;
            }
            for (i, [r, g, b]) in p3_pixels.iter().copied().enumerate() {
                scene.insert_primitive(sprite(&renderer, i, x, [b, g, r, 255], true));
                x += SIDE;
            }
            scene.insert_primitive(sprite(&renderer, 99, x, [0, 0, 255, 255], false));
            scene.finish();
            let pixels = render(&mut renderer, &scene, x / SIDE + 1);

            for (i, color) in ui_colors.iter().enumerate() {
                let expected = [(color >> 16) & 0xff, (color >> 8) & 0xff, color & 0xff];
                for channel in 0..3 {
                    let got = pixels[i][channel] * 255.;
                    assert!(
                        (got - expected[channel] as f32).abs() < 0.5,
                        "{format:?}: UI color {color:06x} channel {channel} came out as {got}"
                    );
                }
            }
            for (i, pixel) in p3_pixels.iter().enumerate() {
                let got = pixels[ui_colors.len() + i];
                let expected = reference_p3_to_extended_srgb(pixel.map(|c| c as f32 / 255.));
                for channel in 0..3 {
                    assert!(
                        (got[channel] - expected[channel]).abs() < 2.5 / 510.,
                        "{format:?}: P3 {pixel:?} came out as {got:?}, ColorSync says {expected:?}"
                    );
                }
            }
            let p3_red = pixels[ui_colors.len()];
            assert!(p3_red[0] > 1.05 && p3_red[1] < -0.1, "{format:?}: P3 red is {p3_red:?}");
            let srgb_red = pixels[ui_colors.len() + p3_pixels.len()];
            assert_eq!(srgb_red, [1., 0., 0.], "{format:?}");
        }
    }
}
