use bitcoin::script::PushBytesBuf;

use crate::runes_etching::OrdResult;

pub fn bytes_to_push_bytes(bytes: &[u8]) -> OrdResult<PushBytesBuf> {
  let mut push_bytes = PushBytesBuf::with_capacity(bytes.len());
  push_bytes.extend_from_slice(bytes)?;

  Ok(push_bytes)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_bytes_to_push_bytes() {
    let bytes = vec![1, 2, 3];
    let push_bytes = bytes_to_push_bytes(&bytes).unwrap();
    assert_eq!(push_bytes.as_bytes(), bytes.as_slice());
  }
}
