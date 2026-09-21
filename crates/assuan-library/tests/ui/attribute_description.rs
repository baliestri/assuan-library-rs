#[assuan_library::assuan_command("INVALID", "line\nbreak")]
async fn invalid(
  _: assuan_library::Command<'_>,
  _: &mut assuan_library::CommandContext<'_>,
) -> Result<(), assuan_library::HandlerError> {
  return Ok(());
}

fn main() {}
