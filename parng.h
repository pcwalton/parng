// parng/parng.h
//
// Copyright (c) 2016 Mozilla Foundation

#ifndef PARNG_H
#define PARNG_H

#include <stdint.h>
#include <stdio.h>

#define PARNG_LOAD_PROGRESS_FINISHED                            0
#define PARNG_LOAD_PROGRESS_NEED_MORE_DATA                      1
#define PARNG_LOAD_PROGRESS_NEED_DATA_PROVIDER_AND_MORE_DATA    2

#define PARNG_COLOR_TYPE_GRAYSCALE                              0
#define PARNG_COLOR_TYPE_RGB                                    2
#define PARNG_COLOR_TYPE_INDEXED                                3
#define PARNG_COLOR_TYPE_GRAYSCALE_ALPHA                        4
#define PARNG_COLOR_TYPE_RGB_ALPHA                              5

#define PARNG_COMPRESSION_METHOD_DEFLATE                        0

#define PARNG_SUCCESS                                           0
#define PARNG_ERROR_NEED_MORE_DATA                              1
#define PARNG_ERROR_IO                                          2
#define PARNG_ERROR_INVALID_METADATA                            3
#define PARNG_ERROR_INVALID_SCANLINE_PREDICTOR                  4
#define PARNG_ERROR_ENTROPY_DECODING_ERROR                      5
#define PARNG_ERROR_NO_DATA_PROVIDER                            6

#define PARNG_FILTER_METHOD_ADAPTIVE                            0

#define PARNG_INTERLACE_METHOD_NONE                             0
#define PARNG_INTERLACE_METHOD_ADAM7                            1

#define PARNG_LEVEL_OF_DETAIL_NONE                              (-1)
#define PARNG_LEVEL_OF_DETAIL_ADAM7_0                           0
#define PARNG_LEVEL_OF_DETAIL_ADAM7_1                           1
#define PARNG_LEVEL_OF_DETAIL_ADAM7_2                           2
#define PARNG_LEVEL_OF_DETAIL_ADAM7_3                           3
#define PARNG_LEVEL_OF_DETAIL_ADAM7_4                           4
#define PARNG_LEVEL_OF_DETAIL_ADAM7_5                           5
#define PARNG_LEVEL_OF_DETAIL_ADAM7_6                           6

#define PARNG_SEEK_FROM_START                                   0
#define PARNG_SEEK_FROM_CURRENT                                 1
#define PARNG_SEEK_FROM_END                                     2

// The color type used in an image.
//
// The color type used in an image. These color types directly correspond to the color types
// defined in the PNG specification.
typedef uint32_t parng_color_type;

// The compression method used in the image.
//
// The compression method used in the image. PNG spec currently defines only one compression
// method:
//
// > At present, only compression method 0 (deflate/inflate compression with a sliding window of
// at most 32768 bytes) is defined.
typedef uint32_t parng_compression_method;

// An interface that `parng` uses to access storage for the image data.
//
// An interface that `parng` uses to access storage for the image data. By implementing this trait,
// you can choose any method you wish to store the image data and it will be transparent to
// `parng`.
//
// Be aware that the data provider will be called on a background thread; i.e. not the thread it
// was created on! You must ensure proper synchronization between the main thread and that
// background thread if you wish to communicate between them.
typedef struct parng_data_provider parng_data_provider;

// Errors that can occur while decoding a PNG image.
typedef uint32_t parng_error;

// The filtering (prediction) method used in the image.
//
// The filtering (prediction) method used in the image.
//
// The PNG specification currently defines only one filter method:
//
// > At present, only filter method 0 (adaptive filtering with five basic filter types) is
// defined.
typedef uint32_t parng_filter_method;

// An in-memory decoded image in big-endian RGBA format, 32 bits per pixel.
typedef struct parng_image parng_image;

// An object that encapsulates the load process for a single image.
typedef struct parng_image_loader parng_image_loader;

// The interlacing method used in the image.
//
// The interlacing method used in the image.
//
// The PNG specification allows either no interlacing or Adam7 interlacing.
typedef uint32_t parng_interlace_method;

// Information about a specific scanline for one level of detail in an interlaced image.
//
// Information about a specific scanline for one level of detail in an interlaced image.
//
// This object exists for the convenience of data providers, so that they do not have to hardcode
// information about Adam7 interlacing.
typedef struct parng_interlacing_info parng_interlacing_info;

// An error that occurred when reading the underlying data stream.
typedef uint32_t parng_io_error;

// A specific level of detail of an interlaced image.
//
// A specific level of detail of an interlaced image. Normal PNG interlacing is known as Adam7
// interlacing, which has 7 levels of detail, from 0 (the smallest; i.e. the blurriest) to 7 (the
// largest; i.e. the sharpest).
typedef uint32_t parng_level_of_detail;

// Describes the progress of loading the image.
//
// Describes the progress of loading the image. This is the value returned from
// `parng_image_loader_add_data()`.
typedef uint32_t parng_load_progress;

// Metadata found in the PNG header (dimensions, bit depth, etc.)
typedef struct parng_metadata parng_metadata;

// An object that defines the low-level access to the data stream.
typedef struct parng_reader parng_reader;

// Data providers use this to supply scanlines to `parng` in response to prediction requests.
typedef struct parng_scanlines_for_prediction parng_scanlines_for_prediction;

// Data providers use this to supply scanlines to `parng` in response to RGBA conversion requests.
typedef struct parng_scanlines_for_rgba_conversion parng_scanlines_for_rgba_conversion;

// Specifies the position in a stream that a seek offset is relative to.
typedef uint32_t parng_seek_from;

// An object that defines the low-level access to the data stream.
struct parng_reader {
    // Reads at most `buffer_length` bytes into the supplied buffer.
    //
    // Reads at most `buffer_length` bytes into the supplied buffer.
    //
    // `bytes_read` must be set to the number of bytes actually read. `user_data` is the contents
    // of the reader's `user_data` field. Returns `PARNG_SUCCESS` on success or any other value on
    // error.
    parng_io_error (*read)(uint8_t *buffer,
                           size_t buffer_length,
                           size_t *bytes_read,
                           void *user_data);

    // Changes the byte position of the stream.
    //
    // Changes the byte position of the stream.
    //
    // `position` specifies the offset of the new position in bytes. `from` specifies the position
    // that `position` is relative to (like `whence` in the corresponding POSIX call): it can be
    // one of `PARNG_SEEK_FROM_START` if `position` is relative to the beginning (like `SEEK_SET`),
    // `PARNG_SEEK_FROM_CURRENT` if `position` is relative to the current offset (like `SEEK_CUR`),
    // or `PARNG_SEEK_FROM_END` if `position` is relative to the end of the stream (like
    // `SEEK_END`). `new_position` must be filled in with the absolute position of the file after
    // the seek call. `user_data` is the contents of the reader's `user_data` field.
    //
    // Returns `PARNG_SUCCESS` on success or any other value on error.
    parng_io_error (*seek)(int64_t position,
                           parng_seek_from from,
                           uint64_t *new_position,
                           void *user_data);

    // An arbitrary pointer that `parng` passes to the `read` and `seek` functions.
    void *user_data;
};

// An object that encapsulates the load process for a single image.
struct parng_image_loader;

// Data providers use this structure to supply scanlines to `parng` in response to prediction
// requests.
struct parng_scanlines_for_prediction {
    // The pixels of the reference scanline.
    //
    // The pixels of the reference scanline.
    //
    // This must be present if `parng` requested a reference scanline. There must be 4 bytes per
    // pixel available in this array for truecolor modes (i.e. when the `indexed` parameter is
    // false), while for indexed modes (i.e. when the `indexed` parameter is true) there must be 1
    // byte per pixel available.
    uint8_t *reference_scanline;

    // The number of valid bytes that `reference_scanline` points to.
    size_t reference_scanline_length;

    // The pixels of the current scanline.
    //
    // The pixels of the current scanline. As with the reference scanline, there must be 4 bytes
    // per pixel available in this array for truecolor modes, and for indexed modes there must be 1
    // byte per pixel available.
    const uint8_t *current_scanline;

    // The number of valid bytes that `current_scanline` points to.
    size_t current_scanline_length;

    // The number of bytes between pixels in `reference_scanline` and `current_scanline`.
    //
    // The number of bytes between pixels in `reference_scanline` and `current_scanline`.
    //
    // For truecolor modes, this must be at least 4. You are free to set any number of bytes here.
    //
    // This field is useful for in-place deinterlacing.
    uint8_t stride;
};

// Data providers use this to supply scanlines to `parng` in response to RGBA conversion requests.
struct parng_scanlines_for_rgba_conversion {
    // The pixels of the RGBA scanline.
    //
    // The pixels of the RGBA scanline. There must be 4 bytes per pixel available in this array.
    //
    // It is recommended that the address of this buffer be aligned properly. To determine the
    // optimum alignment, use the `parng_image_loader_align()` function.
    uint8_t *rgba_scanline;

    // The number of valid bytes that `rgba_scanline` points to.
    size_t rgba_scanline_length;

    // The pixels of the indexed scanline, if applicable.
    //
    // The pixels of the indexed scanline, if applicable. If the image is not indexed, this should
    // be `NULL`. There must be 1 byte per pixel available in this array, if present.
    //
    // It is recommended that the address of this buffer be aligned properly. To determine the
    // optimum alignment, use the `parng_image_loader_align()` function.
    const uint8_t *indexed_scanline;

    // The number of valid bytes that `indexed_scanline_length` points to.
    size_t indexed_scanline_length;

    // The number of bytes between individual pixels in `rgba_scanline`.
    //
    // The number of bytes between individual pixels in `rgba_scanline`. This must be at least 4.
    //
    // This field is useful for in-place deinterlacing.
    uint8_t rgba_stride;

    // The number of bytes between individual pixels in `indexed_scanline`.
    //
    // The number of bytes between individual pixels in `indexed_scanline`. If the image is not
    // indexed, this field is ignored.
    //
    // This field is useful for in-place deinterlacing.
    uint8_t indexed_stride;
};

// An interface that `parng` uses to access storage for the image data.
//
// An interface that `parng` uses to access storage for the image data. By implementing this trait,
// you can choose any method you wish to store the image data and it will be transparent to
// `parng`.
//
// Be aware that the data provider will be called on a background thread; i.e. not the thread it
// was created on! You must ensure proper synchronization between the main thread and that
// background thread if you wish to communicate between them.
struct parng_data_provider {
    // Called when `parng` needs to predict a scanline.
    //
    // Called when `parng` needs to predict a scanline.
    //
    // `parng` requests one or two scanlines using this method: one for writing
    // (`current_scanline`) and, optionally, one for reading (`reference_scanline`). It is
    // guaranteed that the reference scanline will always have a smaller Y value than the current
    // scanline.
    //
    // `lod` specifies the level of detail, if the image is interlaced.
    //
    // `indexed` is true if the image has a color palette. If it is true, then the scanlines
    // returned should have 8 bits of storage per pixel. Otherwise, the data provider should
    // return scanlines with 32 bits of storage per pixel.
    //
    // `user_data` is the contents of the data provider's `user_data` field.
    void (*fetch_scanlines_for_prediction)(int32_t reference_scanline,
                                           uint32_t current_scanline,
                                           parng_level_of_detail lod,
                                           int32_t indexed,
                                           parng_scanlines_for_prediction *scanlines,
                                           void *user_data);

    // Called when `parng` has finished prediction for a scanline, optionally at a level of detail.
    //
    // Called when `parng` has finished prediction for a scanline, optionally at a specific level
    // of detail.
    //
    // If the image is in RGBA or grayscale-alpha format, then the scanline is entirely finished
    // at this time. Otherwise, unless the image is in indexed format, the scanline is finished,
    // but the alpha values are not yet valid. Finally, if the image is in indexed format, the
    // scanline palette values are correct, but the indexed-to-truecolor conversion has not
    // occurred yet, so the scanline is not yet suitable for display.
    //
    // `user_data` is the contents of the data provider's `user_data` field.
    void (*prediction_complete_for_scanline)(uint32_t scanline,
                                             parng_level_of_detail lod,
                                             void *user_data);

    // Called when `parng` needs to perform RGBA conversion for a scanline.
    //
    // Called when `parng` needs to perform RGBA conversion for a scanline, optionally at a
    // specific level of detail. `lod` specifies the level of detail, if the image is interlaced.
    // `indexed` will have a nonzero value if the image is indexed. `user_data` is the contents of
    // the data provider's `user_data` field.
    //
    // This method will be called only if the image is not RGBA.
    void (*fetch_scanlines_for_rgba_conversion)(uint32_t scanline,
                                                parng_level_of_detail lod,
                                                int32_t indexed,
                                                parng_scanlines_for_rgba_conversion *scanlines,
                                                void *user_data);

    // Called when `parng` has finished RGBA conversion for a scanline.
    //
    // Called when `parng` has finished RGBA conversion for a scanline.
    //
    // Optionally, `parng` may specify a specific level of detail. `user_data` is the contents of
    // the data provider's `user_data` field.
    //
    // This method will be called only if the image is not RGBA.
    void (*rgba_conversion_complete_for_scanline)(uint32_t scanline,
                                                  parng_level_of_detail lod,
                                                  void *user_data);

    // Called when `parng` has completely finished decoding the image.
    void (*finished)(void *user_data);

    // An arbitrary pointer that `parng` passes to the `read` and `seek` functions.
    void *user_data;
};

// An in-memory decoded image in big-endian RGBA format, 32 bits per pixel.
struct parng_image {
    // The width of the image, in pixels.
    uint32_t width;

    // The height of the image, in pixels.
    uint32_t height;

    // The number of bytes between successive scanlines.
    //
    // The number of bytes between successive scanlines. This may be any value greater than or
    // equal to `4 * width`.
    //
    // Because of SIMD alignment restrictions, `parng` may well choose a value greater than `4 *
    // width` here.
    size_t stride;

    // The number of bytes in the `pixels` allocation.
    //
    // The number of bytes in the `pixels` allocation. Do not modify this value; `parng` uses it
    // internally.
    size_t capacity;

    // A pointer to the actual pixels.
    uint8_t *pixels;
};

// Metadata found in the PNG header (dimensions, bit depth, etc.)
struct parng_metadata {
    // The width of the image, in pixels.
    uint32_t width;

    // The height of the image, in pixels.
    uint32_t height;

    // Color type used in the image.
    parng_color_type color_type;

    // Color depth (bits per pixel) used in the image.
    parng_compression_method compression_method;

    // Prediction method used in the image.
    parng_filter_method filter_method;

    // Transmission order used in the image.
    parng_interlace_method interlace_method;
};

// Information about a specific scanline for one level of detail in an interlaced image.
//
// Information about a specific scanline for one level of detail in an interlaced image.
//
// This object exists for the convenience of data providers, so that they do not have to hardcode
// information about Adam7 interlacing.
struct parng_interlacing_info {
    // The row of this scanline within the final, deinterlaced image.
    //
    // The row of this scanline within the final, deinterlaced image. 0 represents the first row.
    uint32_t y;

    // The number of bytes between individual pixels for this scanline.
    //
    // The number of bytes between individual pixels for this scanline. This represents the number
    // of bytes between pixels in the final, deinterlaced image.
    uint8_t stride;

    // The byte offset of the first pixel within this scanline in the final, deinterlaced image.
    uint8_t offset;
};

#ifdef __cplusplus
extern "C" {
#endif

// Allocates space for and loads a PNG image stream from a reader into memory.
//
// Allocates space for and loads a PNG image stream from a reader into memory. The returned image
// is big-endian, 32 bits per pixel RGBA.
//
// This method does not return until the image is fully loaded. If you need a different
// in-memory representation, or you need to display the image before it's fully loaded,
// consider using the `parng_image_loader` API instead.
parng_error parng_image_load(parng_image *image, parng_reader *reader);

// A convenience method that calls `parng_image_load` configured to read from a C `FILE` object.
parng_error parng_image_load_from_file(parng_image *image, FILE *file);

// A convenience method that calls `parng_image_load` configured to read from an in-memory buffer.
//
// A convenience method that calls `parng_image_load` configured to read from an in-memory buffer.
// `bytes` points to the PNG image stream in memory, and `length` represents its length in bytes.
parng_error parng_image_load_from_memory(parng_image *image, const uint8_t *bytes, size_t length);

// Destroys a `parng_image` object and frees the memory pointed to by `pixels`.
//
// Destroys a `parng_image` object and frees the memory pointed to by `pixels`. This function does
// not attempt to free the `image` pointer itself.
void parng_image_destroy(parng_image *image);

// Fills the `image_loader` pointer with a new image loader ready to decode a PNG image.
parng_error parng_image_loader_create(parng_image_loader **image_loader);

// Destroys the given `image_loader`, freeing it and all memory associated with it.
void parng_image_loader_destroy(parng_image_loader *image_loader);

// Decodes image data from the given stream.
//
// Decodes image data from the given stream.
//
// This function decodes an arbitrary amount of data, so repeated calls to it are necessary to
// decode the entire image.
//
// If the metadata has been read (which is checkable ether via `parng_image_loader_get_metadata` or
// by looking for a `PARNG_LOAD_PROGRESS_NEED_DATA_PROVIDER_AND_MORE_DATA` result), a data provider
// must have be attached to this image loader via `parng_image_loader_set_data_provider` before
// calling this function, or this function will fail with a `PARNG_ERROR_NO_DATA_PROVIDER` error.
//
// Returns a `parng_load_progress` value that describes the progress of loading the image.
parng_error parng_image_loader_add_data(parng_image_loader *image_loader,
                                        parng_reader *reader,
                                        parng_load_progress *load_progress);

// Blocks the current thread until the image is fully decoded.
//
// Blocks the current thread until the image is fully decoded.
//
// Because `parng` uses a background thread to perform image prediction and color conversion,
// the image may not be fully decoded even when `parng_image_loader_add_data` returns
// `PARNG_LOAD_PROGRESS_FINISHED`. Most applications will therefore want to call this function
// after receiving that result.
parng_error parng_image_loader_wait_until_finished(parng_image_loader *image_loader);

// Attaches a data provider to this image loader.
//
// Attaches a data provider to this image loader.
//
// This can be called at any time, but it must be called prior to calling
// `parng_image_loader_add_data` if the metadata is present. The metadata is present if
// `parng_image_loader_get_metadata` returns a nonzero value.
void parng_image_loader_set_data_provider(parng_image_loader *image_loader,
                                          parng_data_provider *data_provider);

// Fills `metadata_result` with a copy of the image metadata.
//
// Fills `metadata_result` with a copy of the image metadata, which contains image dimensions and
// color info.
//
// If the metadata has been loaded, this function returns 1 and populates
// `metadata_result`; if the metadata hasn't been loaded yet, it returns 0 and leaves
// `metadata_result` untouched.
uint32_t parng_image_loader_get_metadata(parng_image_loader *image_loader,
                                         parng_metadata *metadata_result);

// Rounds the given stride in bytes up to the value that provides the best performance.
//
// Rounds the given stride in bytes up to the value that provides the best performance.
//
// The stride is the distance between scanlines in bytes.
//
// It is recommended that data providers use this function to determine the stride when allocating
// space internally, so as to allow `parng` the most opportunities for use of accelerated SIMD.
uintptr_t parng_image_loader_align(uintptr_t address);

// Information about a specific scanline for one level of detail in an interlaced image.
//
// Information about a specific scanline for one level of detail in an interlaced image.
//
// This object exists for the convenience of data providers, so that they do not have to hardcode
// information about Adam7 interlacing.
void parng_interlacing_info_init(parng_interlacing_info *interlacing_info,
                                 uint32_t y,
                                 uint8_t color_depth,
                                 parng_level_of_detail lod);

#ifdef __cplusplus
}
#endif

#endif

