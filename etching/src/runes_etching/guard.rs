use crate::runes_etching::etching_state::mutate_state;

#[must_use]
pub struct ProcessEtchingMsgGuard(());

impl ProcessEtchingMsgGuard {
  pub fn new() -> Option<Self> {
    mutate_state(|s| {
      if s.is_process_etching_msg {
        return None;
      }
      s.is_process_etching_msg = true;
      Some(ProcessEtchingMsgGuard(()))
    })
  }
}

impl Drop for ProcessEtchingMsgGuard {
  fn drop(&mut self) {
    mutate_state(|s| {
      s.is_process_etching_msg = false;
    });
  }
}

#[must_use]
pub struct RequestEtchingGuard(());

impl RequestEtchingGuard {
  pub fn new() -> Option<Self> {
    mutate_state(|s| {
      if s.is_request_etching {
        return None;
      }
      s.is_request_etching = true;
      Some(RequestEtchingGuard(()))
    })
  }
}

impl Drop for RequestEtchingGuard {
  fn drop(&mut self) {
    mutate_state(|s| {
      s.is_request_etching = false;
    });
  }
}
