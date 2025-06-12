use cairo_lang_starknet_classes::contract_class::ContractEntryPoint;
use cairo_native::starknet::{ResourceBounds, SyscallResult, TxV2Info};
use starknet_api::core::EntryPointSelector;
use starknet_api::transaction::fields::{Resource, ValidResourceBounds};
use starknet_types_core::felt::Felt;

use crate::transaction::objects::CurrentTransactionInfo;

pub fn contract_entrypoint_to_entrypoint_selector(
    entrypoint: &ContractEntryPoint,
) -> EntryPointSelector {
    EntryPointSelector(Felt::from(&entrypoint.selector))
}

pub fn encode_str_as_felts(msg: &str) -> Vec<Felt> {
    const CHUNK_SIZE: usize = 32;

    let data = msg.as_bytes().chunks(CHUNK_SIZE - 1);
    let mut encoding = vec![Felt::default(); data.len()];
    for (i, data_chunk) in data.enumerate() {
        let mut chunk = [0_u8; CHUNK_SIZE];
        chunk[1..data_chunk.len() + 1].copy_from_slice(data_chunk);
        encoding[i] = Felt::from_bytes_be(&chunk);
    }
    encoding
}

pub fn default_tx_v2_info() -> TxV2Info {
    TxV2Info {
        version: Default::default(),
        account_contract_address: Default::default(),
        max_fee: 0,
        signature: vec![],
        transaction_hash: Default::default(),
        chain_id: Default::default(),
        nonce: Default::default(),
        resource_bounds: vec![],
        tip: 0,
        paymaster_data: vec![],
        nonce_data_availability_mode: 0,
        fee_data_availability_mode: 0,
        account_deployment_data: vec![],
    }
}

pub fn calculate_resource_bounds(
    tx_info: &CurrentTransactionInfo,
) -> SyscallResult<Vec<ResourceBounds>> {
    Ok(match tx_info.resource_bounds {
        ValidResourceBounds::L1Gas(l1_bounds) => {
            vec![
                ResourceBounds {
                    resource: Felt::from_hex(Resource::L1Gas.to_hex()).unwrap(),
                    max_amount: l1_bounds.max_amount.0,
                    max_price_per_unit: l1_bounds.max_price_per_unit.0,
                },
                ResourceBounds {
                    resource: Felt::from_hex(Resource::L2Gas.to_hex()).unwrap(),
                    max_amount: 0,
                    max_price_per_unit: 0,
                },
            ]
        }
        ValidResourceBounds::AllResources(all_bounds) => {
            vec![
                ResourceBounds {
                    resource: Felt::from_hex(Resource::L1Gas.to_hex()).unwrap(),
                    max_amount: all_bounds.l1_gas.max_amount.0,
                    max_price_per_unit: all_bounds.l1_gas.max_price_per_unit.0,
                },
                ResourceBounds {
                    resource: Felt::from_hex(Resource::L2Gas.to_hex()).unwrap(),
                    max_amount: all_bounds.l2_gas.max_amount.0,
                    max_price_per_unit: all_bounds.l2_gas.max_price_per_unit.0,
                },
                ResourceBounds {
                    resource: Felt::from_hex(Resource::L1DataGas.to_hex()).unwrap(),
                    max_amount: all_bounds.l1_data_gas.max_amount.0,
                    max_price_per_unit: all_bounds.l1_data_gas.max_price_per_unit.0,
                },
            ]
        }
    })
}

#[cfg(feature = "with-libfunc-profiling")]
pub mod libfunc_profiler {
    use std::collections::HashMap;

    use cairo_lang_sierra::ids::ConcreteLibfuncId;
    use cairo_lang_sierra::program::Program;
    use num_traits::ToPrimitive;
    use serde::Serialize;

    #[derive(Clone, Copy, Debug, Serialize)]
    pub struct LibfuncProfileSummary {
        pub libfunc_idx: u64,
        pub samples: u64,
        pub total_time: u64,
        pub average_time: f64,
        pub std_deviation: f64,
        pub quartiles: [u64; 5],
    }

    pub fn process_profiles(
        profiles: HashMap<ConcreteLibfuncId, (Vec<u64>, u64)>,
        program: &Program,
    ) -> Vec<LibfuncProfileSummary> {
        let mut processed_profiles = profiles
            .into_iter()
            .map(|(libfunc_idx, (mut tick_deltas, extra_count))| {
                // if no deltas were registered, we only return the libfunc's calls amount
                if tick_deltas.is_empty() {
                    return LibfuncProfileSummary {
                        libfunc_idx: libfunc_idx.id,
                        samples: extra_count,
                        total_time: 0,
                        average_time: 0.0,
                        std_deviation: 0.0,
                        quartiles: [0; 5],
                    };
                }

                tick_deltas.sort();

                // Drop outliers.
                {
                    let q1 = tick_deltas[tick_deltas.len() / 4];
                    let q3 = tick_deltas[3 * tick_deltas.len() / 4];
                    let iqr = q3 - q1;

                    let q1_thr = q1.saturating_sub(iqr + iqr / 2);
                    let q3_thr = q3 + (iqr + iqr / 2);

                    tick_deltas.retain(|x| *x >= q1_thr && *x <= q3_thr);
                }

                // Compute the quartiles.
                let quartiles = [
                    *tick_deltas.first().unwrap(),
                    tick_deltas[tick_deltas.len() / 4],
                    tick_deltas[tick_deltas.len() / 2],
                    tick_deltas[3 * tick_deltas.len() / 4],
                    *tick_deltas.last().unwrap(),
                ];

                // Compuite the average.
                let average = tick_deltas.iter().copied().sum::<u64>().to_f64().unwrap()
                    / tick_deltas.len().to_f64().unwrap();

                // Compute the standard deviation.
                let std_dev = {
                    let sum = tick_deltas
                        .iter()
                        .copied()
                        .map(|x| x.to_f64().unwrap())
                        .map(|x| (x - average))
                        .map(|x| x * x)
                        .sum::<f64>();
                    sum / (tick_deltas.len().to_u64().unwrap() + extra_count).to_f64().unwrap()
                };

                LibfuncProfileSummary {
                    libfunc_idx: libfunc_idx.id,
                    samples: tick_deltas.len().to_u64().unwrap() + extra_count,
                    total_time: tick_deltas.iter().sum::<u64>()
                        + (extra_count.to_f64().unwrap() * average).round().to_u64().unwrap(),
                    average_time: average,
                    std_deviation: std_dev,
                    quartiles,
                }
            })
            .collect::<Vec<_>>();

        processed_profiles.sort_by_key(|LibfuncProfileSummary { libfunc_idx, .. }| {
            program
                .libfunc_declarations
                .iter()
                .enumerate()
                .find_map(|(i, x)| (x.id.id == *libfunc_idx).then_some(i))
                .unwrap()
        });

        processed_profiles
    }
}
