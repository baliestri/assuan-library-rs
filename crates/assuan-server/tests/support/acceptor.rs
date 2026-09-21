//! Independent custom acceptor for concurrency and lifecycle tests.
use std::sync::{
  Arc,
  atomic::{AtomicBool, AtomicUsize, Ordering},
};

use assuan_transport::{Accepted, Acceptor, IoFuture, Stream, TransportError};
use tokio::{io::DuplexStream, sync::mpsc};

#[derive(Default)]
pub struct Stats {
  pub accepted: AtomicUsize,
  pub polls: AtomicUsize,
  pub dropped: AtomicBool,
}
pub struct CustomAcceptor {
  receiver: mpsc::Receiver<Accepted>,
  pub stats: Arc<Stats>,
}
pub fn acceptor() -> (mpsc::Sender<Accepted>, CustomAcceptor, Arc<Stats>) {
  let (sender, receiver) = mpsc::channel(8);
  let stats = Arc::new(Stats::default());
  return (
    sender,
    CustomAcceptor {
      receiver,
      stats: stats.clone(),
    },
    stats,
  );
}
impl Acceptor for CustomAcceptor {
  fn accept(&mut self) -> IoFuture<'_, Accepted> {
    self.stats.polls.fetch_add(1, Ordering::SeqCst);
    return Box::pin(async move {
      let accepted = self.receiver.recv().await.ok_or(TransportError::Closed)?;
      self.stats.accepted.fetch_add(1, Ordering::SeqCst);
      return Ok(accepted);
    });
  }
}
impl Drop for CustomAcceptor {
  fn drop(&mut self) {
    self.stats.dropped.store(true, Ordering::SeqCst);
  }
}
pub async fn connect(sender: &mpsc::Sender<Accepted>) -> DuplexStream {
  let (io, peer) = tokio::io::duplex(2048);
  sender
    .send(Accepted {
      stream: Stream::new(io),
      peer: None,
    })
    .await
    .unwrap();
  return peer;
}
