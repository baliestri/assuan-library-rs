//! Inquiry streaming, limits, terminators and forgotten guards.
use assuan_protocol::{Command, PayloadRef, Sensitivity};
use assuan_server::{
  CommandContext, HandlerError, HandlerFuture, InquiryOutcome, Registry, ServerOptions, handler,
};
use tokio::io::AsyncWriteExt;
mod support;
use support::{defaults, eof, expect, start};

fn ask<'a>(cmd: Command<'a>, mut ctx: CommandContext<'a>) -> HandlerFuture<'a> {
  return Box::pin(async move {
    let mut inquiry = ctx.inquire("VALUE", b"challenge", Sensitivity::Secret).await?;
    if cmd.args() == b"drop" {
      drop(inquiry);
      return Ok(());
    }
    if cmd.args() == b"forget" {
      std::mem::forget(inquiry);
      return Ok(());
    }
    if cmd.args() == b"early" {
      inquiry.finish()?;
      return Ok(());
    }
    if cmd.args() == b"cancel-read" {
      let mut next = Box::pin(inquiry.next());
      let waker = std::task::Waker::noop();
      assert!(
        std::future::Future::poll(next.as_mut(), &mut std::task::Context::from_waker(waker))
          .is_pending()
      );
      drop(next);
      std::mem::forget(inquiry);
      return Ok(());
    }
    let mut count = 0;
    while let Some(payload) = inquiry.next().await? {
      assert!(!format!("{payload:?}").contains("SECRET_MARKER"));
      let PayloadRef::Secret(bytes) = payload else {
        panic!("wrong sensitivity");
      };
      count += bytes.expose().len();
    }
    assert!(inquiry.next().await?.is_none());
    assert!(!format!("{inquiry:?}").contains("SECRET_MARKER"));
    if cmd.args() == b"forget-end" {
      std::mem::forget(inquiry);
      return Ok(());
    }
    let outcome = inquiry.finish()?;
    if outcome == InquiryOutcome::Cancelled {
      return if cmd.args() == b"cancel-ok" {
        Ok(())
      } else {
        Err(HandlerError::remote(99, "cancelled").unwrap())
      };
    }
    ctx.send_data(count.to_string().as_bytes()).await?;
    return Ok(());
  });
}

fn registry() -> Registry {
  let mut registry = Registry::new();
  registry.register(handler("ASK", "Ask", ask).unwrap()).unwrap();
  return registry;
}

#[tokio::test]
async fn binary_decoding_empty_chunks_and_comments_share_one_inquiry() {
  let (mut peer, run) = start((), registry(), defaults(), ServerOptions::default());
  expect(&mut peer, b"OK\n").await;
  peer.write_all(b"ASK\n").await.unwrap();
  expect(&mut peer, b"INQUIRE VALUE challenge\n").await;
  peer.write_all(b"# comment\n\nD \nD SECRET_MARKER%00%ff%25\nEND\n").await.unwrap();
  expect(&mut peer, b"D 16\nOK\n").await;
  peer.shutdown().await.unwrap();
  eof(&mut peer).await;
  run.await.unwrap().unwrap();
}

#[tokio::test]
async fn can_leaves_the_final_decision_to_the_handler_and_allows_reuse() {
  let (mut peer, run) = start((), registry(), defaults(), ServerOptions::default());
  expect(&mut peer, b"OK\n").await;
  for (command, final_line) in
    [(b"ASK\n".as_slice(), b"ERR 99 cancelled\n".as_slice()), (b"ASK cancel-ok\n", b"OK\n")]
  {
    peer.write_all(command).await.unwrap();
    expect(&mut peer, b"INQUIRE VALUE challenge\n").await;
    peer.write_all(b"CAN\n").await.unwrap();
    expect(&mut peer, final_line).await;
  }
  peer.shutdown().await.unwrap();
  eof(&mut peer).await;
  run.await.unwrap().unwrap();
}

#[tokio::test]
async fn decoded_byte_budget_accepts_exact_limit_and_rejects_excess() {
  for excess in [false, true] {
    let options = ServerOptions {
      max_inquiry_bytes: 3,
      ..Default::default()
    };
    let (mut peer, run) = start((), registry(), defaults(), options);
    expect(&mut peer, b"OK\n").await;
    peer.write_all(b"ASK\n").await.unwrap();
    expect(&mut peer, b"INQUIRE VALUE challenge\n").await;
    peer
      .write_all(if excess {
        b"D %00%00%00a\nEND\n"
      } else {
        b"D %00%00%00\nEND\n"
      })
      .await
      .unwrap();
    if !excess {
      expect(&mut peer, b"D 3\nOK\n").await;
      peer.shutdown().await.unwrap();
    }
    eof(&mut peer).await;
    let result = run.await.unwrap();
    if excess {
      assert!(matches!(
        result,
        Err(assuan_server::ServerError::Handler(HandlerError::Limit(
          assuan_protocol::LimitError::CapacityExceeded
        )))
      ));
    } else {
      result.unwrap();
    }
  }
}

#[tokio::test]
async fn abandoned_forgotten_and_cancelled_guards_cannot_send_a_final() {
  for arg in ["drop", "forget", "early", "cancel-read", "forget-end"] {
    let (mut peer, run) = start((), registry(), defaults(), ServerOptions::default());
    expect(&mut peer, b"OK\n").await;
    peer.write_all(format!("ASK {arg}\n").as_bytes()).await.unwrap();
    expect(&mut peer, b"INQUIRE VALUE challenge\n").await;
    if arg == "forget-end" {
      peer.write_all(b"END\n").await.unwrap();
    }
    eof(&mut peer).await;
    assert!(run.await.unwrap().is_err());
  }
}

#[tokio::test]
async fn invalid_data_or_eof_during_inquiry_is_fatal() {
  for data in [
    b"D %GG\n".as_slice(),
    b"END garbage\n",
    b"CAN garbage\n",
    b"D \0\n",
    b"ECHO\n",
    b"partial",
    b"",
  ] {
    let (mut peer, run) = start((), registry(), defaults(), ServerOptions::default());
    expect(&mut peer, b"OK\n").await;
    peer.write_all(b"ASK\n").await.unwrap();
    expect(&mut peer, b"INQUIRE VALUE challenge\n").await;
    peer.write_all(data).await.unwrap();
    peer.shutdown().await.unwrap();
    eof(&mut peer).await;
    assert!(run.await.unwrap().is_err());
  }
}

#[tokio::test(start_paused = true)]
async fn inquiry_deadline_is_subordinate_and_never_restarts_per_chunk() {
  use std::time::Duration;

  use tokio::time::{Instant, advance};
  for (command, inquiry) in [(2, 20), (20, 2)] {
    let options = ServerOptions {
      command_timeout: Duration::from_secs(command),
      inquiry_timeout: Duration::from_secs(inquiry),
      ..Default::default()
    };
    let (mut peer, run) = start((), registry(), defaults(), options);
    expect(&mut peer, b"OK\n").await;
    let start = Instant::now();
    peer.write_all(b"ASK\n").await.unwrap();
    expect(&mut peer, b"INQUIRE VALUE challenge\n").await;
    advance(Duration::from_secs(1)).await;
    peer.write_all(b"D x\n").await.unwrap();
    eof(&mut peer).await;
    assert_eq!(Instant::now() - start, Duration::from_secs(2));
    assert!(matches!(
      run.await.unwrap(),
      Err(
        assuan_server::ServerError::Transport(assuan_transport::TransportError::Timeout)
          | assuan_server::ServerError::Handler(HandlerError::Transport(
            assuan_transport::TransportError::Timeout
          ))
      )
    ));
  }
}
