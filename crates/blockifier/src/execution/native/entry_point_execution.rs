use cairo_vm::vm::runners::cairo_runner::ExecutionResources;
use std::time::Instant;

use super::syscall_handler::NativeSyscallHandler;
use super::utils::run_native_executor;
use crate::execution::call_info::CallInfo;
use crate::execution::contract_class::NativeContractClassV1;
use crate::execution::entry_point::{
    CallEntryPoint, EntryPointExecutionContext, EntryPointExecutionResult,
};
use crate::state::state_api::State;

pub fn execute_entry_point_call(
    call: CallEntryPoint,
    contract_class: NativeContractClassV1,
    state: &mut dyn State,
    resources: &mut ExecutionResources,
    context: &mut EntryPointExecutionContext,
) -> EntryPointExecutionResult<CallInfo> {
    let function_id =
        contract_class.get_entrypoint(call.entry_point_type, call.entry_point_selector)?;

    let syscall_handler: NativeSyscallHandler<'_> = NativeSyscallHandler::new(
        state,
        call.caller_address,
        call.storage_address,
        call.entry_point_selector,
        resources,
        context,
    );

    let class_hash = call.class_hash.unwrap().to_string();

    let _contract_span = tracing::info_span!("native contract execution", class_hash).entered();
    tracing::info!("native contract execution started");
    let pre_execution_instant = Instant::now();
    let result = run_native_executor(&contract_class.executor, function_id, call, syscall_handler);
    let execution_time = pre_execution_instant.elapsed().as_millis();
    tracing::info!(time = execution_time, "native contract execution finished");
    result
}
