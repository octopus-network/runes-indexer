use crate::runes_etching::guard::ProcessEtchingMsgGuard;
use crate::runes_etching::sync::handle_etching_result_task;

pub mod runes_etching;

const SEC_NANOS: u64 = 1_000_000_000;
const MIN_NANOS: u64 = 60 * SEC_NANOS;

pub fn process_etching_task() {
    ic_cdk::spawn(async {
        let _guard = match ProcessEtchingMsgGuard::new() {
            Some(guard) => guard,
            None => return,
        };
        handle_etching_result_task().await;
    });
}
