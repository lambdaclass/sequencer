use std::collections::HashMap;

use cairo_native::execution_result::ContractExecutionResult;
use cairo_native::utils::BuiltinCosts;
use cairo_vm::types::builtin_name::BuiltinName;
use cairo_vm::vm::runners::cairo_runner::ExecutionResources;

use crate::execution::call_info::{CallExecution, CallInfo, Retdata};
use crate::execution::contract_class::TrackedResource;
use crate::execution::entry_point::{
    EntryPointExecutionContext,
    EntryPointExecutionResult,
    ExecutableCallEntryPoint,
};
use crate::execution::errors::{EntryPointExecutionError, PostExecutionError, PreExecutionError};
use crate::execution::native::contract_class::NativeCompiledClassV1;
use crate::execution::native::syscall_handler::NativeSyscallHandler;
use crate::state::state_api::State;

// todo(rodrigo): add an `entry point not found` test for Native
pub fn execute_entry_point_call(
    call: ExecutableCallEntryPoint,
    compiled_class: NativeCompiledClassV1,
    state: &mut dyn State,
    context: &mut EntryPointExecutionContext,
) -> EntryPointExecutionResult<CallInfo> {
    let entry_point = compiled_class.get_entry_point(&call.type_and_selector())?;

    let mut syscall_handler: NativeSyscallHandler<'_> =
        NativeSyscallHandler::new(call, state, context);

    let gas_costs = &syscall_handler.base.context.gas_costs();
    let builtin_costs = BuiltinCosts {
        // todo(rodrigo): Unsure of what value `const` means, but 1 is the right value.
        r#const: 1,
        pedersen: gas_costs.builtins.pedersen,
        bitwise: gas_costs.builtins.bitwise,
        ecop: gas_costs.builtins.ecop,
        poseidon: gas_costs.builtins.poseidon,
        add_mod: gas_costs.builtins.add_mod,
        mul_mod: gas_costs.builtins.mul_mod,
    };

    // Pre-charge entry point's initial budget to ensure sufficient gas for executing a minimal
    // entry point code. When redepositing is used, the entry point is aware of this pre-charge
    // and adjusts the gas counter accordingly if a smaller amount of gas is required.
    let initial_budget = syscall_handler.base.context.gas_costs().base.entry_point_initial_budget;
    let call_initial_gas = syscall_handler
        .base
        .call
        .initial_gas
        .checked_sub(initial_budget)
        .ok_or(PreExecutionError::InsufficientEntryPointGas)?;

    let execution_result = compiled_class.executor.run(
        entry_point.selector.0,
        &syscall_handler.base.call.calldata.0.clone(),
        call_initial_gas,
        Some(builtin_costs),
        &mut syscall_handler,
    );

    syscall_handler.finalize();

    let call_result = execution_result.map_err(EntryPointExecutionError::NativeUnexpectedError)?;

    if let Some(error) = syscall_handler.unrecoverable_error {
        return Err(EntryPointExecutionError::NativeUnrecoverableError(Box::new(error)));
    }

    create_callinfo(call_result, syscall_handler)
}

fn create_callinfo(
    call_result: ContractExecutionResult,
    syscall_handler: NativeSyscallHandler<'_>,
) -> Result<CallInfo, EntryPointExecutionError> {
    let remaining_gas = call_result.remaining_gas;

    if remaining_gas > syscall_handler.base.call.initial_gas {
        return Err(PostExecutionError::MalformedReturnData {
            error_message: format!(
                "Unexpected remaining gas. Used gas is greater than initial gas: {} > {}",
                remaining_gas, syscall_handler.base.call.initial_gas
            ),
        }
        .into());
    }

    let gas_consumed = syscall_handler.base.call.initial_gas - remaining_gas;
    let vm_resources = CallInfo::summarize_vm_resources(syscall_handler.base.inner_calls.iter());

    // Retrive the builtin counts from the syscall handler
    let version_constants = syscall_handler.base.context.versioned_constants();
    let syscall_resources =
        version_constants.get_additional_os_syscall_resources(&syscall_handler.syscalls_usage);

    Ok(CallInfo {
        call: syscall_handler.base.call.into(),
        execution: CallExecution {
            retdata: Retdata(call_result.return_values),
            events: syscall_handler.base.events,
            l2_to_l1_messages: syscall_handler.base.l2_to_l1_messages,
            failed: call_result.failure_flag,
            gas_consumed,
        },
        resources: vm_resources,
        inner_calls: syscall_handler.base.inner_calls,
        storage_read_values: syscall_handler.base.read_values,
        accessed_storage_keys: syscall_handler.base.accessed_keys,
        accessed_contract_addresses: syscall_handler.base.accessed_contract_addresses,
        read_class_hash_values: syscall_handler.base.read_class_hash_values,
        tracked_resource: TrackedResource::SierraGas,
        time: std::time::Duration::default(),
        builtin_stats: builtin_stats_to_builtin_counter_map(builtin_stats, syscall_resources),
        call_counter: 0,
    })
}

fn builtin_stats_to_builtin_counter_map(
    builtin_stats: BuiltinStats,
    syscall_counts: ExecutionResources,
) -> BuiltinCounterMap {
    let mut map = HashMap::new();
    let builtin_counts = syscall_counts.builtin_instance_counter;
    builtin_stats.insert(
        BuiltinName::range_check,
        call_result.builtin_stats.range_check
            + builtin_counts.get(&BuiltinName::range_check).unwrap_or_default(),
    );
    builtin_stats.insert(
        BuiltinName::pedersen,
        call_result.builtin_stats.pedersen
            + builtin_counts.get(&BuiltinName::pedersen).unwrap_or_default(),
    );
    builtin_stats
        .insert(BuiltinName::ecdsa, builtin_counts.get(&BuiltinName::ecdsa).unwrap_or_default());
    builtin_stats
        .insert(BuiltinName::keccak, builtin_counts.get(&BuiltinName::keccak).unwrap_or_default());
    builtin_stats.insert(
        BuiltinName::bitwise,
        call_result.builtin_stats.bitwise
            + builtin_counts.get(&BuiltinName::bitwise).unwrap_or_default(),
    );
    builtin_stats.insert(
        BuiltinName::ec_op,
        call_result.builtin_stats.ec_op
            + builtin_counts.get(&BuiltinName::ec_op).unwrap_or_default(),
    );
    builtin_stats.insert(
        BuiltinName::poseidon,
        call_result.builtin_stats.poseidon
            + builtin_counts.get(&BuiltinName::poseidon).unwrap_or_default(),
    );
    builtin_stats.insert(
        BuiltinName::segment_arena,
        call_result.builtin_stats.segment_arena
            + builtin_counts.get(&BuiltinName::segment_arena).unwrap_or_default(),
    );
    builtin_stats.insert(
        BuiltinName::range_check96,
        call_result.builtin_stats.range_check96
            + builtin_counts.get(&BuiltinName::range_check96).unwrap_or_default(),
    );
    builtin_stats.insert(
        BuiltinName::add_mod,
        call_result.builtin_stats.add_mod
            + builtin_counts.get(&BuiltinName::add_mod).unwrap_or_default(),
    );
    builtin_stats.insert(
        BuiltinName::mul_mod,
        call_result.builtin_stats.mul_mod
            + builtin_counts.get(&BuiltinName::mul_mod).unwrap_or_default(),
    );
    builtin_stats.retain(|_, &mut v| v != 0);

    dbg!(builtin_counts);

    map
}
