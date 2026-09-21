//! Independent external and literal peers for the server.
#[cfg(unix)]
mod support;

use assuan_library::{
  Command, CommandContext, Endpoint, HandlerFuture, ListenOptions, Listener, Server, ServerOptions,
  handler,
};

fn echo<'a>(_: Command<'a>, mut context: CommandContext<'a>) -> HandlerFuture<'a> {
  return Box::pin(async move {
    return context.send_data(b"public").await;
  });
}

fn configured_server() -> Server {
  let mut server = Server::new(|| (), ServerOptions::default());
  server.register(handler("ECHO", "Returns public test data", echo).unwrap()).unwrap();
  return server;
}

#[cfg(unix)]
#[tokio::test]
async fn gpg_connect_agent_drives_the_server_over_a_raw_unix_socket() {
  let Some(mut fixture) = support::gnupg::start_or_skip().await.unwrap() else {
    return;
  };
  let socket = fixture.home().join("server.sock");
  let listener =
    Listener::bind(&Endpoint::Unix(socket.clone()), &ListenOptions::default()).await.unwrap();
  let (stop, stopped) = tokio::sync::oneshot::channel();
  let serving = tokio::spawn(configured_server().serve(listener, async {
    let _ = stopped.await;
  }));
  let result = fixture.connect_raw(&socket, b"NOP\nHELP\nECHO\nBYE\n/bye\n").await;
  let _ = stop.send(());
  serving.await.unwrap().unwrap();
  let output = result.unwrap();
  assert_eq!(
    output.split(|byte| return *byte == b'\n').filter(|line| return *line == b"OK").count(),
    4
  );
  assert!(output.windows(b"D public\n".len()).any(|line| return line == b"D public\n"));
  fixture.shutdown().await.unwrap();
}

#[cfg(windows)]
#[tokio::test]
async fn independent_named_pipe_peer_observes_exact_builtin_and_data_bytes() {
  use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
  let root = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"));
  std::fs::create_dir_all(root).unwrap();
  let unique = tempfile::tempdir_in(root).unwrap();
  let name = unique.path().file_name().unwrap().to_string_lossy();
  let path = format!(r"\\.\pipe\assuan-interop-{name}");
  let listener =
    Listener::bind(&Endpoint::NamedPipe(path.clone().into()), &ListenOptions::default())
      .await
      .unwrap();
  let (stop, stopped) = tokio::sync::oneshot::channel();
  let serving = tokio::spawn(configured_server().serve(listener, async {
    let _ = stopped.await;
  }));
  let io = tokio::net::windows::named_pipe::ClientOptions::new().open(&path).unwrap();
  let mut peer = BufReader::new(io);
  let mut greeting = [0; 3];
  peer.read_exact(&mut greeting).await.unwrap();
  assert_eq!(&greeting, b"OK\n");
  peer.write_all(b"NOP\nHELP\nECHO\nBYE\n").await.unwrap();
  peer.read_exact(&mut greeting).await.unwrap();
  assert_eq!(&greeting, b"OK\n");
  let mut saw_echo = false;
  loop {
    let mut line = String::new();
    assert!(peer.read_line(&mut line).await.unwrap() > 0);
    if line == "OK\n" {
      break;
    }
    assert!(line.starts_with("# "));
    saw_echo |= line.starts_with("# ECHO ");
  }
  assert!(saw_echo);
  let mut data = [0; 15];
  peer.read_exact(&mut data).await.unwrap();
  assert_eq!(&data, b"D public\nOK\nOK\n");
  assert_eq!(peer.read(&mut [0]).await.unwrap(), 0);
  let _ = stop.send(());
  serving.await.unwrap().unwrap();
}
