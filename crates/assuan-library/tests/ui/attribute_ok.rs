use assuan_library::{Command, CommandContext, HandlerError, assuan_command};

#[assuan_command("GETINFO", "Returns server information")]
async fn get_info(
  _command: Command<'_>,
  context: &mut CommandContext<'_>,
) -> Result<(), HandlerError> {
  return context.send_data(b"1.0.0").await;
}

fn main() {
  let mut server = assuan_library::Server::new(|| (), Default::default());
  server.register(get_info).unwrap();
  let command = assuan_library::command!("GETINFO version");
  assert_eq!(command.args(), b"version");
  const SEXP: &[u8] = assuan_library::sexpr!(("hash" b"\x00\xff"));
  assert_eq!(SEXP, b"(4:hash2:\x00\xff)");
}
