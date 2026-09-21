use library::{Command, CommandContext, HandlerError};

#[library::assuan_command("RENAMED", "Works without the original dependency name")]
async fn renamed(
  command: Command<'_>,
  context: &mut CommandContext<'_>,
) -> Result<(), HandlerError> {
  return context.send_data(command.args()).await;
}

fn main() {
  let mut server = library::Server::new(|| (), Default::default());
  server.register(renamed).unwrap();
  let command = library::command!("RENAMED value");
  assert_eq!(command.name(), "RENAMED");
  const VALUE: &[u8] = library::sexpr!(("renamed" b"\x00"));
  assert_eq!(VALUE, b"(7:renamed1:\x00)");
}
