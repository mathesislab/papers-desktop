//! Print `names::ascii` of every character above ASCII, for `tests/gen_library_expected.py`:
//! one line per character, `HEX<TAB>transliteration as a JSON string`; the characters that the
//! table drops are not listed.

fn main() {
    for cp in 0x80u32..0x110000 {
        let Some(c) = char::from_u32(cp) else {
            continue;
        };
        let out = papers_rs::names::ascii(&c.to_string());
        if !out.is_empty() {
            println!("{cp:x}\t{}", serde_json::to_string(&out).unwrap());
        }
    }
}
