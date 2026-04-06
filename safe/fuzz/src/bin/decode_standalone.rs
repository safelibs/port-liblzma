use std::env;
use std::fs;
use std::io::{self, Read};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let paths: Vec<_> = env::args_os().skip(1).collect();
    if paths.is_empty() {
        let mut input = Vec::new();
        io::stdin().read_to_end(&mut input)?;
        safe_liblzma_fuzz::decode_one_input(&input);
        return Ok(());
    }

    for path in paths {
        let input = fs::read(&path)?;
        safe_liblzma_fuzz::decode_one_input(&input);
    }

    Ok(())
}
