use std::collections::BTreeMap;

use papyrus_config::dumping::{append_sub_config_name, ser_param, SerializeConfig};
use papyrus_config::{ParamPath, ParamPrivacyInput, SerializedParam};
use serde::{Deserialize, Serialize};
use starknet_api::block::GasPriceVector;
use starknet_api::core::{ChainId, ContractAddress};
use starknet_api::transaction::fields::{
    AllResourceBounds,
    GasVectorComputationMode,
    ValidResourceBounds,
};

use crate::blockifier::block::BlockInfo;
use crate::bouncer::BouncerConfig;
use crate::transaction::objects::{
    CurrentTransactionInfo,
    FeeType,
    HasRelatedFeeType,
    TransactionInfo,
    TransactionInfoCreator,
};
use crate::versioned_constants::VersionedConstants;

#[derive(Clone, Debug)]
pub struct TransactionContext {
    pub block_context: BlockContext,
    pub tx_info: TransactionInfo,
}

impl TransactionContext {
    pub fn fee_token_address(&self) -> ContractAddress {
        self.block_context.chain_info.fee_token_address(&self.tx_info.fee_type())
    }
    pub fn is_sequencer_the_sender(&self) -> bool {
        self.tx_info.sender_address() == self.block_context.block_info.sequencer_address
    }
    pub fn get_gas_vector_computation_mode(&self) -> GasVectorComputationMode {
        self.tx_info.gas_mode()
    }
    pub fn get_gas_prices(&self) -> &GasPriceVector {
        self.block_context
            .block_info
            .gas_prices
            .get_gas_prices_by_fee_type(&self.tx_info.fee_type())
    }

    /// Returns the initial Sierra gas of the transaction.
    /// This value is used to limit the transaction's run.
    // TODO(tzahi): replace returned value from u64 to GasAmount.
    pub fn initial_sierra_gas(&self) -> u64 {
        match &self.tx_info {
            TransactionInfo::Deprecated(_)
            | TransactionInfo::Current(CurrentTransactionInfo {
                resource_bounds: ValidResourceBounds::L1Gas(_),
                ..
            }) => self.block_context.versioned_constants.default_initial_gas_cost(),
            TransactionInfo::Current(CurrentTransactionInfo {
                resource_bounds: ValidResourceBounds::AllResources(AllResourceBounds { l2_gas, .. }),
                ..
            }) => l2_gas.max_amount.0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct BlockContext {
    // TODO(Yoni, 1/10/2024): consider making these fields public.
    pub(crate) block_info: BlockInfo,
    pub(crate) chain_info: ChainInfo,
    pub(crate) versioned_constants: VersionedConstants,
    pub(crate) bouncer_config: BouncerConfig,
}

impl BlockContext {
    pub fn new(
        block_info: BlockInfo,
        chain_info: ChainInfo,
        versioned_constants: VersionedConstants,
        bouncer_config: BouncerConfig,
    ) -> Self {
        BlockContext { block_info, chain_info, versioned_constants, bouncer_config }
    }

    pub fn block_info(&self) -> &BlockInfo {
        &self.block_info
    }

    pub fn chain_info(&self) -> &ChainInfo {
        &self.chain_info
    }

    pub fn versioned_constants(&self) -> &VersionedConstants {
        &self.versioned_constants
    }

    pub fn to_tx_context(
        &self,
        tx_info_creator: &impl TransactionInfoCreator,
    ) -> TransactionContext {
        TransactionContext {
            block_context: self.clone(),
            tx_info: tx_info_creator.create_tx_info(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ChainInfo {
    pub chain_id: ChainId,
    pub fee_token_addresses: FeeTokenAddresses,
}

impl ChainInfo {
    // TODO(Gilad): since fee_type comes from TransactionInfo, we can move this method into
    // TransactionContext, which has both the chain_info (through BlockContext) and the tx_info.
    // That is, add to BlockContext with the signature `pub fn fee_token_address(&self)`.
    pub fn fee_token_address(&self, fee_type: &FeeType) -> ContractAddress {
        self.fee_token_addresses.get_by_fee_type(fee_type)
    }
}

impl Default for ChainInfo {
    fn default() -> Self {
        ChainInfo {
            chain_id: ChainId::Other("0x0".to_string()),
            fee_token_addresses: FeeTokenAddresses::default(),
        }
    }
}

impl SerializeConfig for ChainInfo {
    fn dump(&self) -> BTreeMap<ParamPath, SerializedParam> {
        let members = BTreeMap::from_iter([ser_param(
            "chain_id",
            &self.chain_id,
            "The chain ID of the StarkNet chain.",
            ParamPrivacyInput::Public,
        )]);

        vec![
            members,
            append_sub_config_name(self.fee_token_addresses.dump(), "fee_token_addresses"),
        ]
        .into_iter()
        .flatten()
        .collect()
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct FeeTokenAddresses {
    pub strk_fee_token_address: ContractAddress,
    pub eth_fee_token_address: ContractAddress,
}

impl FeeTokenAddresses {
    pub fn get_by_fee_type(&self, fee_type: &FeeType) -> ContractAddress {
        match fee_type {
            FeeType::Strk => self.strk_fee_token_address,
            FeeType::Eth => self.eth_fee_token_address,
        }
    }
}

impl SerializeConfig for FeeTokenAddresses {
    fn dump(&self) -> BTreeMap<ParamPath, SerializedParam> {
        BTreeMap::from_iter([
            ser_param(
                "strk_fee_token_address",
                &self.strk_fee_token_address,
                "Address of the STRK fee token.",
                ParamPrivacyInput::Public,
            ),
            ser_param(
                "eth_fee_token_address",
                &self.eth_fee_token_address,
                "Address of the ETH fee token.",
                ParamPrivacyInput::Public,
            ),
        ])
    }
}
