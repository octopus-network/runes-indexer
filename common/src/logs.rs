use ic_canister_log::{declare_log_buffer, export as export_logs, GlobalBuffer};
use ic_canisters_http_types::{HttpRequest, HttpResponse, HttpResponseBuilder};
use serde_derive::Deserialize;
use time::OffsetDateTime;

declare_log_buffer!(name = DEBUG, capacity = 1000);
declare_log_buffer!(name = INFO, capacity = 1000);
declare_log_buffer!(name = WARNING, capacity = 1000);
declare_log_buffer!(name = ERROR, capacity = 1000);
declare_log_buffer!(name = CRITICAL, capacity = 1000);

#[derive(Clone, serde::Serialize, Deserialize, Debug, Copy)]
pub enum Priority {
  DEBUG,
  INFO,
  WARNING,
  ERROR,
  CRITICAL,
}

#[derive(Clone, serde::Serialize, Deserialize, Debug)]
pub struct LogEntry {
  pub canister_id: String,
  pub timestamp: u64,
  pub time_str: String,
  pub priority: Priority,
  pub file: String,
  pub line: u32,
  pub message: String,
  pub counter: u64,
}

#[derive(Clone, Default, serde::Serialize, Deserialize, Debug)]
pub struct Log {
  pub entries: Vec<LogEntry>,
}

pub fn do_reply(req: HttpRequest) -> HttpResponse {
  if req.path() == "/logs" {
    use std::str::FromStr;
    let max_skip_timestamp = match req.raw_query_param("time") {
      Some(arg) => match u64::from_str(arg) {
        Ok(value) => value,
        Err(_) => {
          return HttpResponseBuilder::bad_request()
            .with_body_and_content_length("failed to parse the 'time' parameter")
            .build()
        }
      },
      None => 0,
    };

    let limit = match req.raw_query_param("limit") {
      Some(arg) => match u64::from_str(arg) {
        Ok(value) => value,
        Err(_) => {
          return HttpResponseBuilder::bad_request()
            .with_body_and_content_length("failed to parse the 'time' parameter")
            .build()
        }
      },
      None => 5000,
    };

    let offset = match req.raw_query_param("offset") {
      Some(arg) => match u64::from_str(arg) {
        Ok(value) => value,
        Err(_) => {
          return HttpResponseBuilder::bad_request()
            .with_body_and_content_length("failed to parse the 'time' parameter")
            .build()
        }
      },
      None => 0,
    };

    let mut entries: Log = Default::default();

    if let Some("true") = req.raw_query_param("debug") {
      merge_log(&mut entries, &DEBUG, Priority::DEBUG);
    }

    merge_log(&mut entries, &INFO, Priority::INFO);
    merge_log(&mut entries, &WARNING, Priority::WARNING);
    merge_log(&mut entries, &ERROR, Priority::ERROR);
    merge_log(&mut entries, &CRITICAL, Priority::CRITICAL);
    entries
      .entries
      .retain(|entry| entry.timestamp >= max_skip_timestamp);
    entries
      .entries
      .sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
    let logs = entries
      .entries
      .into_iter()
      .skip(offset as usize)
      .take(limit as usize)
      .collect::<Vec<_>>();
    HttpResponseBuilder::ok()
      .header("Content-Type", "application/json; charset=utf-8")
      .with_body_and_content_length(serde_json::to_string(&logs).unwrap_or_default())
      .build()
  } else {
    HttpResponseBuilder::not_found().build()
  }
}

fn merge_log(entries: &mut Log, buffer: &'static GlobalBuffer, priority: Priority) {
  let canister_id = ic_cdk::api::id();
  for entry in export_logs(buffer) {
    entries.entries.push(LogEntry {
      timestamp: entry.timestamp,
      canister_id: canister_id.to_string(),
      time_str: OffsetDateTime::from_unix_timestamp_nanos(entry.timestamp as i128)
        .unwrap()
        .to_string(),
      counter: entry.counter,
      priority,
      file: entry.file.to_string(),
      line: entry.line,
      message: entry.message,
    });
  }
}
