//! Numeric instruction snapshots kept separate from semantic test cases.

pub fn words<T: TryFrom<u32>>(case: &str) -> Vec<(T, u32)> {
    let fixture = include_str!("../fixtures/original-words.tsv");
    let heading = format!("[{case}]\n");
    let (_, body) = fixture
        .split_once(&heading)
        .expect("missing instruction fixture");
    let words: Vec<_> = body
        .lines()
        .take_while(|line| !line.starts_with('['))
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| {
            let (address, expected) = line.split_once('\t').expect("instruction fixture columns");
            let hex = |value: &str| {
                u32::from_str_radix(value.strip_prefix("0x").unwrap(), 16)
                    .expect("hex instruction fixture")
            };
            (
                T::try_from(hex(address))
                    .ok()
                    .expect("instruction address fits"),
                hex(expected),
            )
        })
        .collect();
    assert!(!words.is_empty(), "empty instruction fixture {case}");
    words
}
