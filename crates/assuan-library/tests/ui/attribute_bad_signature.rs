#[assuan_library::assuan_command("INVALID", "Synchronous functions are rejected")]
fn invalid(
  _: assuan_library::Command<'_>,
  _: &mut assuan_library::CommandContext<'_>,
) -> Result<(), assuan_library::HandlerError> {
  return Ok(());
}

fn main() {}
