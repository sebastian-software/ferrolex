#![no_main]

use ferrolex_hunspell::{
    import_bytes, import_bytes_with_encodings, ByteEncoding, ByteImportEncodings, ImportMode,
};
use libfuzzer_sys::fuzz_target;

const PAIR_SEPARATOR: &[u8] = b"\n---FERROLEX-DIC---\n";

fn split_pair(data: &[u8]) -> (&[u8], &[u8]) {
    if let Some(offset) = data
        .windows(PAIR_SEPARATOR.len())
        .position(|window| window == PAIR_SEPARATOR)
    {
        return (&data[..offset], &data[offset + PAIR_SEPARATOR.len()..]);
    }
    data.split_at(data.len() / 2)
}

fuzz_target!(|data: &[u8]| {
    let (aff, dic) = split_pair(data);
    let _ = import_bytes("fuzz.aff", aff, "fuzz.dic", dic, ImportMode::Lenient);

    for encoding in [
        ByteEncoding::Utf8,
        ByteEncoding::Iso8859_1,
        ByteEncoding::Iso8859_2,
        ByteEncoding::Utf8WithIso8859_2Fallback,
    ] {
        let _ = import_bytes_with_encodings(
            "fuzz.aff",
            aff,
            "fuzz.dic",
            dic,
            ByteImportEncodings::same(encoding),
            ImportMode::Lenient,
        );
    }
});
