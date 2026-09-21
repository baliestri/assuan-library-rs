use assuan_protocol::{Command, MAX_LINE_BYTES, Sensitivity, ServerLine, encode_response};
use zeroize::Zeroizing;

use crate::{CommandContext, HandlerError, Registry, context::IoOperation};

const COMMANDS: [(&str, &str); 5] = [
  ("NOP", "No operation"),
  ("BYE", "Close this connection"),
  ("HELP", "List commands"),
  ("RESET", "Reset transient session state"),
  ("OPTION", "Set a session option"),
];

pub(crate) async fn help<S: Send + 'static>(
  registry: &Registry<S>,
  mut context: CommandContext<'_, S>,
) -> Result<(), HandlerError> {
  for (name, description) in COMMANDS {
    entry(&mut context, name, description).await?;
  }
  for (name, handler) in registry.entries() {
    entry(&mut context, name, handler.description()).await?;
  }
  return Ok(());
}

async fn entry<S: Send + 'static>(
  context: &mut CommandContext<'_, S>,
  name: &str,
  description: &str,
) -> Result<(), HandlerError> {
  // Handler metadata can change through interior mutability after registration.
  Command::new(name, description.as_bytes()).map_err(HandlerError::Protocol)?;
  let mut buffer = Box::new(Zeroizing::new([0; MAX_LINE_BYTES]));
  let mut len = 0;
  for part in [name, " ", description] {
    for ch in part.chars() {
      if len + ch.len_utf8() > MAX_LINE_BYTES - 3 {
        comment(context, &buffer[..len]).await?;
        len = 0;
      }
      len += ch.encode_utf8(&mut buffer[len..]).len();
    }
  }
  if len != 0 {
    comment(context, &buffer[..len]).await?;
  }
  return Ok(());
}

async fn comment<S: Send + 'static>(
  context: &mut CommandContext<'_, S>,
  text: &[u8],
) -> Result<(), HandlerError> {
  let mut operation = IoOperation {
    channel: context.channel,
    machine: context.machine,
    complete: false,
  };
  let mut output = Box::new(Zeroizing::new([0; MAX_LINE_BYTES]));
  // The space belongs to the comment payload, so each wire line begins "# ".
  let mut body = Box::new(Zeroizing::new([0; MAX_LINE_BYTES]));
  body[0] = b' ';
  body[1..=text.len()].copy_from_slice(text);
  let line = ServerLine::Comment(&body[..=text.len()]);
  let kind = line.kind();
  let len = encode_response(line, &mut output).map_err(HandlerError::Protocol)?;
  operation.machine.send_response(kind).map_err(HandlerError::State)?;
  operation
    .channel
    .write_line(&output[..len], context.deadline, Sensitivity::Public)
    .await
    .map_err(HandlerError::Transport)?;
  operation.complete = true;
  return Ok(());
}
