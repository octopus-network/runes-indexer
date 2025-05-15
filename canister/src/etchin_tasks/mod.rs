use crate::index::get_etching;
use etching::runes_etching::guard::ProcessEtchingMsgGuard;
use etching::runes_etching::sync::handle_etching_result_task;

pub fn process_etching_task() {
  ic_cdk::spawn(async {
    let _guard = match ProcessEtchingMsgGuard::new() {
      Some(guard) => guard,
      None => return,
    };
    handle_etching_result_task(get_etching).await;
  });
}
