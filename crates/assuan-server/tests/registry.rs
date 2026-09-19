//! Registration rejects invalid metadata without replacing existing handlers.
use assuan_protocol::Command;
use assuan_server::{CommandContext, HandlerFuture, Registry, RegistryError, handler};

fn noop<'a>(_: Command<'a>, _: CommandContext<'a>) -> HandlerFuture<'a> {
  return Box::pin(async {
    return Ok(());
  });
}

struct Named(String);
impl assuan_server::Handler for Named {
  fn name(&self) -> &str {
    return &self.0;
  }
  fn description(&self) -> &str {
    return "Description";
  }
  fn call<'a>(&'a self, command: Command<'a>, context: CommandContext<'a>) -> HandlerFuture<'a> {
    return noop(command, context);
  }
}
#[test]
fn registration_is_explicit_and_duplicates_fail() {
  let mut registry = Registry::<()>::new();
  registry.register(handler("CUSTOM", "Original", noop).unwrap()).unwrap();
  assert!(matches!(
    registry.register(handler("CUSTOM", "Replacement", noop).unwrap()),
    Err(RegistryError::Duplicate(_))
  ));
  assert_eq!(registry.get("CUSTOM").unwrap().description(), "Original");
  assert!(registry.get("custom").is_none());
}

#[test]
fn names_follow_protocol_syntax_and_wire_limit() {
  for name in ["", "#COMMENT", "HAS SPACE", "HAS\tTAB", "HAS%ESCAPE", "ação"] {
    assert!(handler(name, "Description", noop).is_err(), "accepted {name:?}");
  }
  assert!(Registry::new().register(Named("X".repeat(1000))).is_err());
}

#[test]
fn reserved_names_and_line_injection_are_rejected() {
  for name in ["NOP", "BYE", "HELP", "RESET", "OPTION"] {
    assert!(matches!(handler(name, "Description", noop), Err(RegistryError::Reserved(_))));
  }
  for description in ["bad\rtext", "bad\ntext", "bad\0text"] {
    assert!(matches!(handler("CUSTOM", description, noop), Err(RegistryError::InvalidDescription)));
  }
  assert!(handler("CUSTOM", "Descrição válida", noop).is_ok());
}

#[test]
fn direct_handlers_cannot_bypass_validation() {
  for name in ["#COMMENT", "NOP", "BYE", "HELP", "RESET", "OPTION"] {
    assert!(Registry::new().register(Named(name.to_owned())).is_err());
  }
  let mut registry = Registry::new();
  let longest = "X".repeat(999);
  registry.register(Named(longest.clone())).unwrap();
  assert!(registry.get(&longest).is_some());
  assert!(!format!("{registry:?}").contains(&longest));
}
