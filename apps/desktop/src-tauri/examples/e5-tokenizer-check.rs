#[path = "../src/e5-tokenizer.rs"]
pub mod e5_tokenizer;
use e5_tokenizer::{E5Tokenizer, InputKind};
use std::{path::Path, time::Instant};
fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("Pass the path to the independently downloaded pinned tokenizer.json");
    let start = Instant::now();
    let tokenizer = E5Tokenizer::load(Path::new(&path)).unwrap();
    println!("loadMs={:.3}", start.elapsed().as_secs_f64() * 1000.0);
    assert_eq!(
        tokenizer
            .model_input("Привет, как дела?", InputKind::Passage)
            .unwrap(),
        vec![0, 46692, 12, 1813, 18454, 4, 819, 7843, 32, 2]
    );
    assert_eq!(
        tokenizer
            .model_input("Когда встречаемся?", InputKind::Query)
            .unwrap(),
        vec![0, 41, 1294, 12, 25202, 56911, 39065, 32, 2]
    );
    assert_eq!(
        tokenizer.count_passage("👩‍💻  中文\r\n e\u{301}").unwrap(),
        11
    );
    assert!(tokenizer
        .model_input(&"Привет, как дела? ".repeat(200), InputKind::Passage)
        .is_err());
    let mut times = Vec::new();
    for _ in 0..100 {
        let start = Instant::now();
        let n = tokenizer
            .count_passage(&"Привет, как дела? 中文 👩‍💻\n".repeat(20))
            .unwrap();
        assert!(n > 200);
        times.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    times.sort_by(f64::total_cmp);
    println!("tokenizer={}", tokenizer.fingerprint());
    println!(
        "syntheticCount100: p50Ms={:.3} p95Ms={:.3}",
        times[50], times[95]
    );
}
