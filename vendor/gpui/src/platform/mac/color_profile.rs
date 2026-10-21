use crate::ImageColorSpace;
use core_foundation::{
    base::{CFTypeRef, TCFType},
    data::CFData,
    string::{CFString, CFStringRef},
};
use core_graphics::{
    color_space::{CGColorSpace, kCGColorSpaceDisplayP3, kCGColorSpaceSRGB},
    sys::CGColorSpaceRef,
};
use foreign_types::ForeignType;
use std::{ffi::c_void, ptr};

#[repr(C)]
struct VImageCGImageFormat {
    bits_per_component: u32,
    bits_per_pixel: u32,
    color_space: CGColorSpaceRef,
    bitmap_info: u32,
    version: u32,
    decode: *const f64,
    rendering_intent: i32,
}

#[repr(C)]
struct VImageBuffer {
    data: *mut c_void,
    height: usize,
    width: usize,
    row_bytes: usize,
}

/// `kCGImageAlphaLast` with the default byte order: straight RGBA, as the `image` crate decodes.
const RGBA_STRAIGHT: u32 = 3;
const COLOR_SPACE_MODEL_RGB: i32 = 1;

#[link(name = "Accelerate", kind = "framework")]
unsafe extern "C" {
    fn vImageConverter_CreateWithCGImageFormat(
        source: *const VImageCGImageFormat,
        destination: *const VImageCGImageFormat,
        background: *const f64,
        flags: u32,
        error: *mut isize,
    ) -> *mut c_void;
    fn vImageConvert_AnyToAny(
        converter: *const c_void,
        sources: *const VImageBuffer,
        destinations: *const VImageBuffer,
        temp_buffer: *mut c_void,
        flags: u32,
    ) -> isize;
    fn vImageConverter_Release(converter: *mut c_void);
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGColorSpaceCreateWithICCData(data: CFTypeRef) -> CGColorSpaceRef;
    fn CGColorSpaceCopyName(space: CGColorSpaceRef) -> CFStringRef;
    fn CGColorSpaceGetModel(space: CGColorSpaceRef) -> i32;
    fn CGColorSpaceIsWideGamutRGB(space: CGColorSpaceRef) -> bool;
}

/// Converts straight-alpha RGBA pixels from the colorspace `icc_profile` describes to Display P3
/// if that's wider than sRGB and `wide_gamut` is set, otherwise to sRGB, and returns which one
/// they're in. `None` if the profile can't be applied, leaving the pixels alone.
pub(crate) fn convert_rgba(
    rgba: &mut [u8],
    width: u32,
    height: u32,
    icc_profile: &[u8],
    wide_gamut: bool,
) -> Option<ImageColorSpace> {
    if rgba.len() != width as usize * height as usize * 4 || rgba.is_empty() {
        return None;
    }
    let icc_data = CFData::from_buffer(icc_profile);
    let source = unsafe { CGColorSpaceCreateWithICCData(icc_data.as_CFTypeRef()) };
    if source.is_null() {
        return None;
    }
    let source = unsafe { CGColorSpace::from_ptr(source) };
    if unsafe { CGColorSpaceGetModel(source.as_ptr()) } != COLOR_SPACE_MODEL_RGB {
        return None;
    }

    let (target, target_name) =
        if wide_gamut && unsafe { CGColorSpaceIsWideGamutRGB(source.as_ptr()) } {
            (ImageColorSpace::DisplayP3, unsafe { kCGColorSpaceDisplayP3 })
        } else {
            (ImageColorSpace::Srgb, unsafe { kCGColorSpaceSRGB })
        };
    let source_name = unsafe { CGColorSpaceCopyName(source.as_ptr()) };
    if !source_name.is_null() {
        let source_name = unsafe { CFString::wrap_under_create_rule(source_name) };
        if source_name == unsafe { CFString::wrap_under_get_rule(target_name) } {
            return Some(target);
        }
    }
    let Some(destination) = CGColorSpace::create_with_name(target_name) else {
        return None;
    };

    let format = |color_space: &CGColorSpace| VImageCGImageFormat {
        bits_per_component: 8,
        bits_per_pixel: 32,
        color_space: color_space.as_ptr(),
        bitmap_info: RGBA_STRAIGHT,
        version: 0,
        decode: ptr::null(),
        rendering_intent: 0,
    };
    let mut error = 0;
    let converter = unsafe {
        vImageConverter_CreateWithCGImageFormat(
            &format(&source),
            &format(&destination),
            ptr::null(),
            0,
            &mut error,
        )
    };
    if converter.is_null() || error != 0 {
        if !converter.is_null() {
            unsafe { vImageConverter_Release(converter) };
        }
        return None;
    }

    let mut converted = vec![0u8; rgba.len()];
    let buffer = |data: *mut u8| VImageBuffer {
        data: data.cast(),
        height: height as usize,
        width: width as usize,
        row_bytes: width as usize * 4,
    };
    let result = unsafe {
        let error = vImageConvert_AnyToAny(
            converter,
            &buffer(rgba.as_mut_ptr()),
            &buffer(converted.as_mut_ptr()),
            ptr::null_mut(),
            0,
        );
        vImageConverter_Release(converter);
        error
    };
    if result != 0 {
        return None;
    }
    rgba.copy_from_slice(&converted);
    Some(target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{convert_rgba_to_srgb, decode_still_image};
    use core_graphics::color_space::kCGColorSpaceAdobeRGB1998;
    use image::{ImageEncoder, RgbaImage, codecs::png::PngEncoder};

    unsafe extern "C" {
        fn CGColorSpaceCopyICCData(space: CGColorSpaceRef) -> core_foundation::data::CFDataRef;
        fn CGColorCreate(space: CGColorSpaceRef, components: *const f64) -> *mut c_void;
        fn CGColorCreateCopyByMatchingToColorSpace(
            space: CGColorSpaceRef,
            intent: i32,
            color: *mut c_void,
            options: *const c_void,
        ) -> *mut c_void;
        fn CGColorGetComponents(color: *mut c_void) -> *const f64;
        fn CFRelease(object: *mut c_void);
    }

    const PIXELS: [[u8; 3]; 5] = [[255, 0, 0], [0, 255, 0], [0, 0, 255], [204, 77, 51], [128, 128, 128]];

    fn icc_profile(name: CFStringRef) -> Vec<u8> {
        let space = CGColorSpace::create_with_name(name).unwrap();
        unsafe { CFData::wrap_under_create_rule(CGColorSpaceCopyICCData(space.as_ptr())) }.to_vec()
    }

    fn png(icc_profile: Option<Vec<u8>>) -> Vec<u8> {
        let raw: Vec<u8> = PIXELS.iter().flat_map(|[r, g, b]| [*r, *g, *b, 255]).collect();
        let mut bytes = Vec::new();
        let mut encoder = PngEncoder::new(&mut bytes);
        if let Some(icc_profile) = icc_profile {
            encoder.set_icc_profile(icc_profile).unwrap();
        }
        encoder
            .write_image(&raw, PIXELS.len() as u32, 1, image::ExtendedColorType::Rgba8)
            .unwrap();
        bytes
    }

    /// What ColorSync makes of `rgb` in `from`, in `to`, in 8 bits.
    fn reference(from: CFStringRef, to: CFStringRef, rgb: [u8; 3]) -> [u8; 3] {
        let from = CGColorSpace::create_with_name(from).unwrap();
        let to = CGColorSpace::create_with_name(to).unwrap();
        unsafe {
            let components = [rgb[0] as f64 / 255., rgb[1] as f64 / 255., rgb[2] as f64 / 255., 1.];
            let color = CGColorCreate(from.as_ptr(), components.as_ptr());
            let matched = CGColorCreateCopyByMatchingToColorSpace(to.as_ptr(), 0, color, ptr::null());
            let out = std::slice::from_raw_parts(CGColorGetComponents(matched), 3);
            let result = [0, 1, 2].map(|i| (out[i].clamp(0., 1.) * 255.).round() as u8);
            CFRelease(matched);
            CFRelease(color);
            result
        }
    }

    fn rgb_of(image: &RgbaImage) -> Vec<[u8; 3]> {
        image.pixels().map(|p| [p[0], p[1], p[2]]).collect()
    }

    fn assert_close(got: &[[u8; 3]], expected: &[[u8; 3]]) {
        for (got, expected) in got.iter().zip(expected) {
            for channel in 0..3 {
                assert!(
                    got[channel].abs_diff(expected[channel]) <= 1,
                    "got {got:?}, ColorSync says {expected:?}"
                );
            }
        }
    }

    #[test]
    fn display_p3_images_keep_their_values_and_are_marked() {
        let bytes = png(Some(icc_profile(unsafe { kCGColorSpaceDisplayP3 })));
        let ignoring_profile = image::load_from_memory(&bytes).unwrap().into_rgba8();
        let (decoded, color_space) = decode_still_image(&bytes, image::ImageFormat::Png).unwrap();
        assert_eq!(color_space, ImageColorSpace::DisplayP3);
        assert_eq!(rgb_of(&decoded), PIXELS);
        assert_eq!(rgb_of(&ignoring_profile), PIXELS);
        let as_srgb: Vec<_> =
            PIXELS.iter().map(|p| reference(unsafe { kCGColorSpaceDisplayP3 }, unsafe { kCGColorSpaceSRGB }, *p)).collect();
        assert_ne!(as_srgb, PIXELS, "P3 pixels mean different colors than the same sRGB values");
    }

    #[test]
    fn other_wide_profiles_are_converted_to_display_p3() {
        let adobe = unsafe { kCGColorSpaceAdobeRGB1998 };
        let (decoded, color_space) =
            decode_still_image(&png(Some(icc_profile(adobe))), image::ImageFormat::Png).unwrap();
        assert_eq!(color_space, ImageColorSpace::DisplayP3);
        let expected: Vec<_> =
            PIXELS.iter().map(|p| reference(adobe, unsafe { kCGColorSpaceDisplayP3 }, *p)).collect();
        assert_close(&rgb_of(&decoded), &expected);
    }

    #[test]
    fn srgb_and_untagged_images_are_untouched() {
        for icc_profile in [Some(icc_profile(unsafe { kCGColorSpaceSRGB })), None] {
            let (decoded, color_space) = decode_still_image(&png(icc_profile), image::ImageFormat::Png).unwrap();
            assert_eq!(color_space, ImageColorSpace::Srgb);
            assert_eq!(rgb_of(&decoded), PIXELS);
        }
    }

    #[test]
    fn converts_to_srgb_for_reading_colors() {
        let p3 = unsafe { kCGColorSpaceDisplayP3 };
        let (mut image, _) = decode_still_image(&png(None), image::ImageFormat::Png).unwrap();
        assert!(convert_rgba_to_srgb(&mut image, &icc_profile(p3)));
        let expected: Vec<_> = PIXELS.iter().map(|p| reference(p3, unsafe { kCGColorSpaceSRGB }, *p)).collect();
        assert_close(&rgb_of(&image), &expected);
        assert!(!convert_rgba_to_srgb(&mut image, b"not a profile"));
    }
}
