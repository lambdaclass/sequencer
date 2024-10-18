use std::sync::Arc;
use std::time::Instant;

use cairo_vm::vm::runners::cairo_runner::ExecutionResources;

use super::syscall_handler::NativeSyscallHandler;
use super::utils::run_native_executor;
use crate::execution::call_info::CallInfo;
use crate::execution::contract_class::NativeContractClassV1;
use crate::execution::entry_point::{
    CallEntryPoint,
    EntryPointExecutionContext,
    EntryPointExecutionResult,
};
use crate::execution::native::utils::run_sierra_emu_executor;
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

    let mut syscall_handler: NativeSyscallHandler<'_> = NativeSyscallHandler::new(
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

    let result = if cfg!(feature = "use-sierra-emu") {
        let vm = sierra_emu::VirtualMachine::new_starknet(
            Arc::new(contract_class.program.clone()),
            &mut syscall_handler,
        );
        run_sierra_emu_executor(vm, function_id, call.clone())
    } else {
        #[cfg(feature = "with-trace-dump")]
        {
            use std::collections::HashMap;
            use std::sync::atomic::AtomicUsize;
            use std::sync::Mutex;

            use cairo_lang_sierra::program_registry::ProgramRegistry;
            use cairo_native::runtime::trace_dump::TraceDump;
            use cairo_native::types::TypeBuilder;

            // Since the library is statically linked, then dynamically loaded, each instance of
            // `TRACE_DUMP` for each contract is separate (probably). That's why we need this
            // getter and cannot use `cairo_native::runtime::TRACE_DUMP` directly.
            let trace_dump = unsafe {
                let fn_ptr = contract_class
                    .executor
                    .library
                    .get::<extern "C" fn() -> &'static Mutex<HashMap<u64, TraceDump>>>(
                        b"get_trace_dump_ptr\0",
                    )
                    .unwrap();

                fn_ptr()
            };
            let mut trace_dump = trace_dump.lock().unwrap();

            static COUNTER: AtomicUsize = AtomicUsize::new(0);
            let counter_value = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            trace_dump.insert(
                u64::try_from(counter_value).unwrap(),
                TraceDump::new(
                    ProgramRegistry::new(&contract_class.program).unwrap(),
                    |x, registry| x.layout(registry).unwrap(),
                ),
            );

            // Set the active trace id.
            let trace_id_ref = unsafe {
                contract_class
                    .executor
                    .library
                    .get::<u64>(b"TRACE_DUMP__TRACE_ID\0")
                    .unwrap()
                    .try_as_raw_ptr()
                    .unwrap()
                    .cast::<u64>()
                    .as_mut()
                    .unwrap()
            };
            let old_trace_id = *trace_id_ref;

            *trace_id_ref = u64::try_from(counter_value).unwrap();

            println!("Execution started for trace #{counter_value}.");
            dbg!(trace_dump.keys().collect::<Vec<_>>());

            drop(trace_dump);

            let x = run_native_executor(
                &contract_class.executor,
                function_id,
                call,
                syscall_handler,
                counter_value,
            );

            println!("Execution finished for trace #{counter_value}.");

            *trace_id_ref = old_trace_id;

            println!("Resetting to trace #{old_trace_id}.");

            x
        }

        #[cfg(not(feature = "with-trace-dump"))]
        run_native_executor(&contract_class.executor, function_id, call, syscall_handler)
    };
    let execution_time = pre_execution_instant.elapsed().as_millis();
    tracing::info!(time = execution_time, "native contract execution finished");
    result
}
