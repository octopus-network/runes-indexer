use crate::index::get_etching;
use common::logs::INFO;
use etching::runes_etching::guard::ProcessEtchingMsgGuard;
use etching::runes_etching::sync::handle_etching_result_task;
use ic_canister_log::log;

pub fn process_etching_task() {
  ic_cdk::spawn(async {
    let _guard = match ProcessEtchingMsgGuard::new() {
      Some(guard) => guard,
      None => {
        log!(INFO, "GUARD ProcessEtchingMsgGuard init");
        return;
      }
    };
    handle_etching_result_task(get_etching).await;
  });
}
