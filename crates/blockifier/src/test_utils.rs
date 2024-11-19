pub mod cached_state;
pub mod cairo_compile;
pub mod contracts;
pub mod declare;
pub mod deploy_account;
pub mod dict_state_reader;
pub mod initial_test_state;
pub mod invoke;
pub mod prices;
pub mod struct_impls;
pub mod transfers_generator;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use cairo_native::starknet::SyscallResult;
use cairo_vm::vm::runners::cairo_runner::ExecutionResources;
use starknet_api::core::{
    ClassHash,
    ContractAddress,
    Nonce,
    PatriciaKey,
    calculate_contract_address,
};
use starknet_api::state::StorageKey;
use starknet_api::transaction::{
    Calldata,
    ContractAddressSalt,
    DeprecatedResourceBoundsMapping,
    Resource,
    ResourceBounds,
    TransactionVersion,
};
use starknet_api::{class_hash, contract_address, felt, patricia_key};
use starknet_types_core::felt::Felt;

use self::dict_state_reader::DictStateReader;
use crate::abi::abi_utils::{get_fee_token_var_address, selector_from_name};
use crate::context::{BlockContext, TransactionContext};
use crate::execution::call_info::{CallInfo, OrderedEvent};
use crate::execution::common_hints::ExecutionMode;
use crate::execution::deprecated_syscalls::hint_processor::SyscallCounter;
use crate::execution::entry_point::{
    CallEntryPoint,
    ConstructorContext,
    EntryPointExecutionContext,
};
use crate::execution::execution_utils::execute_deployment;
use crate::execution::native::utils::{
    contract_address_to_native_felt,
    decode_felts_as_str,
    encode_str_as_felts,
};
use crate::execution::syscalls::SyscallSelector;
use crate::execution::syscalls::hint_processor::FAILED_TO_CALCULATE_CONTRACT_ADDRESS;
use crate::state::cached_state::{CachedState, StateChangesCount};
use crate::state::state_api::State;
use crate::test_utils::cached_state::get_erc20_class_hash_mapping;
use crate::test_utils::contracts::FeatureContract;
use crate::transaction::objects::{StarknetResources, TransactionInfo};
use crate::transaction::transaction_types::TransactionType;
use crate::utils::{const_max, u128_from_usize};
use crate::versioned_constants::VersionedConstants;
// TODO(Dori, 1/2/2024): Remove these constants once all tests use the `contracts` and
//   `initial_test_state` modules for testing.
// Addresses.
pub const TEST_SEQUENCER_ADDRESS: &str = "0x1000";
pub const TEST_ERC20_CONTRACT_ADDRESS: &str = "0x1001";
pub const TEST_ERC20_CONTRACT_ADDRESS2: &str = "0x1002";
pub const TEST_ERC20_FULL_CONTRACT_ADDRESS: &str = "0x1003";

// Class hashes.
// TODO(Adi, 15/01/2023): Remove and compute the class hash corresponding to the ERC20 contract in
// starkgate once we use the real ERC20 contract.
pub const TEST_EMPTY_CONTRACT_CLASS_HASH: &str = "0x112"; // REBASE NOTE: remove
pub const TEST_ERC20_CONTRACT_CLASS_HASH: &str = "0x1010";

pub const TEST_ERC20_FULL_CONTRACT_CLASS_HASH: &str = "0x1011";

// Paths.
pub const TEST_EMPTY_CONTRACT_CAIRO1_PATH: &str =
    "./feature_contracts/cairo1/compiled/empty_contract.casm.json"; // REBASE NOTE: remove
pub const ERC20_CONTRACT_PATH: &str = "./ERC20/ERC20_Cairo0/ERC20_without_some_syscalls/ERC20/\
                                       erc20_contract_without_some_syscalls_compiled.json";
pub const ERC20_FULL_CONTRACT_PATH: &str =
    "./oz_erc20/target/dev/oz_erc20_Native.contract_class.json"; // REBASE NOTE: change name
pub const TEST_CONTRACT_SIERRA_PATH: &str =
    "./feature_contracts/cairo1/compiled/sierra_test_contract.sierra.json";

// TODO(Aviv, 14/7/2024): Move from test utils module, and use it in ContractClassVersionMismatch
// error.
#[derive(Clone, Hash, PartialEq, Eq, Copy, Debug)]
pub enum CairoVersion {
    Cairo0,
    Cairo1,
    Native,
}

impl Default for CairoVersion {
    fn default() -> Self {
        Self::Cairo0
    }
}

impl CairoVersion {
    // A declare transaction of the given version, can be used to declare contracts of the returned
    // cairo version.
    // TODO: Make TransactionVersion an enum and use match here.
    pub fn from_declare_tx_version(tx_version: TransactionVersion) -> Self {
        if tx_version == TransactionVersion::ZERO || tx_version == TransactionVersion::ONE {
            CairoVersion::Cairo0
        } else if tx_version == TransactionVersion::TWO || tx_version == TransactionVersion::THREE {
            CairoVersion::Cairo1
        } else {
            panic!("Transaction version {:?} is not supported.", tx_version)
        }
    }

    pub fn other(&self) -> Self {
        match self {
            Self::Cairo0 => Self::Cairo1,
            Self::Cairo1 => Self::Cairo0,
            Self::Native => Self::Cairo1,
        }
    }
}

// Storage keys.
pub fn test_erc20_sequencer_balance_key() -> StorageKey {
    get_fee_token_var_address(contract_address!(TEST_SEQUENCER_ADDRESS))
}

// The max_fee / resource bounds used for txs in this test.
pub const MAX_L1_GAS_AMOUNT: u64 = 1000000;
#[allow(clippy::as_conversions)]
pub const MAX_L1_GAS_AMOUNT_U128: u128 = MAX_L1_GAS_AMOUNT as u128;
pub const MAX_L1_GAS_PRICE: u128 = DEFAULT_STRK_L1_GAS_PRICE;
pub const MAX_RESOURCE_COMMITMENT: u128 = MAX_L1_GAS_AMOUNT_U128 * MAX_L1_GAS_PRICE;
pub const MAX_FEE: u128 = MAX_L1_GAS_AMOUNT_U128 * DEFAULT_ETH_L1_GAS_PRICE;

// The amount of test-token allocated to the account in this test, set to a multiple of the max
// amount deprecated / non-deprecated transactions commit to paying.
pub const BALANCE: u128 = 10 * const_max(MAX_FEE, MAX_RESOURCE_COMMITMENT);

pub const DEFAULT_ETH_L1_GAS_PRICE: u128 = 100 * u128::pow(10, 9); // Given in units of Wei.
pub const DEFAULT_STRK_L1_GAS_PRICE: u128 = 100 * u128::pow(10, 9); // Given in units of STRK.
pub const DEFAULT_ETH_L1_DATA_GAS_PRICE: u128 = u128::pow(10, 6); // Given in units of Wei.
pub const DEFAULT_STRK_L1_DATA_GAS_PRICE: u128 = u128::pow(10, 9); // Given in units of STRK.

// The block number of the BlockContext being used for testing.
pub const CURRENT_BLOCK_NUMBER: u64 = 2001;
pub const CURRENT_BLOCK_NUMBER_FOR_VALIDATE: u64 = 2000;

// The block timestamp of the BlockContext being used for testing.
pub const CURRENT_BLOCK_TIMESTAMP: u64 = 1072023;
pub const CURRENT_BLOCK_TIMESTAMP_FOR_VALIDATE: u64 = 1069200;

pub const CHAIN_ID_NAME: &str = "SN_GOERLI";

#[derive(Default)]
pub struct NonceManager {
    next_nonce: HashMap<ContractAddress, Felt>,
}

impl NonceManager {
    pub fn next(&mut self, account_address: ContractAddress) -> Nonce {
        let next = self.next_nonce.remove(&account_address).unwrap_or_default();
        self.next_nonce.insert(account_address, next + 1);
        Nonce(next)
    }

    /// Decrements the nonce of the account, unless it is zero.
    pub fn rollback(&mut self, account_address: ContractAddress) {
        let current = *self.next_nonce.get(&account_address).unwrap_or(&Felt::default());
        if current != Felt::ZERO {
            self.next_nonce.insert(account_address, current - 1);
        }
    }
}

// TODO(Yoni, 1/1/2025): move to SN API.
/// A utility macro to create a [`Nonce`] from a hex string / unsigned integer representation.
#[macro_export]
macro_rules! nonce {
    ($s:expr) => {
        starknet_api::core::Nonce(starknet_types_core::felt::Felt::from($s))
    };
}

// TODO(Yoni, 1/1/2025): move to SN API.
/// A utility macro to create a [`StorageKey`] from a hex string / unsigned integer representation.
#[macro_export]
macro_rules! storage_key {
    ($s:expr) => {
        starknet_api::state::StorageKey(starknet_api::patricia_key!($s))
    };
}

// TODO(Yoni, 1/1/2025): move to SN API.
/// A utility macro to create a [`CompiledClassHash`] from a hex string / unsigned integer
/// representation.
#[macro_export]
macro_rules! compiled_class_hash {
    ($s:expr) => {
        starknet_api::core::CompiledClassHash(starknet_types_core::felt::Felt::from($s))
    };
}

#[derive(Default)]
pub struct SaltManager {
    next_salt: u8,
}

impl SaltManager {
    pub fn next_salt(&mut self) -> ContractAddressSalt {
        let next_contract_address_salt = ContractAddressSalt(felt!(self.next_salt));
        self.next_salt += 1;
        next_contract_address_salt
    }
}

pub fn pad_address_to_64(address: &str) -> String {
    let trimmed_address = address.strip_prefix("0x").unwrap_or(address);
    String::from("0x") + format!("{trimmed_address:0>64}").as_str()
}

pub fn get_raw_contract_class(contract_path: &str) -> String {
    let path: PathBuf = [env!("CARGO_MANIFEST_DIR"), contract_path].iter().collect();
    fs::read_to_string(path).unwrap()
}

pub fn trivial_external_entry_point_new(contract: FeatureContract) -> CallEntryPoint {
    let address = contract.get_instance_address(0);
    trivial_external_entry_point_with_address(address)
}

pub fn trivial_external_entry_point_with_address(
    contract_address: ContractAddress,
) -> CallEntryPoint {
    CallEntryPoint {
        code_address: Some(contract_address),
        storage_address: contract_address,
        initial_gas: VersionedConstants::create_for_testing()
            .os_constants
            .gas_costs
            .initial_gas_cost,
        ..Default::default()
    }
}

pub fn erc20_external_entry_point() -> CallEntryPoint {
    trivial_external_entry_point_with_address(contract_address!(TEST_ERC20_FULL_CONTRACT_ADDRESS))
}

pub fn default_testing_resource_bounds() -> DeprecatedResourceBoundsMapping {
    DeprecatedResourceBoundsMapping::try_from(vec![
        (Resource::L1Gas, ResourceBounds { max_amount: 0, max_price_per_unit: 1 }),
        // TODO(Dori, 1/2/2024): When fee market is developed, change the default price of
        //   L2 gas.
        (Resource::L2Gas, ResourceBounds { max_amount: 0, max_price_per_unit: 0 }),
    ])
    .unwrap()
}

#[macro_export]
macro_rules! check_inner_exc_for_custom_hint {
    ($inner_exc:expr, $expected_hint:expr) => {
        if let cairo_vm::vm::errors::vm_errors::VirtualMachineError::Hint(hint) = $inner_exc {
            if let cairo_vm::vm::errors::hint_errors::HintError::Internal(
                cairo_vm::vm::errors::vm_errors::VirtualMachineError::Other(error),
            ) = &hint.1
            {
                assert_eq!(error.to_string(), $expected_hint.to_string());
            } else {
                panic!("Unexpected hint: {:?}", hint);
            }
        } else {
            panic!("Unexpected structure for inner_exc: {:?}", $inner_exc);
        }
    };
}

#[macro_export]
macro_rules! check_inner_exc_for_invalid_scenario {
    ($inner_exc:expr) => {
        if let cairo_vm::vm::errors::vm_errors::VirtualMachineError::DiffAssertValues(_) =
            $inner_exc
        {
        } else {
            panic!("Unexpected structure for inner_exc: {:?}", $inner_exc)
        }
    };
}

#[macro_export]
macro_rules! check_entry_point_execution_error {
    ($error:expr, $expected_hint:expr $(,)?) => {
        if let $crate::execution::errors::EntryPointExecutionError::CairoRunError(
            cairo_vm::vm::errors::cairo_run_errors::CairoRunError::VmException(
                cairo_vm::vm::errors::vm_exception::VmException { inner_exc, .. },
            ),
        ) = $error
        {
            match $expected_hint {
                Some(expected_hint) => {
                    $crate::check_inner_exc_for_custom_hint!(inner_exc, expected_hint)
                }
                None => $crate::check_inner_exc_for_invalid_scenario!(inner_exc),
            };
        } else {
            panic!("Unexpected structure for error: {:?}", $error);
        }
    };
}

/// Checks that the given error is a `HintError::CustomHint` with the given hint.
#[macro_export]
macro_rules! check_entry_point_execution_error_for_custom_hint {
    ($error:expr, $expected_hint:expr $(,)?) => {
        $crate::check_entry_point_execution_error!($error, Some($expected_hint))
    };
}

#[macro_export]
macro_rules! check_transaction_execution_error_inner {
    ($error:expr, $expected_hint:expr, $validate_constructor:expr $(,)?) => {
        if $validate_constructor {
            match $error {
                $crate::transaction::errors::TransactionExecutionError::
                    ContractConstructorExecutionFailed(
                    $crate::execution::errors::ConstructorEntryPointExecutionError::ExecutionError {
                        error, ..
                    }
                ) => {
                    $crate::check_entry_point_execution_error!(error, $expected_hint)
                }
                _ => panic!("Unexpected structure for error: {:?}", $error),
            }
        } else {
            match $error {
                $crate::transaction::errors::TransactionExecutionError::ValidateTransactionError {
                    error,
                    ..
                } => {
                    $crate::check_entry_point_execution_error!(error, $expected_hint)
                }
                _ => panic!("Unexpected structure for error: {:?}", $error),
            }
        }
    };
}

#[macro_export]
macro_rules! check_transaction_execution_error_for_custom_hint {
    ($error:expr, $expected_hint:expr, $validate_constructor:expr $(,)?) => {
        $crate::check_transaction_execution_error_inner!(
            $error,
            Some($expected_hint),
            $validate_constructor,
        );
    };
}

/// Checks that a given error is an assertion error with the expected message.
/// Formatted for test_validate_accounts_tx.
#[macro_export]
macro_rules! check_transaction_execution_error_for_invalid_scenario {
    ($cairo_version:expr, $error:expr, $validate_constructor:expr $(,)?) => {
        match $cairo_version {
            CairoVersion::Cairo0 => {
                $crate::check_transaction_execution_error_inner!(
                    $error,
                    None::<&str>,
                    $validate_constructor,
                );
            }
            CairoVersion::Cairo1 | CairoVersion::Native => {
                if let $crate::transaction::errors::TransactionExecutionError::ValidateTransactionError {
                    error, ..
                } = $error {
                    assert_eq!(
                        error.to_string(),
                        "Execution failed. Failure reason: 0x496e76616c6964207363656e6172696f \
                         ('Invalid scenario')."
                    )
                }
            }
        }
    };
}

pub fn get_syscall_resources(syscall_selector: SyscallSelector) -> ExecutionResources {
    let versioned_constants = VersionedConstants::create_for_testing();
    let syscall_counter: SyscallCounter = HashMap::from([(syscall_selector, 1)]);
    versioned_constants.get_additional_os_syscall_resources(&syscall_counter).unwrap()
}

pub fn get_tx_resources(tx_type: TransactionType) -> ExecutionResources {
    let versioned_constants = VersionedConstants::create_for_testing();
    let starknet_resources =
        StarknetResources::new(1, 0, 0, StateChangesCount::default(), None, std::iter::empty());

    versioned_constants.get_additional_os_tx_resources(tx_type, &starknet_resources, false).unwrap()
}

/// Creates the calldata for the Cairo function "test_deploy" in the featured contract TestContract.
/// The format of the calldata is:
/// [
///     class_hash,
///     contract_address_salt,
///     constructor_calldata_len,
///     *constructor_calldata,
///     deploy_from_zero
/// ]
pub fn calldata_for_deploy_test(
    class_hash: ClassHash,
    constructor_calldata: &[Felt],
    valid_deploy_from_zero: bool,
) -> Calldata {
    Calldata(
        [
            vec![
                class_hash.0,
                ContractAddressSalt::default().0,
                felt!(u128_from_usize(constructor_calldata.len())),
            ],
            constructor_calldata.into(),
            vec![felt!(if valid_deploy_from_zero { 0_u8 } else { 2_u8 })],
        ]
        .concat()
        .into(),
    )
}

/// Creates the calldata for the "__execute__" entry point in the featured contracts
/// AccountWithLongValidate and AccountWithoutValidations. The format of the returned calldata is:
/// [
///     contract_address,
///     entry_point_name,
///     calldata_length,
///     *calldata,
/// ]
/// The contract_address is the address of the called contract, the entry_point_name is the
/// name of the called entry point in said contract, and the calldata is the calldata for the called
/// entry point.
pub fn create_calldata(
    contract_address: ContractAddress,
    entry_point_name: &str,
    entry_point_args: &[Felt],
) -> Calldata {
    Calldata(
        [
            vec![
                *contract_address.0.key(),              // Contract address.
                selector_from_name(entry_point_name).0, // EP selector name.
                felt!(u128_from_usize(entry_point_args.len())),
            ],
            entry_point_args.into(),
        ]
        .concat()
        .into(),
    )
}

/// Calldata for a trivial entry point in the test contract.
pub fn create_trivial_calldata(test_contract_address: ContractAddress) -> Calldata {
    create_calldata(
        test_contract_address,
        "return_result",
        &[felt!(2_u8)], // Calldata: num.
    )
}

pub fn u64_from_usize(val: usize) -> u64 {
    val.try_into().unwrap()
}

pub fn update_json_value(base: &mut serde_json::Value, update: serde_json::Value) {
    match (base, update) {
        (serde_json::Value::Object(base_map), serde_json::Value::Object(update_map)) => {
            base_map.extend(update_map);
        }
        _ => panic!("Both base and update should be of type serde_json::Value::Object."),
    }
}

pub fn create_erc20_deploy_test_state() -> CachedState<DictStateReader> {
    let address_to_class_hash: HashMap<ContractAddress, ClassHash> = HashMap::from([(
        contract_address!(TEST_ERC20_FULL_CONTRACT_ADDRESS),
        class_hash!(TEST_ERC20_FULL_CONTRACT_CLASS_HASH),
    )]);

    CachedState::from(DictStateReader {
        address_to_class_hash,
        class_hash_to_class: get_erc20_class_hash_mapping(),
        ..Default::default()
    })
}

pub fn deploy_contract(
    state: &mut dyn State,
    class_hash: Felt,
    contract_address_salt: Felt,
    calldata: &[Felt],
) -> SyscallResult<(Felt, Vec<Felt>)> {
    let deployer_address = ContractAddress::default();

    let class_hash = ClassHash(class_hash);

    let wrapper_calldata = Calldata(Arc::new(calldata.to_vec()));

    let calculated_contract_address = calculate_contract_address(
        ContractAddressSalt(contract_address_salt),
        class_hash,
        &wrapper_calldata,
        deployer_address,
    )
    .map_err(|_| vec![Felt::from_hex(FAILED_TO_CALCULATE_CONTRACT_ADDRESS).unwrap()])?;

    let ctor_context = ConstructorContext {
        class_hash,
        code_address: Some(calculated_contract_address),
        storage_address: calculated_contract_address,
        caller_address: deployer_address,
    };

    let call_info = execute_deployment(
        state,
        &mut Default::default(),
        &mut EntryPointExecutionContext::new(
            Arc::new(TransactionContext {
                block_context: BlockContext::create_for_testing(),
                tx_info: TransactionInfo::Current(Default::default()),
            }),
            ExecutionMode::Execute,
            false,
        )
        .unwrap(),
        ctor_context,
        wrapper_calldata,
        u64::MAX,
    )
    .map_err(|err| encode_str_as_felts(&err.to_string()))?;

    let return_data = call_info.execution.retdata.0;
    let contract_address_felt = *calculated_contract_address.0.key();

    Ok((contract_address_felt, return_data))
}

pub fn prepare_erc20_deploy_test_state() -> (ContractAddress, CachedState<DictStateReader>) {
    let mut state = create_erc20_deploy_test_state();

    let class_hash = Felt::from_hex(TEST_ERC20_FULL_CONTRACT_CLASS_HASH).unwrap();

    let (contract_address, _) = deploy_contract(&mut state, class_hash, Felt::from(0), &[
        contract_address_to_native_felt(Signers::Alice.into()), // Recipient
        contract_address_to_native_felt(Signers::Alice.into()), // Owner
    ])
    .unwrap_or_else(|e| panic!("Failed to deploy contract: {:?}", decode_felts_as_str(&e)));

    let contract_address = ContractAddress(PatriciaKey::try_from(contract_address).unwrap());

    (contract_address, state)
}

#[derive(Debug, Clone, Copy)]
pub enum Signers {
    Alice,
    Bob,
    Charlie,
}

impl Signers {
    pub fn get_address(&self) -> ContractAddress {
        match self {
            Signers::Alice => ContractAddress(patricia_key!(0x001u128)),
            Signers::Bob => ContractAddress(patricia_key!(0x002u128)),
            Signers::Charlie => ContractAddress(patricia_key!(0x003u128)),
        }
    }
}

impl From<Signers> for ContractAddress {
    fn from(val: Signers) -> ContractAddress {
        val.get_address()
    }
}

impl From<Signers> for Felt {
    fn from(val: Signers) -> Felt {
        contract_address_to_native_felt(val.get_address())
    }
}

#[derive(Debug, Clone)]
pub struct TestEvent {
    pub data: Vec<Felt>,
    pub keys: Vec<Felt>,
}

impl From<OrderedEvent> for TestEvent {
    fn from(value: OrderedEvent) -> Self {
        let event_data = value.event.data.0;
        let event_keys = value.event.keys.iter().map(|e| e.0).collect();
        Self { data: event_data, keys: event_keys }
    }
}

pub struct TestContext {
    pub contract_address: ContractAddress,
    pub state: CachedState<DictStateReader>,
    pub caller_address: ContractAddress,
    pub events: Vec<TestEvent>,
}

impl Default for TestContext {
    fn default() -> Self {
        let (contract_address, state) = prepare_erc20_deploy_test_state();
        Self { contract_address, state, caller_address: contract_address, events: vec![] }
    }
}

impl TestContext {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_caller(mut self, caller_address: ContractAddress) -> Self {
        self.caller_address = caller_address;
        self
    }

    pub fn call_entry_point(&mut self, entry_point_name: &str, calldata: Vec<Felt>) -> Vec<Felt> {
        let result = self.call_entry_point_raw(entry_point_name, calldata).unwrap();
        result.execution.retdata.0.to_vec()
    }

    pub fn call_entry_point_raw(
        &mut self,
        entry_point_name: &str,
        calldata: Vec<Felt>,
    ) -> Result<CallInfo, String> {
        let entry_point_selector = selector_from_name(entry_point_name);
        let calldata = Calldata(Arc::new(calldata));

        let entry_point_call = CallEntryPoint {
            calldata,
            entry_point_selector,
            code_address: Some(self.contract_address),
            storage_address: self.contract_address,
            caller_address: self.caller_address,
            ..erc20_external_entry_point()
        };

        let result =
            entry_point_call.execute_directly(&mut self.state).map_err(|e| e.to_string())?;

        let events = result.execution.events.clone();

        self.events.extend(events.iter().map(|e| e.clone().into()));

        Ok(result)
    }

    pub fn get_event(&self, index: usize) -> Option<TestEvent> {
        self.events.get(index).cloned()
    }

    pub fn get_caller(&self) -> ContractAddress {
        self.caller_address
    }
}
