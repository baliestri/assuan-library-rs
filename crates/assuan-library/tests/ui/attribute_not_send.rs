#[assuan_library::assuan_command("INVALID", "Future must be Send")]
async fn invalid(
  _: assuan_library::Command<'_>,
  _: &mut assuan_library::CommandContext<'_>,
) -> Result<(), assuan_library::HandlerError> {
  let value = std::rc::Rc::new(1);
  tokio::task::yield_now().await;
  drop(value);
  return Ok(());
}

fn main() {}
