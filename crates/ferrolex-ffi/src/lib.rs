//! Experimental C ABI for ferrolex.
//!
//! This crate is a Phase 8 spike, not a stable published API. Enable the
//! `c-abi` feature to compile its exported functions and generated header.
//! It is **experimental**, unpublished (`publish = false`), and excluded from
//! the supported API under ferrolex's [pre-1.0 release
//! contract](https://github.com/sebastian-software/ferrolex/blob/main/docs/release-contract.md).

#![cfg_attr(not(feature = "c-abi"), allow(dead_code))]
#![allow(unsafe_code, reason = "C ABI boundary requires validated raw pointers")]

#[cfg(feature = "c-abi")]
mod c_abi {
    use std::panic::{catch_unwind, AssertUnwindSafe};
    use std::slice;
    use std::str;

    use ferrolex_core::{Dictionary, Normalization, WordList};
    use ferrolex_suggest::{SuggestConfig, Suggester};

    /// Opaque immutable checker handle owned by the C caller.
    pub struct FerrolexChecker {
        words: WordList,
    }

    /// Structured outcomes returned by the experimental C ABI.
    #[repr(C)]
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub enum FerrolexStatus {
        /// The operation completed successfully.
        Ok = 0,
        /// A required pointer was null.
        NullPointer = 1,
        /// An input byte span was not valid UTF-8.
        InvalidUtf8 = 2,
        /// The caller-provided output buffer was too small.
        BufferTooSmall = 3,
        /// An internal panic was caught before crossing the C boundary.
        Panic = 4,
    }

    fn protect(operation: impl FnOnce() -> FerrolexStatus) -> FerrolexStatus {
        catch_unwind(AssertUnwindSafe(operation)).unwrap_or(FerrolexStatus::Panic)
    }

    fn input_utf8<'input>(
        pointer: *const u8,
        length: usize,
    ) -> Result<&'input str, FerrolexStatus> {
        if length == 0 {
            return Ok("");
        }
        if pointer.is_null() {
            return Err(FerrolexStatus::NullPointer);
        }

        // The C caller promises that a non-null input span covers `length`
        // readable bytes for this call. We validate UTF-8 before exposing it.
        let bytes = unsafe { slice::from_raw_parts(pointer, length) };
        str::from_utf8(bytes).map_err(|_| FerrolexStatus::InvalidUtf8)
    }

    fn checker_from_pointer<'checker>(
        pointer: *const FerrolexChecker,
    ) -> Result<&'checker FerrolexChecker, FerrolexStatus> {
        if pointer.is_null() {
            return Err(FerrolexStatus::NullPointer);
        }

        // Handle validity and lifetime are C-side ownership obligations; this
        // conversion only follows a pointer returned by the constructor.
        Ok(unsafe { &*pointer })
    }

    fn write_output<T>(pointer: *mut T, value: T) -> Result<(), FerrolexStatus> {
        if pointer.is_null() {
            return Err(FerrolexStatus::NullPointer);
        }

        // The C caller promises writable storage for one `T` at this pointer.
        unsafe { pointer.write(value) };
        Ok(())
    }

    /// Creates a checker from newline-delimited UTF-8 plain-word-list text.
    ///
    /// The caller owns the resulting handle and must release it exactly once
    /// with [`ferrolex_checker_free`].
    /// # Safety
    ///
    /// Non-null input and output pointers must refer to readable or writable
    /// storage, respectively, for their advertised lengths.
    #[no_mangle]
    pub unsafe extern "C" fn ferrolex_checker_create_from_utf8(
        words: *const u8,
        words_length: usize,
        out_checker: *mut *mut FerrolexChecker,
    ) -> FerrolexStatus {
        protect(|| {
            if out_checker.is_null() {
                return FerrolexStatus::NullPointer;
            }
            let words = match input_utf8(words, words_length) {
                Ok(words) => words,
                Err(status) => return status,
            };
            let checker = Box::new(FerrolexChecker {
                words: WordList::from_text(Normalization::Exact, words),
            });
            write_output(out_checker, Box::into_raw(checker))
                .map_or_else(|status| status, |()| FerrolexStatus::Ok)
        })
    }

    /// Releases a checker handle returned by [`ferrolex_checker_create_from_utf8`].
    ///
    /// Passing null is a no-op. The handle must not be used concurrently with
    /// this call and must not be released more than once.
    /// # Safety
    ///
    /// A non-null pointer must be an unreleased handle returned by this ABI.
    #[no_mangle]
    pub unsafe extern "C" fn ferrolex_checker_free(checker: *mut FerrolexChecker) {
        if checker.is_null() {
            return;
        }

        let _ = catch_unwind(AssertUnwindSafe(|| {
            // The pointer must be an unreleased handle created by this API.
            unsafe { drop(Box::from_raw(checker)) };
        }));
    }

    /// Checks whether one UTF-8 word is recognized by the immutable checker.
    /// # Safety
    ///
    /// checker must be a live handle from this ABI. Non-null word and output
    /// pointers must refer to readable or writable storage for their lengths.
    #[no_mangle]
    pub unsafe extern "C" fn ferrolex_checker_check(
        checker: *const FerrolexChecker,
        word: *const u8,
        word_length: usize,
        out_is_correct: *mut u8,
    ) -> FerrolexStatus {
        protect(|| {
            let checker = match checker_from_pointer(checker) {
                Ok(checker) => checker,
                Err(status) => return status,
            };
            let word = match input_utf8(word, word_length) {
                Ok(word) => word,
                Err(status) => return status,
            };

            write_output(out_is_correct, u8::from(checker.words.contains(word)))
                .map_or_else(|status| status, |()| FerrolexStatus::Ok)
        })
    }

    /// Encodes ranked suggestions as NUL-separated UTF-8 bytes.
    ///
    /// Set `buffer` to null and `buffer_length` to zero to discover the exact
    /// required byte count. `out_required_bytes` and `out_suggestion_count`
    /// are written for both successful calls and `BufferTooSmall` responses.
    /// # Safety
    ///
    /// checker must be a live handle from this ABI. Non-null input and output
    /// pointers must refer to readable or writable storage for their lengths.
    #[no_mangle]
    pub unsafe extern "C" fn ferrolex_checker_suggest(
        checker: *const FerrolexChecker,
        word: *const u8,
        word_length: usize,
        buffer: *mut u8,
        buffer_length: usize,
        out_required_bytes: *mut usize,
        out_suggestion_count: *mut usize,
    ) -> FerrolexStatus {
        protect(|| {
            if out_required_bytes.is_null() || out_suggestion_count.is_null() {
                return FerrolexStatus::NullPointer;
            }
            let checker = match checker_from_pointer(checker) {
                Ok(checker) => checker,
                Err(status) => return status,
            };
            let word = match input_utf8(word, word_length) {
                Ok(word) => word,
                Err(status) => return status,
            };

            let result = Suggester::new(&checker.words, SuggestConfig::default()).suggest(word);
            let mut encoded = Vec::new();
            for suggestion in result.suggestions() {
                if !encoded.is_empty() {
                    encoded.push(0);
                }
                encoded.extend_from_slice(suggestion.word().as_bytes());
            }

            let required_bytes = encoded.len();
            let suggestion_count = result.suggestions().len();
            if let Err(status) = write_output(out_required_bytes, required_bytes) {
                return status;
            }
            if let Err(status) = write_output(out_suggestion_count, suggestion_count) {
                return status;
            }
            if buffer_length < required_bytes {
                return FerrolexStatus::BufferTooSmall;
            }
            if required_bytes == 0 {
                return FerrolexStatus::Ok;
            }
            if buffer.is_null() {
                return FerrolexStatus::NullPointer;
            }

            // Capacity was checked above and the C caller owns this writable
            // output span for the duration of the call.
            unsafe { slice::from_raw_parts_mut(buffer, required_bytes) }.copy_from_slice(&encoded);
            FerrolexStatus::Ok
        })
    }

    #[cfg(test)]
    mod tests {
        use std::ptr;

        use super::{
            ferrolex_checker_check, ferrolex_checker_create_from_utf8, ferrolex_checker_free,
            ferrolex_checker_suggest, FerrolexChecker, FerrolexStatus,
        };

        fn checker(words: &str) -> *mut FerrolexChecker {
            let mut result = ptr::null_mut();
            assert_eq!(
                unsafe {
                    ferrolex_checker_create_from_utf8(words.as_ptr(), words.len(), &mut result)
                },
                FerrolexStatus::Ok
            );
            result
        }

        #[test]
        fn checks_utf8_words_and_rejects_invalid_inputs() {
            let checker = checker("ferrolex\nStraße");
            let mut is_correct = 0;

            assert_eq!(
                unsafe {
                    ferrolex_checker_check(
                        checker,
                        "Straße".as_ptr(),
                        "Straße".len(),
                        &mut is_correct,
                    )
                },
                FerrolexStatus::Ok
            );
            assert_eq!(is_correct, 1);
            assert_eq!(
                unsafe { ferrolex_checker_check(checker, [0xff].as_ptr(), 1, &mut is_correct) },
                FerrolexStatus::InvalidUtf8
            );
            assert_eq!(
                unsafe { ferrolex_checker_check(checker, ptr::null(), 1, &mut is_correct) },
                FerrolexStatus::NullPointer
            );

            unsafe { ferrolex_checker_free(checker) };
        }

        #[test]
        fn suggests_into_a_caller_owned_buffer() {
            let checker = checker("ferrolex\nferrous\nFerris");
            let mut required = 0;
            let mut count = 0;

            assert_eq!(
                unsafe {
                    ferrolex_checker_suggest(
                        checker,
                        b"ferolex".as_ptr(),
                        b"ferolex".len(),
                        ptr::null_mut(),
                        0,
                        &mut required,
                        &mut count,
                    )
                },
                FerrolexStatus::BufferTooSmall
            );
            assert_eq!(count, 1);
            let mut buffer = vec![0; required];
            assert_eq!(
                unsafe {
                    ferrolex_checker_suggest(
                        checker,
                        b"ferolex".as_ptr(),
                        b"ferolex".len(),
                        buffer.as_mut_ptr(),
                        buffer.len(),
                        &mut required,
                        &mut count,
                    )
                },
                FerrolexStatus::Ok
            );
            assert_eq!(buffer, b"ferrolex");

            unsafe { ferrolex_checker_free(checker) };
        }
    }
}

#[cfg(feature = "c-abi")]
pub use c_abi::*;
