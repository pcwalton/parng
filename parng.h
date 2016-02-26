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

typedef uint32_t parng_color_type;
typedef uint32_t parng_compression_method;
typedef struct parng_data_provider parng_data_provider;
typedef uint32_t parng_error;
typedef uint32_t parng_filter_method;
typedef struct parng_image parng_image;
typedef struct parng_image_loader parng_image_loader;
typedef uint32_t parng_interlace_method;
typedef struct parng_interlacing_info parng_interlacing_info;
typedef uint32_t parng_io_error;
typedef uint32_t parng_level_of_detail;
typedef uint32_t parng_load_progress;
typedef struct parng_metadata parng_metadata;
typedef struct parng_reader parng_reader;
typedef struct parng_scanlines_for_prediction parng_scanlines_for_prediction;
typedef struct parng_scanlines_for_rgba_conversion parng_scanlines_for_rgba_conversion;
typedef uint32_t parng_seek_from;

struct parng_reader {
    parng_io_error (*read)(uint8_t *buffer,
                           size_t buffer_length,
                           size_t *bytes_read,
                           void *user_data);
    parng_io_error (*seek)(int64_t position,
                           parng_seek_from from,
                           uint64_t *new_position,
                           void *user_data);
    void *user_data;
};

struct parng_scanlines_for_prediction {
    uint8_t *reference_scanline;
    size_t reference_scanline_length;
    const uint8_t *current_scanline;
    size_t current_scanline_length;
    uint8_t stride;
};

struct parng_scanlines_for_rgba_conversion {
    uint8_t *rgba_scanline;
    size_t rgba_scanline_length;
    const uint8_t *indexed_scanline;
    size_t indexed_scanline_length;
    uint8_t rgba_stride;
    uint8_t indexed_stride;
};

struct parng_data_provider {
    void (*fetch_scanlines_for_prediction)(int32_t reference_scanline,
                                           uint32_t current_scanline,
                                           parng_level_of_detail lod,
                                           int32_t indexed,
                                           parng_scanlines_for_prediction *scanlines,
                                           void *user_data);
    void (*prediction_complete_for_scanline)(uint32_t scanline,
                                             parng_level_of_detail lod,
                                             void *user_data);
    void (*fetch_scanlines_for_rgba_conversion)(uint32_t scanline,
                                                parng_level_of_detail lod,
                                                parng_scanlines_for_rgba_conversion *scanlines,
                                                void *user_data);
    void (*rgba_conversion_complete_for_scanline)(uint32_t scanline,
                                                  parng_level_of_detail lod,
                                                  void *user_data);
    void (*finished)(void *user_data);
    void *user_data;
};

struct parng_image {
    uint32_t width;
    uint32_t height;
    size_t stride;
    size_t capacity;
    uint8_t *pixels;
};

struct parng_metadata {
    uint32_t width;
    uint32_t height;
    parng_color_type color_type;
    parng_compression_method compression_method;
    parng_filter_method filter_method;
    parng_interlace_method interlace_method;
};

struct parng_interlacing_info {
    uint32_t y;
    uint8_t stride;
    uint8_t offset;
};

parng_error parng_image_load(parng_image *image, parng_reader *reader);
parng_error parng_image_load_from_file(parng_image *image, FILE *file);
parng_error parng_image_load_from_memory(parng_image *image, const uint8_t *bytes, size_t length);
void parng_image_destroy(parng_image *image);

parng_error parng_image_loader_create(parng_image_loader **image_loader);
void parng_image_loader_destroy(parng_image_loader *image_loader);
parng_error parng_image_loader_add_data(parng_image_loader *image_loader,
                                        parng_reader *reader,
                                        parng_load_progress *load_progress);
parng_error parng_image_loader_wait_until_finished(parng_image_loader *image_loader);
void parng_image_loader_set_data_provider(parng_image_loader *image_loader,
                                          parng_data_provider *data_provider);
uint32_t parng_image_loader_get_metadata(parng_image_loader *image_loader,
                                         parng_metadata *metadata_result);
uintptr_t parng_image_loader_align(uintptr_t address);

void parng_interlacing_info_init(parng_interlacing_info *interlacing_info,
                                 uint32_t y,
                                 uint8_t color_depth,
                                 parng_level_of_detail lod);

#endif

