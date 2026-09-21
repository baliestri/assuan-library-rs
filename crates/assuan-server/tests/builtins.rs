//! Built-in commands use literal peers and exactly one final response.
use assuan_server::{OptionRequest, Registry, ServerOptions};
use tokio::io::AsyncWriteExt;
mod support;
use support::{defaults, eof, expect, start};

#[test]
fn option_accepts_documented_separators() {
  let request = OptionRequest::parse(b" --name = value ").unwrap();
  assert_eq!(request.name(), b"name");
  assert_eq!(request.value(), Some(b"value".as_slice()));
}

#[tokio::test]
async fn nop_succeeds_and_bye_acknowledges_before_eof() {
  let (mut peer, run) = start((), Registry::new(), defaults(), ServerOptions::default());
  expect(&mut peer, b"OK\n").await;
  peer.write_all(b"NOP\n").await.unwrap();
  expect(&mut peer, b"OK\n").await;
  peer.write_all(b"BYE\n").await.unwrap();
  expect(&mut peer, b"OK\n").await;
  eof(&mut peer).await;
  run.await.unwrap().unwrap();
}
use assuan_protocol::Command;
use assuan_server::{CommandContext, Handler, HandlerError, HandlerFuture, handler};
use tokio::io::{AsyncBufReadExt, BufReader};

struct Named {
  name: String,
  description: String,
}
impl Handler for Named {
  fn name(&self) -> &str {
    return &self.name;
  }

  fn description(&self) -> &str {
    return &self.description;
  }

  fn call<'a>(&'a self, _: Command<'a>, _: CommandContext<'a>) -> HandlerFuture<'a> {
    return Box::pin(async {
      return Ok(());
    });
  }
}

#[tokio::test]
async fn help_streams_long_metadata_as_utf8_comments_within_the_wire_limit() {
  let name = "N".repeat(999);
  let description = "é🦀".repeat(700);
  let expected = format!("{name} {description}");
  let mut registry = Registry::new();
  registry
    .register(Named {
      name,
      description,
    })
    .unwrap();
  let (mut peer, run) = start((), registry, defaults(), ServerOptions::default());
  expect(&mut peer, b"OK\n").await;
  peer.write_all(b"HELP\n").await.unwrap();
  {
    let mut reader = BufReader::new(&mut peer);
    let mut custom = String::new();
    let mut comments = 0;
    loop {
      let mut line = String::new();
      assert!(reader.read_line(&mut line).await.unwrap() > 0);
      assert!(line.len() <= 1000);
      if line == "OK\n" {
        break;
      }
      let text = line.strip_prefix("# ").unwrap().strip_suffix('\n').unwrap();
      if comments >= 5 {
        custom.push_str(text);
      }
      comments += 1;
    }
    assert!(comments > 6);
    assert_eq!(custom, expected);
  }
  peer.write_all(b"BYE\n").await.unwrap();
  expect(&mut peer, b"OK\n").await;
  eof(&mut peer).await;
  run.await.unwrap().unwrap();
}

fn fail<'a>(_: Command<'a>, _: CommandContext<'a>) -> HandlerFuture<'a> {
  return Box::pin(async {
    return Err(HandlerError::remote(99, "failure").unwrap());
  });
}

#[tokio::test]
async fn command_error_followed_by_nop_has_no_leftover_final() {
  let mut registry = Registry::new();
  registry.register(handler("FAIL", "Fail", fail).unwrap()).unwrap();
  let (mut peer, run) = start((), registry, defaults(), ServerOptions::default());
  expect(&mut peer, b"OK\n").await;
  peer.write_all(b"FAIL\nNOP\nUNKNOWN\nNOP\nBYE\n").await.unwrap();
  expect(&mut peer, b"ERR 99 failure\nOK\nERR 175 unknown command\nOK\nOK\n").await;
  eof(&mut peer).await;
  run.await.unwrap().unwrap();
}
