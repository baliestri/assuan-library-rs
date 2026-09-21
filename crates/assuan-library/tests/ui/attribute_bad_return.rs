#[assuan_library::assuan_command("INVALID", "Invalid result")]
async fn invalid(
  _: assuan_library::Command<'_>,
  _: &mut assuan_library::CommandContext<'_>,
) -> bool {
  return true;
}

fn main() {}
