// parng/c-example.c
//
// Copyright (c) 2016 Mozilla Foundation

#include <assert.h>
#include <limits.h>
#include <pthread.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include "parng.h"

#define OUTPUT_BPP 4

struct decoded_image {
    uint32_t width;
    uint32_t height;
    uint8_t *rgba_pixels;
    uint8_t *indexed_pixels;

    bool finished;
    pthread_mutex_t finished_mutex;
    pthread_cond_t finished_cond;
};

static void die(parng_error err) {
    fprintf(stderr, "parng error: %d\n", (int)err);
    exit(1);
}

static parng_io_error read_from_file(uint8_t *buffer,
                                     size_t buffer_length,
                                     size_t *bytes_read,
                                     void *user_data) {
    FILE *in_file = (FILE *)user_data;
    *bytes_read = fread(buffer, 1, buffer_length, in_file);
    return ferror(in_file) ? PARNG_ERROR_IO : PARNG_SUCCESS;
}

static parng_io_error seek_in_file(int64_t position,
                                   parng_seek_from from,
                                   uint64_t *new_position,
                                   void *user_data) {
    if (position > LONG_MAX)
        return PARNG_ERROR_IO;

    FILE *in_file = (FILE *)user_data;
    int whence;
    switch (from) {
    case PARNG_SEEK_FROM_START:     whence = SEEK_SET; break;
    case PARNG_SEEK_FROM_CURRENT:   whence = SEEK_CUR; break;
    case PARNG_SEEK_FROM_END:       whence = SEEK_END; break;
    default:                        abort();
    }

    if (fseek(in_file, (long)position, whence) < 0)
       return PARNG_ERROR_IO;
    long tell_result = ftell(in_file);
    if (tell_result < 0)
        return PARNG_ERROR_IO;
    *new_position = tell_result;
    return PARNG_SUCCESS;
}

static void fetch_scanlines_for_prediction(int32_t reference_scanline,
                                           uint32_t current_scanline,
                                           parng_level_of_detail lod,
                                           parng_scanlines_for_prediction *scanlines,
                                           void *user_data) {
    struct decoded_image *decoded_image = (struct decoded_image *)user_data;
    assert(reference_scanline <= (int32_t)decoded_image->height);
    assert(current_scanline <= decoded_image->height);

    parng_interlacing_info reference_interlacing_info;
    if (reference_scanline >= 0) {
        parng_interlacing_info_init(&reference_interlacing_info,
                                    reference_scanline,
                                    OUTPUT_BPP * 8,
                                    lod);
    }
    parng_interlacing_info current_interlacing_info;
    parng_interlacing_info_init(&current_interlacing_info, current_scanline, OUTPUT_BPP * 8, lod);

    uintptr_t aligned_stride = parng_image_loader_align(decoded_image->width * 4);
    if (reference_scanline >= 0) {
        uintptr_t start = reference_interlacing_info.y * aligned_stride +
            reference_interlacing_info.offset;
        scanlines->reference_scanline = &decoded_image->rgba_pixels[start];
        scanlines->reference_scanline_length = aligned_stride;
    }

    uintptr_t start = current_interlacing_info.y * aligned_stride +
        current_interlacing_info.offset;
    scanlines->current_scanline = &decoded_image->pixels[start];
    scanlines->current_scanline_length = aligned_stride;

    scanlines->stride = current_interlacing_info.stride;
}

static void fetch_scanlines_for_rgba_conversion(uint32_t scanline,
                                                parng_level_of_detail lod,
                                                parng_scanlines_for_rgba_conversion *scanlines,
                                                void *user_data) {
    struct decoded_image *decoded_image = (struct decoded_image *)user_data;
    assert(scanline <= (int32_t)decoded_image->height);

    parng_interlacing_info rgba_interlacing_info, indexed_interlacing_info;
    parng_interlacing_info_init(&rgba_interlacing_info, scanline, OUTPUT_BPP * 8, lod);
    parng_interlacing_info_init(&indexed_interlacing_info, scanline, 1 * 8, lod);
    
    uintptr_t aligned_stride = parng_image_loader_align(decoded_image->width * 4);
    scanlines->rgba_scanline = &decoded_image->rgba_pixels;
    scanlines->indexed_scanline = &decoded_image->indexed_pixels;
    scanlines->rgba_scanline_length = scanlines->indexed_scanline_length = aligned_stride;
    scanlines->rgba_stride = scanlines->indexed_stride = current_interlacing_info.stride;
}

void extract_data(void *user_data) {
    struct decoded_image *decoded_image = (struct decoded_image *)user_data;
    pthread_mutex_lock(&decoded_image->finished_mutex);
    decoded_image->finished = true;
    pthread_cond_signal(&decoded_image->finished_cond);
    pthread_mutex_unlock(&decoded_image->finished_mutex);
}

int main(int argc, const char **argv) {
    if (argc < 3) {
        fprintf(stderr, "usage: c-example input.png output.tga\n");
        return 0;
    }

    FILE *in_file = fopen(argv[1], "r");
    if (in_file == NULL) {
        perror("Failed to open input file");
        return 1;
    }

    parng_error err;
    parng_image_loader *image_loader;
    if ((err = parng_image_loader_create(&image_loader)) != PARNG_SUCCESS)
        die(err);
    parng_reader reader = { read_from_file, seek_in_file, in_file };

    parng_metadata metadata;
    parng_add_data_result add_data_result;
    do {
        err = parng_image_loader_add_data(image_loader, &reader, &add_data_result);
        if (err != PARNG_SUCCESS)
            die(err);
        assert(add_data_result == PARNG_ADD_DATA_RESULT_CONTINUE);
    } while (!parng_image_loader_get_metadata(image_loader, &metadata));

    uintptr_t aligned_stride = parng_image_loader_align(metadata.width * 4);
    uint8_t *buffer = malloc(aligned_stride * metadata.height);
    if (buffer == NULL) {
        fprintf(stderr, "Failed to allocate space for the pixels!\n");
        exit(1);
    }
    pthread_mutex_t finished_mutex;
    if (pthread_mutex_init(&finished_mutex, NULL) != 0) {
        perror(NULL);
        exit(1);
    }
    pthread_cond_t finished_cond;
    if (pthread_cond_init(&finished_cond, NULL) != 0) {
        perror(NULL);
        exit(1);
    }
    struct decoded_image decoded_image = {
        metadata.width,
        metadata.height,
        buffer,
        false,
        finished_mutex,
        finished_cond
    };

    struct parng_data_provider data_provider = { get_scanline_data, extract_data, &decoded_image };
    parng_image_loader_set_data_provider(image_loader, &data_provider);

    do {
        parng_image_loader_add_data(image_loader, &reader, &add_data_result);
    } while (add_data_result == PARNG_ADD_DATA_RESULT_CONTINUE);
    parng_image_loader_wait_until_finished(image_loader);
    parng_image_loader_extract_data(image_loader);

    pthread_mutex_lock(&decoded_image.finished_mutex);
    while (!decoded_image.finished)
        pthread_cond_wait(&decoded_image.finished_cond, &decoded_image.finished_mutex);
    pthread_mutex_unlock(&decoded_image.finished_mutex);

    fclose(in_file);

    FILE *out_file = fopen(argv[2], "w");
    if (out_file == NULL) {
        perror("Failed to open output file");
        return 1;
    }
    uint8_t tga_header_data[18] = {
        0, 0, 2, 0,
        0, 0, 0, 0,
        0, 0, 0, 0,
        (metadata.width & 0xff), (metadata.width >> 8) & 0xff,
        (metadata.height & 0xff), (metadata.height >> 8) & 0xff,
        24, 0
    };
    if (fwrite(tga_header_data, 18, 1, out_file) < 1) {
        perror("Failed to write TGA header");
        return 1;
    }

    for (uint32_t row = 0; row < metadata.height; row++) {
        uint32_t y = metadata.height - row - 1;
        for (uint32_t x = 0; x < metadata.width; x++) {
            size_t start = aligned_stride * y + x * OUTPUT_BPP;
            uint8_t pixel_data[3] = {
                decoded_image.pixels[start + 2],
                decoded_image.pixels[start + 1],
                decoded_image.pixels[start + 0]
            };
            if (fwrite(pixel_data, 3, 1, out_file) < 1) {
                perror("Failed to write decoded pixels");
                return 1;
            }
        }
    }

    fclose(out_file);
    return 0;
}

