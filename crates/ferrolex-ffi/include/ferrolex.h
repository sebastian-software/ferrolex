#ifndef FERROLEX_H
#define FERROLEX_H

#pragma once

#include <stddef.h>
#include <stdint.h>

// Structured outcomes returned by the experimental C ABI.
typedef enum FerrolexStatus {
  // The operation completed successfully.
  Ok = 0,
  // A required pointer was null.
  NullPointer = 1,
  // An input byte span was not valid UTF-8.
  InvalidUtf8 = 2,
  // The caller-provided output buffer was too small.
  BufferTooSmall = 3,
  // An internal panic was caught before crossing the C boundary.
  Panic = 4,
} FerrolexStatus;

// Opaque immutable checker handle owned by the C caller.
typedef struct FerrolexChecker FerrolexChecker;

// Creates a checker from newline-delimited UTF-8 plain-word-list text.
//
// The caller owns the resulting handle and must release it exactly once
// with [`ferrolex_checker_free`].
// # Safety
//
// Non-null input and output pointers must refer to readable or writable
// storage, respectively, for their advertised lengths.
enum FerrolexStatus ferrolex_checker_create_from_utf8(const uint8_t *words,
                                                      size_t words_length,
                                                      struct FerrolexChecker **out_checker);

// Releases a checker handle returned by [`ferrolex_checker_create_from_utf8`].
//
// Passing null is a no-op. The handle must not be used concurrently with
// this call and must not be released more than once.
// # Safety
//
// A non-null pointer must be an unreleased handle returned by this ABI.
void ferrolex_checker_free(struct FerrolexChecker *checker);

// Checks whether one UTF-8 word is recognized by the immutable checker.
// # Safety
//
// checker must be a live handle from this ABI. Non-null word and output
// pointers must refer to readable or writable storage for their lengths.
enum FerrolexStatus ferrolex_checker_check(const struct FerrolexChecker *checker,
                                           const uint8_t *word,
                                           size_t word_length,
                                           uint8_t *out_is_correct);

// Encodes ranked suggestions as NUL-separated UTF-8 bytes.
//
// Set `buffer` to null and `buffer_length` to zero to discover the exact
// required byte count. `out_required_bytes` and `out_suggestion_count`
// are written for both successful calls and `BufferTooSmall` responses.
// # Safety
//
// checker must be a live handle from this ABI. Non-null input and output
// pointers must refer to readable or writable storage for their lengths.
enum FerrolexStatus ferrolex_checker_suggest(const struct FerrolexChecker *checker,
                                             const uint8_t *word,
                                             size_t word_length,
                                             uint8_t *buffer,
                                             size_t buffer_length,
                                             size_t *out_required_bytes,
                                             size_t *out_suggestion_count);

#endif  /* FERROLEX_H */
