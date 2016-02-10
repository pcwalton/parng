//! Adam7 interlacing support.

use Image;
use std::ptr;

pub trait LodDimensionQuery {
    fn width_for_lod(&self, lod: LevelOfDetail) -> u32;
    fn height_for_lod(&self, lod: LevelOfDetail) -> u32;
    fn stride_for_lod_and_color_depth(&self, lod: LevelOfDetail, color_depth: u8) -> u32;
    fn stride_for_lod(&self, lod: LevelOfDetail) -> u32;
}

impl LodDimensionQuery for Image {
    fn width_for_lod(&self, lod: LevelOfDetail) -> u32 {
        let metadata = self.metadata.as_ref().unwrap();
        let image_width = metadata.dimensions.width;
        match lod {
            LevelOfDetail::Adam7(0) | LevelOfDetail::Adam7(1) => image_width / 8,
            LevelOfDetail::Adam7(2) | LevelOfDetail::Adam7(3) => image_width / 4,
            LevelOfDetail::Adam7(4) | LevelOfDetail::Adam7(5) => image_width / 2,
            _ => image_width,
        }
    }

    fn height_for_lod(&self, lod: LevelOfDetail) -> u32 {
        let metadata = self.metadata.as_ref().unwrap();
        let image_height = metadata.dimensions.height;
        match lod {
            LevelOfDetail::Adam7(0) |
            LevelOfDetail::Adam7(1) |
            LevelOfDetail::Adam7(2) => image_height / 8,
            LevelOfDetail::Adam7(3) | LevelOfDetail::Adam7(4) => image_height / 4,
            LevelOfDetail::Adam7(5) => image_height / 2,
            _ => image_height,
        }
    }

    fn stride_for_lod_and_color_depth(&self, lod: LevelOfDetail, color_depth: u8) -> u32 {
        self.width_for_lod(lod) * ((color_depth / 8) as u32)
    }

    fn stride_for_lod(&self, lod: LevelOfDetail) -> u32 {
        let metadata = self.metadata.as_ref().unwrap();
        let color_depth = metadata.color_depth;
        self.width_for_lod(lod) * ((color_depth / 8) as u32)
    }
}

#[derive(Copy, Clone, PartialEq, Debug)]
pub enum LevelOfDetail {
    None,
    Adam7(u8),
}

/// TODO(pcwalton): This could be nicer by allowing images to be rendered with partially-complete
/// LODs.
#[inline(never)]
pub fn deinterlace_adam7(out_scanlines: &mut [u8],
                         in_scanlines: &Adam7Scanlines,
                         width: u32,
                         color_depth: u8) {
    let stride = (width as usize) * (color_depth as usize) / 8;
    assert!(out_scanlines.len() >= 8 * stride);
    assert!(in_scanlines.are_well_formed(stride));
    unsafe {
        parng_deinterlace_adam7_scanline_04(dest(out_scanlines,
                                                 0,
                                                 width,
                                                 color_depth).as_mut_ptr(),
                                            in_scanlines.lod0.as_opt_ptr(0),
                                            in_scanlines.lod1.as_opt_ptr(0),
                                            in_scanlines.lod3.as_opt_ptr(0),
                                            in_scanlines.lod5.as_opt_ptr(0),
                                            width as u64);

        if in_scanlines.lod2.is_some() {
            parng_deinterlace_adam7_scanline_04(dest(out_scanlines,
                                                     4,
                                                     width,
                                                     color_depth).as_mut_ptr(),
                                                in_scanlines.lod2.as_opt_ptr(0),
                                                in_scanlines.lod2.as_opt_ptr(0).offset(4),
                                                in_scanlines.lod3.as_opt_ptr(1),
                                                in_scanlines.lod5.as_opt_ptr(2),
                                                width as u64);

            if in_scanlines.lod4.is_some() {
                parng_deinterlace_adam7_scanline_26(dest(out_scanlines,
                                                         2,
                                                         width,
                                                         color_depth).as_mut_ptr(),
                                                    in_scanlines.lod4.as_opt_ptr(0),
                                                    in_scanlines.lod5.as_opt_ptr(1),
                                                    width as u64);
                parng_deinterlace_adam7_scanline_26(dest(out_scanlines,
                                                         6,
                                                         width,
                                                         color_depth).as_mut_ptr(),
                                                    in_scanlines.lod4.as_opt_ptr(1),
                                                    in_scanlines.lod5.as_opt_ptr(3),
                                                    width as u64);

                match in_scanlines.lod6 {
                    Some(ref lod6) => {
                        copy_scanline_to_dest(dest(out_scanlines, 1, width, color_depth),
                                              &lod6[0][..],
                                              width,
                                              color_depth);
                        copy_scanline_to_dest(dest(out_scanlines, 3, width, color_depth),
                                              &lod6[1][..],
                                              width,
                                              color_depth);
                        copy_scanline_to_dest(dest(out_scanlines, 5, width, color_depth),
                                              &lod6[2][..],
                                              width,
                                              color_depth);
                        copy_scanline_to_dest(dest(out_scanlines, 7, width, color_depth),
                                              &lod6[3][..],
                                              width,
                                              color_depth);
                    }
                    None => {
                        duplicate_decoded_scanline(out_scanlines, 1, 0, width, color_depth);
                        duplicate_decoded_scanline(out_scanlines, 3, 2, width, color_depth);
                        duplicate_decoded_scanline(out_scanlines, 5, 4, width, color_depth);
                        duplicate_decoded_scanline(out_scanlines, 7, 6, width, color_depth);
                    }
                }
            } else {
                duplicate_decoded_scanline(out_scanlines, 2, 0, width, color_depth);
                duplicate_decoded_scanline(out_scanlines, 6, 4, width, color_depth);
            }
        } else {
            duplicate_decoded_scanline(out_scanlines, 4, 0, width, color_depth)
        }
    }

    fn stride_for_width_and_color_depth(width: u32, color_depth: u8) -> usize {
        (width as usize) * (color_depth as usize) / 8
    }

    fn dest_index(y: u8, width: u32, color_depth: u8) -> usize {
        (y as usize) * stride_for_width_and_color_depth(width, color_depth)
    }

    fn dest(out_scanlines: &mut [u8], y: u8, width: u32, color_depth: u8) -> &mut [u8] {
        let start = dest_index(y, width, color_depth);
        let end = dest_index(y + 1, width, color_depth);
        &mut out_scanlines[start..end]
    }

    fn copy_scanline_to_dest(dest: &mut [u8], src: &[u8], width: u32, color_depth: u8) {
        let stride = stride_for_width_and_color_depth(width, color_depth);
        dest[0..stride].clone_from_slice(&src[0..stride])
    }

    fn duplicate_decoded_scanline(out_scanlines: &mut [u8],
                                  dest_y: u8,
                                  src_y: u8,
                                  width: u32,
                                  color_depth: u8) {
        debug_assert!(dest_y > src_y);
        let (head, tail) = out_scanlines.split_at_mut(dest_index(dest_y, width, color_depth));
        let src_start = dest_index(src_y, width, color_depth);
        copy_scanline_to_dest(tail, &head[src_start..], width, color_depth)
    }
}

pub struct Adam7Scanlines<'a> {
    pub lod0: [&'a [u8]; 1],            // width / 8
    pub lod1: Option<[&'a [u8]; 1]>,    // width / 8
    pub lod2: Option<[&'a [u8]; 1]>,    // width / 4
    pub lod3: Option<[&'a [u8]; 2]>,    // width / 4
    pub lod4: Option<[&'a [u8]; 2]>,    // width / 2
    pub lod5: Option<[&'a [u8]; 4]>,    // width / 2
    pub lod6: Option<[&'a [u8]; 4]>,    // width
}

impl<'a> Adam7Scanlines<'a> {
    // NB: This must be correct for memory safety!
    pub fn are_well_formed(&self, stride: usize) -> bool {
        self.lod0[0].len() >= stride / 8 &&
            self.lod1.are_well_formed(stride / 8) &&
            self.lod2.are_well_formed(stride / 4) &&
            self.lod3.are_well_formed(stride / 4) &&
            self.lod4.are_well_formed(stride / 2) &&
            self.lod5.are_well_formed(stride / 2) &&
            self.lod6.are_well_formed(stride)
    }
}

trait ScanlinesForLod {
    fn as_opt_ptr(&self, index: u8) -> *const u8;
    fn are_well_formed(&self, stride: usize) -> bool;
}

impl<'a> ScanlinesForLod for [&'a [u8]; 1] {
    fn as_opt_ptr(&self, index: u8) -> *const u8 {
        self[index as usize].as_ptr()
    }
    fn are_well_formed(&self, stride: usize) -> bool {
        self.iter().all(|scanline| scanline.len() >= stride)
    }
}

impl<'a> ScanlinesForLod for Option<[&'a [u8]; 1]> {
    fn as_opt_ptr(&self, index: u8) -> *const u8 {
        match *self {
            None => ptr::null(),
            Some(ref buffer) => buffer[index as usize].as_ptr(),
        }
    }
    fn are_well_formed(&self, stride: usize) -> bool {
        match *self {
            None => true,
            Some(ref scanlines) => scanlines.iter().all(|scanline| scanline.len() >= stride),
        }
    }
}

impl<'a> ScanlinesForLod for Option<[&'a [u8]; 2]> {
    fn as_opt_ptr(&self, index: u8) -> *const u8 {
        match *self {
            None => ptr::null(),
            Some(ref buffer) => buffer[index as usize].as_ptr(),
        }
    }
    fn are_well_formed(&self, stride: usize) -> bool {
        match *self {
            None => true,
            Some(ref scanlines) => scanlines.iter().all(|scanline| scanline.len() >= stride),
        }
    }
}

impl<'a> ScanlinesForLod for Option<[&'a [u8]; 4]> {
    fn as_opt_ptr(&self, index: u8) -> *const u8 {
        match *self {
            None => ptr::null(),
            Some(ref buffer) => buffer[index as usize].as_ptr(),
        }
    }
    fn are_well_formed(&self, stride: usize) -> bool {
        match *self {
            None => true,
            Some(ref scanlines) => scanlines.iter().all(|scanline| scanline.len() >= stride),
        }
    }
}

#[link(name="parngacceleration")]
extern {
    fn parng_deinterlace_adam7_scanline_04(dest: *mut u8,
                                           lod0: *const u8,
                                           lod1: *const u8,
                                           lod3: *const u8,
                                           lod5: *const u8,
                                           width: u64);
    fn parng_deinterlace_adam7_scanline_26(dest: *mut u8,
                                           lod4: *const u8,
                                           lod5: *const u8,
                                           width: u64);
}

