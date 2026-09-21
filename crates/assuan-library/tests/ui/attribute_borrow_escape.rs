struct State {
  retained: Option<&'static [u8]>,
}

#[assuan_library::assuan_command("RETAIN", "Cannot retain command borrows")]
async fn retain(
  command: assuan_library::Command<'_>,
  context: &mut assuan_library::CommandContext<'_, State>,
) -> Result<(), assuan_library::HandlerError> {
  context.state_mut().retained = Some(command.args());
  return Ok(());
}

fn main() {}
