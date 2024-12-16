use std::collections::HashMap;

use blockifier::{state::cached_state::CachedState, test_utils::dict_state_reader::DictStateReader};

pub const ACCOUNT_ADDRESS: u32 = 4321;
pub const OWNER_ADDRESS: u32 = 4321;
const WARMUP_TIME: Duration = Duration::from_secs(3);
const BENCHMARK_TIME: Duration = Duration::from_secs(5);

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let cairo_native = match &*args[1] {
        "native" => true,
        "vm" | "" => false,
        arg => {
            info!("Not a valid mode: {}, using vm", arg);
            false
        }
    };
    let state = generate_state()?;

    Ok(())
}

fn generate_state() -> Result<CachedState<DictStateReader>, Box<dyn std::error::Error>> {
    let mut class_hash_to_class = HashMap::new();
    let mut address_to_class_hash = HashMap::new();
    let mut address_to_nonce = HashMap::new();

    Ok(DictStateReader {
        address_to_class_hash,
        class_hash_to_class,
        address_to_nonce,
        ..Default::default()
    })
}

fn declare() -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}

fn deploy() -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}

fn invoke() -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}
