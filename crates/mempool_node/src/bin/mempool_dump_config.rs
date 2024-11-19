use papyrus_config::dumping::SerializeConfig;
use starknet_mempool_node::config::{DEFAULT_CONFIG_PATH, MempoolNodeConfig};

/// Updates the default config file by:
/// cargo run --bin mempool_dump_config -q
fn main() {
    MempoolNodeConfig::default()
        .dump_to_file(&vec![], DEFAULT_CONFIG_PATH)
        .expect("dump to file error");
}
