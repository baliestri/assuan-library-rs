#[assuan_library::assuan_command("NOP", "Cannot replace a built-in")]
async fn invalid(
  _: assuan_library::Command<'_>,
  _: &mut assuan_library::CommandContext<'_>,
) -> Result<(), assuan_library::HandlerError> {
  return Ok(());
}

fn main() {}
