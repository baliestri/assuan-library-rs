//! Direct consumers of the explicit command attribute.

use std::cell::Cell;

use assuan_macros::assuan_command;
use assuan_protocol::Command;
use assuan_server::{CommandContext, Handler, HandlerError, Registry, RegistryError};

#[assuan_command("ECHO", "Returns borrowed arguments")]
/// Returns the command's public arguments.
pub async fn echo(
  command: Command<'_>,
  context: &mut CommandContext<'_>,
) -> Result<(), HandlerError> {
  std::future::ready(()).await;
  return context.send_data(command.args()).await;
}

#[assuan_command("COUNT", "Updates session-local state")]
async fn count(
  command: Command<'_>,
  context: &mut CommandContext<'_, Cell<usize>>,
) -> Result<(), HandlerError> {
  context.state().set(context.state().get() + command.args().len());
  std::future::ready(()).await;
  return context.send_data(command.args()).await;
}

fn __assuan_command_body() -> &'static [u8] {
  return b"public";
}

#[assuan_command("QUALIFIED", "Uses qualified paths")]
async fn qualified(
  _: assuan_protocol::Command<'_>,
  context: &mut assuan_server::CommandContext<'_>,
) -> core::result::Result<(), assuan_server::HandlerError> {
  return context.send_data(__assuan_command_body()).await;
}

#[test]
fn public_adapter_registers_and_reports_metadata() {
  let mut registry = Registry::<()>::new();
  registry.register(echo).unwrap();
  let handler = registry.get("ECHO").unwrap();
  assert_eq!(handler.description(), "Returns borrowed arguments");
  assert_eq!(handler.name(), "ECHO");
  assert!(matches!(registry.register(echo), Err(RegistryError::Duplicate(_))));
}

#[test]
fn state_need_not_be_sync_and_helper_names_do_not_capture_module_functions() {
  let mut registry = Registry::<Cell<usize>>::new();
  registry.register(count).unwrap();
  assert!(registry.get("COUNT").is_some());
  let mut unit_registry = Registry::<()>::new();
  unit_registry.register(qualified).unwrap();
  assert_eq!(qualified.name(), "QUALIFIED");
}
