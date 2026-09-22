use std::error::Error;

use zones_niri::NiriEventStream;

fn main() -> Result<(), Box<dyn Error>> {
    let limit = std::env::args().nth(1).and_then(|value| value.parse::<usize>().ok()).unwrap_or(32);
    let mut stream = NiriEventStream::connect()?;

    for _ in 0..limit {
        println!("{:?}", stream.next_event()?);
    }
    Ok(())
}
